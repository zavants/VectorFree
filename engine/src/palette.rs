//! Automatic palette selection: weighted k-means in CIE Lab over a color
//! histogram, with an automatic choice of the number of colors.
//!
//! Artwork: add colors until (nearly) every pixel is explained, either by a
//! palette color or as an anti-aliasing blend of two palette colors. A small
//! but distinct shape therefore always gets its own color.
//! Photos: add colors until the mean color error drops below a target set by
//! the requested level of detail.

use crate::color::{lab_to_rgb, rgb_to_lab, Lab};
use crate::{ImageKind, Quality};
use std::collections::HashMap;

/// A histogram bin: mean color of all pixels falling into it plus their count.
#[derive(Clone, Copy)]
struct Bin {
    lab: Lab,
    rgb: [f32; 3],
    w: f32,
}

fn histogram(img: &image::RgbaImage, bits: u32) -> Vec<Bin> {
    let shift = 8 - bits;
    let mut acc: HashMap<u32, ([f64; 3], u32)> = HashMap::new();
    for p in img.pixels() {
        if p[3] < 128 {
            continue;
        }
        let key = ((p[0] as u32 >> shift) << (2 * bits)) | ((p[1] as u32 >> shift) << bits) | (p[2] as u32 >> shift);
        let e = acc.entry(key).or_insert(([0.0; 3], 0));
        e.0[0] += p[0] as f64;
        e.0[1] += p[1] as f64;
        e.0[2] += p[2] as f64;
        e.1 += 1;
    }
    let mut bins: Vec<Bin> = acc
        .into_values()
        .map(|(s, n)| {
            let n_f = n as f64;
            let rgb = [(s[0] / n_f) as f32, (s[1] / n_f) as f32, (s[2] / n_f) as f32];
            let lab = rgb_to_lab([rgb[0].round() as u8, rgb[1].round() as u8, rgb[2].round() as u8]);
            Bin { lab, rgb, w: n as f32 }
        })
        .collect();
    // Deterministic order regardless of HashMap iteration.
    bins.sort_by(|a, b| {
        b.w.partial_cmp(&a.w)
            .unwrap()
            .then(a.rgb[0].partial_cmp(&b.rgb[0]).unwrap())
            .then(a.rgb[1].partial_cmp(&b.rgb[1]).unwrap())
            .then(a.rgb[2].partial_cmp(&b.rgb[2]).unwrap())
    });
    bins
}

/// Histogram detailed enough for clustering but bounded in size.
fn working_bins(img: &image::RgbaImage) -> Vec<Bin> {
    let exact = histogram(img, 8);
    if exact.len() <= 4096 {
        exact
    } else {
        histogram(img, 5)
    }
}

struct KMeans {
    centers: Vec<Lab>,
    assign: Vec<usize>,
}

fn kmeans(bins: &[Bin], k: usize) -> KMeans {
    let k = k.min(bins.len()).max(1);
    // Deterministic k-means++-style seeding: heaviest bin, then the bin that
    // maximises sqrt(weight) * distance^2 to the chosen centers.
    let mut centers = vec![bins[0].lab];
    let mut d2: Vec<f32> = bins.iter().map(|b| b.lab.dist2(&centers[0])).collect();
    while centers.len() < k {
        let (idx, best) = bins
            .iter()
            .enumerate()
            .map(|(i, b)| (i, b.w.sqrt() * d2[i]))
            .fold((0, -1.0f32), |acc, x| if x.1 > acc.1 { x } else { acc });
        if best <= 0.0 {
            break; // every bin already coincides with a center
        }
        let c = bins[idx].lab;
        centers.push(c);
        for (i, b) in bins.iter().enumerate() {
            d2[i] = d2[i].min(b.lab.dist2(&c));
        }
    }
    let mut assign = vec![0usize; bins.len()];
    for _ in 0..25 {
        let mut changed = false;
        for (i, b) in bins.iter().enumerate() {
            let best = nearest(&centers, &b.lab);
            if best != assign[i] {
                assign[i] = best;
                changed = true;
            }
        }
        let mut sums = vec![(0f32, 0f32, 0f32, 0f32); centers.len()];
        for (i, b) in bins.iter().enumerate() {
            let s = &mut sums[assign[i]];
            s.0 += b.lab.l * b.w;
            s.1 += b.lab.a * b.w;
            s.2 += b.lab.b * b.w;
            s.3 += b.w;
        }
        for (c, s) in centers.iter_mut().zip(&sums) {
            if s.3 > 0.0 {
                *c = Lab { l: s.0 / s.3, a: s.1 / s.3, b: s.2 / s.3 };
            }
        }
        if !changed {
            break;
        }
    }
    KMeans { centers, assign }
}

