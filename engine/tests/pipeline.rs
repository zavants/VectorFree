use engine::export::{export_bytes, ExportOptions, Format, Layout};
use engine::{segment_image, vectorize, ImageKind, Settings};

/// Anti-aliased test picture: background, a disc, a ring, and a square.
fn picture() -> image::RgbaImage {
    let (w, h) = (120u32, 90u32);
    let bg = [240.0, 240.0, 235.0];
    let disc = [200.0, 30.0, 40.0];
    let ring = [20.0, 40.0, 160.0];
    let sq = [30.0, 30.0, 30.0];
    image::RgbaImage::from_fn(w, h, |x, y| {
        // 4x4 supersampling for coverage-based anti-aliasing.
        let mut acc = [0.0f64; 3];
        for sy in 0..4 {
            for sx in 0..4 {
                let (fx, fy) = (x as f64 + (sx as f64 + 0.5) / 4.0, y as f64 + (sy as f64 + 0.5) / 4.0);
                let d = ((fx - 40.0).powi(2) + (fy - 45.0).powi(2)).sqrt();
                let c = if (75.0..105.0).contains(&fx) && (20.0..50.0).contains(&fy) {
                    sq
                } else if d < 18.0 {
                    disc
                } else if (24.0..30.0).contains(&d) {
                    ring
                } else {
                    bg
                };
                for k in 0..3 {
                    acc[k] += c[k] / 16.0;
                }
            }
        }
        image::Rgba([acc[0].round() as u8, acc[1].round() as u8, acc[2].round() as u8, 255])
    })
}

fn agreement(layout: Layout) -> f64 {
    let img = picture();
    let settings = Settings { kind: ImageKind::ArtBlended, ..Settings::default() };
    let seg = segment_image(&img, &settings);
    assert_eq!(seg.palette.len(), 4, "should find the four flat colors");
    let vec = vectorize(&seg, &settings);
    let opts = ExportOptions { layout, ..ExportOptions::default() };
    let png = export_bytes(&vec, Format::Png, &opts).unwrap();
    let rendered = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(rendered.dimensions(), img.dimensions());
    // Compare "deep interior" source pixels only (whole 3x3 neighbourhood has
    // the same color). Edge pixels are blends whose rendering depends on
    // sub-pixel placement; a gap or missing shape still shows up here.
    let (w, h) = img.dimensions();
    let (mut same, mut total) = (0usize, 0usize);
    for (x, y, b) in img.enumerate_pixels() {
        if x == 0 || y == 0 || x + 1 >= w || y + 1 >= h {
            continue;
        }
        let uniform = (0..9).all(|k| img.get_pixel(x + k % 3 - 1, y + k / 3 - 1) == b);
        if !uniform {
            continue;
        }
        let a = rendered.get_pixel(x, y);
        total += 1;
        let d: i32 = (0..3).map(|k| (a[k] as i32 - b[k] as i32).abs()).sum();
        if d < 30 && a[3] == 255 {
            same += 1;
        }
    }
    same as f64 / total as f64
}

#[test]
fn cutout_reproduces_image() {
    let a = agreement(Layout::Cutout);
    assert!(a > 0.999, "agreement {a}");
}

#[test]
fn stacked_reproduces_image() {
    let a = agreement(Layout::Stacked);
    assert!(a > 0.999, "agreement {a}");
}

#[test]
fn all_formats_have_valid_headers() {
    let img = picture();
    let s = Settings::default();
    let vec = vectorize(&segment_image(&img, &s), &s);
    let opts = ExportOptions::default();
    let head = |f: Format| export_bytes(&vec, f, &opts).unwrap();
    assert!(head(Format::Svg).starts_with(b"<?xml"));
    assert!(head(Format::Eps).starts_with(b"%!PS-Adobe-3.0 EPSF-3.0"));
    assert!(head(Format::Ai).windows(17).any(|w| w == b"%AI5_FileFormat 3"));
    let pdf = head(Format::Pdf);
    assert!(pdf.starts_with(b"%PDF-1.4") && pdf.ends_with(b"%%EOF\n"));
    assert!(head(Format::Dxf).ends_with(b"0\r\nEOF\r\n"));
    for f in [Format::Png, Format::Jpg, Format::Bmp, Format::Tif] {
        image::load_from_memory(&head(f)).unwrap();
    }
}

