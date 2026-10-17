//! AutoCAD DXF R12 (AC1009) writer. Curves are flattened into closed
//! POLYLINE entities, the most widely supported representation in CAD/CAM and
//! cutting software. Each color goes on its own layer named after its hex
//! value, with the nearest AutoCAD Color Index as the layer color.

use super::{num, Item};
use crate::color::{rgb_to_lab, Lab};
use crate::geom::VectorImage;
use std::fmt::Write;

/// Approximate AutoCAD Color Index palette (1..=255).
fn aci_table() -> Vec<(u8, [u8; 3])> {
    let mut t = vec![
        (1, [255, 0, 0]),
        (2, [255, 255, 0]),
        (3, [0, 255, 0]),
        (4, [0, 255, 255]),
        (5, [0, 0, 255]),
        (6, [255, 0, 255]),
        (7, [255, 255, 255]),
        (8, [128, 128, 128]),
        (9, [192, 192, 192]),
    ];
    let values = [1.0, 0.65, 0.5, 0.3, 0.15];
    for hue in 0..24 {
        for (vi, v) in values.iter().enumerate() {
            for sat in 0..2 {
                let idx = 10 + hue * 10 + vi * 2 + sat;
                let h = hue as f64 * 15.0;
                let s = if sat == 0 { 1.0 } else { 0.5 };
                t.push((idx as u8, hsv(h, s, *v)));
            }
        }
    }
    for (i, g) in [51u8, 91, 132, 173, 214, 255].iter().enumerate() {
        t.push((250 + i as u8, [*g, *g, *g]));
    }
    t
}

fn hsv(h: f64, s: f64, v: f64) -> [u8; 3] {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [((r + m) * 255.0) as u8, ((g + m) * 255.0) as u8, ((b + m) * 255.0) as u8]
}

fn nearest_aci(table: &[(u8, Lab)], rgb: [u8; 3]) -> u8 {
    let lab = rgb_to_lab(rgb);
    table.iter().min_by(|a, b| a.1.dist2(&lab).partial_cmp(&b.1.dist2(&lab)).unwrap()).map_or(7, |e| e.0)
}

fn pair(s: &mut String, code: i32, v: impl std::fmt::Display) {
    let _ = write!(s, "{code}\r\n{v}\r\n");
}

pub fn write(img: &VectorImage, items: &[Item]) -> String {
    let table: Vec<(u8, Lab)> = aci_table().into_iter().map(|(i, c)| (i, rgb_to_lab(c))).collect();
    let h = img.height as f64;
    let mut layers: Vec<([u8; 3], u8)> = Vec::new();
    for it in items {
        if !layers.iter().any(|l| l.0 == it.rgb) {
            layers.push((it.rgb, nearest_aci(&table, it.rgb)));
        }
    }
    let lname = |c: [u8; 3]| format!("C_{:02X}{:02X}{:02X}", c[0], c[1], c[2]);

    let mut s = String::new();
    pair(&mut s, 999, "Vector Free");
    pair(&mut s, 0, "SECTION");
    pair(&mut s, 2, "HEADER");
    pair(&mut s, 9, "$ACADVER");
    pair(&mut s, 1, "AC1009");
    pair(&mut s, 9, "$EXTMIN");
    pair(&mut s, 10, 0);
    pair(&mut s, 20, 0);
    pair(&mut s, 9, "$EXTMAX");
    pair(&mut s, 10, img.width);
    pair(&mut s, 20, img.height);
    pair(&mut s, 0, "ENDSEC");

    pair(&mut s, 0, "SECTION");
    pair(&mut s, 2, "TABLES");
    pair(&mut s, 0, "TABLE");
    pair(&mut s, 2, "LAYER");
    pair(&mut s, 70, layers.len());
    for (rgb, aci) in &layers {
        pair(&mut s, 0, "LAYER");
        pair(&mut s, 2, lname(*rgb));
        pair(&mut s, 70, 0);
        pair(&mut s, 62, aci);
        pair(&mut s, 6, "CONTINUOUS");
    }
    pair(&mut s, 0, "ENDTAB");
    pair(&mut s, 0, "ENDSEC");

    pair(&mut s, 0, "SECTION");
    pair(&mut s, 2, "ENTITIES");
    for it in items {
        let layer = lname(it.rgb);
        for p in &it.paths {
            let pts = p.flatten(0.25);
            if pts.len() < 2 {
                continue;
            }
            pair(&mut s, 0, "POLYLINE");
            pair(&mut s, 8, &layer);
            pair(&mut s, 66, 1);
            pair(&mut s, 70, 1); // closed
            for q in pts {
                pair(&mut s, 0, "VERTEX");
                pair(&mut s, 8, &layer);
                pair(&mut s, 10, num(q.x));
                pair(&mut s, 20, num(h - q.y));
            }
            pair(&mut s, 0, "SEQEND");
            pair(&mut s, 8, &layer);
        }
    }
    pair(&mut s, 0, "ENDSEC");
    pair(&mut s, 0, "EOF");
    s
}

