# Vector Free command-line tool

`vectorfree-cli` traces images from a terminal, a script or a scheduled task, without opening the app. It uses the same engine as the desktop app and writes the same formats: SVG, PDF, EPS, AI, DXF and bitmaps.

```bash
vectorfree-cli logo.png -f svg,pdf --clean-lines
```

## Contents

- [Install on Windows](#install-on-windows)
  - [Quick install with PowerShell](#quick-install-with-powershell)
  - [Manual install](#manual-install)
  - [Update and uninstall](#update-and-uninstall)
- [Build from source (Windows, Linux, macOS)](#build-from-source-windows-linux-macos)
- [Usage](#usage)
- [Options](#options)
- [Recipes](#recipes)
- [Exit codes](#exit-codes)
- [Troubleshooting](#troubleshooting)

## Install on Windows

The tool is `vectorfree-cli.exe`, a single file with no dependencies. It is included in the **portable zip** on the [releases page](https://github.com/zavants/VectorFree/releases/latest) (`VectorFree-x.y.z-windows-x64-portable.zip`), next to the desktop app. The Windows installer (`setup.exe`) installs only the app.

Installing means putting `vectorfree-cli.exe` in a permanent folder and adding that folder to your `PATH`, so the command works from any folder. No administrator rights are needed.

### Quick install with PowerShell

Open **PowerShell** (Start menu → type *PowerShell*) and paste this block. It downloads the latest portable zip, copies its files to `%LOCALAPPDATA%\Programs\VectorFree` and adds that folder to your user `PATH`:

```powershell
$dir = "$env:LOCALAPPDATA\Programs\VectorFree"
$tmp = Join-Path $env:TEMP "vectorfree-install"
$ProgressPreference = "SilentlyContinue"   # much faster downloads in Windows PowerShell
Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $tmp, $dir | Out-Null

# Download and unpack the latest portable build
$release = Invoke-RestMethod "https://api.github.com/repos/zavants/VectorFree/releases/latest"
$asset = $release.assets | Where-Object name -like "*portable.zip"
Invoke-WebRequest $asset.browser_download_url -OutFile "$tmp\vectorfree.zip" -UseBasicParsing
Expand-Archive "$tmp\vectorfree.zip" $tmp -Force
Copy-Item "$tmp\VectorFree-*\*" $dir -Force
Remove-Item $tmp -Recurse -Force

# Add the folder to your user PATH (once)
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (($userPath -split ";") -notcontains $dir) {
    [Environment]::SetEnvironmentVariable("Path", ((@($userPath, $dir) | Where-Object { $_ }) -join ";"), "User")
}
"Installed $($release.tag_name) in $dir"
```

Then **close the window and open a new terminal** (a new `PATH` only reaches new windows) and check:

```bash
vectorfree-cli --version
```

The same folder also contains `VectorFree.exe`, the portable desktop app.

### Manual install

1. Download `VectorFree-x.y.z-windows-x64-portable.zip` from the [latest release](https://github.com/zavants/VectorFree/releases/latest).
2. Unzip it. Inside the `VectorFree-x.y.z` folder you will find `vectorfree-cli.exe`.
3. Copy the files to a permanent folder, for example `C:\Users\<you>\AppData\Local\Programs\VectorFree` or `C:\Tools\VectorFree`.
4. Add that folder to your `PATH`:
   1. Open the Start menu and type **environment variables**, then choose **Edit environment variables for your account**.
   2. Under *User variables*, select **Path** and click **Edit…**.
   3. Click **New**, paste the folder path and confirm every dialog with **OK**.
5. Open a **new** terminal (Command Prompt, PowerShell or Windows Terminal) and run `vectorfree-cli --version`.

Prefer not to touch `PATH`? Call the tool by its full path instead:

```bash
"C:\Tools\VectorFree\vectorfree-cli.exe" logo.png
```

### Update and uninstall

- **Update:** run the quick-install block again; it replaces the files with the latest release. With a manual install, unzip the new release over the old files.
- **Uninstall:** delete the folder, then remove it from `Path` in *Edit environment variables for your account*.

## Build from source (Windows, Linux, macOS)

You need [Rust](https://rustup.rs) (stable). Then:

```bash
git clone https://github.com/zavants/VectorFree.git
cd VectorFree
cargo install --path cli --locked
```

`cargo install` builds an optimized binary and puts it in `~/.cargo/bin` (`%USERPROFILE%\.cargo\bin` on Windows), which `rustup` already added to your `PATH`. To update, pull the latest code and run the same command again; to remove it, run `cargo uninstall vectorfree-cli`.

The command-line tool does not need WebView2 or any other part of the desktop app. Linux and macOS builds are not tested yet; please [report](https://github.com/zavants/VectorFree/issues) anything that does not work.

## Usage

```text
vectorfree-cli [OPTIONS] <IMAGE|FOLDER>...
```

- Give one or more images, folders, or both. For a folder, every image directly inside it is traced (subfolders are not), in alphabetical order. Recognized extensions: `png`, `jpg`, `jpeg`, `gif`, `bmp`, `tif`, `tiff`, `ico`, `webp`.
- Each result gets the image's name with the new extension: `logo.png` → `logo.svg`. It is written next to the image, or into the `--out` folder (created if needed). Existing results are replaced.
- The tool never replaces a source image: `vectorfree-cli logo.png -f png` stops with an error; add `--out` to write the PNG elsewhere.
- Without `--type`, each image's type (photo, blended artwork, flat artwork) is detected automatically, like *Fully Automatic* in the app.
- One line is printed per image: type, number of colors, time and the files written. Errors go to standard error and the remaining images are still processed.

Options can be written before or after the inputs. `vectorfree-cli --help` lists them all.

## Options

### Output

| Option | Meaning | Default |
|---|---|---|
| `-o`, `--out <DIR>` | Output folder | next to each image |
| `-f`, `--format <LIST>` | Comma-separated: `svg`, `eps`, `pdf`, `ai`, `dxf`, `png`, `gif`, `jpg`, `bmp`, `tif` | `svg` |
| `--layout <L>` | `cutout`: shapes never overlap; `stacked`: shapes are stacked on top of each other (easier to edit) | `cutout` |
| `--no-group` | One path per shape instead of one per color | grouped by color |
| `--stroke` | Also stroke every shape with its fill color, to hide hairline gaps some viewers show | off |
| `--scale <X>` | Size factor for bitmap outputs (`png`, `gif`, `jpg`, `bmp`, `tif`) | `1` |
| `--dxf-splines` | DXF 2000 with splines instead of R12 polylines | R12 polylines |

### Tracing

| Option | Meaning | Default |
|---|---|---|
| `-t`, `--type <KIND>` | `auto`, `photo`, `blended` (anti-aliased artwork), `flat` (pixel art, hard pixel steps) | `auto` |
| `-q`, `--quality <Q>` | `low`, `medium`, `high`. For artwork: how clean the source is. For photos: how much detail to keep. | `medium` |
| `-c`, `--colors <N>` | Number of colors | automatic |
| `-p`, `--palette <LIST>` | Fixed palette, e.g. `"#ffffff,#1404ab"` (overrides `--colors`) | automatic |
| `--remove-bg` | Make the color covering most of the image border transparent | off |
| `--clean-lines` | Clean Lines mode: straight stretches of curved outlines become true straight lines, and the curves join them smoothly. Best for logos and lettering. | off |
| `--lines` | Straight segments only (polygons), for machines that do not handle curves | curves |

### Fine-tuning

| Option | Meaning | Default |
|---|---|---|
| `--tolerance <PX>` | Maximum curve-fitting error, in source pixels | from `--quality` |
| `--smoothing <X>` | Smoothing strength, `0` to `4` | `1` |
| `--corner <DEG>` | Turns sharper than this angle become corners, `60` to `175` | `120` |
| `--min-area <PX>` | Merge regions smaller than this many pixels | from the image size |
| `--denoise` / `--no-denoise` | Force the noise filter on or off | on for photos and low-quality artwork |

### Information

| Option | Meaning |
|---|---|
| `-h`, `--help` | Show all options and examples |
| `-V`, `--version` | Show the version |

## Recipes

```bash
# Logo for a website, with crisp straight edges
vectorfree-cli logo.png --clean-lines

# The same logo for print and for Illustrator
vectorfree-cli logo.png -f pdf,eps,ai --clean-lines

# Laser cutter or CNC: DXF made of straight lines
vectorfree-cli part.png -f dxf --lines

# Cutting plotter or vinyl cutter: SVG with a transparent background
vectorfree-cli sticker.png --remove-bg

# Signature or stamp in exactly two colors
vectorfree-cli signature.jpg -t blended -q low -c 2 --remove-bg

# Exact brand colors
vectorfree-cli logo.png -p "#ffffff,#1404ab,#f6a623"

# Poster-like vector from a photo
vectorfree-cli photo.jpg -t photo -q low -f svg,pdf

# A whole folder, results into another folder
vectorfree-cli C:\images -o C:\vectors -f svg,pdf

# Enlarged, sharp PNG re-rendered from the vectors
vectorfree-cli icon.png -f png --scale 8 -o C:\big
```

A folder and all its subfolders, from PowerShell:

```powershell
Get-ChildItem C:\images -Directory -Recurse | ForEach-Object { vectorfree-cli $_.FullName -f svg }
vectorfree-cli C:\images -f svg
```

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Every image was traced |
| `1` | At least one image failed (the others were still written) |
| `2` | Invalid command line (unknown option, bad value, no input) |

## Troubleshooting

**`'vectorfree-cli' is not recognized as an internal or external command`**
The folder is not on your `PATH`, or the terminal was opened before you changed it. Open a new terminal; if it still fails, check the folder in *Edit environment variables for your account* → *Path*, or call the tool by its full path.

**`running scripts is disabled on this system` when pasting the quick install**
The block is pasted, not run as a script file, so this should not appear. If you saved it as a `.ps1` file, run it with `powershell -ExecutionPolicy Bypass -File install.ps1` or paste its contents instead.

**Windows warns that the file is not signed, or blocks it**
The executables are not code-signed yet. If Windows blocks the downloaded zip, right-click it → *Properties* → tick **Unblock** → *OK*, then unzip it again.

**`... would overwrite the source image`**
The output would have the same name and type as the input (for example PNG output next to a PNG). Add `--out <folder>`.

**`cannot read ...`**
The file is not a supported image, or is damaged. Supported: PNG, JPEG, GIF, BMP, TIFF, ICO, WebP.
