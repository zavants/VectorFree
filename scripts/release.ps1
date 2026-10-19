# Build and publish a Vector Free release (Windows).
#
#   .\scripts\release.ps1 -Version 0.4.0 -Notes .\notes.md            # build + publish
#   .\scripts\release.ps1 -Version 0.4.0 -Notes .\notes.md -NoPublish # build only
#
# Produces in dist\<version>\:
#   VectorFree_<v>_x64-setup.exe                  Windows installer (NSIS, per user)
#   VectorFree-<v>-windows-x64-portable.zip       portable build
#   latest.json                                   update manifest read by installed apps
#
# Updates are signed with the private key in %USERPROFILE%\.tauri\vectorfree.key
# (override with -KeyPath). Keep that key safe and private: without it no
# further update can be published to existing installations.
param(
    [Parameter(Mandatory)] [string] $Version,
    [Parameter(Mandatory)] [string] $Notes,
    [string] $KeyPath = "$env:USERPROFILE\.tauri\vectorfree.key",
    [string] $Repo = "zavants/VectorFree",
    [switch] $NoPublish
)
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root
$env:Path += ";$env:USERPROFILE\.cargo\bin"

function Run($what, [scriptblock] $cmd) {
    Write-Host "==> $what" -ForegroundColor Cyan
    & $cmd
    if ($LASTEXITCODE -ne 0) { throw "$what failed (exit $LASTEXITCODE)" }
}

if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "Version must look like 1.2.3" }
if (-not (Test-Path $KeyPath)) { throw "Signing key not found: $KeyPath" }
$notesText = (Get-Content $Notes -Raw).Trim()
if (-not $NoPublish -and (git status --porcelain)) { throw "Working tree is not clean; commit first." }

# ---- version bump
$files = 'Cargo.toml', 'app\src-tauri\tauri.conf.json', 'app\package.json', 'app\package-lock.json', 'app\ui\index.html', 'app\ui\app.js'
foreach ($f in $files) {
    $t = [IO.File]::ReadAllText("$root\$f")
    $n = $t -replace '(?m)^version = "\d+\.\d+\.\d+"', "version = `"$Version`"" `
            -replace '("version": )"\d+\.\d+\.\d+"', "`$1`"$Version`"" `
            -replace 'Vector Free v\d+\.\d+\.\d+', "Vector Free v$Version" `
            -replace 'Vector Free \d+\.\d+\.\d+<', "Vector Free $Version<" `
            -replace 'const VERSION = "\d+\.\d+\.\d+"', "const VERSION = `"$Version`""
    if ($f -eq 'app\package-lock.json') {
        # only the package's own version, not dependencies
        $n = $t -replace '("name": "vectorfree-app",\s*"version": )"\d+\.\d+\.\d+"', "`$1`"$Version`""
    }
    [IO.File]::WriteAllText("$root\$f", $n)
}

# ---- build
Run "engine tests" { cargo test --release -p vectorfree-engine }
Run "command-line tool" { cargo build --release -p vectorfree-cli }
$env:TAURI_SIGNING_PRIVATE_KEY = [IO.File]::ReadAllText($KeyPath)
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
Push-Location app
try { Run "desktop app + installer" { npx tauri build } } finally { Pop-Location }
Remove-Item Env:\TAURI_SIGNING_PRIVATE_KEY, Env:\TAURI_SIGNING_PRIVATE_KEY_PASSWORD

# ---- collect artifacts
$dist = "$root\dist\$Version"
Remove-Item $dist -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $dist | Out-Null
$nsis = Get-ChildItem "$root\target\release\bundle\nsis" -Filter "*_${Version}_x64-setup.exe" | Select-Object -First 1
if (-not $nsis) { throw "Installer for $Version not found" }
$setupName = "VectorFree_${Version}_x64-setup.exe"
Copy-Item $nsis.FullName "$dist\$setupName"
$signature = (Get-Content "$($nsis.FullName).sig" -Raw).Trim()

$stage = "$dist\VectorFree-$Version"
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item "$root\target\release\VectorFree.exe", "$root\target\release\vectorfree-cli.exe", "$root\LICENSE-MIT", "$root\LICENSE-APACHE" $stage
if (Test-Path "$root\portable\README.txt") {
    [IO.File]::WriteAllText("$stage\README.txt", ([IO.File]::ReadAllText("$root\portable\README.txt") -replace 'Vector Free \d+\.\d+\.\d+', "Vector Free $Version"))
}
$zipName = "VectorFree-$Version-windows-x64-portable.zip"
Compress-Archive -Path $stage -DestinationPath "$dist\$zipName"

$manifest = [ordered]@{
    version   = $Version
    notes     = $notesText
    pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    platforms = [ordered]@{
        "windows-x86_64" = [ordered]@{
            signature = $signature
            url       = "https://github.com/$Repo/releases/download/v$Version/$setupName"
        }
    }
}
[IO.File]::WriteAllText("$dist\latest.json", ($manifest | ConvertTo-Json -Depth 5))

# keep the local portable copy current
if (Test-Path "$root\portable") { Copy-Item "$stage\*" "$root\portable" -Force }

Write-Host "Artifacts in $dist" -ForegroundColor Green
Get-ChildItem $dist -File | ForEach-Object { "{0,-48} {1,10:n0} bytes  sha256 {2}" -f $_.Name, $_.Length, (Get-FileHash $_.FullName).Hash.ToLower() }
if ($NoPublish) { return }

# ---- publish
Run "commit" { git commit -q -am "Release $Version" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>" }
Run "push" { git push -q origin HEAD }
$setupHash = (Get-FileHash "$dist\$setupName").Hash.ToLower()
$zipHash = (Get-FileHash "$dist\$zipName").Hash.ToLower()
$body = @"
$notesText

## Download

- **$setupName** - Windows installer (per user, no administrator rights). Installed copies check GitHub for new versions and offer to update.
- **$zipName** - portable build: unzip anywhere and run ``VectorFree.exe``.

SHA-256: installer ``$setupHash``, portable ``$zipHash``

The executables are not code-signed, so Windows SmartScreen may warn on first launch (More info -> Run anyway). Updates are verified with the project's update signing key.
"@
$bodyFile = "$dist\release-notes.md"
[IO.File]::WriteAllText($bodyFile, $body)
Run "GitHub release" { gh release create "v$Version" "$dist\$setupName" "$dist\$zipName" "$dist\latest.json" --repo $Repo --target (git rev-parse HEAD) --title "Vector Free $Version" --notes-file $bodyFile }
Write-Host "Published https://github.com/$Repo/releases/tag/v$Version" -ForegroundColor Green
