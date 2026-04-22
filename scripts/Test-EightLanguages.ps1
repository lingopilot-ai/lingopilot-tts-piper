param(
    [string]$VoicesRoot = (Join-Path $env:LOCALAPPDATA "LingoPilot\PiperVoices"),
    [string]$OrtDylibPath = (Join-Path $env:LOCALAPPDATA "LingoPilot\OnnxRuntime\1.20.0\onnxruntime.dll"),
    [string]$OutputDir = (Join-Path $PSScriptRoot "..\dist\eight-languages"),
    [switch]$SkipPlayback
)

$ErrorActionPreference = "Stop"

$voices = @(
    [pscustomobject]@{ Id = "en_US-hfc_female-medium"; Path = "en/en_US/hfc_female/medium"; Language = "English";    Text = "Hello, welcome! How are you today?" },
    [pscustomobject]@{ Id = "zh_CN-huayan-medium";     Path = "zh/zh_CN/huayan/medium";     Language = "Mandarin";   Text = "你好，欢迎！你今天过得怎么样？" },
    [pscustomobject]@{ Id = "es_ES-davefx-medium";     Path = "es/es_ES/davefx/medium";     Language = "Spanish";    Text = "¡Hola, bienvenido! ¿Cómo estás hoy?" },
    [pscustomobject]@{ Id = "fr_FR-siwis-medium";      Path = "fr/fr_FR/siwis/medium";      Language = "French";     Text = "Bonjour, bienvenue ! Comment allez-vous aujourd'hui ?" },
    [pscustomobject]@{ Id = "ar_JO-kareem-medium";     Path = "ar/ar_JO/kareem/medium";     Language = "Arabic";     Text = "مرحباً، أهلاً بك! كيف حالك اليوم؟" },
    [pscustomobject]@{ Id = "pt_BR-edresson-low";      Path = "pt/pt_BR/edresson/low";      Language = "Portuguese"; Text = "Olá, bem-vindo! Como você está hoje?" },
    [pscustomobject]@{ Id = "ru_RU-dmitri-medium";     Path = "ru/ru_RU/dmitri/medium";     Language = "Russian";    Text = "Привет, добро пожаловать! Как дела сегодня?" },
    [pscustomobject]@{ Id = "de_DE-thorsten-medium";   Path = "de/de_DE/thorsten/medium";   Language = "German";     Text = "Hallo, willkommen! Wie geht es dir heute?" }
)

function Invoke-DownloadIfNeeded {
    param([string]$Uri, [string]$OutFile)
    if (Test-Path -LiteralPath $OutFile) {
        return
    }
    $parent = [System.IO.Path]::GetDirectoryName($OutFile)
    if (-not (Test-Path -LiteralPath $parent)) {
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
    }
    Write-Host "Downloading $Uri"
    Invoke-WebRequest -Uri $Uri -OutFile $OutFile
}

function Read-LineBytes {
    param([System.IO.Stream]$Stream)
    $buffer = New-Object System.Collections.Generic.List[byte]
    while ($true) {
        $value = $Stream.ReadByte()
        if ($value -lt 0) { throw "Unexpected EOF while reading JSON header." }
        if ($value -eq 10) { return [System.Text.Encoding]::UTF8.GetString($buffer.ToArray()) }
        if ($value -ne 13) { $buffer.Add([byte]$value) }
    }
}

function Read-ExactBytes {
    param([System.IO.Stream]$Stream, [int]$Count)
    $buffer = New-Object byte[] $Count
    $offset = 0
    while ($offset -lt $Count) {
        $read = $Stream.Read($buffer, $offset, $Count - $offset)
        if ($read -le 0) { throw "Unexpected EOF while reading PCM payload." }
        $offset += $read
    }
    return ,$buffer
}

function Write-WavFile {
    param(
        [string]$Path,
        [byte[]]$Pcm,
        [int]$SampleRate,
        [int]$Channels = 1,
        [int]$BitsPerSample = 16
    )
    $byteRate = $SampleRate * $Channels * ($BitsPerSample / 8)
    $blockAlign = $Channels * ($BitsPerSample / 8)
    $dataLen = $Pcm.Length
    $riffLen = 36 + $dataLen

    $fs = [System.IO.File]::Open($Path, [System.IO.FileMode]::Create)
    $bw = New-Object System.IO.BinaryWriter($fs)
    try {
        $bw.Write([byte[]][char[]]"RIFF")
        $bw.Write([int]$riffLen)
        $bw.Write([byte[]][char[]]"WAVE")
        $bw.Write([byte[]][char[]]"fmt ")
        $bw.Write([int]16)                      # fmt chunk size
        $bw.Write([int16]1)                     # PCM
        $bw.Write([int16]$Channels)
        $bw.Write([int]$SampleRate)
        $bw.Write([int]$byteRate)
        $bw.Write([int16]$blockAlign)
        $bw.Write([int16]$BitsPerSample)
        $bw.Write([byte[]][char[]]"data")
        $bw.Write([int]$dataLen)
        $bw.Write($Pcm)
    }
    finally {
        $bw.Dispose()
        $fs.Dispose()
    }
}

$baseUrl = "https://huggingface.co/rhasspy/piper-voices/resolve/main"

