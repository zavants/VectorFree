// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use base64::Engine as _;
use engine::color::{hex, parse_hex};
use engine::export::{export_bytes, export_file, ExportOptions, Format};
use engine::segment::Problem;
use engine::{ImageKind, Segmentation, Settings, VectorImage};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

/// Undo history is bounded by memory, not by count.
const HISTORY_BYTES: usize = 256 << 20;

/// The image currently being worked on.
struct Doc {
    img: image::RgbaImage,
    /// File the image came from (None when pasted); used by Quick Save.
    path: Option<PathBuf>,
    settings: Settings,
    seg: Option<Segmentation>,
    /// Segmentation as produced by the last automatic run ("Reset edits").
    pristine: Option<Segmentation>,
    vec: Option<VectorImage>,
    undo: Vec<Segmentation>,
    redo: Vec<Segmentation>,
}

impl Doc {
    fn seg(&mut self) -> Result<&mut Segmentation, String> {
        self.seg.as_mut().ok_or_else(|| "Vectorize the image first".into())
    }

    /// Remember the current segmentation before an edit.
    fn checkpoint(&mut self) {
        if let Some(s) = &self.seg {
            self.undo.push(s.clone());
            self.redo.clear();
            let per = (s.labels.len() * 2).max(1);
            let max = (HISTORY_BYTES / per).clamp(3, 200);
            while self.undo.len() > max {
                self.undo.remove(0);
            }
        }
    }
}

#[derive(Default)]
struct AppState {
    doc: Mutex<Option<Doc>>,
    cancel_batch: AtomicBool,
}

type Res<T> = Result<T, String>;

#[derive(Serialize)]
struct ImageInfo {
    name: String,
    path: Option<String>,
    width: u32,
    height: u32,
    png: String,
    suggested_kind: ImageKind,
    exact_colors: usize,
}

#[derive(Serialize)]
struct PalItem {
    hex: String,
    transparent: bool,
    source_alpha: bool,
    alpha: u8,
    percent: f64,
}

#[derive(Serialize)]
struct View {
    palette: Vec<PalItem>,
    seg_png: String,
    svg: String,
    shapes: usize,
    paths: usize,
    nodes: usize,
    can_undo: bool,
    can_redo: bool,
    /// Outlines of shapes removed with the background eraser (SVG), if any.
    erased_svg: Option<String>,
}

/// Result of an edit: always the new segmentation preview, plus the vectors
/// when the caller asked for an immediate update.
#[derive(Serialize)]
struct EditResult {
    seg_png: String,
    view: Option<View>,
    can_undo: bool,
    can_redo: bool,
}

fn encode_png(img: &image::RgbaImage) -> Res<String> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::ImageEncoder;
    let mut buf = Vec::new();
    PngEncoder::new_with_quality(&mut buf, CompressionType::Fast, FilterType::Sub)
        .write_image(img, img.width(), img.height(), image::ExtendedColorType::Rgba8)
        .map_err(|e| e.to_string())?;
    Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(buf)))
}

/// Segmentation preview, reduced to at most ~4 MP so edits stay snappy.
fn seg_preview(seg: &Segmentation) -> Res<String> {
    let img = seg.to_rgba();
    let px = img.width() as u64 * img.height() as u64;
    if px > 4_000_000 {
        let f = (4_000_000.0 / px as f64).sqrt();
        let (w, h) = (((img.width() as f64 * f) as u32).max(1), ((img.height() as f64 * f) as u32).max(1));
        return encode_png(&image::imageops::resize(&img, w, h, image::imageops::FilterType::Nearest));
    }
    encode_png(&img)
}

fn with_doc<T>(state: &State<AppState>, f: impl FnOnce(&mut Doc) -> Res<T>) -> Res<T> {
    let mut guard = state.doc.lock().map_err(|_| "state poisoned")?;
    let doc = guard.as_mut().ok_or("Open an image first")?;
    f(doc)
}

