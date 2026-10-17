//! Vector Free engine: turns a bitmap into a stack of flat-colored vector shapes.
//!
//! Pipeline (inspired by the segmentation-then-fit approach described in
//! J. Diebel, "Bayesian Image Vectorization", Stanford PhD thesis, 2008):
//!
//! 1. `palette`  - choose a small set of colors (k-means in CIE Lab, or fixed).
//! 2. `segment`  - assign every pixel to a palette entry, clean up anti-aliasing
//!                 noise and merge regions that are too small.
//! 3. `trace`    - extract the pixel-crack boundary graph between regions and
//!                 split it into chains shared by exactly two regions.
//! 4. `fit`      - smooth each chain once and fit cubic Beziers, so neighbouring
//!                 shapes share identical edges (no gaps, no overlaps).
//! 5. `export`   - SVG, EPS, PDF, AI, DXF and raster formats.

pub mod color;
pub mod export;
pub mod filter;
pub mod fit;
pub mod geom;
pub mod palette;
pub mod segment;
pub mod snap;
pub mod trace;

use serde::{Deserialize, Serialize};

pub use geom::{Path, Pt, Seg, Shape, VectorImage};
pub use segment::Segmentation;

/// What kind of picture is being traced. Mirrors the questions a user can
/// answer about their input; each one picks different defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ImageKind {
    /// Photos and detailed artwork: many colors, gradients.
    Photo,
    /// Logos/artwork with anti-aliased (blended) edges.
    #[default]
    ArtBlended,
    /// Pixel-exact artwork, every pixel is one of a few flat colors.
    ArtFlat,
}

/// For artwork: how clean the source is. For photos: the level of detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    Low,
    #[default]
    Medium,
    High,
}

/// Which primitives the fitted outlines may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CurveMode {
    /// Cubic Beziers and straight lines.
    #[default]
    Curves,
    /// Straight line segments only (polygons), e.g. for CNC or plotters.
    Lines,
}

/// Fine-grained overrides ("advanced mode"). `None` keeps the value derived
/// from image type and quality.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Advanced {
    /// Maximum fitting error in source pixels.
    pub tolerance: Option<f64>,
    /// Multiplier on the automatic fitting error (fewer nodes > 1 > more nodes).
    pub tolerance_scale: Option<f64>,
    /// Smoothing strength multiplier (0 = none, 1 = default, up to 4).
    pub smoothing: Option<f64>,
    /// Turns sharper than this angle (degrees) become corners. Higher finds
    /// more corners.
    pub corner_angle: Option<f64>,
    /// Regions smaller than this many source pixels are merged away.
    pub min_area: Option<f64>,
    /// Edge-preserving denoise before segmentation (overrides the default).
    pub denoise: Option<bool>,
    pub curves: CurveMode,
    /// How hard to suppress the thin slivers anti-aliasing leaves between
    /// shapes.
    pub aa_rejection: Option<AaRejection>,
    /// Find sharp corners (off: every outline is a smooth curve).
    pub detect_corners: Option<bool>,
    /// Use anti-aliasing to place edges with sub-pixel precision.
    pub aa_engine: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AaRejection {
    None,
    Moderate,
    Aggressive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub kind: ImageKind,
    pub quality: Quality,
    /// Number of colors; `None` = detect automatically.
    pub num_colors: Option<usize>,
    /// Fixed palette (overrides automatic detection when non-empty).
    pub palette: Vec<[u8; 3]>,
    /// Make the color covering most of the image border transparent.
    pub remove_background: bool,
    pub advanced: Advanced,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            kind: ImageKind::ArtBlended,
            quality: Quality::Medium,
            num_colors: None,
            palette: Vec::new(),
            remove_background: false,
            advanced: Advanced::default(),
        }
    }
}

/// Numeric parameters derived from `Settings`, in segmentation pixels.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    pub min_region_area: usize,
    pub mode_filter_passes: usize,
    pub smooth_passes: usize,
    pub fit_tolerance: f64,
    pub corner_angle_deg: f64,
    /// Arm length (in segmentation pixels) used to measure corner angles.
    pub corner_span: usize,
    pub curves: CurveMode,
    /// Merge small regions into the most similar color (photos) rather than
    /// the neighbour sharing the longest border (artwork).
    pub merge_by_color: bool,
    /// Recover sub-pixel edge positions from anti-aliasing.
    pub snap_edges: bool,
    /// Assign anti-aliasing blend colors to the two colors they mix.
    pub blend_aware: bool,
}

