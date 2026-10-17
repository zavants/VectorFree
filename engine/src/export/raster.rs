//! Raster export by rendering the vectors with tiny-skia.

use super::{ExportOptions, Format, Item};
use crate::geom::{Seg, VectorImage};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

pub fn render(img: &VectorImage, items: &[Item], opt: &ExportOptions) -> Result<Pixmap, String> {
    let scale = opt.raster_scale.clamp(0.05, 16.0) as f32;
    let (w, h) = (((img.width as f32) * scale).round().max(1.0) as u32, ((img.height as f32) * scale).round().max(1.0) as u32);
    let mut pm = Pixmap::new(w, h).ok_or("image too large to render")?;
    let tf = Transform::from_scale(scale, scale);
    for it in items {
        let mut pb = PathBuilder::new();
        for p in &it.paths {
            pb.move_to(p.start.x as f32, p.start.y as f32);
            for s in &p.segs {
                match *s {
                    Seg::Line(e) => pb.line_to(e.x as f32, e.y as f32),
                    Seg::Cubic(a, b, e) => pb.cubic_to(a.x as f32, a.y as f32, b.x as f32, b.y as f32, e.x as f32, e.y as f32),
                }
            }
            pb.close();
        }
        let Some(path) = pb.finish() else { continue };
        let mut paint = Paint::default();
        paint.set_color_rgba8(it.rgb[0], it.rgb[1], it.rgb[2], it.alpha);
        paint.anti_alias = true;
        pm.fill_path(&path, &paint, FillRule::EvenOdd, tf, None);
        if opt.stroke {
            let st = Stroke { width: opt.stroke_width as f32, ..Stroke::default() };
            pm.stroke_path(&path, &paint, &st, tf, None);
        }
    }
    Ok(pm)
}

pub fn write(img: &VectorImage, items: &[Item], opt: &ExportOptions, fmt: Format) -> Result<Vec<u8>, String> {
    let pm = render(img, items, opt)?;
    let (w, h) = (pm.width(), pm.height());
    let mut rgba = image::RgbaImage::new(w, h);
    for (dst, src) in rgba.pixels_mut().zip(pm.pixels()) {
        let c = src.demultiply();
        *dst = image::Rgba([c.red(), c.green(), c.blue(), c.alpha()]);
    }
    let mut buf = std::io::Cursor::new(Vec::new());
    let res = match fmt {
        Format::Png => image::DynamicImage::ImageRgba8(rgba).write_to(&mut buf, image::ImageFormat::Png),
        Format::Tif => image::DynamicImage::ImageRgba8(rgba).write_to(&mut buf, image::ImageFormat::Tiff),
        Format::Gif => {
            let mut enc = image::codecs::gif::GifEncoder::new_with_speed(&mut buf, 10);
            enc.encode_frame(image::Frame::new(rgba)).map_err(|e| e.to_string())?;
            drop(enc);
            Ok(())
        }
        _ => {
            // No alpha in JPEG/BMP: composite onto white.
            let mut rgb = image::RgbImage::new(w, h);
            for (d, s) in rgb.pixels_mut().zip(rgba.pixels()) {
                let a = s[3] as u32;
                let mix = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
                *d = image::Rgb([mix(s[0]), mix(s[1]), mix(s[2])]);
            }
            let f = if fmt == Format::Jpg { image::ImageFormat::Jpeg } else { image::ImageFormat::Bmp };
            image::DynamicImage::ImageRgb8(rgb).write_to(&mut buf, f)
        }
    };
    res.map_err(|e| e.to_string())?;
    Ok(buf.into_inner())
}
