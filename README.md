# Vector Free

Open-source bitmap-to-vector tracer. Turn PNG, JPG and other raster images into clean, scalable vector art (SVG, PDF, EPS, AI, DXF) — automatically, or step by step with a guided wizard and hand-editing tools.

- **Engine** in Rust: palette segmentation, shared-boundary tracing (no gaps or overlaps between shapes), sub-pixel edge recovery from anti-aliasing, corner detection and Bézier fitting.
- **Desktop app** built with [Tauri](https://tauri.app): a wizard (Fully Automatic / Basic / Advanced), review and editing tools, batch processing. Portable single executable on Windows.
- **Command-line tool** for batch work and scripting.

## Features

**Tracing**
- Automatic image-type detection: photograph, artwork with blended (anti-aliased) edges, artwork without blending
- Input quality (artwork) or detail level (photos); two colors, custom palette or unlimited colors
- Suggested palettes (2–40 colors), eyedropper, per-color opacity
- Sub-pixel edges: anti-aliased edge pixels are mixes of the two neighbouring colors, so the mixing ratio locates the true edge between pixels
- Sharp corners preserved, straight edges kept straight, small shapes kept round
- Advanced controls: complexity, minimum region size, anti-aliasing artifact rejection, noise filter, smoothness, corner detection, number of nodes, lines-only output

**Editing**
- Edit the segmentation with Pencil, Fill, Zap (dissolve slivers or whole regions) and Color tools; undo / redo / revert
- Problem finder for pinch points
- Remove background shape by shape (or a whole color), reversibly
- Views: bitmap, segments, vector, paths with nodes, side by side

**Output**
- SVG, PDF, EPS, AI (Illustrator 3), DXF (R12 polylines or DXF 2000 splines), PNG, GIF, JPEG, BMP, TIFF
- Shape modes: stacked, cut-outs, cut-outs grouped by color; optional stroking; copy SVG to the clipboard
- Batch processing with per-image type detection

## Download

Windows builds are portable: no installer, just run `VectorFree.exe` (requires WebView2, included in Windows 10/11).

## Building from source

Requirements: [Rust](https://rustup.rs) (stable), [Node.js](https://nodejs.org) 18+, and the platform prerequisites for Tauri (on Windows: Visual Studio C++ Build Tools and WebView2).

```bash
# engine tests
cargo test -p vectorfree-engine

# command-line tool -> target/release/vectorfree-cli
cargo build --release -p vectorfree-cli

# desktop app -> target/release/VectorFree.exe
cd app
npm install
npx tauri build --no-bundle
```

## Command line

```bash
vectorfree-cli logo.png -f svg,pdf
vectorfree-cli C:\images -o C:\output -t auto -q high --remove-bg -f svg,dxf --dxf-splines
vectorfree-cli photo.jpg -t photo -q low -f svg
vectorfree-cli --help
```

## Project layout

```
engine/   tracing library (palette, segmentation, boundary graph, fitting, exporters) + tests
cli/      command-line front end
app/      Tauri desktop app (src-tauri = Rust commands, ui = HTML/CSS/JS)
```

## How it works

1. **Palette** – colors are clustered in CIE Lab (weighted k-means over a color histogram). For artwork, colors are added until every pixel is explained by a palette color or by a blend of two of them; for photos, until the mean color error meets the detail level.
2. **Segmentation** – each pixel gets a palette entry (blend-aware for anti-aliased art), small regions are merged, and small images are supersampled first.
3. **Boundary graph** – region borders are traced on the pixel-crack grid and split into chains shared by exactly two regions, so neighbours share identical edges.
4. **Fitting** – chain points are moved to the 50% mixing iso-line of the original image (sub-pixel edges), corners are detected and sharpened, the rest is smoothed and fitted with cubic Béziers (Schneider's algorithm).
5. **Export** – shapes are written as compound paths in the chosen format and layout.

## Acknowledgements

- J. Diebel, *Bayesian Image Vectorization: the probabilistic inversion of vector image rasterization*, PhD thesis, Stanford University, 2008 — the idea of using anti-aliasing to recover edge positions.
- P. J. Schneider, *An Algorithm for Automatically Fitting Digitized Curves*, Graphics Gems, 1990.

Vector Free's wizard follows the workflow and option names that users of commercial tracers such as Vector Magic are used to, so they can find their way without a manual. Vector Free is an independent project: it contains no code, artwork or text from other products and is not affiliated with them.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