impl Settings {
    /// `width`/`height` are the original image size; `upscale` is the
    /// supersampling factor the segmentation works at.
    pub fn params(&self, width: u32, height: u32, upscale: u32) -> Params {
        let adv = &self.advanced;
        // Scale the minimum region size with the image so a 4000px photo is
        // not shattered into thousands of specks.
        let scale = ((width as f64 * height as f64) / (500.0 * 500.0)).max(1.0).sqrt();
        let up = upscale.max(1) as f64;
        let (area, tol, smooth) = match (self.kind, self.quality) {
            (ImageKind::Photo, Quality::High) => (6.0, 0.6, 2.0),
            (ImageKind::Photo, Quality::Medium) => (20.0, 0.9, 3.0),
            (ImageKind::Photo, Quality::Low) => (60.0, 1.4, 4.0),
            (_, Quality::High) => (3.0, 0.3, 2.0),
            (_, Quality::Medium) => (8.0, 0.45, 3.0),
            (_, Quality::Low) => (25.0, 0.8, 4.0),
        };
        // Pixel-exact art has no anti-aliasing to locate edges with: the
        // intended edge runs through the middle of the pixel staircase, so it
        // needs looser fitting and more smoothing than blended art.
        let (area, tol, smooth) = if self.kind == ImageKind::ArtFlat { (area * 0.5, tol * 1.8, smooth + 1.0) } else { (area, tol, smooth) };
        // Small images need proportionally tighter curves: a 0.5px error on a
        // 32px icon is clearly visible.
        let size = width.max(height) as f64;
        let small = (size / 256.0).clamp(0.35, 1.0);
        let tol = adv.tolerance.unwrap_or(tol * small * adv.tolerance_scale.unwrap_or(1.0).clamp(0.1, 10.0)).max(0.05);
        let smooth = smooth * adv.smoothing.unwrap_or(1.0).clamp(0.0, 4.0);
        let min_area = adv.min_area.unwrap_or(area * scale);
        Params {
            min_region_area: (min_area * up * up).round().max(1.0) as usize,
            mode_filter_passes: match (adv.aa_rejection, self.kind) {
                (Some(AaRejection::None), _) => 0,
                (Some(AaRejection::Moderate), _) => 1,
                (Some(AaRejection::Aggressive), _) => 2,
                (None, ImageKind::ArtFlat) => 0,
                (None, ImageKind::ArtBlended) => 1,
                (None, ImageKind::Photo) => 2,
            },
            smooth_passes: (smooth * up * up).round() as usize,
            fit_tolerance: tol * up,
            // Corner detection off = no angle qualifies as a corner.
            corner_angle_deg: if adv.detect_corners == Some(false) { 0.0 } else { adv.corner_angle.unwrap_or(120.0).clamp(60.0, 175.0) },
            corner_span: (4.0 * up).round() as usize,
            curves: adv.curves,
            merge_by_color: self.kind == ImageKind::Photo,
            snap_edges: self.aa_engine(),
            blend_aware: self.kind != ImageKind::Photo && adv.aa_rejection != Some(AaRejection::None),
        }
    }

    /// Supersampling factor. Anti-aliased edge pixels carry sub-pixel edge
    /// position information (the key observation of Diebel's thesis);
    /// bilinear upsampling before segmentation turns that into sub-pixel
    /// boundaries. Pixel-exact art and big images are traced as is.
    pub fn upscale_for(&self, width: u32, height: u32) -> u32 {
        let m = width.max(height);
        if !self.aa_engine() {
            return 1;
        }
        match self.kind {
            ImageKind::ArtFlat => 1,
            ImageKind::Photo if m <= 800 => 2,
            ImageKind::Photo => 1,
            _ if m <= 400 => 3,
            _ if m <= 1600 => 2,
            _ => 1,
        }
    }

