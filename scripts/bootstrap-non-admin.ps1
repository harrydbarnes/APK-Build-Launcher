[CmdletBinding()]
param(
    [switch]$Build,
    [switch]$Test
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
$toolsRoot = Join-Path $repoRoot ".tools"
$cargoHome = Join-Path $toolsRoot "cargo"
$rustupHome = Join-Path $toolsRoot "rustup"
$w64Root = Join-Path $toolsRoot "w64devkit"
$w64Bin = Join-Path $w64Root "w64devkit\bin"

New-Item -ItemType Directory -Force -Path $toolsRoot | Out-Null

$rustup = Join-Path $cargoHome "bin\rustup.exe"
if (-not (Test-Path $rustup)) {
    $installer = Join-Path $toolsRoot "rustup-init.exe"
    Invoke-WebRequest `
        -Uri "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe" `
        -OutFile $installer
    $env:CARGO_HOME = $cargoHome
    $env:RUSTUP_HOME = $rustupHome
    & $installer -y --no-modify-path --profile minimal --default-host x86_64-pc-windows-gnu --default-toolchain stable
    if ($LASTEXITCODE -ne 0) {
        throw "Rust installation failed with exit code $LASTEXITCODE."
    }
}

$gcc = Join-Path $w64Bin "gcc.exe"
if (-not (Test-Path $gcc)) {
    $release = Invoke-RestMethod `
        -Uri "https://api.github.com/repos/skeeto/w64devkit/releases/latest" `
        -Headers @{ "User-Agent" = "APK-Build-Launcher" }
    $asset = $release.assets |
        Where-Object { $_.name -like "w64devkit-x64-*.7z.exe" } |
        Select-Object -First 1
    if (-not $asset) {
        throw "The latest portable w64devkit release has no x64 archive."
    }
    $archive = Join-Path $toolsRoot "w64devkit-installer.exe"
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $archive
    New-Item -ItemType Directory -Force -Path $w64Root | Out-Null
    & $archive -y "-o$w64Root"
    if ($LASTEXITCODE -ne 0) {
        throw "Portable GCC extraction failed with exit code $LASTEXITCODE."
    }
    Remove-Item -LiteralPath $archive -Force
}

$libgcc = Get-ChildItem -Path $w64Root -Recurse -Filter "libgcc.a" | Select-Object -First 1
if ($libgcc) {
    $libgccEh = Join-Path $libgcc.DirectoryName "libgcc_eh.a"
    if (-not (Test-Path $libgccEh)) {
        Copy-Item -LiteralPath $libgcc.FullName -Destination $libgccEh
    }
}

$env:CARGO_HOME = $cargoHome
$env:RUSTUP_HOME = $rustupHome
$env:RUSTUP_TOOLCHAIN = "stable-x86_64-pc-windows-gnu"
$env:PATH = "$(Join-Path $cargoHome 'bin');$w64Bin;$env:PATH"

$rustc = Join-Path $rustupHome "toolchains\stable-x86_64-pc-windows-gnu\bin\rustc.exe"
if (-not (Test-Path $rustc)) {
    & $rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal
    if ($LASTEXITCODE -ne 0) {
        throw "Rust GNU toolchain installation failed with exit code $LASTEXITCODE."
    }
}

Write-Host "Non-admin Rust and GCC are ready under $toolsRoot"

if ($Test) {
    Push-Location (Join-Path $repoRoot "src-tauri")
    try {
        & (Join-Path $cargoHome "bin\cargo.exe") test -p apk-build-launcher-core-tests
        if ($LASTEXITCODE -ne 0) {
            throw "Core tests failed with exit code $LASTEXITCODE."
        }
    }
    finally {
        Pop-Location
    }
}

if ($Build) {
    Push-Location $repoRoot
    try {
        npm.cmd ci
        if ($LASTEXITCODE -ne 0) {
            throw "npm ci failed with exit code $LASTEXITCODE."
        }
        npm.cmd run tauri build
        if ($LASTEXITCODE -ne 0) {
            throw "Tauri build failed with exit code $LASTEXITCODE."
        }
    }
    finally {
        Pop-Location
    }
}
