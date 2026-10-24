//! Batch command-line front end.

use engine::color::parse_hex;
use engine::export::{export_file, ExportOptions, Format, Layout};
use engine::{CurveMode, ImageKind, Quality, Settings};
use std::path::{Path, PathBuf};

const USAGE: &str = concat!(
    "Vector Free ",
    env!("CARGO_PKG_VERSION"),
    " - bitmap to vector tracer

USAGE: vectorfree-cli [OPTIONS] <IMAGE|FOLDER>...

Traces each image (or every image in a folder) and writes the result next to
it with the same name, or into the --out folder.

OPTIONS:
  -o, --out <DIR>          Output folder (default: next to each input)
  -f, --format <LIST>      svg,eps,pdf,ai,dxf,png,gif,jpg,bmp,tif (default: svg)
  -t, --type <KIND>        auto | photo | blended | flat        (default: auto)
  -q, --quality <Q>        low | medium | high (detail, for photos) (default: medium)
  -c, --colors <N>         Number of colors (default: automatic)
  -p, --palette <LIST>     Fixed palette, e.g. \"#ffffff,#1404ab\"
      --remove-bg          Make the border color transparent
      --layout <L>         cutout | stacked                     (default: cutout)
      --stroke             Stroke shapes with their fill color
      --no-group           One path per shape instead of per color
      --scale <X>          Scale for raster outputs (default: 1)
      --dxf-splines        DXF 2000 with splines instead of R12 polylines

ADVANCED:
      --lines              Straight segments only (no curves)
      --clean-lines        Keep straight stretches of curved outlines as true lines
      --tolerance <PX>     Max curve fitting error in pixels
      --smoothing <X>      Smoothing strength, 0..4 (default 1)
      --corner <DEG>       Corner angle threshold, 60..175 (default 120)
      --min-area <PX>      Merge regions smaller than this
      --denoise / --no-denoise
  -h, --help               Show this help
  -V, --version            Show the version

EXAMPLES:
  vectorfree-cli logo.png
  vectorfree-cli logo.png -f svg,pdf --clean-lines
  vectorfree-cli C:\\images -o C:\\output -f svg,dxf
  vectorfree-cli scan.jpg -t blended -q low --remove-bg -f eps

Guide: https://github.com/zavants/VectorFree/blob/main/docs/CLI.md
"
);

fn fail(msg: &str) -> ! {
    eprintln!("error: {msg}\n\n{USAGE}");
    std::process::exit(2)
}

fn is_image(p: &Path) -> bool {
    matches!(
        p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "bmp" | "tif" | "tiff" | "ico" | "webp")
    )
}