pub fn nearest(centers: &[Lab], c: &Lab) -> usize {
    let mut best = 0;
    let mut bd = f32::MAX;
    for (j, cc) in centers.iter().enumerate() {
        let d = c.dist2(cc);
        if d < bd {
            bd = d;
            best = j;
        }
    }
    best
}

/// Squared RGB distance from `v` to the segment a-b, if the projection falls
/// strictly inside it (i.e. `v` could be an anti-aliasing mix of a and b).
fn blend_dist2(v: [f32; 3], a: [f32; 3], b: [f32; 3]) -> Option<f32> {
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let av = [v[0] - a[0], v[1] - a[1], v[2] - a[2]];
    let len2 = ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2];
    if len2 < 1.0 {
        return None;
    }
    let t = (av[0] * ab[0] + av[1] * ab[1] + av[2] * ab[2]) / len2;
    if !(0.03..=0.97).contains(&t) {
        return None;
    }
    let d = [av[0] - t * ab[0], av[1] - t * ab[1], av[2] - t * ab[2]];
    Some(d[0] * d[0] + d[1] * d[1] + d[2] * d[2])
}

fn rgbf(c: [u8; 3]) -> [f32; 3] {
    [c[0] as f32, c[1] as f32, c[2] as f32]
}

/// Fraction of pixels explained by a palette: close to one of its colors, or
/// (when `blends`) close to a mix of two of them.
fn explained(bins: &[Bin], palette: &[[u8; 3]], radius: f32, blends: bool) -> f32 {
    let labs: Vec<Lab> = palette.iter().map(|&c| rgb_to_lab(c)).collect();
    let rgbs: Vec<[f32; 3]> = palette.iter().map(|&c| rgbf(c)).collect();
    let r2 = radius * radius;
    // RGB radius roughly equivalent to the Lab one for mid-tones.
    let rb2 = (radius * 2.2) * (radius * 2.2);
    let total: f32 = bins.iter().map(|b| b.w).sum();
    // Anti-aliasing blends are spread thinly over many different tones; a
    // tone covering a sizeable area is a real color even if it happens to lie
    // between two others (e.g. a grey panel between black and white).
    let blend_cap = total * 0.004;
    let mut inside = 0.0;
    for b in bins {
        if labs.iter().any(|l| b.lab.dist2(l) <= r2) {
            inside += b.w;
            continue;
        }
        if blends && b.w <= blend_cap {
            let mut ok = false;
            'pairs: for i in 0..rgbs.len() {
                for j in i + 1..rgbs.len() {
                    if blend_dist2(b.rgb, rgbs[i], rgbs[j]).is_some_and(|d| d <= rb2) {
                        ok = true;
                        break 'pairs;
                    }
                }
            }
            if ok {
                inside += b.w;
            }
        }
    }
    inside / total.max(1.0)
}

/// Turn clusters into concrete colors. Artwork uses the most common color in
/// each cluster (the "true" flat color, not one pulled toward edge blends);
/// photos use the mean.
fn cluster_colors(bins: &[Bin], km: &KMeans, kind: ImageKind) -> Vec<([u8; 3], f32)> {
    let mut out: Vec<([u8; 3], f32)> = Vec::with_capacity(km.centers.len());
    for j in 0..km.centers.len() {
        let weight: f32 = bins.iter().enumerate().filter(|(i, _)| km.assign[*i] == j).map(|(_, b)| b.w).sum();
        if weight <= 0.0 {
            continue;
        }
        let color = match kind {
            ImageKind::Photo => lab_to_rgb(km.centers[j]),
            _ => {
                let (_, b) = bins
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| km.assign[*i] == j)
                    .max_by(|a, b| a.1.w.partial_cmp(&b.1.w).unwrap())
                    .unwrap();
                [b.rgb[0].round() as u8, b.rgb[1].round() as u8, b.rgb[2].round() as u8]
            }
        };
        match out.iter_mut().find(|(c, _)| *c == color) {
            Some(e) => e.1 += weight,
            None => out.push((color, weight)),
        }
    }
    out
}

