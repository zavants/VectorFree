//! Pre-processing filters.

use image::RgbaImage;

/// Per-channel median filter over a (2r+1)^2 window. Removes JPEG ringing
/// and sensor noise while keeping edges in place. Transparent pixels are left
/// untouched and are ignored as neighbours.
pub fn median(img: &RgbaImage, r: u32) -> RgbaImage {
    let (w, h) = img.dimensions();
    let r = r as i64;
    let mut out = img.clone();
    let mut ch: [Vec<u8>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            let p = img.get_pixel(x as u32, y as u32);
            if p[3] < 128 {
                continue;
            }
            for c in &mut ch {
                c.clear();
            }
            for dy in -r..=r {
                let ny = y + dy;
                if ny < 0 || ny >= h as i64 {
                    continue;
                }
                for dx in -r..=r {
                    let nx = x + dx;
                    if nx < 0 || nx >= w as i64 {
                        continue;
                    }
                    let q = img.get_pixel(nx as u32, ny as u32);
                    if q[3] < 128 {
                        continue;
                    }
                    for k in 0..3 {
                        ch[k].push(q[k]);
                    }
                }
            }
            let o = out.get_pixel_mut(x as u32, y as u32);
            for k in 0..3 {
                let n = ch[k].len();
                let (_, m, _) = ch[k].select_nth_unstable(n / 2);
                o[k] = *m;
            }
        }
    }
    out
}

/// Bilinear upscale by an integer factor with premultiplied alpha, so the
/// (often black) color of fully transparent pixels does not bleed into the
/// edges of opaque shapes.
pub fn upscale(img: &RgbaImage, factor: u32) -> RgbaImage {
    let has_alpha = img.pixels().any(|p| p[3] < 255);
    let (w, h) = (img.width() * factor, img.height() * factor);
    if !has_alpha {
        return image::imageops::resize(img, w, h, image::imageops::FilterType::Triangle);
    }
    let mut pre = img.clone();
    for p in pre.pixels_mut() {
        let a = p[3] as u32;
        for k in 0..3 {
            p[k] = ((p[k] as u32 * a + 127) / 255) as u8;
        }
    }
    let mut big = image::imageops::resize(&pre, w, h, image::imageops::FilterType::Triangle);
    for p in big.pixels_mut() {
        let a = p[3] as u32;
        if a > 0 {
            for k in 0..3 {
                p[k] = ((p[k] as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    big
}
