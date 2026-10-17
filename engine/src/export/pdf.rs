//! Minimal single-page PDF 1.4 writer (uncompressed content stream).

use super::{num, ExportOptions, Item};
use crate::geom::{Pt, Seg, VectorImage};
use std::fmt::Write;

fn flip(p: Pt, h: f64) -> String {
    format!("{} {}", num(p.x), num(h - p.y))
}

pub fn write(img: &VectorImage, items: &[Item], opt: &ExportOptions) -> Vec<u8> {
    let h = img.height as f64;
    let mut c = String::new();
    c.push_str("1 j\n");
    // One graphics state per distinct opacity.
    let mut alphas: Vec<u8> = items.iter().map(|it| it.alpha).filter(|&a| a < 255).collect();
    alphas.sort_unstable();
    alphas.dedup();
    let mut current_alpha = 255u8;
    for it in items {
        if it.alpha != current_alpha {
            match alphas.iter().position(|&a| a == it.alpha) {
                Some(i) => {
                    let _ = writeln!(c, "/GA{i} gs");
                }
                None => c.push_str("/GA_OPAQUE gs\n"),
            }
            current_alpha = it.alpha;
        }
        let rgb = format!(
            "{} {} {}",
            num(it.rgb[0] as f64 / 255.0),
            num(it.rgb[1] as f64 / 255.0),
            num(it.rgb[2] as f64 / 255.0)
        );
        let _ = writeln!(c, "{rgb} rg");
        if opt.stroke {
            let _ = writeln!(c, "{rgb} RG {} w", num(opt.stroke_width));
        }
        for p in &it.paths {
            let _ = writeln!(c, "{} m", flip(p.start, h));
            for s in &p.segs {
                match *s {
                    Seg::Line(e) => {
                        let _ = writeln!(c, "{} l", flip(e, h));
                    }
                    Seg::Cubic(a, b, e) => {
                        let _ = writeln!(c, "{} {} {} c", flip(a, h), flip(b, h), flip(e, h));
                    }
                }
            }
            c.push_str("h\n");
        }
        c.push_str(if opt.stroke { "B*\n" } else { "f*\n" });
    }

    let mut gs = String::new();
    if !alphas.is_empty() {
        gs.push_str("/ExtGState << /GA_OPAQUE << /ca 1 /CA 1 >> ");
        for (i, a) in alphas.iter().enumerate() {
            let v = num(*a as f64 / 255.0);
            let _ = write!(gs, "/GA{i} << /ca {v} /CA {v} >> ");
        }
        gs.push_str(">>");
    }
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << {gs} >> /Contents 4 0 R >>",
            img.width, img.height
        ),
        format!("<< /Length {} >>\nstream\n{}endstream", c.len(), c),
        "<< /Producer (Vector Free) >>".to_string(),
    ];
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", i + 1, o).as_bytes());
    }
    let xref = out.len();
    let mut x = format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for off in offsets {
        let _ = writeln!(x, "{off:010} 00000 n ");
    }
    let _ = write!(
        x,
        "trailer\n<< /Size {} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    );
    out.extend_from_slice(x.as_bytes());
    out
}