/// Drop minor colors that are themselves just a blend of two other palette
/// colors: they would steal the anti-aliased edge pixels and form slivers.
fn prune_blend_colors(mut pal: Vec<([u8; 3], f32)>, total: f32) -> Vec<[u8; 3]> {
    loop {
        let mut removed = false;
        for i in 0..pal.len() {
            if pal[i].1 > total * 0.03 || pal.len() <= 2 {
                continue;
            }
            let v = rgbf(pal[i].0);
            let others: Vec<[f32; 3]> = pal.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, p)| rgbf(p.0)).collect();
            let is_blend = (0..others.len())
                .any(|a| (a + 1..others.len()).any(|b| blend_dist2(v, others[a], others[b]).is_some_and(|d| d < 14.0 * 14.0)));
            if is_blend {
                pal.remove(i);
                removed = true;
                break;
            }
        }
        if !removed {
            break;
        }
    }
    // Most common colors first: background tends to come first.
    pal.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    pal.into_iter().map(|p| p.0).collect()
}

fn mean_error(bins: &[Bin], km: &KMeans) -> f32 {
    let total: f32 = bins.iter().map(|b| b.w).sum();
    let err: f32 = bins.iter().enumerate().map(|(i, b)| b.lab.dist2(&km.centers[km.assign[i]]).sqrt() * b.w).sum();
    err / total.max(1.0)
}

fn art_palette(bins: &[Bin], k: usize, kind: ImageKind) -> Vec<[u8; 3]> {
    let total: f32 = bins.iter().map(|b| b.w).sum();
    let km = kmeans(bins, k);
    prune_blend_colors(cluster_colors(bins, &km, kind), total)
}

/// Automatic artwork palette: smallest k whose palette explains the image.
fn auto_art(bins: &[Bin], kind: ImageKind, quality: Quality) -> Vec<[u8; 3]> {
    let (radius, target) = match quality {
        Quality::High => (10.0, 0.997),
        Quality::Medium => (12.0, 0.995),
        Quality::Low => (16.0, 0.99),
    };
    let max_k = 24.min(bins.len());
    let mut best = Vec::new();
    for k in 1..=max_k {
        let pal = art_palette(bins, k, kind);
        let frac = explained(bins, &pal, radius, kind == ImageKind::ArtBlended);
        best = pal;
        if frac >= target {
            break;
        }
    }
    best
}

/// Automatic photo palette: smallest k reaching the mean error target.
fn auto_photo(bins: &[Bin], quality: Quality) -> Vec<[u8; 3]> {
    // Gradients band visibly even when the average error is small, so each
    // detail level also has a floor on the number of shades.
    let (min_k, target) = match quality {
        Quality::High => (32, 2.5),
        Quality::Medium => (16, 4.0),
        Quality::Low => (8, 6.0),
    };
    let mut last = None;
    for k in [8, 12, 16, 20, 24, 32, 40, 48, 64, 80, 96] {
        if k < min_k {
            continue;
        }
        if k > bins.len() {
            break;
        }
        let km = kmeans(bins, k);
        let ok = mean_error(bins, &km) <= target;
        last = Some(km);
        if ok {
            break;
        }
    }
    match last {
        Some(km) => cluster_colors(bins, &km, ImageKind::Photo).into_iter().map(|c| c.0).collect(),
        None => bins.iter().map(|b| [b.rgb[0] as u8, b.rgb[1] as u8, b.rgb[2] as u8]).collect(),
    }
}

/// Pick the palette for an image. `num_colors = None` means automatic.
pub fn choose_palette(img: &image::RgbaImage, kind: ImageKind, quality: Quality, num_colors: Option<usize>) -> Vec<[u8; 3]> {
    let exact = histogram(img, 8);
    if exact.is_empty() {
        return vec![[255, 255, 255]];
    }
    // Flat artwork with few exact colors: use them directly.
    if kind == ImageKind::ArtFlat && num_colors.is_none() && exact.len() <= 64 {
        return exact
            .iter()
            .map(|b| [b.rgb[0].round() as u8, b.rgb[1].round() as u8, b.rgb[2].round() as u8])
            .collect();
    }
    let bins = if exact.len() > 4096 { histogram(img, 5) } else { exact };
    match (num_colors, kind) {
        (Some(k), ImageKind::Photo) => {
            let km = kmeans(&bins, k.clamp(1, 256));
            cluster_colors(&bins, &km, kind).into_iter().map(|c| c.0).collect()
        }
        (Some(k), _) => {
            let km = kmeans(&bins, k.clamp(1, 256));
            let mut cols: Vec<([u8; 3], f32)> = cluster_colors(&bins, &km, kind);
            cols.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            cols.into_iter().map(|c| c.0).collect()
        }
        (None, ImageKind::Photo) => auto_photo(&bins, quality),
        (None, _) => auto_art(&bins, kind, quality),
    }
}

