//! Sub-pixel edge recovery by inverting anti-aliasing.
//!
//! An anti-aliased edge pixel is a mix of the two colors meeting there, and
//! the mixing ratio tells where the true edge passes (the central idea of
//! Diebel's "Bayesian Image Vectorization"). For a boundary between regions
//! with colors A and B we define the field
//!     f(q) = projection of the bilinearly-sampled source color at q
//!            onto the segment A->B   (0 at A, 1 at B)
//! and move each boundary point along the field gradient to f = 0.5.
//! Edges against transparency use the alpha channel as the field.

use crate::geom::Pt;
use crate::segment::Segmentation;
use crate::trace::{Chain, OUTSIDE};

pub struct Snapper<'a> {
    src: &'a image::RgbaImage,
    scale: f64,
    /// Mean source color of each label (robust to palette recoloring).
    means: Vec<[f32; 3]>,
    seg_w: f64,
    seg_h: f64,
}

impl<'a> Snapper<'a> {
    pub fn new(seg: &Segmentation, src: &'a image::RgbaImage) -> Self {
        let scale = seg.scale.max(1);
        let mut sums = vec![[0f64; 4]; seg.palette.len()];
        for y in 0..seg.height {
            for x in 0..seg.width {
                let l = seg.at(x, y) as usize;
                let p = src.get_pixel((x / scale).min(src.width() - 1), (y / scale).min(src.height() - 1));
                if p[3] >= 128 {
                    for k in 0..3 {
                        sums[l][k] += p[k] as f64;
                    }
                    sums[l][3] += 1.0;
                }
            }
        }
        let means = sums
            .iter()
            .zip(&seg.palette)
            .map(|(s, e)| {
                if s[3] > 0.0 {
                    [(s[0] / s[3]) as f32, (s[1] / s[3]) as f32, (s[2] / s[3]) as f32]
                } else {
                    [e.rgb[0] as f32, e.rgb[1] as f32, e.rgb[2] as f32]
                }
            })
            .collect();
        Snapper { src, scale: scale as f64, means, seg_w: seg.width as f64, seg_h: seg.height as f64 }
    }

    /// Bilinear sample at continuous source coordinates (pixel centers at i+0.5).
    fn sample(&self, x: f64, y: f64) -> [f32; 4] {
        let (w, h) = (self.src.width() as i64, self.src.height() as i64);
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = ((fx - x0) as f32, (fy - y0) as f32);
        let px = |xi: i64, yi: i64| self.src.get_pixel(xi.clamp(0, w - 1) as u32, yi.clamp(0, h - 1) as u32);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let (a, b, c, d) = (px(x0, y0), px(x0 + 1, y0), px(x0, y0 + 1), px(x0 + 1, y0 + 1));
        let mut out = [0f32; 4];
        for k in 0..4 {
            let top = a[k] as f32 * (1.0 - tx) + b[k] as f32 * tx;
            let bot = c[k] as f32 * (1.0 - tx) + d[k] as f32 * tx;
            out[k] = top * (1.0 - ty) + bot * ty;
        }
        out
    }

    /// Snap the interior points of a chain onto the 50% iso-line of its edge.
    /// Points are in segmentation coordinates. Chain ends (junctions shared
    /// with other chains) and points flagged in keep stay fixed.
    pub fn snap(&self, chain: &Chain, pts: &mut [Pt], keep: &[bool]) {
        enum Field {
            Color([f32; 3], [f32; 3], f32),
            Alpha { left_transparent: bool },
        }
        let on_border = |p: &Pt| p.x <= 0.0 || p.y <= 0.0 || p.x >= self.seg_w || p.y >= self.seg_h;
        let field = match (chain.left, chain.right) {
            (OUTSIDE, OUTSIDE) => return,
            (OUTSIDE, _) | (_, OUTSIDE) => {
                if pts.iter().all(on_border) {
                    return; // the image frame, not an edge in the picture
                }
                Field::Alpha { left_transparent: chain.left == OUTSIDE }
            }
            (l, r) => {
                let (a, b) = (self.means[l as usize], self.means[r as usize]);
                let len2 = (0..3).map(|k| (b[k] - a[k]).powi(2)).sum::<f32>();
                if len2 < 30.0 * 30.0 {
                    return; // colors too similar for a reliable mixing ratio
                }
                Field::Color(a, b, len2)
            }
        };
        let f = |x: f64, y: f64| -> f64 {
            let c = self.sample(x, y);
            match &field {
                Field::Color(a, b, len2) => {
                    let t: f32 = (0..3).map(|k| (c[k] - a[k]) * (b[k] - a[k])).sum::<f32>() / len2;
                    t.clamp(-0.5, 1.5) as f64
                }
                Field::Alpha { left_transparent } => {
                    let al = c[3] as f64 / 255.0;
                    if *left_transparent { al } else { 1.0 - al }
                }
            }
        };
        let n = pts.len();
        let (first, last) = if chain.closed { (0, n) } else { (1, n.saturating_sub(1)) };
        let h = 0.35;
        for (i, p) in pts.iter_mut().enumerate().take(last).skip(first) {
            if keep[i] {
                continue;
            }
            let start = Pt::new(p.x / self.scale, p.y / self.scale);
            let mut q = start;
            let mut ok = false;
            for _ in 0..4 {
                let v = f(q.x, q.y);
                let g = Pt::new((f(q.x + h, q.y) - f(q.x - h, q.y)) / (2.0 * h), (f(q.x, q.y + h) - f(q.x, q.y - h)) / (2.0 * h));
                let g2 = g.dot(g);
                if g2 < 0.15 * 0.15 {
                    break; // no edge here (e.g. hand-painted boundary)
                }
                let mut step = g.scale((0.5 - v) / g2);
                let sl = step.len();
                if sl > 0.5 {
                    step = step.scale(0.5 / sl);
                }
                q = q.add(step);
                ok = true;
                if sl < 0.01 {
                    break;
                }
            }
            if ok && q.dist(start) <= 1.0 {
                *p = q.scale(self.scale);
            }
        }
    }
}