#[test]
fn background_removal_makes_border_transparent() {
    let img = picture();
    let s = Settings { remove_background: true, ..Settings::default() };
    let seg = segment_image(&img, &s);
    let vec = vectorize(&seg, &s);
    assert_eq!(vec.shapes.len(), 3, "background shape is dropped");
    let png = export_bytes(&vec, Format::Png, &ExportOptions::default()).unwrap();
    let r = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(r.get_pixel(2, 2)[3], 0);
}

/// Transparent hole inside a ring that sits on a disc: in the stacked layout
/// the disc must keep its hole, otherwise it shows through the ring's center.
#[test]
fn stacked_keeps_nested_transparency() {
    let img = image::RgbaImage::from_fn(80, 80, |x, y| {
        let d = ((x as f64 + 0.5 - 40.0).powi(2) + (y as f64 + 0.5 - 40.0).powi(2)).sqrt();
        if d < 12.0 || d >= 30.0 {
            image::Rgba([0, 0, 0, 0])
        } else if d < 18.0 {
            image::Rgba([0, 0, 0, 255])
        } else {
            image::Rgba([40, 60, 220, 255])
        }
    });
    let s = Settings { kind: ImageKind::ArtFlat, ..Settings::default() };
    let vec = vectorize(&segment_image(&img, &s), &s);
    for layout in [Layout::Cutout, Layout::Stacked] {
        let opts = ExportOptions { layout, ..ExportOptions::default() };
        let r = image::load_from_memory(&export_bytes(&vec, Format::Png, &opts).unwrap()).unwrap().to_rgba8();
        assert_eq!(r.get_pixel(40, 40)[3], 0, "{layout:?}: center must stay transparent");
        assert_eq!(r.get_pixel(40, 16)[3], 255, "{layout:?}: disc must be drawn");
    }
}

#[test]
fn pencil_edit_changes_output() {
    let img = picture();
    let s = Settings::default();
    let mut seg = segment_image(&img, &s);
    let before = vectorize(&seg, &s).shapes.iter().map(|sh| sh.outers.len()).sum::<usize>();
    let bg = seg.border_label();
    // Paint a background-colored blob in the middle of the square (seg coords).
    let k = seg.scale as f64;
    seg.paint_line(90.0 * k, 35.0 * k, 90.0 * k, 35.0 * k, 4.0 * k, bg);
    let after = vectorize(&seg, &s);
    let holes: usize = after.shapes.iter().map(|sh| sh.holes.len()).sum();
    assert!(holes > 0 && before > 0);
}

/// Regression: a small but clearly distinct color (3% of the image) used to
/// vanish because automatic mode stopped once 95% of pixels were covered.
#[test]
fn small_distinct_color_survives() {
    let img = image::RgbaImage::from_fn(200, 100, |x, y| {
        let d = ((x as f64 - 170.0).powi(2) + (y as f64 - 50.0).powi(2)).sqrt();
        if d < 13.0 {
            image::Rgba([0, 128, 128, 255])
        } else if (20..140).contains(&x) && (30..70).contains(&y) {
            image::Rgba([139, 0, 0, 255])
        } else {
            image::Rgba([255, 255, 255, 255])
        }
    });
    let s = Settings::default();
    let seg = segment_image(&img, &s);
    assert!(seg.palette.iter().any(|e| e.rgb == [0, 128, 128]), "teal lost: {:?}", seg.palette);
}

/// Regression: on small shapes one corner of a short side suppressed the
/// other, turning a 12x4 rectangle into an ellipse.
#[test]
fn small_rectangle_keeps_its_corners() {
    let img = image::RgbaImage::from_fn(32, 32, |x, y| {
        if (10..22).contains(&x) && (14..18).contains(&y) {
            image::Rgba([255, 255, 255, 255])
        } else {
            image::Rgba([230, 60, 20, 255])
        }
    });
    let s = Settings::default();
    let vec = vectorize(&segment_image(&img, &s), &s);
    let white = vec.shapes.iter().find(|sh| sh.rgb == [255, 255, 255]).expect("white shape");
    let outline = &white.outers[0];
    let lines = outline.segs.iter().filter(|s| matches!(s, engine::Seg::Line(_))).count();
    assert!(lines >= 4, "expected 4 straight sides, got {:?}", outline.segs);
    let (lo, hi) = outline.bbox();
    assert!((lo.x - 10.0).abs() < 0.6 && (hi.x - 22.0).abs() < 0.6, "bbox {lo:?} {hi:?}");
}