Write-Host ""
Write-Host "=== Step 1/3: Ensure voice models are available ==="
foreach ($voice in $voices) {
    $voiceDir = Join-Path $VoicesRoot $voice.Id
    $onnxPath = Join-Path $voiceDir ("{0}.onnx" -f $voice.Id)
    $jsonPath = Join-Path $voiceDir ("{0}.onnx.json" -f $voice.Id)
    $onnxUrl = "$baseUrl/$($voice.Path)/$($voice.Id).onnx"
    $jsonUrl = "$baseUrl/$($voice.Path)/$($voice.Id).onnx.json"
    Invoke-DownloadIfNeeded -Uri $onnxUrl -OutFile $onnxPath
    Invoke-DownloadIfNeeded -Uri $jsonUrl -OutFile $jsonPath
}

if (-not (Test-Path -LiteralPath $OrtDylibPath)) {
    throw "ONNX Runtime DLL missing at '$OrtDylibPath'. Run scripts\Download-RealVoiceFixture.ps1 first."
}

if (-not (Test-Path -LiteralPath $OutputDir)) {
    New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
}

$sidecarPath = (Resolve-Path (Join-Path $PSScriptRoot "..\target\release\lingopilot-tts-piper.exe")).Path

Write-Host ""
Write-Host "=== Step 2/3: Start sidecar and synthesize 8 languages ==="
Write-Host "Sidecar: $sidecarPath"
Write-Host "ORT_DYLIB_PATH: $OrtDylibPath"

$env:ORT_DYLIB_PATH = $OrtDylibPath

$startInfo = [System.Diagnostics.ProcessStartInfo]::new()
$startInfo.FileName = $sidecarPath
$startInfo.UseShellExecute = $false
$startInfo.RedirectStandardInput = $true
$startInfo.RedirectStandardOutput = $true
$startInfo.RedirectStandardError = $true
$startInfo.StandardInputEncoding  = [System.Text.UTF8Encoding]::new($false)
$startInfo.StandardOutputEncoding = [System.Text.UTF8Encoding]::new($false)
$startInfo.StandardErrorEncoding  = [System.Text.UTF8Encoding]::new($false)

$process = [System.Diagnostics.Process]::new()
$process.StartInfo = $startInfo
$null = $process.Start()

$stdout = $process.StandardOutput.BaseStream
$stdin = $process.StandardInput

$readyLine = Read-LineBytes -Stream $stdout
$ready = $readyLine | ConvertFrom-Json
if ($ready.op -ne "ready") {
    throw "Expected ready response, got: $readyLine"
}
Write-Host "Sidecar ready, version $($ready.version)"

$wavFiles = @()
$requestIndex = 0

foreach ($voice in $voices) {
    $voiceDir = Join-Path $VoicesRoot $voice.Id
    $modelPath  = Join-Path $voiceDir ("{0}.onnx" -f $voice.Id)
    $configPath = Join-Path $voiceDir ("{0}.onnx.json" -f $voice.Id)
    $requestIndex += 1
    $request = [ordered]@{
        op                = "synthesize"
        id                = "eight-lang-$requestIndex"
        text              = $voice.Text
        voice_model_path  = $modelPath
        voice_config_path = $configPath
        speed             = 1.0
    } | ConvertTo-Json -Compress

    Write-Host ""
    Write-Host "[$($voice.Language)] $($voice.Text)"

    $stdin.WriteLine($request)
    $stdin.Flush()

    $responseLine = Read-LineBytes -Stream $stdout
    $response = $responseLine | ConvertFrom-Json
    if ($response.op -ne "audio") {
        Write-Warning "Failed to synthesize $($voice.Language): $responseLine"
        continue
    }

    $pcm = Read-ExactBytes -Stream $stdout -Count ([int]$response.bytes)

    $doneLine = Read-LineBytes -Stream $stdout
    $done = $doneLine | ConvertFrom-Json
    if ($done.op -ne "done") {
        Write-Warning "Expected done envelope, got: $doneLine"
    }

    $wavPath = Join-Path $OutputDir ("{0:d2}-{1}.wav" -f ($wavFiles.Count + 1), $voice.Language)
    Write-WavFile -Path $wavPath -Pcm $pcm -SampleRate ([int]$response.sample_rate)
    $wavFiles += [pscustomobject]@{ Language = $voice.Language; Path = $wavPath; SampleRate = $response.sample_rate; Bytes = $response.bytes }
    Write-Host "  -> $wavPath  ($($response.bytes) bytes @ $($response.sample_rate) Hz)"
}

$stdin.Close()
$null = $process.WaitForExit(5000)

Write-Host ""
Write-Host "=== Step 3/3: Play audio files in sequence ==="
if ($SkipPlayback) {
    Write-Host "Playback skipped (-SkipPlayback). WAV files saved to $OutputDir."
} else {
    foreach ($wav in $wavFiles) {
        Write-Host ""
        Write-Host ">>> Now playing: $($wav.Language)"
        $player = New-Object System.Media.SoundPlayer
        $player.SoundLocation = $wav.Path
        $player.Load()
        $player.PlaySync()
        Start-Sleep -Milliseconds 400
    }
}

Write-Host ""
Write-Host "Done. WAV files:"
$wavFiles | Format-Table Language, Path, SampleRate, Bytes -AutoSize
