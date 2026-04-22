param(
    [Parameter(Mandatory = $true)]
    [string]$DestinationDir,
    [string]$OrtVersion = "1.24.4",
    [switch]$Force
)

# ONNX Runtime alignment with Kokoro is a directive invariant: both TTS
# sidecars track the same major.minor. Per the 2026-04-22c directive the
# pinned major.minor is 1.24.x.

$ErrorActionPreference = "Stop"

New-Item -ItemType Directory -Force -Path $DestinationDir | Out-Null

$targetDll = Join-Path $DestinationDir "onnxruntime.dll"
if ((-not $Force) -and (Test-Path -LiteralPath $targetDll)) {
    Write-Host "Using existing ONNX Runtime DLL: $targetDll"
    return $targetDll
}

$archiveUrl = "https://github.com/microsoft/onnxruntime/releases/download/v$OrtVersion/onnxruntime-win-x64-$OrtVersion.zip"
$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("lingopilot-tts-piper-ort-" + [System.Guid]::NewGuid().ToString("N"))
$archivePath = Join-Path $tempRoot "onnxruntime-win-x64-$OrtVersion.zip"
$expandedPath = Join-Path $tempRoot "expanded"

try {
    New-Item -ItemType Directory -Force -Path $tempRoot | Out-Null
    Write-Host "Downloading $archiveUrl"
    Invoke-WebRequest -Uri $archiveUrl -OutFile $archivePath
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