    /// Anti-aliasing engine: on for blended artwork and photos by default.
    pub fn aa_engine(&self) -> bool {
        self.advanced.aa_engine.unwrap_or(self.kind != ImageKind::ArtFlat)
    }

    fn denoise(&self) -> bool {
        self.advanced.denoise.unwrap_or(match self.kind {
            ImageKind::Photo => true,
            ImageKind::ArtBlended => self.quality == Quality::Low,
            ImageKind::ArtFlat => false,
        })
    }
}

/// Load any supported raster file into RGBA8.
pub fn load_image(path: &std::path::Path) -> Result<image::RgbaImage, String> {
    image::open(path)
        .map(|i| i.to_rgba8())
        .map_err(|e| format!("cannot read {}: {e}", path.display()))
}

/// Decode an in-memory image (e.g. pasted from the clipboard).
pub fn load_image_bytes(bytes: &[u8]) -> Result<image::RgbaImage, String> {
    image::load_from_memory(bytes).map(|i| i.to_rgba8()).map_err(|e| format!("cannot decode image: {e}"))
}

/// Image after optional denoising, as used for palette and segmentation.
fn prepared(img: &image::RgbaImage, settings: &Settings) -> image::RgbaImage {
    if settings.denoise() {
        let radius = if settings.quality == Quality::Low { 2 } else { 1 };
        filter::median(img, radius)
    } else {
        img.clone()
    }
}

/// Steps 1+2: palette + segmentation (the part a user edits interactively).
pub fn segment_image(img: &image::RgbaImage, settings: &Settings) -> Segmentation {
    let src = prepared(img, settings);
    let pal = if !settings.palette.is_empty() {
        settings.palette.clone()
    } else {
        palette::choose_palette(&src, settings.kind, settings.quality, settings.num_colors)
    };
    segment_prepared(&src, settings, &pal)
}

/// Segment with an explicit palette (used after the user edits colors).
pub fn segment_with_palette(img: &image::RgbaImage, settings: &Settings, pal: &[[u8; 3]]) -> Segmentation {
    segment_prepared(&prepared(img, settings), settings, pal)
}

fn segment_prepared(img: &image::RgbaImage, settings: &Settings, pal: &[[u8; 3]]) -> Segmentation {
    let up = settings.upscale_for(img.width(), img.height());
    let params = settings.params(img.width(), img.height(), up);
    let work;
    let src = if up > 1 {
        work = filter::upscale(img, up);
        &work
    } else {
        img
    };
    let mut seg = Segmentation::from_palette(src, pal, params.blend_aware);
    seg.scale = up;
    seg.source = Some(std::sync::Arc::new(img.clone()));
    seg.cleanup(&params);
    if settings.remove_background {
        seg.remove_background();
    }
    seg
}

/// Steps 3+4: boundary extraction and curve fitting.
pub fn vectorize(seg: &Segmentation, settings: &Settings) -> VectorImage {
    let up = seg.scale.max(1);
    let (w, h) = (seg.width / up, seg.height / up);
    let params = settings.params(w, h, up);
    let graph = trace::BoundaryGraph::build(seg);
    let snapper = match (&seg.source, params.snap_edges) {
        (Some(src), true) => Some(snap::Snapper::new(seg, src)),
        _ => None,
    };
    let curves = fit::fit_chains(&graph, &params, snapper.as_ref());
    let mut img = trace::assemble(seg, &graph, &curves);
    if up > 1 {
        img.scale(1.0 / up as f64);
        img.width = w;
        img.height = h;
    }
    img
}

/// Outlines of the shapes hidden with the background eraser, for display.
pub fn hidden_outlines(seg: &Segmentation, settings: &Settings) -> Option<VectorImage> {
    if !seg.hidden.iter().any(|&h| h) {
        return None;
    }
    let mut inv = seg.clone();
    inv.hidden = seg.hidden.iter().map(|h| !h).collect();
    Some(vectorize(&inv, settings))
}

/// One-shot convenience for batch processing.
pub fn trace_file(path: &std::path::Path, settings: &Settings) -> Result<VectorImage, String> {
    let img = load_image(path)?;
    let seg = segment_image(&img, settings);
    Ok(vectorize(&seg, settings))
}