/// Palettes with 2..=max_k colors for the user to pick from, plus the index of
/// the one automatic mode would choose (by color count).
pub fn suggested_palettes(img: &image::RgbaImage, kind: ImageKind, quality: Quality, max_k: usize) -> (Vec<Vec<[u8; 3]>>, usize) {
    let bins = working_bins(img);
    if bins.is_empty() {
        return (vec![vec![[255, 255, 255]]], 0);
    }
    let kind = if kind == ImageKind::Photo { ImageKind::ArtBlended } else { kind };
    let mut out: Vec<Vec<[u8; 3]>> = Vec::new();
    for k in 2..=max_k.clamp(2, 64).min(bins.len().max(2)) {
        let km = kmeans(&bins, k);
        let mut cols = cluster_colors(&bins, &km, kind);
        cols.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let pal: Vec<[u8; 3]> = cols.into_iter().map(|c| c.0).collect();
        if !out.iter().any(|p| p.len() == pal.len()) {
            out.push(pal);
        }
    }
    let auto_n = auto_art(&bins, kind, quality).len();
    let idx = out.iter().position(|p| p.len() >= auto_n).unwrap_or(out.len() - 1);
    (out, idx)
}

/// Guess the image type, as the wizard's first question.
pub fn suggest_kind(img: &image::RgbaImage) -> (ImageKind, usize) {
    let exact = histogram(img, 8);
    let n = exact.len();
    let total: f32 = exact.iter().map(|b| b.w).sum::<f32>().max(1.0);
    // Flat artwork: few colors and every one of them covers real area.
    // Anti-aliasing instead leaves many tones with only a few pixels each.
    let rare: f32 = exact.iter().filter(|b| b.w < (total * 0.002).max(3.0)).map(|b| b.w).sum();
    // Anti-aliasing can also live purely in the alpha channel (one RGB color
    // with soft, semi-transparent edges).
    let visible = img.pixels().filter(|p| p[3] >= 16).count().max(1) as f32;
    let soft = img.pixels().filter(|p| p[3] >= 16 && p[3] <= 240).count() as f32;
    if n <= 64 && rare / total < 0.003 && soft / visible < 0.005 {
        return (ImageKind::ArtFlat, n);
    }
    // Work on a bounded copy; nearest-neighbour keeps the color statistics.
    let (w, h) = img.dimensions();
    let small;
    let img = if w.max(h) > 600 {
        let f = 600.0 / w.max(h) as f32;
        small = image::imageops::resize(
            img,
            ((w as f32 * f) as u32).max(1),
            ((h as f32 * f) as u32).max(1),
            image::imageops::FilterType::Nearest,
        );
        &small
    } else {
        img
    };
    let bins = working_bins(img);
    let pal = auto_art(&bins, ImageKind::ArtBlended, Quality::Low);
    if pal.len() > 16 {
        return (ImageKind::Photo, n);
    }
    // Pixels not close to any palette color are "mixed". In anti-aliased art
    // they form 1-2px bands along edges; in photos and gradients they fill
    // whole areas, so most of them are surrounded by other mixed pixels.
    let labs: Vec<Lab> = pal.iter().map(|&c| rgb_to_lab(c)).collect();
    let (w, h) = img.dimensions();
    let mixed: Vec<bool> = img
        .pixels()
        .map(|p| p[3] >= 128 && labs.iter().all(|l| rgb_to_lab([p[0], p[1], p[2]]).dist2(l) > 12.0 * 12.0))
        .collect();
    let (mut m, mut interior) = (0usize, 0usize);
    for y in 1..h.saturating_sub(1) {
        for x in 1..w.saturating_sub(1) {
            let i = (y * w + x) as usize;
            if !mixed[i] {
                continue;
            }
            m += 1;
            let wi = w as usize;
            if mixed[i - 1] && mixed[i + 1] && mixed[i - wi] && mixed[i + wi] {
                interior += 1;
            }
        }
    }
    // Calibrated on test images: photos ~20% mixed (mostly interior), logos
    // with glossy gradients or JPEG noise stay below ~15%.
    let frac = m as f32 / (w as f32 * h as f32);
    if frac > 0.18 && interior as f32 > 0.5 * m as f32 {
        (ImageKind::Photo, n)
    } else {
        (ImageKind::ArtBlended, n)
    }
}