#[test]
fn photos_get_a_rich_palette() {
    // Smooth two-axis gradient: posterising it to a handful of colors was the bug.
    let img = image::RgbaImage::from_fn(300, 200, |x, y| {
        image::Rgba([(x * 255 / 299) as u8, (y * 255 / 199) as u8, 128, 255])
    });
    let s = Settings { kind: ImageKind::Photo, ..Settings::default() };
    let seg = segment_image(&img, &s);
    assert!(seg.palette.len() >= 16, "only {} colors", seg.palette.len());
}

#[test]
fn zap_and_fill_edit_regions() {
    let img = picture();
    let s = Settings { kind: ImageKind::ArtFlat, ..Settings::default() };
    let mut seg = segment_image(&img, &s);
    let k = seg.scale;
    let square = seg.at(90 * k, 35 * k);
    // Fill the square with the background color: the square disappears.
    let bg = seg.border_label();
    seg.fill(90 * k, 35 * k, bg);
    assert_eq!(seg.at(90 * k, 35 * k), bg);
    assert!(seg.labels.iter().all(|&l| l != square), "square color still present");
    // Zap the disc: it is absorbed by its surroundings.
    let disc = seg.at(40 * k, 45 * k);
    seg.zap_region(40 * k, 45 * k);
    assert_ne!(seg.at(40 * k, 45 * k), disc);
}

#[test]
fn problem_finder_reports_pinches() {
    // Two black pixels touching only diagonally inside white.
    let img = image::RgbaImage::from_fn(20, 20, |x, y| {
        if (x, y) == (9, 9) || (x, y) == (10, 10) || (x < 9 && y < 9) {
            image::Rgba([0, 0, 0, 255])
        } else {
            image::Rgba([255, 255, 255, 255])
        }
    });
    let seg = engine::Segmentation::from_palette(&img, &[[0, 0, 0], [255, 255, 255]], false);
    let p = seg.problems(10);
    assert!(p.iter().any(|q| (q.x - 10.0).abs() < 1.5 && (q.y - 10.0).abs() < 1.5), "{p:?}");
}

#[test]
fn lines_mode_emits_only_lines() {
    let img = picture();
    let mut s = Settings::default();
    s.advanced.curves = engine::CurveMode::Lines;
    let vec = vectorize(&segment_image(&img, &s), &s);
    for sh in &vec.shapes {
        for p in sh.outers.iter().chain(&sh.holes) {
            assert!(p.segs.iter().all(|s| matches!(s, engine::Seg::Line(_))));
        }
    }
}

