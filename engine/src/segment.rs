//! Per-pixel label map ("segmentation") plus the editing operations the UI
//! exposes: recoloring, opacity, merging, pencil, fill, zap, transparency and
//! the problem finder.

use crate::color::{rgb_to_lab, Lab};
use crate::palette::nearest;
use crate::Params;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

fn opaque() -> u8 {
    255
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PaletteEntry {
    pub rgb: [u8; 3],
    pub transparent: bool,
    /// Entry created for pixels that were transparent in the source image.
    #[serde(default)]
    pub source_alpha: bool,
    /// Fill opacity 0..=255 (applied by formats that support it).
    #[serde(default = "opaque")]
    pub alpha: u8,
}

impl PaletteEntry {
    pub fn new(rgb: [u8; 3]) -> Self {
        PaletteEntry { rgb, transparent: false, source_alpha: false, alpha: 255 }
    }
}

#[derive(Debug, Clone)]
pub struct Segmentation {
    pub width: u32,
    pub height: u32,
    /// Row-major label per pixel, indexing `palette`.
    pub labels: Vec<u16>,
    pub palette: Vec<PaletteEntry>,
    /// Supersampling factor relative to the source image.
    pub scale: u32,
    /// Source image (original resolution) for sub-pixel edge recovery.
    pub source: Option<std::sync::Arc<image::RgbaImage>>,
    /// Pixels whose shapes were removed with the background eraser (empty =
    /// none). Labels stay intact so a click can restore the shape.
    pub hidden: Vec<bool>,
}

/// If `c` looks like an anti-aliasing blend of two palette colors, return the
/// one it is closer to. Without this, the grey pixels on a black/white edge
/// would be claimed by a grey palette entry and form slivers along the edge.
fn blend_owner(c: [u8; 3], pal: &[[u8; 3]], nearest_d2: f32) -> Option<usize> {
    let v = [c[0] as f32, c[1] as f32, c[2] as f32];
    // Exact (or near-exact) palette colors are never reinterpreted.
    if nearest_d2 < 12.0 * 12.0 {
        return None;
    }
    let mut best: Option<(f32, usize)> = None;
    for i in 0..pal.len() {
        for j in i + 1..pal.len() {
            let a = [pal[i][0] as f32, pal[i][1] as f32, pal[i][2] as f32];
            let b = [pal[j][0] as f32, pal[j][1] as f32, pal[j][2] as f32];
            let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let av = [v[0] - a[0], v[1] - a[1], v[2] - a[2]];
            let len2 = ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2];
            if len2 < 1.0 {
                continue;
            }
            let t = (av[0] * ab[0] + av[1] * ab[1] + av[2] * ab[2]) / len2;
            if !(0.0..=1.0).contains(&t) {
                continue;
            }
            let d = [av[0] - t * ab[0], av[1] - t * ab[1], av[2] - t * ab[2]];
            let d2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
            if best.map_or(true, |(bd, _)| d2 < bd) {
                best = Some((d2, if t < 0.5 { i } else { j }));
            }
        }
    }
    match best {
        Some((d2, idx)) if d2 < 10.0 * 10.0 && d2 * 4.0 < nearest_d2 => Some(idx),
        _ => None,
    }
}

/// A spot the problem finder thinks deserves a look, in source pixels.
#[derive(Debug, Clone, Serialize)]
pub struct Problem {
    pub x: f64,
    pub y: f64,
    pub kind: &'static str,
}

