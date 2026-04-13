# build_windows.ps1 — Build lingopilot-tts-piper on Windows
# Requires: Visual Studio 2022, CMake, Ninja, LLVM (libclang.dll)

param(
    [switch]$Release,
    [switch]$Locked
)

$ErrorActionPreference = "Stop"

# ── 5. Build ──
Push-Location $PSScriptRoot
try {
    . (Join-Path $PSScriptRoot "scripts\Set-WindowsBuildEnv.ps1")

    $cargoArgs = @("build")
    if ($Release) {
        Write-Host "Building release..." -ForegroundColor Cyan
        $cargoArgs += "--release"
    } else {
        Write-Host "Building debug..." -ForegroundColor Cyan
    }

    if ($Locked) {
        $cargoArgs += "--locked"
    }

    cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) {
        Write-Host "Build failed!" -ForegroundColor Red
        exit 1
    }
    Write-Host "Build succeeded!" -ForegroundColor Green
} finally {
    Pop-Location
}
