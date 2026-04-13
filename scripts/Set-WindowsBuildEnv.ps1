# Configure the Windows MSVC + bindgen environment in the current PowerShell session.
# Dot-source this script before running Cargo commands on Windows.

$ErrorActionPreference = "Stop"

$editions = @("Community", "BuildTools", "Professional", "Enterprise")
$vsDevShell = $null
$vsRoot = $null
foreach ($edition in $editions) {
    $candidateRoot = "C:\Program Files\Microsoft Visual Studio\2022\$edition"
    $candidateShell = Join-Path $candidateRoot "Common7\Tools\Launch-VsDevShell.ps1"
    if (Test-Path $candidateShell) {
        $vsDevShell = $candidateShell
        $vsRoot = $candidateRoot
        break
    }
}

if (-not $vsDevShell) {
    throw "Could not find Launch-VsDevShell.ps1. Install Visual Studio 2022."
}

Write-Host "Loading MSVC environment from $vsDevShell" -ForegroundColor Cyan
. $vsDevShell -Arch amd64 -HostArch amd64

$env:RUSTFLAGS = "-C target-feature=-crt-static"
$env:CXXFLAGS = "/std:c++17 /EHsc"
$env:CMAKE_GENERATOR = "Ninja"
$env:CC = "cl.exe"
$env:CXX = "cl.exe"

if (-not (Get-Command cmake -ErrorAction SilentlyContinue)) {
    throw "cmake was not found on PATH. Install CMake before building."
}

if (-not (Get-Command ninja -ErrorAction SilentlyContinue)) {
    $ninjaCandidates = @(
        "C:\Program Files\Ninja"
    )

    if ($vsRoot) {
        $ninjaCandidates += @(
            (Join-Path $vsRoot "Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja"),
            (Join-Path $vsRoot "Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin")
        )
    }

    foreach ($candidate in $ninjaCandidates) {
        if (Test-Path (Join-Path $candidate "ninja.exe")) {
            $env:PATH = "$candidate;$env:PATH"
            Write-Host "Using Ninja from $candidate" -ForegroundColor Green
            break
        }
    }
}

if (-not (Get-Command ninja -ErrorAction SilentlyContinue)) {
    throw "ninja.exe not found. Install Ninja before building."
}

$libclangCandidates = @(
    "C:\Program Files\LLVM\bin"
)

if ($vsRoot) {
    $libclangCandidates += Join-Path $vsRoot "VC\Tools\Llvm\x64\bin"
}

foreach ($candidate in $libclangCandidates) {
    if (Test-Path (Join-Path $candidate "libclang.dll")) {
        $env:LIBCLANG_PATH = $candidate
        Write-Host "Using libclang from $candidate" -ForegroundColor Green
        break
    }
}

if (-not $env:LIBCLANG_PATH) {
    throw "libclang.dll not found. Install LLVM or enable 'C++ Clang tools' in Visual Studio."
}

# Git\usr\bin contains a link.exe that can shadow MSVC link.exe. Move it to the end of PATH.
$gitUsrBin = $env:PATH -split ";" | Where-Object { $_ -match "Git\\usr\\bin" }
$otherPaths = $env:PATH -split ";" | Where-Object { $_ -notmatch "Git\\usr\\bin" }
$env:PATH = (($otherPaths + $gitUsrBin) | Where-Object { $_ }) -join ";"

Write-Host "Windows build environment configured." -ForegroundColor Green
