param(
    [string]$VoiceId = "en_US-hfc_female-medium",
    [string]$DestinationRoot = (Join-Path $env:LOCALAPPDATA "LingoPilot\PiperVoices"),
    [string]$OrtRuntimeRoot = (Join-Path $env:LOCALAPPDATA "LingoPilot\OnnxRuntime\1.24.4"),
    [switch]$Force
)

$ErrorActionPreference = "Stop"

function Invoke-DownloadIfNeeded {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Uri,
        [Parameter(Mandatory = $true)]
        [string]$OutFile,
        [switch]$ForceDownload
    )

    if ((-not $ForceDownload) -and (Test-Path -LiteralPath $OutFile)) {
        Write-Host "Using existing fixture file: $OutFile"
        return
    }

    if (Test-Path -LiteralPath $OutFile) {
        Remove-Item -LiteralPath $OutFile -Force
    }

    Write-Host "Downloading $Uri"
    Invoke-WebRequest -Uri $Uri -OutFile $OutFile
}

function Install-OrtRuntime {
    param(
        [Parameter(Mandatory = $true)]
        [string]$DestinationDir,
        [switch]$ForceDownload
    )

    $ortVersion = "1.24.4"
    $archiveUrl = "https://github.com/microsoft/onnxruntime/releases/download/v$ortVersion/onnxruntime-win-x64-$ortVersion.zip"
    $onnxruntimeDll = Join-Path $DestinationDir "onnxruntime.dll"
    $directMlDll = Join-Path $DestinationDir "DirectML.dll"

    if ((-not $ForceDownload) -and (Test-Path -LiteralPath $onnxruntimeDll)) {
        Write-Host "Using existing ONNX Runtime DLL: $onnxruntimeDll"
        return $onnxruntimeDll
    }

    New-Item -ItemType Directory -Force -Path $DestinationDir | Out-Null

    $tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("lingopilot-tts-piper-ort-" + [System.Guid]::NewGuid().ToString("N"))
    $archivePath = Join-Path $tempRoot "onnxruntime-win-x64-$ortVersion.zip"
    $expandedPath = Join-Path $tempRoot "expanded"

    try {
        New-Item -ItemType Directory -Force -Path $tempRoot | Out-Null
        Invoke-DownloadIfNeeded -Uri $archiveUrl -OutFile $archivePath -ForceDownload:$ForceDownload
        Expand-Archive -LiteralPath $archivePath -DestinationPath $expandedPath -Force

        $ortRoot = Get-ChildItem -LiteralPath $expandedPath -Directory | Select-Object -First 1
        if (-not $ortRoot) {
            throw "Failed to locate the extracted ONNX Runtime root."
        }

        $sourceOnnxruntimeDll = Join-Path $ortRoot.FullName "lib\onnxruntime.dll"
        if (-not (Test-Path -LiteralPath $sourceOnnxruntimeDll)) {
            throw "Failed to locate 'onnxruntime.dll' in the extracted ONNX Runtime archive."
        }

        Copy-Item -LiteralPath $sourceOnnxruntimeDll -Destination $onnxruntimeDll -Force

        $sourceDirectMlDll = Join-Path $ortRoot.FullName "lib\DirectML.dll"
        if (Test-Path -LiteralPath $sourceDirectMlDll) {
            Copy-Item -LiteralPath $sourceDirectMlDll -Destination $directMlDll -Force
        }

        return $onnxruntimeDll
    }
    finally {
        if (Test-Path -LiteralPath $tempRoot) {
            Remove-Item -LiteralPath $tempRoot -Recurse -Force
        }
    }
}

$canonicalFixtures = @{
    "en_US-hfc_female-medium" = @{
        ModelUrl = "https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/hfc_female/medium/en_US-hfc_female-medium.onnx"
        ConfigUrl = "https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/hfc_female/medium/en_US-hfc_female-medium.onnx.json"
    }
}

if (-not $canonicalFixtures.ContainsKey($VoiceId)) {
    throw "Unsupported fixture voice '$VoiceId'. The scripted readiness fixture currently supports only 'en_US-hfc_female-medium'."
}

if ([string]::IsNullOrWhiteSpace($DestinationRoot)) {
    throw "DestinationRoot must not be empty."
}

$fixture = $canonicalFixtures[$VoiceId]
$destinationDir = Join-Path $DestinationRoot $VoiceId
$modelPath = Join-Path $destinationDir "$VoiceId.onnx"
$configPath = Join-Path $destinationDir "$VoiceId.onnx.json"

New-Item -ItemType Directory -Force -Path $destinationDir | Out-Null

Invoke-DownloadIfNeeded -Uri $fixture.ModelUrl -OutFile $modelPath -ForceDownload:$Force
Invoke-DownloadIfNeeded -Uri $fixture.ConfigUrl -OutFile $configPath -ForceDownload:$Force

foreach ($requiredPath in @($modelPath, $configPath)) {
    if (-not (Test-Path -LiteralPath $requiredPath)) {
        throw "Fixture download is incomplete: missing '$requiredPath'."
    }
}

$expectedFiles = @("$VoiceId.onnx", "$VoiceId.onnx.json")
$actualFiles = Get-ChildItem -LiteralPath $destinationDir -File | Select-Object -ExpandProperty Name
foreach ($expectedFile in $expectedFiles) {
    if ($actualFiles -notcontains $expectedFile) {
        throw "Fixture validation failed: expected '$expectedFile' under '$destinationDir'."
    }
}

$ortDylibPath = Install-OrtRuntime -DestinationDir $OrtRuntimeRoot -ForceDownload:$Force

if (-not (Test-Path -LiteralPath $ortDylibPath)) {
    throw "ONNX Runtime validation failed: missing '$ortDylibPath'."
}

Write-Host ""
Write-Host "Fixture ready."
Write-Host "PIPER_TTS_REAL_VOICE_DIR=$destinationDir"
Write-Host "PIPER_TTS_REAL_VOICE_ID=$VoiceId"
Write-Host "ORT_DYLIB_PATH=$ortDylibPath"