fn build_view(doc: &Doc) -> Res<View> {
    let seg = doc.seg.as_ref().ok_or("No segmentation yet")?;
    let vec = doc.vec.as_ref().ok_or("No vectors yet")?;
    let svg = engine::export::to_svg(vec, &ExportOptions::default());
    let areas = seg.label_area();
    let total = areas.iter().sum::<usize>().max(1) as f64;
    let palette = seg
        .palette
        .iter()
        .zip(&areas)
        .map(|(e, &a)| PalItem {
            hex: hex(e.rgb),
            transparent: e.transparent,
            source_alpha: e.source_alpha,
            alpha: e.alpha,
            percent: 100.0 * a as f64 / total,
        })
        .collect();
    let paths = vec.shapes.iter().map(|s| s.outers.len() + s.holes.len()).sum();
    let nodes = vec.shapes.iter().flat_map(|s| s.outers.iter().chain(&s.holes)).map(|p| p.segs.len()).sum();
    let erased_svg = engine::hidden_outlines(seg, &doc.settings).map(|v| engine::export::to_svg(&v, &ExportOptions::default()));
    Ok(View {
        palette,
        seg_png: seg_preview(seg)?,
        svg,
        shapes: vec.shapes.len(),
        paths,
        nodes,
        can_undo: !doc.undo.is_empty(),
        can_redo: !doc.redo.is_empty(),
        erased_svg,
    })
}

/// Re-fit the vectors from the current segmentation.
fn refresh(doc: &mut Doc) -> Res<View> {
    let seg = doc.seg.as_ref().ok_or("No segmentation yet")?;
    doc.vec = Some(engine::vectorize(seg, &doc.settings));
    build_view(doc)
}

/// Apply color/opacity changes without re-fitting geometry.
fn recolor_only(doc: &mut Doc) -> Res<View> {
    let seg = doc.seg.as_ref().ok_or("No segmentation yet")?;
    match doc.vec.as_mut() {
        Some(v) => v.apply_palette(&seg.palette),
        None => return refresh(doc),
    }
    build_view(doc)
}

fn edit_result(doc: &mut Doc, revectorize: bool) -> Res<EditResult> {
    let view = if revectorize { Some(refresh(doc)?) } else { None };
    let seg = doc.seg.as_ref().ok_or("No segmentation yet")?;
    Ok(EditResult {
        seg_png: match &view {
            Some(v) => v.seg_png.clone(),
            None => seg_preview(seg)?,
        },
        view,
        can_undo: !doc.undo.is_empty(),
        can_redo: !doc.redo.is_empty(),
    })
}

fn image_info(name: String, path: Option<&PathBuf>, img: &image::RgbaImage) -> Res<ImageInfo> {
    let (kind, n) = engine::palette::suggest_kind(img);
    Ok(ImageInfo { name, path: path.map(|p| p.display().to_string()), width: img.width(), height: img.height(), png: encode_png(img)?, suggested_kind: kind, exact_colors: n })
}

fn set_image(state: &State<AppState>, name: String, path: Option<PathBuf>, img: image::RgbaImage) -> Res<ImageInfo> {
    let info = image_info(name, path.as_ref(), &img)?;
    *state.doc.lock().map_err(|_| "state poisoned")? = Some(Doc {
        img,
        path,
        settings: Settings::default(),
        seg: None,
        pristine: None,
        vec: None,
        undo: vec![],
        redo: vec![],
    });
    Ok(info)
}

#[tauri::command(async)]
fn open_image(state: State<AppState>, path: String) -> Res<ImageInfo> {
    let p = PathBuf::from(&path);
    let img = engine::load_image(&p)?;
    let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    set_image(&state, name, Some(p), img)
}

/// Image bytes pasted from the clipboard (raw request body).
#[tauri::command(async)]
fn open_image_bytes(state: State<AppState>, request: tauri::ipc::Request<'_>) -> Res<ImageInfo> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("Expected raw image bytes".into());
    };
    let img = engine::load_image_bytes(bytes)?;
    set_image(&state, "pasted image.png".into(), None, img)
}

/// File passed on the command line (e.g. an image dropped onto the .exe).
#[tauri::command]
fn startup_file() -> Option<String> {
    std::env::args().skip(1).find(|a| !a.starts_with('-') && std::path::Path::new(a).is_file())
}

/// Crop the source image to a rectangle (source pixels). Clears the result.
#[tauri::command(async)]
fn crop(state: State<AppState>, x: u32, y: u32, width: u32, height: u32) -> Res<ImageInfo> {
    let mut guard = state.doc.lock().map_err(|_| "state poisoned")?;
    let doc = guard.as_mut().ok_or("Open an image first")?;
    let (w, h) = doc.img.dimensions();
    let (x, y) = (x.min(w.saturating_sub(1)), y.min(h.saturating_sub(1)));
    let (cw, ch) = (width.min(w - x).max(1), height.min(h - y).max(1));
    let cropped = image::imageops::crop_imm(&doc.img, x, y, cw, ch).to_image();
    let info = image_info(String::new(), doc.path.as_ref(), &cropped)?;
    doc.img = cropped;
    doc.seg = None;
    doc.pristine = None;
    doc.vec = None;
    doc.undo.clear();
    doc.redo.clear();
    Ok(info)
}

