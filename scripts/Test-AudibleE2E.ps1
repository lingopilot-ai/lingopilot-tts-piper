param(
    [string]$BinaryPath,
    [string]$ModelDir = $env:PIPER_TTS_REAL_VOICE_DIR,
    [string]$VoiceId = $env:PIPER_TTS_REAL_VOICE_ID,
    [string]$OrtDylibPath = $env:ORT_DYLIB_PATH,
    [string]$Text = "The lingo pilot sidecar is speaking to validate real synthesis.",
    [bool]$Play = $true
)

$ErrorActionPreference = "Stop"
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")

if ([string]::IsNullOrWhiteSpace($BinaryPath)) {
    $BinaryPath = Join-Path $repoRoot "target\release\lingopilot-tts-piper.exe"
}

if (-not (Test-Path -LiteralPath $BinaryPath)) {
    Write-Host "Building release binary..."
    Push-Location $repoRoot
    try { cargo build --release } finally { Pop-Location }
}

if ([string]::IsNullOrWhiteSpace($ModelDir) -or [string]::IsNullOrWhiteSpace($VoiceId) -or [string]::IsNullOrWhiteSpace($OrtDylibPath)) {
    Write-Host "Resolving fixture via Download-RealVoiceFixture.ps1..."
    $defaultVoice = "en_US-hfc_female-medium"
    & (Join-Path $PSScriptRoot "Download-RealVoiceFixture.ps1") -VoiceId $defaultVoice | Out-Host
    $VoiceId = $defaultVoice
    $ModelDir = Join-Path (Join-Path $env:LOCALAPPDATA "LingoPilot\PiperVoices") $defaultVoice
    $OrtDylibPath = Join-Path (Join-Path $env:LOCALAPPDATA "LingoPilot\OnnxRuntime\1.24.4") "onnxruntime.dll"
}

$resolvedBinary = (Resolve-Path $BinaryPath).Path
$resolvedModelDir = (Resolve-Path $ModelDir).Path
$resolvedOrt = (Resolve-Path $OrtDylibPath).Path
$modelPath = Join-Path $resolvedModelDir ("{0}.onnx" -f $VoiceId)
$configPath = Join-Path $resolvedModelDir ("{0}.onnx.json" -f $VoiceId)

foreach ($p in @($modelPath, $configPath, $resolvedOrt)) {
    if (-not (Test-Path -LiteralPath $p)) { throw "Missing fixture file: $p" }
}

function Read-LineBytes([System.IO.Stream]$s) {
    $buf = New-Object System.Collections.Generic.List[byte]
    while ($true) {
        $v = $s.ReadByte()
        if ($v -lt 0) { throw "EOF while reading JSON header" }
        if ($v -eq 10) { return [System.Text.Encoding]::UTF8.GetString($buf.ToArray()) }
        if ($v -ne 13) { $buf.Add([byte]$v) }
    }
}

function Read-ExactBytes([System.IO.Stream]$s, [int]$n) {
    $buf = New-Object byte[] $n
    $off = 0
    while ($off -lt $n) {
        $r = $s.Read($buf, $off, $n - $off)
        if ($r -le 0) { throw "EOF while reading PCM payload (got $off of $n)" }
        $off += $r
    }
    return $buf
}

function New-WavBytes([byte[]]$pcm, [int]$sampleRate, [int]$channels) {
    $ms = New-Object System.IO.MemoryStream
    $w = New-Object System.IO.BinaryWriter($ms)
    $byteRate = $sampleRate * $channels * 2
    $w.Write([byte[]][char[]]'RIFF')
    $w.Write([uint32](36 + $pcm.Length))
    $w.Write([byte[]][char[]]'WAVE')
    $w.Write([byte[]][char[]]'fmt ')
    $w.Write([uint32]16)
    $w.Write([uint16]1)
    $w.Write([uint16]$channels)
    $w.Write([uint32]$sampleRate)
    $w.Write([uint32]$byteRate)
    $w.Write([uint16]($channels * 2))
    $w.Write([uint16]16)
    $w.Write([byte[]][char[]]'data')
    $w.Write([uint32]$pcm.Length)
    $w.Write($pcm)
    $w.Flush()
    return $ms.ToArray()
}