/// Whether `target` is the very file `input`: writing it would replace the
/// source image (e.g. PNG output next to a PNG).
fn is_same_file(input: &Path, target: &Path) -> bool {
    match (std::fs::canonicalize(input), std::fs::canonicalize(target)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut settings = Settings::default();
    let mut opts = ExportOptions::default();
    let mut formats = vec![Format::Svg];
    let mut auto_kind = true;
    let mut out_dir: Option<PathBuf> = None;
    let mut inputs = Vec::new();
    while let Some(a) = args.next() {
        let mut val = || args.next().unwrap_or_else(|| fail(&format!("{a} needs a value")));
        match a.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return;
            }
            "-V" | "--version" => {
                println!("vectorfree-cli {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "-o" | "--out" => out_dir = Some(PathBuf::from(val())),
            "-f" | "--format" => {
                formats = val()
                    .split(',')
                    .map(|f| Format::from_ext(f).unwrap_or_else(|| fail(&format!("unknown format {f}"))))
                    .collect()
            }
            "-t" | "--type" => {
                let v = val();
                auto_kind = v == "auto";
                settings.kind = match v.as_str() {
                    "auto" => ImageKind::ArtBlended,
                    "photo" => ImageKind::Photo,
                    "blended" => ImageKind::ArtBlended,
                    "flat" => ImageKind::ArtFlat,
                    o => fail(&format!("unknown type {o}")),
                }
            }
            "-q" | "--quality" => {
                settings.quality = match val().as_str() {
                    "low" => Quality::Low,
                    "medium" => Quality::Medium,
                    "high" => Quality::High,
                    o => fail(&format!("unknown quality {o}")),
                }
            }
            "-c" | "--colors" => settings.num_colors = Some(val().parse().unwrap_or_else(|_| fail("bad --colors"))),
            "-p" | "--palette" => {
                settings.palette = val()
                    .split(',')
                    .map(|c| parse_hex(c).unwrap_or_else(|| fail(&format!("bad color {c}"))))
                    .collect()
            }
            "--remove-bg" => settings.remove_background = true,
            "--layout" => {
                opts.layout = match val().as_str() {
                    "cutout" => Layout::Cutout,
                    "stacked" => Layout::Stacked,
                    o => fail(&format!("unknown layout {o}")),
                }
            }
            "--stroke" => opts.stroke = true,
            "--no-group" => opts.group_by_color = false,
            "--scale" => opts.raster_scale = val().parse().unwrap_or_else(|_| fail("bad --scale")),
            "--dxf-splines" => opts.dxf_splines = true,
            "--lines" => settings.advanced.curves = CurveMode::Lines,
            "--clean-lines" => settings.advanced.clean_lines = true,
            "--tolerance" => settings.advanced.tolerance = Some(val().parse().unwrap_or_else(|_| fail("bad --tolerance"))),
            "--smoothing" => settings.advanced.smoothing = Some(val().parse().unwrap_or_else(|_| fail("bad --smoothing"))),
            "--corner" => settings.advanced.corner_angle = Some(val().parse().unwrap_or_else(|_| fail("bad --corner"))),
            "--min-area" => settings.advanced.min_area = Some(val().parse().unwrap_or_else(|_| fail("bad --min-area"))),
            "--denoise" => settings.advanced.denoise = Some(true),
            "--no-denoise" => settings.advanced.denoise = Some(false),
            s if s.starts_with('-') => fail(&format!("unknown option {s}")),
            s => {
                let p = PathBuf::from(s);
                if p.is_dir() {
                    let mut v: Vec<PathBuf> = std::fs::read_dir(&p)
                        .unwrap_or_else(|e| fail(&e.to_string()))
                        .filter_map(|e| e.ok().map(|e| e.path()))
                        .filter(|p| p.is_file() && is_image(p))
                        .collect();
                    v.sort();
                    inputs.extend(v);
                } else {
                    inputs.push(p);
                }
            }
        }
    }
    if inputs.is_empty() {
        fail("no input images");
    }
    if let Some(d) = &out_dir {
        std::fs::create_dir_all(d).unwrap_or_else(|e| fail(&e.to_string()));
    }
    let mut failures = 0;
    for (i, input) in inputs.iter().enumerate() {
        let t0 = std::time::Instant::now();
        let stem = input.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let dir = out_dir.clone().unwrap_or_else(|| input.parent().map(Path::to_path_buf).unwrap_or_default());
        let targets: Vec<(Format, PathBuf)> = formats.iter().map(|f| (*f, dir.join(format!("{stem}.{}", f.ext())))).collect();
        let result = match targets.iter().find(|(_, t)| is_same_file(input, t)) {
            Some((_, t)) => Err(format!("{} would overwrite the source image; use --out to write elsewhere", t.display())),
            None => engine::load_image(input).and_then(|img| {
                let mut s = settings.clone();
                if auto_kind {
                    s.kind = engine::palette::suggest_kind(&img).0;
                }
                let vec = engine::vectorize(&engine::segment_image(&img, &s), &s);
                let mut written = Vec::new();
                for (f, target) in &targets {
                    export_file(&vec, target, *f, &opts)?;
                    written.push(target.display().to_string());
                }
                Ok((format!("{:?}, {}", s.kind, vec.shapes.len()), written))
            }),
        };
        match result {
            Ok((n, files)) => println!(
                "[{}/{}] {} -> {} colors, {:.2}s: {}",
                i + 1,
                inputs.len(),
                input.display(),
                n,
                t0.elapsed().as_secs_f64(),
                files.join(", ")
            ),
            Err(e) => {
                failures += 1;
                eprintln!("[{}/{}] {}: {e}", i + 1, inputs.len(), input.display());
            }
        }
    }
    if failures > 0 {
        std::process::exit(1);
    }
}
