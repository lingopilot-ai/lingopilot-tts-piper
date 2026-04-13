# build_windows.ps1 — Build lingopilot-tts-piper on Windows
# Requires: Visual Studio 2022, CMake, Ninja, LLVM (libclang.dll)

param(
    [switch]$Release
)

$ErrorActionPreference = "Stop"

# ── 1. Load MSVC environment ──
$editions = @('Community', 'BuildTools', 'Professional', 'Enterprise')
$vsDevShell = $null
foreach ($edition in $editions) {
    $candidate = "C:\Program Files\Microsoft Visual Studio\2022\$edition\Common7\Tools\Launch-VsDevShell.ps1"
    if (Test-Path $candidate) {
        $vsDevShell = $candidate
        break
    }
}
if (-not $vsDevShell) {
    Write-Host "Could not find Launch-VsDevShell.ps1. Install Visual Studio 2022." -ForegroundColor Red
    exit 1
}

Write-Host "Loading MSVC environment..." -ForegroundColor Cyan
& $vsDevShell -Arch amd64 -HostArch amd64

# ── 2. Set build flags ──
$env:RUSTFLAGS = "-C target-feature=-crt-static"
$env:CXXFLAGS = "/std:c++17 /EHsc"
$env:CMAKE_GENERATOR = "Ninja"
$env:CC = "cl.exe"
$env:CXX = "cl.exe"

# ── 3. Find libclang ──
$libclangCandidates = @(
    'C:\Program Files\LLVM\bin'
)
foreach ($edition in $editions) {
    $libclangCandidates += "C:\Program Files\Microsoft Visual Studio\2022\$edition\VC\Tools\Llvm\x64\bin"
}
foreach ($candidate in $libclangCandidates) {
    if (Test-Path (Join-Path $candidate 'libclang.dll')) {
        $env:LIBCLANG_PATH = $candidate
        Write-Host "Using libclang from $candidate" -ForegroundColor Green
        break
    }
}
if (-not $env:LIBCLANG_PATH) {
    Write-Host "libclang.dll not found. Install LLVM or enable 'C++ Clang tools' in VS." -ForegroundColor Red
    exit 1
}

# ── 4. Filter Git\usr\bin from PATH (MSVC link.exe conflict) ──
# Git\usr\bin contains a link.exe that shadows MSVC link.exe.
# However, git submodule (needed by espeak-ng CMake build) lives there too.
# Solution: move Git\usr\bin to the END of PATH so MSVC link.exe wins,
# but git submodule is still reachable.
$gitUsrBin = $env:PATH -split ';' | Where-Object { $_ -match 'Git\\usr\\bin' }
$otherPaths = $env:PATH -split ';' | Where-Object { $_ -notmatch 'Git\\usr\\bin' }
$env:PATH = (($otherPaths + $gitUsrBin) | Where-Object { $_ }) -join ';'

# ── 5. Build ──
Push-Location $PSScriptRoot
try {
    if ($Release) {
        Write-Host "Building release..." -ForegroundColor Cyan
        cargo build --release
    } else {
        Write-Host "Building debug..." -ForegroundColor Cyan
        cargo build
    }
    if ($LASTEXITCODE -ne 0) {
        Write-Host "Build failed!" -ForegroundColor Red
        exit 1
    }
    Write-Host "Build succeeded!" -ForegroundColor Green
} finally {
    Pop-Location
}