#[tauri::command(async)]
fn process(state: State<AppState>, settings: Settings) -> Res<View> {
    with_doc(&state, |doc| {
        doc.settings = settings;
        let seg = engine::segment_image(&doc.img, &doc.settings);
        doc.pristine = Some(seg.clone());
        doc.seg = Some(seg);
        doc.undo.clear();
        doc.redo.clear();
        refresh(doc)
    })
}

/// Palettes of 2..12 colors to choose from, and the automatic choice.
#[derive(Serialize)]
struct Suggestions {
    palettes: Vec<Vec<String>>,
    auto_index: usize,
}

#[tauri::command(async)]
fn suggested_palettes(state: State<AppState>, settings: Settings, max_colors: Option<usize>) -> Res<Suggestions> {
    with_doc(&state, |doc| {
        let (pals, idx) = engine::palette::suggested_palettes(&doc.img, settings.kind, settings.quality, max_colors.unwrap_or(12));
        Ok(Suggestions { palettes: pals.iter().map(|p| p.iter().map(|&c| hex(c)).collect()).collect(), auto_index: idx })
    })
}

/// Re-segment with a palette the user edited, picked or chose from suggestions.
#[tauri::command(async)]
fn resegment(state: State<AppState>, palette: Vec<String>, settings: Option<Settings>) -> Res<View> {
    let pal: Vec<[u8; 3]> = palette.iter().filter_map(|h| parse_hex(h)).collect();
    if pal.is_empty() {
        return Err("The palette is empty".into());
    }
    with_doc(&state, |doc| {
        if let Some(s) = settings {
            doc.settings = s;
        }
        let keep: Vec<([u8; 3], bool, u8)> = doc
            .seg
            .as_ref()
            .map(|s| s.palette.iter().map(|e| (e.rgb, e.transparent, e.alpha)).collect())
            .unwrap_or_default();
        let mut seg = engine::segment_with_palette(&doc.img, &doc.settings, &pal);
        // Keep transparency/opacity choices for colors that survived the edit.
        for e in seg.palette.iter_mut() {
            if let Some(k) = keep.iter().find(|k| k.0 == e.rgb) {
                e.transparent |= k.1;
                e.alpha = k.2;
            }
        }
        doc.pristine = Some(seg.clone());
        doc.seg = Some(seg);
        doc.undo.clear();
        doc.redo.clear();
        refresh(doc)
    })
}

#[tauri::command(async)]
fn recolor(state: State<AppState>, index: usize, color: String) -> Res<View> {
    let rgb = parse_hex(&color).ok_or("Invalid color")?;
    with_doc(&state, |doc| {
        doc.checkpoint();
        doc.seg()?.recolor(index, rgb);
        recolor_only(doc)
    })
}

#[tauri::command(async)]
fn set_alpha(state: State<AppState>, index: usize, alpha: u8) -> Res<View> {
    with_doc(&state, |doc| {
        doc.checkpoint();
        doc.seg()?.set_alpha(index, alpha);
        recolor_only(doc)
    })
}

#[tauri::command(async)]
fn set_transparent(state: State<AppState>, index: usize, transparent: bool) -> Res<View> {
    with_doc(&state, |doc| {
        doc.checkpoint();
        doc.seg()?.set_transparent(index, transparent);
        refresh(doc)
    })
}

#[tauri::command(async)]
fn merge_colors(state: State<AppState>, from: u16, into: u16) -> Res<View> {
    with_doc(&state, |doc| {
        doc.checkpoint();
        doc.seg()?.merge_colors(from, into);
        refresh(doc)
    })
}

/// Palette index of the segment at (x, y) in source pixels (the Color tool).
#[tauri::command(async)]
fn label_at(state: State<AppState>, x: f64, y: f64) -> Res<u16> {
    with_doc(&state, |doc| {
        let seg = doc.seg.as_ref().ok_or("No segmentation yet")?;
        let s = seg.scale as f64;
        let (xi, yi) = ((x * s).floor().max(0.0) as u32, (y * s).floor().max(0.0) as u32);
        if xi >= seg.width || yi >= seg.height {
            return Err("Outside the image".into());
        }
        Ok(seg.at(xi, yi))
    })
}

