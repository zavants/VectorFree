//! PostScript-family writers: EPS and Adobe Illustrator 3 (".ai").
//! PostScript has y pointing up, so every y is flipped.

use super::{num, ExportOptions, Item};
use crate::geom::{Path, Pt, Seg, VectorImage};
use std::fmt::Write;

fn flip(p: Pt, h: f64) -> String {
    format!("{} {}", num(p.x), num(h - p.y))
}

fn ps_path(out: &mut String, paths: &[&Path], h: f64, ai: bool, paint: &str) {
    let (m, l, c) = if ai { ("m", "L", "C") } else { ("moveto", "lineto", "curveto") };
    for p in paths {
        let _ = writeln!(out, "{} {m}", flip(p.start, h));
        for s in &p.segs {
            match *s {
                Seg::Line(e) => {
                    let _ = writeln!(out, "{} {l}", flip(e, h));
                }
                Seg::Cubic(a, b, e) => {
                    let _ = writeln!(out, "{} {} {} {c}", flip(a, h), flip(b, h), flip(e, h));
                }
            }
        }
        if ai {
            // In AI every sub-path of a compound path carries its own paint op.
            let _ = writeln!(out, "{paint}");
        } else {
            out.push_str("closepath\n");
        }
    }
}

fn rgb_f(c: [u8; 3]) -> String {
    format!("{} {} {}", num(c[0] as f64 / 255.0), num(c[1] as f64 / 255.0), num(c[2] as f64 / 255.0))
}

pub fn write_eps(img: &VectorImage, items: &[Item], opt: &ExportOptions) -> String {
    let (w, h) = (img.width, img.height as f64);
    let mut s = String::new();
    let _ = writeln!(s, "%!PS-Adobe-3.0 EPSF-3.0");
    let _ = writeln!(s, "%%Creator: Vector Free");
    let _ = writeln!(s, "%%BoundingBox: 0 0 {w} {}", img.height);
    let _ = writeln!(s, "%%HiResBoundingBox: 0 0 {w} {}", img.height);
    let _ = writeln!(s, "%%LanguageLevel: 2\n%%Pages: 1\n%%EndComments\n%%Page: 1 1");
    let _ = writeln!(s, "1 setlinejoin");
    for it in items {
        s.push_str("newpath\n");
        ps_path(&mut s, &it.paths, h, false, "");
        let _ = write!(s, "{} setrgbcolor ", rgb_f(it.rgb));
        if opt.stroke {
            let _ = writeln!(s, "gsave eofill grestore {} setlinewidth stroke", num(opt.stroke_width));
        } else {
            s.push_str("eofill\n");
        }
    }
    s.push_str("showpage\n%%EOF\n");
    s
}

fn cmyk(c: [u8; 3]) -> String {
    let (r, g, b) = (c[0] as f64 / 255.0, c[1] as f64 / 255.0, c[2] as f64 / 255.0);
    let k = 1.0 - r.max(g).max(b);
    if k >= 0.9999 {
        return "0 0 0 1".into();
    }
    let f = |x: f64| num((1.0 - x - k) / (1.0 - k));
    format!("{} {} {} {}", f(r), f(g), f(b), num(k))
}

/// Illustrator 3 file format: a constrained PostScript dialect that Adobe
/// Illustrator, CorelDRAW and most cutting-plotter software import.
pub fn write_ai(img: &VectorImage, items: &[Item], opt: &ExportOptions) -> String {
    let (w, h) = (img.width, img.height);
    let mut s = String::new();
    let _ = writeln!(s, "%!PS-Adobe-3.0");
    let _ = writeln!(s, "%%Creator: Vector Free");
    let _ = writeln!(s, "%%Title: (Untitled)");
    let _ = writeln!(s, "%%BoundingBox: 0 0 {w} {h}");
    let _ = writeln!(s, "%%HiResBoundingBox: 0 0 {w} {h}");
    let _ = writeln!(s, "%%DocumentProcessColors: Cyan Magenta Yellow Black");
    let _ = writeln!(s, "%AI5_FileFormat 3");
    let _ = writeln!(s, "%AI3_ColorUsage: Color");
    let _ = writeln!(s, "%AI5_ArtSize: {w} {h}");
    let _ = writeln!(s, "%AI5_RulerUnits: 0");
    let _ = writeln!(s, "%AI5_NumLayers: 1");
    let _ = writeln!(s, "%%EndComments\n%%EndProlog\n%%BeginSetup\n%%EndSetup");
    let _ = writeln!(s, "%AI5_BeginLayer\n1 1 1 1 0 0 0 79 128 255 Lb\n(Layer 1) Ln");
    let _ = writeln!(s, "0 A\n1 j\n0 O\n0 R");
    let paint = if opt.stroke { "b" } else { "f" };
    for it in items {
        let compound = it.paths.len() > 1;
        if compound {
            s.push_str("*u\n");
        }
        let col = cmyk(it.rgb);
        let _ = writeln!(s, "{col} k");
        if opt.stroke {
            let _ = writeln!(s, "{col} K\n{} w", num(opt.stroke_width));
        }
        ps_path(&mut s, &it.paths, h as f64, true, paint);
        if compound {
            s.push_str("*U\n");
        }
    }
    let _ = writeln!(s, "LB\n%AI5_EndLayer--");
    s.push_str("%%PageTrailer\ngsave annotatepage grestore showpage\n%%Trailer\n%%EOF\n");
    s
}