impl Segmentation {
    /// Assign each pixel to its perceptually nearest palette color. Pixels that
    /// are mostly transparent get a dedicated transparent entry.
    pub fn from_palette(img: &image::RgbaImage, palette: &[[u8; 3]], blend_aware: bool) -> Self {
        let mut entries: Vec<PaletteEntry> = palette.iter().map(|&rgb| PaletteEntry::new(rgb)).collect();
        if entries.is_empty() {
            entries.push(PaletteEntry::new([255, 255, 255]));
        }
        let labs: Vec<Lab> = entries.iter().map(|e| rgb_to_lab(e.rgb)).collect();
        let has_alpha = img.pixels().any(|p| p[3] < 128);
        let transparent_idx = if has_alpha {
            entries.push(PaletteEntry { rgb: [255, 255, 255], transparent: true, source_alpha: true, alpha: 255 });
            Some((entries.len() - 1) as u16)
        } else {
            None
        };
        let mut cache: HashMap<[u8; 3], u16> = HashMap::new();
        let labels = img
            .pixels()
            .map(|p| {
                if p[3] < 128 {
                    return transparent_idx.unwrap();
                }
                let rgb = [p[0], p[1], p[2]];
                *cache.entry(rgb).or_insert_with(|| {
                    let n = nearest(&labs, &rgb_to_lab(rgb));
                    if blend_aware && palette.len() > 2 {
                        let pc = palette[n];
                        let d2 = (0..3).map(|k| (rgb[k] as f32 - pc[k] as f32).powi(2)).sum::<f32>();
                        if let Some(o) = blend_owner(rgb, palette, d2) {
                            return o as u16;
                        }
                    }
                    n as u16
                })
            })
            .collect();
        Segmentation { width: img.width(), height: img.height(), labels, palette: entries, scale: 1, source: None, hidden: Vec::new() }
    }

    #[inline]
    pub fn at(&self, x: u32, y: u32) -> u16 {
        self.labels[(y * self.width + x) as usize]
    }

    pub fn cleanup(&mut self, p: &Params) {
        for _ in 0..p.mode_filter_passes {
            self.mode_filter();
        }
        for _ in 0..3 {
            if self.merge_small_regions(p.min_region_area, p.merge_by_color) == 0 {
                break;
            }
        }
        self.compact_palette();
    }