function Get-PcmStats([byte[]]$pcm) {
    $n = [int]($pcm.Length / 2)
    if ($n -le 0) { return [pscustomobject]@{ Rms = 0.0; Peak = 0 } }
    $sum = 0.0
    $peak = 0
    for ($i = 0; $i -lt $n; $i++) {
        $s = [int][System.BitConverter]::ToInt16($pcm, 2 * $i)
        $sum += [double]$s * [double]$s
        $abs = [math]::Abs($s)
        if ($abs -gt $peak) { $peak = $abs }
    }
    return [pscustomobject]@{ Rms = [math]::Sqrt($sum / $n); Peak = $peak }
}

$process = $null
try {
    $psi = [System.Diagnostics.ProcessStartInfo]::new()
    $psi.FileName = $resolvedBinary
    $psi.UseShellExecute = $false
    $psi.RedirectStandardInput = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.Environment["ORT_DYLIB_PATH"] = $resolvedOrt

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $psi
    $null = $process.Start()
    $out = $process.StandardOutput.BaseStream

    $readyLine = Read-LineBytes $out
    $ready = $readyLine | ConvertFrom-Json
    if ($ready.op -ne "ready") { throw "Expected ready, got: $readyLine" }

    $req = @{
        op = "synthesize"
        id = "audible-e2e-1"
        text = $Text
        voice_model_path = $modelPath
        voice_config_path = $configPath
        speed = 1.0
    } | ConvertTo-Json -Compress

    $process.StandardInput.WriteLine($req)
    $process.StandardInput.Flush()

    $headerLine = Read-LineBytes $out
    $header = $headerLine | ConvertFrom-Json
    if ($header.op -ne "audio") { throw "Expected audio envelope, got: $headerLine" }
    if ([int]$header.channels -ne 1) { throw "Expected mono, got channels=$($header.channels)" }
    if ([int]$header.sample_rate -le 0) { throw "Invalid sample_rate: $($header.sample_rate)" }
    if ([int]$header.bytes -le 0) { throw "Invalid bytes: $($header.bytes)" }

    $pcm = Read-ExactBytes $out ([int]$header.bytes)
    if ($pcm.Length -ne [int]$header.bytes) { throw "PCM length mismatch" }

    $doneLine = Read-LineBytes $out
    $done = $doneLine | ConvertFrom-Json
    if ($done.op -ne "done") { throw "Expected done, got: $doneLine" }
    if ($done.id -ne "audible-e2e-1") { throw "done id mismatch: $($done.id)" }

    $stats = Get-PcmStats $pcm
    Write-Host ("Audio: {0} bytes @ {1} Hz, RMS={2:N1}, Peak={3}" -f $pcm.Length, $header.sample_rate, $stats.Rms, $stats.Peak)
    $durationSec = [double]$pcm.Length / (2.0 * [int]$header.sample_rate)
    if ($durationSec -lt 0.5) { throw "Audio too short ($([math]::Round($durationSec,2))s). Real synthesis likely failed." }
    if ($stats.Peak -lt 2000) { throw "Audio peak too low (peak=$($stats.Peak)). Real synthesis likely failed." }
    if ($stats.Rms -lt 50) { throw "Audio effectively silent (RMS=$($stats.Rms)). Real synthesis likely failed." }

    if ($Play) {
        Write-Host "Playing synthesized audio..."
        $wav = New-WavBytes $pcm ([int]$header.sample_rate) 1
        $ms = New-Object System.IO.MemoryStream(,$wav)
        $player = New-Object System.Media.SoundPlayer($ms)
        $player.PlaySync()
        $player.Dispose()
        $ms.Dispose()
    }

    $process.StandardInput.Close()
    $stderrText = $process.StandardError.ReadToEnd()
    $process.WaitForExit()
    if ($process.ExitCode -ne 0) { throw "Sidecar exited $($process.ExitCode). stderr: $stderrText" }

    Write-Host "PASS: audible E2E succeeded for '$VoiceId'."
}
catch {
    Write-Error "FAIL: $($_.Exception.Message)"
    exit 1
}
finally {
    if ($process -and -not $process.HasExited) {
        $process.Kill()
        $process.WaitForExit()
    }
}