#[test]
fn opacity_gif_and_dxf_splines() {
    let img = picture();
    let s = Settings::default();
    let mut seg = segment_image(&img, &s);
    seg.set_alpha(0, 128);
    let vec = vectorize(&seg, &s);
    let svg = String::from_utf8(export_bytes(&vec, Format::Svg, &ExportOptions::default()).unwrap()).unwrap();
    assert!(svg.contains(r#"opacity="0.5""#));
    let pdf = String::from_utf8_lossy(&export_bytes(&vec, Format::Pdf, &ExportOptions::default()).unwrap()).to_string();
    assert!(pdf.contains("/ExtGState") && pdf.contains("/GA0 gs"));
    image::load_from_memory(&export_bytes(&vec, Format::Gif, &ExportOptions::default()).unwrap()).unwrap();
    let opts = ExportOptions { dxf_splines: true, ..ExportOptions::default() };
    let dxf = String::from_utf8(export_bytes(&vec, Format::Dxf, &opts).unwrap()).unwrap();
    assert!(dxf.contains("AC1015") && dxf.contains("\r\nSPLINE\r\n") && dxf.ends_with("0\r\nEOF\r\n"));
    // Every handle is unique.
    let lines: Vec<&str> = dxf.split("\r\n").collect();
    let mut handles = std::collections::HashSet::new();
    for w in lines.chunks(2) {
        if w.len() == 2 && (w[0] == "5" || w[0] == "105") {
            assert!(handles.insert(w[1]), "duplicate handle {}", w[1]);
        }
    }
}
#[test]
fn recolor_without_refit_applies_color_and_opacity() {
    let img = picture();
    let s = Settings::default();
    let mut seg = segment_image(&img, &s);
    let mut vec = vectorize(&seg, &s);
    seg.set_alpha(1, 128);
    seg.recolor(1, [1, 2, 3]);
    vec.apply_palette(&seg.palette);
    let svg = String::from_utf8(export_bytes(&vec, Format::Svg, &ExportOptions::default()).unwrap()).unwrap();
    assert!(svg.contains(r##"fill="#010203""##), "{svg}");
    assert!(svg.contains(r#"opacity="0.5""#), "{svg}");
}
#[test]
fn image_type_is_detected() {
    use engine::palette::suggest_kind;
    let flat = image::RgbaImage::from_fn(80, 60, |x, _| if x < 40 { image::Rgba([0, 0, 0, 255]) } else { image::Rgba([255, 255, 255, 255]) });
    assert_eq!(suggest_kind(&flat).0, ImageKind::ArtFlat);
    assert_eq!(suggest_kind(&picture()).0, ImageKind::ArtBlended);
    // A smooth multi-hue gradient behaves like a photo.
    let photo = image::RgbaImage::from_fn(300, 200, |x, y| {
        let t = (x as f32 / 299.0, y as f32 / 199.0);
        image::Rgba([(255.0 * t.0) as u8, (255.0 * t.1) as u8, (255.0 * (1.0 - t.0 * t.1)) as u8, 255])
    });
    assert_eq!(suggest_kind(&photo).0, ImageKind::Photo);
    // Anti-aliasing carried only by alpha (one RGB color, soft edges).
    let soft = image::RgbaImage::from_fn(32, 32, |x, y| {
        let d = ((x as f32 - 15.5).powi(2) + (y as f32 - 15.5).powi(2)).sqrt();
        image::Rgba([255, 69, 0, ((13.0 - d).clamp(0.0, 1.0) * 255.0) as u8])
    });
    assert_eq!(suggest_kind(&soft).0, ImageKind::ArtBlended);
}
#[test]
fn background_eraser_hides_and_restores_one_shape() {
    let img = picture();
    let s = Settings::default();
    let mut seg = segment_image(&img, &s);
    let k = seg.scale;
    let before = vectorize(&seg, &s).shapes.len();
    // Hide the square: its color disappears from the output, others stay.
    assert!(seg.toggle_hidden(90 * k, 35 * k, false));
    let hidden = vectorize(&seg, &s);
    assert_eq!(hidden.shapes.len(), before - 1);
    let outlines = engine::hidden_outlines(&seg, &s).expect("outlines of the hidden shape");
    assert_eq!(outlines.shapes.len(), 1);
    // Clicking again restores it.
    assert!(!seg.toggle_hidden(90 * k, 35 * k, false));
    assert_eq!(vectorize(&seg, &s).shapes.len(), before);
    assert!(engine::hidden_outlines(&seg, &s).is_none());
}

#[test]
fn corner_detection_can_be_switched_off() {
    let img = image::RgbaImage::from_fn(60, 60, |x, y| {
        if (15..45).contains(&x) && (15..45).contains(&y) { image::Rgba([0, 0, 0, 255]) } else { image::Rgba([255, 255, 255, 255]) }
    });
    let mut s = Settings { kind: ImageKind::ArtFlat, ..Settings::default() };
    let with = vectorize(&segment_image(&img, &s), &s);
    let curves = |v: &engine::VectorImage| v.shapes.iter().flat_map(|sh| sh.outers.iter()).flat_map(|p| p.segs.iter()).filter(|g| matches!(g, engine::Seg::Cubic(..))).count();
    assert_eq!(curves(&with), 0, "with corners the square is four straight sides");
    s.advanced.detect_corners = Some(false);
    let without = vectorize(&segment_image(&img, &s), &s);
    assert!(curves(&without) > 0, "without corner detection the corners become curves");
}

#[test]
fn dxf_line_density_follows_option() {
    let img = picture();
    let s = Settings::default();
    let vec = vectorize(&segment_image(&img, &s), &s);
    let count = |flatten: f64| {
        let o = ExportOptions { dxf_flatten: flatten, ..ExportOptions::default() };
        String::from_utf8(export_bytes(&vec, Format::Dxf, &o).unwrap()).unwrap().matches("VERTEX").count()
    };
    assert!(count(1.0) < count(0.1), "fewer lines must give fewer vertices");
}
/// Anti-aliased "V" (dark on light), `s` scales the 420x320 design.
fn aa_v(s: f64) -> (image::RgbaImage, Vec<(f64, f64)>) {
    let poly: Vec<(f64, f64)> =
        [(40.0, 40.0), (140.0, 40.0), (210.0, 230.0), (280.0, 40.0), (380.0, 40.0), (250.0, 290.0), (170.0, 290.0)].iter().map(|&(x, y)| (x * s, y * s)).collect();
    let inside = |x: f64, y: f64| {
        let mut c = false;
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            if (a.1 > y) != (b.1 > y) && x < a.0 + (y - a.1) / (b.1 - a.1) * (b.0 - a.0) {
                c = !c;
            }
        }
        c
    };
    let (fg, bg) = ([30.0, 40.0, 110.0], [245.0, 242.0, 230.0]);
    let img = image::RgbaImage::from_fn((420.0 * s) as u32, (320.0 * s) as u32, |x, y| {
        let n = 8;
        let mut cov = 0.0;
        for sy in 0..n {
            for sx in 0..n {
                if inside(x as f64 + (sx as f64 + 0.5) / n as f64, y as f64 + (sy as f64 + 0.5) / n as f64) {
                    cov += 1.0 / (n * n) as f64;
                }
            }
        }
        let c: Vec<u8> = (0..3).map(|k| (fg[k] * cov + bg[k] * (1.0 - cov)).round() as u8).collect();
        image::Rgba([c[0], c[1], c[2], 255])
    });
    (img, poly)
}

/// Regression: long straight diagonal edges of anti-aliased art came out as
/// many slightly wavy cubics instead of one straight segment.
#[test]
fn aa_diagonal_edges_stay_straight() {
    for (s, q) in [(1.0, engine::Quality::High), (1.0, engine::Quality::Medium), (0.7, engine::Quality::High)] {
        let (img, poly) = aa_v(s);
        let settings = Settings { kind: ImageKind::ArtBlended, quality: q, ..Settings::default() };
        let vec = vectorize(&segment_image(&img, &settings), &settings);
        let v = vec.shapes.iter().find(|sh| sh.rgb[2] as i32 > sh.rgb[0] as i32 + 40).expect("V shape");
        let outline = &v.outers[0];
        // Every segment as (start, seg).
        let mut segs = Vec::new();
        let mut prev = outline.start;
        for g in &outline.segs {
            segs.push((prev, *g));
            prev = g.end();
        }
        let at = |p: engine::Pt, g: &engine::Seg, t: f64| match *g {
            engine::Seg::Line(e) => p.add(e.sub(p).scale(t)),
            engine::Seg::Cubic(a, b, e) => engine::geom::bezier(p, a, b, e, t),
        };
        for i in 0..poly.len() {
            let (a, b) = (engine::Pt::new(poly[i].0, poly[i].1), engine::Pt::new(poly[(i + 1) % poly.len()].0, poly[(i + 1) % poly.len()].1));
            if (a.x - b.x).abs() < 1e-9 || (a.y - b.y).abs() < 1e-9 {
                continue; // axis-aligned sides are easy
            }
            let ab = b.sub(a);
            let len = ab.len();
            let dist = |p: engine::Pt| (ab.x * p.sub(a).y - ab.y * p.sub(a).x).abs() / len;
            let along = |p: engine::Pt| p.sub(a).dot(ab) / (len * len);
            // Segments whose middle lies on this side.
            let on: Vec<_> = segs.iter().filter(|(p, g)| {
                let m = at(*p, g, 0.5);
                dist(m) < 1.0 && (0.0..=1.0).contains(&along(m))
            }).collect();
            let mut worst: f64 = 0.0;
            for (p, g) in &on {
                for k in 0..=20 {
                    let q = at(*p, g, k as f64 / 20.0);
                    // Corners themselves are checked elsewhere.
                    if q.dist(a) > 2.0 && q.dist(b) > 2.0 {
                        worst = worst.max(dist(q));
                    }
                }
            }
            assert!(on.len() <= 2 && worst < 0.25, "scale {s} {q:?} side {i}: {} segments, deviation {worst:.3}: {on:?}", on.len());
        }
    }
}