    /// 3x3 majority filter: removes the single-pixel fringes that anti-aliased
    /// edges produce, while keeping straight edges intact.
    fn mode_filter(&mut self) {
        let (w, h) = (self.width as i64, self.height as i64);
        let src = self.labels.clone();
        let mut counts: Vec<(u16, u8)> = Vec::with_capacity(9);
        for y in 0..h {
            for x in 0..w {
                counts.clear();
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let (nx, ny) = (x + dx, y + dy);
                        if nx < 0 || ny < 0 || nx >= w || ny >= h {
                            continue;
                        }
                        let l = src[(ny * w + nx) as usize];
                        match counts.iter_mut().find(|c| c.0 == l) {
                            Some(c) => c.1 += 1,
                            None => counts.push((l, 1)),
                        }
                    }
                }
                let cur = src[(y * w + x) as usize];
                let cur_n = counts.iter().find(|c| c.0 == cur).map_or(0, |c| c.1);
                let best = counts.iter().max_by_key(|c| c.1).unwrap();
                // Only flip clear minorities so corners are not eroded.
                if best.0 != cur && cur_n <= 2 && best.1 >= 5 {
                    self.labels[(y * w + x) as usize] = best.0;
                }
            }
        }
    }

    /// 4-connected components, as (component id per pixel, component sizes).
    pub fn components(&self) -> (Vec<u32>, Vec<usize>) {
        let (w, h) = (self.width as usize, self.height as usize);
        let mut comp = vec![u32::MAX; w * h];
        let mut sizes = Vec::new();
        let mut stack = Vec::new();
        for start in 0..w * h {
            if comp[start] != u32::MAX {
                continue;
            }
            let id = sizes.len() as u32;
            let lbl = self.labels[start];
            let mut n = 0;
            comp[start] = id;
            stack.push(start);
            while let Some(i) = stack.pop() {
                n += 1;
                let (x, y) = (i % w, i / w);
                let mut visit = |j: usize| {
                    if comp[j] == u32::MAX && self.labels[j] == lbl {
                        comp[j] = id;
                        stack.push(j);
                    }
                };
                if x > 0 { visit(i - 1); }
                if x + 1 < w { visit(i + 1); }
                if y > 0 { visit(i - w); }
                if y + 1 < h { visit(i + w); }
            }
            sizes.push(n);
        }
        (comp, sizes)
    }

    /// Merge every component smaller than `min_area` into a neighbouring
    /// label: the one sharing the longest border (artwork), or the most
    /// similar color (photos). Returns the number of pixels relabelled.
    fn merge_small_regions(&mut self, min_area: usize, by_color: bool) -> usize {
        if min_area <= 1 {
            return 0;
        }
        let (w, h) = (self.width as usize, self.height as usize);
        let (comp, sizes) = self.components();
        let labs: Vec<Lab> = self.palette.iter().map(|e| rgb_to_lab(e.rgb)).collect();
        // For each small component, tally neighbouring labels.
        let mut border: HashMap<u32, (u16, HashMap<u16, u32>)> = HashMap::new();
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                let c = comp[i];
                if sizes[c as usize] >= min_area {
                    continue;
                }
                let own = self.labels[i];
                let mut tally = |j: usize| {
                    if comp[j] != c {
                        *border.entry(c).or_insert_with(|| (own, HashMap::new())).1.entry(self.labels[j]).or_default() += 1;
                    }
                };
                if x > 0 { tally(i - 1); }
                if x + 1 < w { tally(i + 1); }
                if y > 0 { tally(i - w); }
                if y + 1 < h { tally(i + w); }
            }
        }
        let target: HashMap<u32, u16> = border
            .into_iter()
            .filter_map(|(c, (own, t))| {
                let pick = if by_color && !self.palette[own as usize].transparent {
                    t.into_iter().min_by(|a, b| {
                        let da = labs[own as usize].dist2(&labs[a.0 as usize]) / (a.1 as f32).sqrt();
                        let db = labs[own as usize].dist2(&labs[b.0 as usize]) / (b.1 as f32).sqrt();
                        da.partial_cmp(&db).unwrap().then(a.0.cmp(&b.0))
                    })
                } else {
                    t.into_iter().max_by_key(|&(l, n)| (n, std::cmp::Reverse(l)))
                };
                pick.map(|(l, _)| (c, l))
            })
            .collect();
        let mut merged = 0;
        for i in 0..w * h {
            if let Some(&l) = target.get(&comp[i]) {
                self.labels[i] = l;
                merged += 1;
            }
        }
        merged
    }

    /// Drop palette entries no pixel uses and renumber labels.
    pub fn compact_palette(&mut self) {
        let mut used = vec![false; self.palette.len()];
        for &l in &self.labels {
            used[l as usize] = true;
        }
        let mut remap = vec![0u16; self.palette.len()];
        let mut pal = Vec::new();
        for (i, e) in self.palette.iter().enumerate() {
            if used[i] {
                remap[i] = pal.len() as u16;
                pal.push(e.clone());
            }
        }
        if pal.len() == self.palette.len() {
            return;
        }
        for l in &mut self.labels {
            *l = remap[*l as usize];
        }
        self.palette = pal;
    }

    /// Palette entry that dominates the image border.
    pub fn border_label(&self) -> u16 {
        let (w, h) = (self.width, self.height);
        let mut counts = vec![0usize; self.palette.len()];
        for x in 0..w {
            counts[self.at(x, 0) as usize] += 1;
            counts[self.at(x, h - 1) as usize] += 1;
        }
        for y in 0..h {
            counts[self.at(0, y) as usize] += 1;
            counts[self.at(w - 1, y) as usize] += 1;
        }
        counts.iter().enumerate().max_by_key(|c| c.1).map_or(0, |c| c.0 as u16)
    }

    pub fn remove_background(&mut self) {
        let l = self.border_label() as usize;
        self.palette[l].transparent = true;
    }

    pub fn set_transparent(&mut self, idx: usize, transparent: bool) {
        if let Some(e) = self.palette.get_mut(idx) {
            e.transparent = transparent;
        }
    }

    pub fn set_alpha(&mut self, idx: usize, alpha: u8) {
        if let Some(e) = self.palette.get_mut(idx) {
            e.alpha = alpha;
        }
    }

    pub fn recolor(&mut self, idx: usize, rgb: [u8; 3]) {
        if let Some(e) = self.palette.get_mut(idx) {
            e.rgb = rgb;
        }
    }

    /// Reassign all pixels of `from` to `into`, then compact.
    pub fn merge_colors(&mut self, from: u16, into: u16) {
        if from == into || into as usize >= self.palette.len() {
            return;
        }
        for l in &mut self.labels {
            if *l == from {
                *l = into;
            }
        }
        self.compact_palette();
    }

    pub fn add_color(&mut self, rgb: [u8; 3]) -> u16 {
        self.palette.push(PaletteEntry::new(rgb));
        (self.palette.len() - 1) as u16
    }

    /// Pixels covered by a stroke of discs of `radius` from (x0,y0) to (x1,y1).
    fn stroke_pixels(&self, x0: f64, y0: f64, x1: f64, y1: f64, radius: f64) -> Vec<usize> {
        let r = radius.max(0.5);
        let steps = ((x1 - x0).hypot(y1 - y0) / (r * 0.5)).ceil().max(1.0) as usize;
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for s in 0..=steps {
            let t = s as f64 / steps as f64;
            let (cx, cy) = (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t);
            let (xa, xb) = ((cx - r).floor().max(0.0) as i64, (cx + r).ceil().min(self.width as f64 - 1.0) as i64);
            let (ya, yb) = ((cy - r).floor().max(0.0) as i64, (cy + r).ceil().min(self.height as f64 - 1.0) as i64);
            for y in ya..=yb {
                for x in xa..=xb {
                    let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
                    if dx * dx + dy * dy <= r * r {
                        let i = (y as u32 * self.width + x as u32) as usize;
                        if seen.insert(i) {
                            out.push(i);
                        }
                    }
                }
            }
        }
        out
    }

    /// Pencil: paint a stroke with `label`, so fast strokes don't skip pixels.
    pub fn paint_line(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, radius: f64, label: u16) {
        if label as usize >= self.palette.len() {
            return;
        }
        for i in self.stroke_pixels(x0, y0, x1, y1, radius) {
            self.labels[i] = label;
        }
    }

    /// Relabel the given pixels from their surroundings: every pixel takes the
    /// label of the nearest pixel outside the set (multi-source BFS). This
    /// splits a region among its neighbours.
    fn dissolve(&mut self, pixels: &[usize]) {
        let (w, h) = (self.width as usize, self.height as usize);
        let mut inside = vec![false; w * h];
        for &i in pixels {
            inside[i] = true;
        }
        let mut queue = VecDeque::new();
        let neighbours = |i: usize| {
            let (x, y) = (i % w, i / w);
            let mut v = [usize::MAX; 4];
            if x > 0 { v[0] = i - 1; }
            if x + 1 < w { v[1] = i + 1; }
            if y > 0 { v[2] = i - w; }
            if y + 1 < h { v[3] = i + w; }
            v
        };
        for &i in pixels {
            for j in neighbours(i) {
                if j != usize::MAX && !inside[j] {
                    self.labels[i] = self.labels[j];
                    inside[i] = false;
                    queue.push_back(i);
                    break;
                }
            }
        }
        while let Some(i) = queue.pop_front() {
            for j in neighbours(i) {
                if j != usize::MAX && inside[j] {
                    self.labels[j] = self.labels[i];
                    inside[j] = false;
                    queue.push_back(j);
                }
            }
        }
    }

    /// Zap brush: dissolve the pixels under a stroke into their surroundings.
    /// Removes anti-aliasing remnants and cuts thin bridges.
    pub fn zap_line(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, radius: f64) {
        let px = self.stroke_pixels(x0, y0, x1, y1, radius);
        self.dissolve(&px);
    }

    /// Indices of the 4-connected region containing (x, y).
    fn region_at(&self, x: u32, y: u32) -> Vec<usize> {
        let (w, h) = (self.width as usize, self.height as usize);
        let start = y as usize * w + x as usize;
        let lbl = self.labels[start];
        let mut seen = vec![false; w * h];
        let mut out = vec![start];
        seen[start] = true;
        let mut k = 0;
        while k < out.len() {
            let i = out[k];
            k += 1;
            let (px, py) = (i % w, i / w);
            for (ok, j) in [(px > 0, i.wrapping_sub(1)), (px + 1 < w, i + 1), (py > 0, i.wrapping_sub(w)), (py + 1 < h, i + w)] {
                if ok && !seen[j] && self.labels[j] == lbl {
                    seen[j] = true;
                    out.push(j);
                }
            }
        }
        out
    }

    /// Zap a whole region: split it among the regions around it.
    pub fn zap_region(&mut self, x: u32, y: u32) {
        if x >= self.width || y >= self.height {
            return;
        }
        let px = self.region_at(x, y);
        if px.len() == self.labels.len() {
            return; // the only region; nothing to merge into
        }
        self.dissolve(&px);
    }

    #[inline]
    pub fn is_hidden(&self, i: usize) -> bool {
        self.hidden.get(i).copied().unwrap_or(false)
    }

    /// Background eraser: hide the shape under (x, y), or restore it when it is
    /// already hidden. With `whole_color`, every shape of that color toggles.
    /// Returns true when shapes were hidden, false when restored.
    pub fn toggle_hidden(&mut self, x: u32, y: u32, whole_color: bool) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        if self.hidden.is_empty() {
            self.hidden = vec![false; self.labels.len()];
        }
        let start = (y * self.width + x) as usize;
        let hide = !self.hidden[start];
        if whole_color {
            let l = self.labels[start];
            for i in 0..self.labels.len() {
                if self.labels[i] == l {
                    self.hidden[i] = hide;
                }
            }
        } else {
            for i in self.region_at(x, y) {
                self.hidden[i] = hide;
            }
        }
        hide
    }

    /// Bucket fill: give the region containing (x, y) the color `label`.
    pub fn fill(&mut self, x: u32, y: u32, label: u16) {
        if x >= self.width || y >= self.height || label as usize >= self.palette.len() {
            return;
        }
        for i in self.region_at(x, y) {
            self.labels[i] = label;
        }
    }

    /// Problem finder: places where a region is "pinched", i.e. two parts of
    /// one color touch only diagonally while two other pixels cross between
    /// them. Such spots usually produce an unwanted break or bridge. Nearby
    /// hits are reported once. Coordinates are in source pixels.
    pub fn problems(&self, limit: usize) -> Vec<Problem> {
        let (w, h) = (self.width as usize, self.height as usize);
        let cell = (8 * self.scale.max(1)) as usize;
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        let s = self.scale.max(1) as f64;
        for y in 0..h.saturating_sub(1) {
            for x in 0..w.saturating_sub(1) {
                let i = y * w + x;
                let (a, b, c, d) = (self.labels[i], self.labels[i + 1], self.labels[i + w], self.labels[i + w + 1]);
                let pinch = (a == d && b != a && c != a) || (b == c && a != b && d != b);
                if !pinch {
                    continue;
                }
                if seen.insert((x / cell, y / cell)) {
                    out.push(Problem { x: (x + 1) as f64 / s, y: (y + 1) as f64 / s, kind: "pinch" });
                    if out.len() >= limit {
                        return out;
                    }
                }
            }
        }
        out
    }

    /// Render the label map as an RGBA image (transparent entries -> alpha 0).
    pub fn to_rgba(&self) -> image::RgbaImage {
        let mut img = image::RgbaImage::new(self.width, self.height);
        for (i, (px, &l)) in img.pixels_mut().zip(&self.labels).enumerate() {
            let e = &self.palette[l as usize];
            // Erased shapes show faintly so they can be found and restored.
            let a = if e.transparent { 0 } else if self.is_hidden(i) { 60 } else { e.alpha };
            *px = image::Rgba([e.rgb[0], e.rgb[1], e.rgb[2], a]);
        }
        img
    }

    pub fn label_area(&self) -> Vec<usize> {
        let mut a = vec![0; self.palette.len()];
        for &l in &self.labels {
            a[l as usize] += 1;
        }
        a
    }
}
