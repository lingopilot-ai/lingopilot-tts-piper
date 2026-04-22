param(
    [Parameter(Mandatory = $true)]
    [string]$DestinationDir,
    [switch]$Force
)

# Downloads and stages `onnxruntime.dll` (plus `DirectML.dll` when present)
# into $DestinationDir. The URL and SHA-256 come from `release-sources.toml`
# at the repo root — this script hard-fails on any hash mismatch. The
# parity check in `Assert-OrtPinParity.ps1` guarantees the pin stays aligned
# with `lingopilot-tts-kokoro/release-sources.toml`.

$ErrorActionPreference = "Stop"

. (Join-Path $PSScriptRoot "ReleaseSources.Common.ps1")

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$config = Get-ReleaseSourcesConfig -RepoRoot $repoRoot
$section = $config['onnxruntime']

if (-not $section -or [string]::IsNullOrWhiteSpace($section['url']) -or [string]::IsNullOrWhiteSpace($section['sha256'])) {
    throw "release-sources.toml is missing [onnxruntime] url/sha256 entries at '$repoRoot\release-sources.toml'."
}

$archiveUrl = $section['url']
$expectedSha256 = $section['sha256']

New-Item -ItemType Directory -Force -Path $DestinationDir | Out-Null

$targetDll = Join-Path $DestinationDir "onnxruntime.dll"
if ((-not $Force) -and (Test-Path -LiteralPath $targetDll)) {
    Write-Host "Using existing ONNX Runtime DLL: $targetDll"
    return $targetDll
}

$archiveName = [System.IO.Path]::GetFileName(([System.Uri]$archiveUrl).AbsolutePath)
$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("lingopilot-tts-piper-ort-" + [System.Guid]::NewGuid().ToString("N"))
$archivePath = Join-Path $tempRoot $archiveName
$expandedPath = Join-Path $tempRoot "expanded"

try {
    New-Item -ItemType Directory -Force -Path $tempRoot | Out-Null
    Write-Host "Downloading $archiveUrl"
    Invoke-WebRequest -Uri $archiveUrl -OutFile $archivePath

    Assert-FileSha256 -Path $archivePath -Expected $expectedSha256 | Out-Null
    Write-Host "Verified SHA-256 for $archiveName."

    Expand-Archive -LiteralPath $archivePath -DestinationPath $expandedPath -Force

    $ortRoot = Get-ChildItem -LiteralPath $expandedPath -Directory | Select-Object -First 1
    if (-not $ortRoot) {
        throw "Failed to locate the extracted ONNX Runtime root."
    }

    $sourceOnnxruntimeDll = Join-Path $ortRoot.FullName "lib\onnxruntime.dll"
    if (-not (Test-Path -LiteralPath $sourceOnnxruntimeDll)) {
        throw "Failed to locate 'onnxruntime.dll' in the extracted archive."
    }

    Copy-Item -LiteralPath $sourceOnnxruntimeDll -Destination $targetDll -Force

    $sourceDirectMlDll = Join-Path $ortRoot.FullName "lib\DirectML.dll"
    if (Test-Path -LiteralPath $sourceDirectMlDll) {
        Copy-Item -LiteralPath $sourceDirectMlDll -Destination (Join-Path $DestinationDir "DirectML.dll") -Force
    }

    return $targetDll
}
finally {
    if (Test-Path -LiteralPath $tempRoot) {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force
    }
}