/// Handle allocator for DXF 2000 objects.
struct Handles(u32);
impl Handles {
    fn next(&mut self) -> String {
        self.0 += 1;
        format!("{:X}", self.0)
    }
}

fn table_head(s: &mut String, h: &mut Handles, name: &str, count: usize) -> String {
    let th = h.next();
    pair(s, 0, "TABLE");
    pair(s, 2, name);
    pair(s, 5, &th);
    pair(s, 330, 0);
    pair(s, 100, "AcDbSymbolTable");
    pair(s, 70, count);
    th
}

fn record_head(s: &mut String, h: &mut Handles, kind: &str, owner: &str, subclass: &str) {
    pair(s, 0, kind);
    pair(s, if kind == "DIMSTYLE" { 105 } else { 5 }, h.next());
    pair(s, 330, owner);
    pair(s, 100, "AcDbSymbolTableRecord");
    pair(s, 100, subclass);
}

/// AutoCAD 2000 DXF with SPLINE entities. Each compound path becomes one
/// closed entity per sub-path: a LWPOLYLINE when it only has straight
/// segments, otherwise a clamped cubic B-spline whose control polygon is
/// exactly the Bezier chain (interior knots of multiplicity 3).
pub fn write_r2000(img: &VectorImage, items: &[Item]) -> String {
    let table: Vec<(u8, Lab)> = aci_table().into_iter().map(|(i, c)| (i, rgb_to_lab(c))).collect();
    let height = img.height as f64;
    let mut layers: Vec<([u8; 3], u8)> = Vec::new();
    for it in items {
        if !layers.iter().any(|l| l.0 == it.rgb) {
            layers.push((it.rgb, nearest_aci(&table, it.rgb)));
        }
    }
    let lname = |c: [u8; 3]| format!("C_{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
    let mut h = Handles(0x1F);
    // Handles referenced before they are written.
    let root_dict = h.next();
    let group_dict = h.next();
    let plotstyle_dict = h.next();
    let plotstyle_normal = h.next();

    let mut body = String::new();
    // ---- TABLES
    pair(&mut body, 0, "SECTION");
    pair(&mut body, 2, "TABLES");
    let th = table_head(&mut body, &mut h, "VPORT", 0);
    let _ = th;
    pair(&mut body, 0, "ENDTAB");

    let th = table_head(&mut body, &mut h, "LTYPE", 3);
    for (name, desc) in [("ByBlock", ""), ("ByLayer", ""), ("Continuous", "Solid line")] {
        record_head(&mut body, &mut h, "LTYPE", &th, "AcDbLinetypeTableRecord");
        pair(&mut body, 2, name);
        pair(&mut body, 70, 0);
        pair(&mut body, 3, desc);
        pair(&mut body, 72, 65);
        pair(&mut body, 73, 0);
        pair(&mut body, 40, "0.0");
    }
    pair(&mut body, 0, "ENDTAB");

    let th = table_head(&mut body, &mut h, "LAYER", layers.len() + 1);
    let mut all_layers = vec![("0".to_string(), 7u8)];
    all_layers.extend(layers.iter().map(|(rgb, aci)| (lname(*rgb), *aci)));
    for (name, aci) in &all_layers {
        record_head(&mut body, &mut h, "LAYER", &th, "AcDbLayerTableRecord");
        pair(&mut body, 2, name);
        pair(&mut body, 70, 0);
        pair(&mut body, 62, aci);
        pair(&mut body, 6, "Continuous");
        pair(&mut body, 370, -3);
        pair(&mut body, 390, &plotstyle_normal);
    }
    pair(&mut body, 0, "ENDTAB");

    let th = table_head(&mut body, &mut h, "STYLE", 1);
    record_head(&mut body, &mut h, "STYLE", &th, "AcDbTextStyleTableRecord");
    pair(&mut body, 2, "Standard");
    pair(&mut body, 70, 0);
    pair(&mut body, 40, "0.0");
    pair(&mut body, 41, "1.0");
    pair(&mut body, 50, "0.0");
    pair(&mut body, 71, 0);
    pair(&mut body, 42, "2.5");
    pair(&mut body, 3, "txt");
    pair(&mut body, 4, "");
    pair(&mut body, 0, "ENDTAB");

    for name in ["VIEW", "UCS"] {
        table_head(&mut body, &mut h, name, 0);
        pair(&mut body, 0, "ENDTAB");
    }

    let th = table_head(&mut body, &mut h, "APPID", 1);
    record_head(&mut body, &mut h, "APPID", &th, "AcDbRegAppTableRecord");
    pair(&mut body, 2, "ACAD");
    pair(&mut body, 70, 0);
    pair(&mut body, 0, "ENDTAB");

    let th = table_head(&mut body, &mut h, "DIMSTYLE", 1);
    pair(&mut body, 100, "AcDbDimStyleTable");
    record_head(&mut body, &mut h, "DIMSTYLE", &th, "AcDbDimStyleTableRecord");
    pair(&mut body, 2, "Standard");
    pair(&mut body, 70, 0);
    pair(&mut body, 0, "ENDTAB");

    let th = table_head(&mut body, &mut h, "BLOCK_RECORD", 2);
    let mut block_records = Vec::new();
    for name in ["*Model_Space", "*Paper_Space"] {
        pair(&mut body, 0, "BLOCK_RECORD");
        let bh = h.next();
        pair(&mut body, 5, &bh);
        pair(&mut body, 330, &th);
        pair(&mut body, 100, "AcDbSymbolTableRecord");
        pair(&mut body, 100, "AcDbBlockTableRecord");
        pair(&mut body, 2, name);
        block_records.push(bh);
    }
    pair(&mut body, 0, "ENDTAB");
    pair(&mut body, 0, "ENDSEC");
    let model = block_records[0].clone();

    // ---- BLOCKS
    pair(&mut body, 0, "SECTION");
    pair(&mut body, 2, "BLOCKS");
    for (i, name) in ["*Model_Space", "*Paper_Space"].iter().enumerate() {
        let owner = &block_records[i];
        pair(&mut body, 0, "BLOCK");
        pair(&mut body, 5, h.next());
        pair(&mut body, 330, owner);
        pair(&mut body, 100, "AcDbEntity");
        if i == 1 {
            pair(&mut body, 67, 1);
        }
        pair(&mut body, 8, "0");
        pair(&mut body, 100, "AcDbBlockBegin");
        pair(&mut body, 2, name);
        pair(&mut body, 70, 0);
        pair(&mut body, 10, "0.0");
        pair(&mut body, 20, "0.0");
        pair(&mut body, 30, "0.0");
        pair(&mut body, 3, name);
        pair(&mut body, 1, "");
        pair(&mut body, 0, "ENDBLK");
        pair(&mut body, 5, h.next());
        pair(&mut body, 330, owner);
        pair(&mut body, 100, "AcDbEntity");
        if i == 1 {
            pair(&mut body, 67, 1);
        }
        pair(&mut body, 8, "0");
        pair(&mut body, 100, "AcDbBlockEnd");
    }
    pair(&mut body, 0, "ENDSEC");

    // ---- ENTITIES
    pair(&mut body, 0, "SECTION");
    pair(&mut body, 2, "ENTITIES");
    let pt = |p: crate::geom::Pt| (num(p.x), num(height - p.y));
    for it in items {
        let layer = lname(it.rgb);
        for p in &it.paths {
            if p.segs.is_empty() {
                continue;
            }
            let straight = p.segs.iter().all(|s| matches!(s, crate::geom::Seg::Line(_)));
            pair(&mut body, 0, if straight { "LWPOLYLINE" } else { "SPLINE" });
            pair(&mut body, 5, h.next());
            pair(&mut body, 330, &model);
            pair(&mut body, 100, "AcDbEntity");
            pair(&mut body, 8, &layer);
            if straight {
                let mut verts = vec![p.start];
                verts.extend(p.segs.iter().map(|s| s.end()));
                if verts.len() > 2 && verts.last().unwrap().dist(verts[0]) < 1e-9 {
                    verts.pop();
                }
                pair(&mut body, 100, "AcDbPolyline");
                pair(&mut body, 90, verts.len());
                pair(&mut body, 70, 1);
                pair(&mut body, 43, "0.0");
                for v in verts {
                    let (x, y) = pt(v);
                    pair(&mut body, 10, x);
                    pair(&mut body, 20, y);
                }
                continue;
            }
            // Control polygon of the Bezier chain.
            let mut ctrl = vec![p.start];
            let mut prev = p.start;
            for s in &p.segs {
                match *s {
                    crate::geom::Seg::Line(e) => {
                        ctrl.push(prev.add(e.sub(prev).scale(1.0 / 3.0)));
                        ctrl.push(prev.add(e.sub(prev).scale(2.0 / 3.0)));
                        ctrl.push(e);
                    }
                    crate::geom::Seg::Cubic(a, b, e) => {
                        ctrl.push(a);
                        ctrl.push(b);
                        ctrl.push(e);
                    }
                }
                prev = s.end();
            }
            let n = p.segs.len();
            let mut knots = vec![0.0f64; 4];
            for k in 1..n {
                knots.extend([k as f64; 3]);
            }
            knots.extend([n as f64; 4]);
            pair(&mut body, 100, "AcDbSpline");
            pair(&mut body, 210, "0.0");
            pair(&mut body, 220, "0.0");
            pair(&mut body, 230, "1.0");
            pair(&mut body, 70, 8); // planar
            pair(&mut body, 71, 3);
            pair(&mut body, 72, knots.len());
            pair(&mut body, 73, ctrl.len());
            pair(&mut body, 74, 0);
            pair(&mut body, 42, "0.0000001");
            pair(&mut body, 43, "0.0000001");
            for k in knots {
                pair(&mut body, 40, k);
            }
            for c in ctrl {
                let (x, y) = pt(c);
                pair(&mut body, 10, x);
                pair(&mut body, 20, y);
                pair(&mut body, 30, "0.0");
            }
        }
    }
    pair(&mut body, 0, "ENDSEC");

    // ---- OBJECTS
    pair(&mut body, 0, "SECTION");
    pair(&mut body, 2, "OBJECTS");
    pair(&mut body, 0, "DICTIONARY");
    pair(&mut body, 5, &root_dict);
    pair(&mut body, 330, 0);
    pair(&mut body, 100, "AcDbDictionary");
    pair(&mut body, 281, 1);
    pair(&mut body, 3, "ACAD_GROUP");
    pair(&mut body, 350, &group_dict);
    pair(&mut body, 3, "ACAD_PLOTSTYLENAME");
    pair(&mut body, 350, &plotstyle_dict);
    pair(&mut body, 0, "DICTIONARY");
    pair(&mut body, 5, &group_dict);
    pair(&mut body, 330, &root_dict);
    pair(&mut body, 100, "AcDbDictionary");
    pair(&mut body, 281, 1);
    pair(&mut body, 0, "ACDBDICTIONARYWDFLT");
    pair(&mut body, 5, &plotstyle_dict);
    pair(&mut body, 330, &root_dict);
    pair(&mut body, 100, "AcDbDictionary");
    pair(&mut body, 281, 1);
    pair(&mut body, 3, "Normal");
    pair(&mut body, 350, &plotstyle_normal);
    pair(&mut body, 100, "AcDbDictionaryWithDefault");
    pair(&mut body, 340, &plotstyle_normal);
    pair(&mut body, 0, "ACDBPLACEHOLDER");
    pair(&mut body, 5, &plotstyle_normal);
    pair(&mut body, 330, &plotstyle_dict);
    pair(&mut body, 0, "ENDSEC");
    pair(&mut body, 0, "EOF");

    // ---- HEADER (needs the final handle seed)
    let mut s = String::new();
    pair(&mut s, 999, "Vector Free");
    pair(&mut s, 0, "SECTION");
    pair(&mut s, 2, "HEADER");
    pair(&mut s, 9, "$ACADVER");
    pair(&mut s, 1, "AC1015");
    pair(&mut s, 9, "$HANDSEED");
    pair(&mut s, 5, format!("{:X}", h.0 + 1));
    pair(&mut s, 9, "$INSUNITS");
    pair(&mut s, 70, 0);
    pair(&mut s, 9, "$EXTMIN");
    pair(&mut s, 10, "0.0");
    pair(&mut s, 20, "0.0");
    pair(&mut s, 30, "0.0");
    pair(&mut s, 9, "$EXTMAX");
    pair(&mut s, 10, img.width);
    pair(&mut s, 20, img.height);
    pair(&mut s, 30, "0.0");
    pair(&mut s, 0, "ENDSEC");
    pair(&mut s, 0, "SECTION");
    pair(&mut s, 2, "CLASSES");
    pair(&mut s, 0, "ENDSEC");
    s.push_str(&body);
    s
}