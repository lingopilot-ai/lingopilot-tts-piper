param(
    [string]$BinaryPath = (Join-Path (Join-Path $PSScriptRoot "..") "target\release\lingopilot-tts-piper.exe"),
    [string]$EspeakDataDir = (Join-Path (Join-Path $PSScriptRoot "..") "target\release\espeak-runtime"),
    [string]$ModelDir = $env:PIPER_TTS_REAL_VOICE_DIR,
    [string]$VoiceId = $env:PIPER_TTS_REAL_VOICE_ID,
    [string]$OrtDylibPath = $env:ORT_DYLIB_PATH,
    [string]$Text = "Real voice release-readiness validation"
)

$ErrorActionPreference = "Stop"

function Read-LineBytes {
    param(
        [Parameter(Mandatory = $true)]
        [System.IO.Stream] $Stream
    )

    $buffer = New-Object System.Collections.Generic.List[byte]
    while ($true) {
        $value = $Stream.ReadByte()
        if ($value -lt 0) {
            throw "Unexpected EOF while reading a JSON header."
        }

        if ($value -eq 10) {
            return [System.Text.Encoding]::UTF8.GetString($buffer.ToArray())
        }

        if ($value -ne 13) {
            $buffer.Add([byte]$value)
        }
    }
}

function Read-ExactBytes {
    param(
        [Parameter(Mandatory = $true)]
        [System.IO.Stream] $Stream,
        [Parameter(Mandatory = $true)]
        [int] $Count
    )

    $buffer = New-Object byte[] $Count
    $offset = 0
    while ($offset -lt $Count) {
        $read = $Stream.Read($buffer, $offset, $Count - $offset)
        if ($read -le 0) {
            throw "Unexpected EOF while reading the PCM payload."
        }
        $offset += $read
    }

    return $buffer
}

function Read-AudioResponse {
    param(
        [Parameter(Mandatory = $true)]
        [System.IO.Stream] $StdoutStream
    )

    $headerLine = Read-LineBytes -Stream $StdoutStream
    $header = $headerLine | ConvertFrom-Json

    if ($header.type -ne "audio") {
        throw "Expected an audio response, got '$headerLine'."
    }

    if ([int]$header.channels -ne 1) {
        throw "Expected mono output, got channels=$($header.channels)."
    }

    if ([int]$header.sample_rate -le 0) {
        throw "Expected a positive sample_rate, got $($header.sample_rate)."
    }

    if ([int]$header.byte_length -le 0) {
        throw "Expected a positive byte_length, got $($header.byte_length)."
    }

    $payload = Read-ExactBytes -Stream $StdoutStream -Count ([int]$header.byte_length)
    if ($payload.Length -ne [int]$header.byte_length) {
        throw "PCM payload length mismatch: expected $($header.byte_length) bytes, got $($payload.Length)."
    }

    return $header
}

if ([string]::IsNullOrWhiteSpace($ModelDir) -or [string]::IsNullOrWhiteSpace($VoiceId)) {
    throw "Set PIPER_TTS_REAL_VOICE_DIR and PIPER_TTS_REAL_VOICE_ID, or pass -ModelDir and -VoiceId."
}

$resolvedBinaryPath = (Resolve-Path $BinaryPath).Path
$resolvedEspeakDataDir = (Resolve-Path $EspeakDataDir).Path
$resolvedModelDir = (Resolve-Path $ModelDir).Path
$resolvedOrtDylibPath = $null

if (-not [string]::IsNullOrWhiteSpace($OrtDylibPath)) {
    $resolvedOrtDylibPath = (Resolve-Path $OrtDylibPath).Path
}

$process = $null

try {
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $resolvedBinaryPath
    $startInfo.ArgumentList.Add("--espeak-data-dir")
    $startInfo.ArgumentList.Add($resolvedEspeakDataDir)
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardInput = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    if ($resolvedOrtDylibPath) {
        $startInfo.Environment["ORT_DYLIB_PATH"] = $resolvedOrtDylibPath
    }

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo

    $null = $process.Start()

    $stdoutStream = $process.StandardOutput.BaseStream
    $readyLine = Read-LineBytes -Stream $stdoutStream
    $ready = $readyLine | ConvertFrom-Json
    if ($ready.type -ne "ready") {
        throw "Expected a ready response, got '$readyLine'."
    }

    foreach ($requestText in @(
        "$Text one.",
        "$Text two."
    )) {
        $request = @{
            text = $requestText
            voice = $VoiceId
            speed = 1.0
            model_dir = $resolvedModelDir
        } | ConvertTo-Json -Compress

        $process.StandardInput.WriteLine($request)
        $process.StandardInput.Flush()

        $null = Read-AudioResponse -StdoutStream $stdoutStream
    }

    $process.StandardInput.Close()
    $stderrText = $process.StandardError.ReadToEnd()
    $process.WaitForExit()

    if ($process.ExitCode -ne 0) {
        throw "The sidecar exited with code $($process.ExitCode). stderr: $stderrText"
    }

    Write-Host "PASS: real voice validation succeeded for '$VoiceId'."
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