/// Color of the source image at (x, y) in original pixel coordinates.
#[tauri::command(async)]
fn pick_color(state: State<AppState>, x: f64, y: f64) -> Res<String> {
    with_doc(&state, |doc| {
        let (xi, yi) = (x.floor().max(0.0) as u32, y.floor().max(0.0) as u32);
        if xi >= doc.img.width() || yi >= doc.img.height() {
            return Err("Outside the image".into());
        }
        let p = doc.img.get_pixel(xi, yi);
        Ok(hex([p[0], p[1], p[2]]))
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", tag = "tool")]
enum EditOp {
    /// Paint a stroke with a palette color.
    Pencil { points: Vec<[f64; 2]>, radius: f64, label: u16 },
    /// Dissolve the pixels under a stroke into their surroundings.
    Zap { points: Vec<[f64; 2]>, radius: f64 },
    /// Dissolve a whole region into its neighbours.
    ZapRegion { x: f64, y: f64 },
    /// Recolor a whole region.
    Fill { x: f64, y: f64, label: u16 },
    /// Background eraser: hide or restore the shape (or color) under the point.
    Erase { x: f64, y: f64, whole_color: bool },
}

/// Segmentation edit in source pixel coordinates.
#[tauri::command(async)]
fn edit(state: State<AppState>, op: EditOp, revectorize: bool) -> Res<EditResult> {
    with_doc(&state, |doc| {
        doc.checkpoint();
        let seg = doc.seg()?;
        let s = seg.scale as f64;
        let scaled = |pts: &[[f64; 2]]| -> Vec<[f64; 2]> {
            let mut v: Vec<[f64; 2]> = pts.iter().map(|p| [p[0] * s, p[1] * s]).collect();
            if v.len() == 1 {
                v.push(v[0]);
            }
            v
        };
        let cell = |x: f64, y: f64| ((x * s).floor().max(0.0) as u32, (y * s).floor().max(0.0) as u32);
        match op {
            EditOp::Pencil { points, radius, label } => {
                for w in scaled(&points).windows(2) {
                    seg.paint_line(w[0][0], w[0][1], w[1][0], w[1][1], radius * s, label);
                }
            }
            EditOp::Zap { points, radius } => {
                for w in scaled(&points).windows(2) {
                    seg.zap_line(w[0][0], w[0][1], w[1][0], w[1][1], radius * s);
                }
            }
            EditOp::ZapRegion { x, y } => {
                let (cx, cy) = cell(x, y);
                seg.zap_region(cx, cy);
            }
            EditOp::Fill { x, y, label } => {
                let (cx, cy) = cell(x, y);
                seg.fill(cx, cy, label);
            }
            EditOp::Erase { x, y, whole_color } => {
                let (cx, cy) = cell(x, y);
                seg.toggle_hidden(cx, cy, whole_color);
            }
        }
        edit_result(doc, revectorize)
    })
}

#[tauri::command(async)]
fn undo(state: State<AppState>, revectorize: bool) -> Res<EditResult> {
    with_doc(&state, |doc| {
        let prev = doc.undo.pop().ok_or("Nothing to undo")?;
        if let Some(cur) = doc.seg.replace(prev) {
            doc.redo.push(cur);
        }
        edit_result(doc, revectorize)
    })
}

#[tauri::command(async)]
fn redo(state: State<AppState>, revectorize: bool) -> Res<EditResult> {
    with_doc(&state, |doc| {
        let next = doc.redo.pop().ok_or("Nothing to redo")?;
        if let Some(cur) = doc.seg.replace(next) {
            doc.undo.push(cur);
        }
        edit_result(doc, revectorize)
    })
}

/// Discard all manual edits since the last automatic segmentation.
#[tauri::command(async)]
fn reset_edits(state: State<AppState>) -> Res<EditResult> {
    with_doc(&state, |doc| {
        let base = doc.pristine.clone().ok_or("Nothing to reset")?;
        doc.checkpoint();
        doc.seg = Some(base);
        edit_result(doc, true)
    })
}

#[tauri::command(async)]
fn update(state: State<AppState>) -> Res<View> {
    with_doc(&state, refresh)
}

/// Re-fit the vectors with new smoothing/node settings, keeping the current
/// segmentation and its edits.
#[tauri::command(async)]
fn revectorize(state: State<AppState>, settings: Settings) -> Res<View> {
    with_doc(&state, |doc| {
        doc.settings = settings;
        refresh(doc)
    })
}

/// Problem finder: pinch points in the current segmentation.
#[tauri::command(async)]
fn find_problems(state: State<AppState>) -> Res<Vec<Problem>> {
    with_doc(&state, |doc| Ok(doc.seg.as_ref().ok_or("No segmentation yet")?.problems(500)))
}

#[tauri::command(async)]
fn export(state: State<AppState>, path: String, options: ExportOptions) -> Res<String> {
    let p = PathBuf::from(&path);
    let fmt = p
        .extension()
        .and_then(|e| e.to_str())
        .and_then(Format::from_ext)
        .ok_or("Unsupported extension (use svg, eps, pdf, ai, dxf, png, gif, jpg, bmp or tif)")?;
    with_doc(&state, |doc| {
        let vec = doc.vec.as_ref().ok_or("Vectorize the image first")?;
        export_file(vec, &p, fmt, &options)?;
        Ok(path.clone())
    })
}

/// Save next to the source image with the same name and the given format.
#[tauri::command(async)]
fn quick_save(state: State<AppState>, format: Format, options: ExportOptions) -> Res<String> {
    with_doc(&state, |doc| {
        let src = doc.path.as_ref().ok_or("This image has no file yet; use Save As")?;
        let vec = doc.vec.as_ref().ok_or("Vectorize the image first")?;
        let target = src.with_extension(format.ext());
        if target == *src {
            return Err("Quick Save would overwrite the source image; use Save As".into());
        }
        export_file(vec, &target, format, &options)?;
        Ok(target.display().to_string())
    })
}

/// Export to a temporary file and open it with the program Windows associates
/// with that format (the double-click action of the drag-and-drop box).
#[tauri::command(async)]
fn open_external(state: State<AppState>, format: Format, options: ExportOptions) -> Res<String> {
    with_doc(&state, |doc| {
        let vec = doc.vec.as_ref().ok_or("Vectorize the image first")?;
        let stem = doc.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "vector".into());
        let dir = std::env::temp_dir().join("VectorFree");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let target = dir.join(format!("{stem}.{}", format.ext()));
        export_file(vec, &target, format, &options)?;
        open_path(&target)?;
        Ok(target.display().to_string())
    })
}

fn open_path(p: &std::path::Path) -> Res<()> {
    #[cfg(windows)]
    let r = std::process::Command::new("explorer").arg(p).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(p).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(p).spawn();
    r.map(|_| ()).map_err(|e| format!("cannot open {}: {e}", p.display()))
}

/// SVG text of the result, for copying to the clipboard.
#[tauri::command(async)]
fn export_svg_text(state: State<AppState>, options: ExportOptions) -> Res<String> {
    with_doc(&state, |doc| {
        let vec = doc.vec.as_ref().ok_or("Vectorize the image first")?;
        String::from_utf8(export_bytes(vec, Format::Svg, &options)?).map_err(|e| e.to_string())
    })
}

const IMAGE_EXTS: [&str; 9] = ["png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff", "webp", "ico"];

#[tauri::command(async)]
fn pick_images(app: AppHandle, multiple: bool, directory: Option<String>) -> Vec<String> {
    let mut d = app.dialog().file().add_filter("Images", &IMAGE_EXTS);
    if let Some(dir) = directory.filter(|d| std::path::Path::new(d).is_dir()) {
        d = d.set_directory(dir);
    }
    let picked = if multiple { d.blocking_pick_files() } else { d.blocking_pick_file().map(|f| vec![f]) };
    picked
        .unwrap_or_default()
        .into_iter()
        .filter_map(|f| f.into_path().ok())
        .map(|p| p.display().to_string())
        .collect()
}

#[tauri::command(async)]
fn pick_save(app: AppHandle, default_name: String, ext: String) -> Option<String> {
    // The chosen file type (extension) decides the export format; the
    // default one is listed first.
    let mut d = app.dialog().file().set_file_name(&default_name).add_filter(ext.to_uppercase(), &[ext.as_str()]);
    for f in Format::ALL {
        if f.ext() != ext {
            d = d.add_filter(f.ext().to_uppercase(), &[f.ext()]);
        }
    }
    d.blocking_save_file()
        .and_then(|f| f.into_path().ok())
        .map(|p| if p.extension().is_none() { p.with_extension(&ext) } else { p })
        .map(|p| p.display().to_string())
}

#[tauri::command(async)]
fn pick_folder(app: AppHandle) -> Option<String> {
    app.dialog().file().blocking_pick_folder().and_then(|f| f.into_path().ok()).map(|p| p.display().to_string())
}

#[derive(Serialize, Clone)]
struct BatchProgress {
    index: usize,
    total: usize,
    file: String,
    ok: bool,
    message: String,
}

#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
fn batch(
    app: AppHandle,
    state: State<AppState>,
    paths: Vec<String>,
    out_dir: String,
    formats: Vec<Format>,
    settings: Settings,
    options: ExportOptions,
    auto_kind: bool,
) -> Res<usize> {
    // Empty output folder = save next to each source image.
    let fixed_out = if out_dir.trim().is_empty() { None } else { Some(PathBuf::from(&out_dir)) };
    if let Some(o) = &fixed_out {
        std::fs::create_dir_all(o).map_err(|e| e.to_string())?;
    }
    state.cancel_batch.store(false, Ordering::SeqCst);
    let total = paths.len();
    let mut ok = 0;
    for (i, path) in paths.iter().enumerate() {
        if state.cancel_batch.load(Ordering::SeqCst) {
            let _ = app.emit(
                "batch-progress",
                BatchProgress { index: i, total, file: path.clone(), ok: false, message: "Cancelled".into() },
            );
            break;
        }
        let p = PathBuf::from(path);
        let result = engine::load_image(&p).and_then(|img| {
            let mut s = settings.clone();
            if auto_kind {
                s.kind = engine::palette::suggest_kind(&img).0;
            }
            let vec = engine::vectorize(&engine::segment_image(&img, &s), &s);
            let stem = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
            let out = fixed_out.clone().unwrap_or_else(|| p.parent().map(PathBuf::from).unwrap_or_default());
            for f in &formats {
                let target = out.join(format!("{stem}.{}", f.ext()));
                if target == p {
                    return Err("output would overwrite the source image".into());
                }
                export_file(&vec, &target, *f, &options)?;
            }
            Ok(format!("{} colors ({:?})", vec.shapes.len(), s.kind))
        });
        let (good, message) = match result {
            Ok(m) => (true, m),
            Err(e) => (false, e),
        };
        ok += good as usize;
        let _ = app.emit("batch-progress", BatchProgress { index: i + 1, total, file: path.clone(), ok: good, message });
    }
    Ok(ok)
}

#[tauri::command]
fn cancel_batch(state: State<AppState>) {
    state.cancel_batch.store(true, Ordering::SeqCst);
}

// ---------------------------------------------------------------- updates

/// An update found by `check_update`, kept until the user accepts it.
#[derive(Default)]
struct PendingUpdate(Mutex<Option<tauri_plugin_updater::Update>>);

#[derive(Serialize)]
struct UpdateInfo {
    version: String,
    current: String,
    notes: Option<String>,
    date: Option<String>,
}

/// Ask GitHub (latest.json of the newest release) whether a newer, signed
/// version exists. Nothing is downloaded here.
#[tauri::command]
async fn check_update(app: AppHandle, pending: State<'_, PendingUpdate>) -> Res<Option<UpdateInfo>> {
    use tauri_plugin_updater::UpdaterExt;
    let update = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    let info = update.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        current: u.current_version.clone(),
        notes: u.body.clone(),
        date: u.date.map(|d| d.to_string()),
    });
    *pending.0.lock().map_err(|_| "state poisoned")? = update;
    Ok(info)
}

#[derive(Serialize, Clone)]
struct UpdateProgress {
    downloaded: u64,
    total: Option<u64>,
}

/// Download the update the user accepted, verify its signature, run the
/// installer and restart. Emits `update-progress` while downloading.
#[tauri::command]
async fn install_update(app: AppHandle, pending: State<'_, PendingUpdate>) -> Res<()> {
    let update = pending.0.lock().map_err(|_| "state poisoned")?.take().ok_or("No update to install; check again")?;
    let mut downloaded = 0u64;
    let emitter = app.clone();
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let _ = emitter.emit("update-progress", UpdateProgress { downloaded, total });
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(AppState::default())
        .manage(PendingUpdate::default())
        .invoke_handler(tauri::generate_handler![
            open_image,
            open_image_bytes,
            startup_file,
            crop,
            process,
            suggested_palettes,
            resegment,
            recolor,
            set_alpha,
            set_transparent,
            merge_colors,
            label_at,
            pick_color,
            edit,
            undo,
            redo,
            reset_edits,
            update,
            revectorize,
            find_problems,
            export,
            quick_save,
            open_external,
            export_svg_text,
            pick_images,
            pick_save,
            pick_folder,
            batch,
            cancel_batch,
            check_update,
            install_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running Vector Free");
}
