//! Exporters. All formats share one drawing model: an ordered list of filled
//! items, each a compound path (outer loops + holes) in one color.

mod dxf;
mod pdf;
mod ps;
mod raster;
mod svg;

use crate::geom::{Path, Pt, VectorImage};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    /// Every region has holes where other regions sit; shapes never overlap.
    #[default]
    Cutout,
    /// Shapes are filled solid and stacked, larger ones underneath. Easier to
    /// edit in illustration programs and immune to hairline seams.
    Stacked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportOptions {
    pub layout: Layout,
    /// Stroke each shape with its own fill color (hides antialiasing seams).
    pub stroke: bool,
    pub stroke_width: f64,
    /// One compound path per color instead of one path per connected shape.
    pub group_by_color: bool,
    /// Scale factor for raster exports.
    pub raster_scale: f64,
    /// DXF: write curves as SPLINE entities (DXF 2000) instead of flattening
    /// them into polylines (DXF R12, readable by nearly everything).
    pub dxf_splines: bool,
    /// DXF lines-only: curve flattening tolerance in pixels (smaller = more
    /// lines, larger file).
    pub dxf_flatten: f64,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions {
            layout: Layout::Cutout,
            stroke: false,
            stroke_width: 0.5,
            group_by_color: true,
            raster_scale: 1.0,
            dxf_splines: false,
            dxf_flatten: 0.25,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Svg,
    Eps,
    Pdf,
    Ai,
    Dxf,
    Png,
    Gif,
    Jpg,
    Bmp,
    Tif,
}

impl Format {
    pub const ALL: [Format; 10] = [
        Format::Svg,
        Format::Eps,
        Format::Pdf,
        Format::Ai,
        Format::Dxf,
        Format::Png,
        Format::Gif,
        Format::Jpg,
        Format::Bmp,
        Format::Tif,
    ];

    pub fn ext(self) -> &'static str {
        match self {
            Format::Svg => "svg",
            Format::Eps => "eps",
            Format::Pdf => "pdf",
            Format::Ai => "ai",
            Format::Dxf => "dxf",
            Format::Png => "png",
            Format::Gif => "gif",
            Format::Jpg => "jpg",
            Format::Bmp => "bmp",
            Format::Tif => "tif",
        }
    }

    pub fn from_ext(s: &str) -> Option<Format> {
        let s = s.trim().trim_start_matches('.').to_ascii_lowercase();
        Some(match s.as_str() {
            "svg" => Format::Svg,
            "eps" | "ps" => Format::Eps,
            "pdf" => Format::Pdf,
            "ai" => Format::Ai,
            "dxf" => Format::Dxf,
            "png" => Format::Png,
            "gif" => Format::Gif,
            "jpg" | "jpeg" => Format::Jpg,
            "bmp" => Format::Bmp,
            "tif" | "tiff" => Format::Tif,
            _ => return None,
        })
    }
}

/// One filled compound path.
pub struct Item<'a> {
    pub rgb: [u8; 3],
    pub alpha: u8,
    pub paths: Vec<&'a Path>,
}

/// Point-in-polygon (even-odd) on a flattened path.
fn contains(poly: &[Pt], p: Pt) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn bbox_area(p: &Path) -> f64 {
    let (lo, hi) = p.bbox();
    (hi.x - lo.x) * (hi.y - lo.y)
}

/// Resolve the drawing order and composition for the chosen layout.
pub fn items<'a>(img: &'a VectorImage, opt: &ExportOptions) -> Vec<Item<'a>> {
    let mut out = Vec::new();
    match (opt.layout, opt.group_by_color) {
        (Layout::Cutout, true) => {
            for s in &img.shapes {
                out.push(Item { rgb: s.rgb, alpha: s.alpha, paths: s.outers.iter().chain(&s.holes).collect() });
            }
        }
        _ => {
            // One item per outer loop, with the holes it directly contains.
            let mut loose: Vec<(f64, Item<'a>)> = Vec::new();
            for s in &img.shapes {
                let flat: Vec<Vec<Pt>> = s.outers.iter().map(|o| o.flatten(0.5)).collect();
                let mut holes_of: Vec<Vec<&Path>> = vec![Vec::new(); s.outers.len()];
                for (hi, h) in s.holes.iter().enumerate() {
                    if opt.layout == Layout::Stacked && !s.hole_is_transparent[hi] {
                        continue; // covered by the shapes stacked on top
                    }
                    // Probe a point just inside the hole boundary's first segment.
                    let probe = h.flatten(0.5);
                    let p = probe[0].add(probe[1 % probe.len()]).scale(0.5);
                    let owner = (0..s.outers.len())
                        .filter(|&i| contains(&flat[i], p))
                        .min_by(|&a, &b| bbox_area(&s.outers[a]).partial_cmp(&bbox_area(&s.outers[b])).unwrap());
                    if let Some(o) = owner {
                        holes_of[o].push(h);
                    }
                }
                for (i, o) in s.outers.iter().enumerate() {
                    let mut paths = vec![o];
                    paths.extend(holes_of[i].iter().copied());
                    loose.push((bbox_area(o), Item { rgb: s.rgb, alpha: s.alpha, paths }));
                }
            }
            if opt.layout == Layout::Stacked {
                // Containers first: an enclosing shape always has a larger bbox.
                loose.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
            }
            out.extend(loose.into_iter().map(|(_, it)| it));
        }
    }
    out
}

pub fn export_bytes(img: &VectorImage, fmt: Format, opt: &ExportOptions) -> Result<Vec<u8>, String> {
    let items = items(img, opt);
    Ok(match fmt {
        Format::Svg => svg::write(img, &items, opt).into_bytes(),
        Format::Eps => ps::write_eps(img, &items, opt).into_bytes(),
        Format::Ai => ps::write_ai(img, &items, opt).into_bytes(),
        Format::Pdf => pdf::write(img, &items, opt),
        Format::Dxf if opt.dxf_splines => dxf::write_r2000(img, &items).into_bytes(),
        Format::Dxf => dxf::write(img, &items, opt.dxf_flatten.clamp(0.02, 10.0)).into_bytes(),
        Format::Png | Format::Gif | Format::Jpg | Format::Bmp | Format::Tif => raster::write(img, &items, opt, fmt)?,
    })
}

pub fn export_file(img: &VectorImage, path: &std::path::Path, fmt: Format, opt: &ExportOptions) -> Result<(), String> {
    let bytes = export_bytes(img, fmt, opt)?;
    std::fs::write(path, bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

pub fn to_svg(img: &VectorImage, opt: &ExportOptions) -> String {
    svg::write(img, &items(img, opt), opt)
}

/// Format a coordinate compactly: 2 decimals, no trailing zeros.
pub(crate) fn num(v: f64) -> String {
    let s = format!("{:.2}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}
