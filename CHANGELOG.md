# Changelog

All notable changes to Vector Free are listed here. Versions follow [semantic versioning](https://semver.org); downloads are on the [releases page](https://github.com/zavants/VectorFree/releases).

## [Unreleased]

### Added
- **Clean Lines** mode on the Wizard Mode page (and in batch processing): fully automatic like *Fully Automatic*, but straight stretches of curved outlines, such as a side running into a rounded corner or the stem of a letter, are traced as true straight lines, and the curves between them join them smoothly instead of wobbling around them. The other modes are unchanged.
- Command-line option `--clean-lines`.

## [0.4.1] - 2026-10-19

### Changed
- **Check for Updates** always answers with a dialog showing the installed and the latest published version, and states clearly when no update is available or why the check failed.

### Fixed
- Highlighted buttons (Close, Install Now, the active tool or view) no longer turn white when the mouse is over them.

## [0.4.0] - 2026-10-19

### Added
- **Windows installer** (NSIS, per user, no administrator rights) with Start menu entry and uninstaller.
- **Automatic updates** from GitHub releases: at start-up the app checks for a newer version and asks before downloading; it offers *Install Now*, *Later* or *Skip This Version*, shows download progress and restarts after installing. Manual check under **More Options → Check for Updates…**; the start-up check can be turned off.
- Updates are verified with the project's signing key before installation.
- `scripts/release.ps1` builds, signs and publishes a release (installer, portable zip and update manifest).

## [0.3.2] - 2026-10-18

### Fixed
- Long straight anti-aliased diagonal edges are traced as a single straight line with accurate corners, instead of a chain of slightly wavy curves.

## [0.3.1] - 2026-10-18

### Changed
- The About dialog, README and package metadata credit **zavants** as author and **Claude** (Anthropic) as collaborator.

## [0.3.0] - 2026-10-17

First public release.

### Added
- **Wizard interface**: image viewer on the left, step-by-step panel on the right with Start Page / Back / Next and contextual help (F1).
  - *Load Image*: import from folder, paste from clipboard, batch processing, recent folders and images.
  - *Wizard Mode*: Fully Automatic, Basic or Advanced; recent and saved settings; crop.
  - *Basic*: Image Type, Input Quality / Detail Level, Color Mode, Palette Selection with suggested palettes.
  - *Review Result*: detail level, colors, remove background, edit result, hand-pick settings.
  - *Advanced*: Image Segmentation (complexity, minimum pixels, anti-aliasing artifact rejection, noise filter) and Path Smoothing (smoothness, sharp corners, anti-aliasing engine, number of nodes, curve types).
  - *Export Result*: Save As, open in another program, copy SVG, Quick Save, bitmap export, shape / stroking / DXF modes, saved settings.
- **Remove Background** shape by shape (or a whole color), reversible.
- Views: Bitmap (B), Segments (S), Vector (V), Paths with nodes, side by side.
- DXF "lines only" density option; suggested palettes beyond 12 colors.
- README, MIT / Apache-2.0 license files.

### Fixed
- The More Options menu stays inside the window.

## [0.2.0] - 2026-10-17

Not published as a release.

### Added
- Sub-pixel edge recovery from anti-aliasing; corner detection and sharpening.
- Automatic image-type detection; blend-aware palettes that never drop a small distinct color; richer palettes for photos.
- Editing tools: pencil, fill, zap, eyedropper, undo / redo / reset, problem finder; per-color opacity.
- Advanced settings, lines-only output, denoise filter.
- Export to GIF and DXF 2000 splines; crop, paste from clipboard, copy SVG; batch cancel.

[0.4.1]: https://github.com/zavants/VectorFree/releases/tag/v0.4.1
[0.4.0]: https://github.com/zavants/VectorFree/releases/tag/v0.4.0
[0.3.2]: https://github.com/zavants/VectorFree/releases/tag/v0.3.2
[0.3.1]: https://github.com/zavants/VectorFree/releases/tag/v0.3.1
[0.3.0]: https://github.com/zavants/VectorFree/releases/tag/v0.3.0
[0.2.0]: https://github.com/zavants/VectorFree/commit/fbe6351
