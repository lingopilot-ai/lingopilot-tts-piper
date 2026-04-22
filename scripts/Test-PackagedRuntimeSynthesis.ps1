param(
    [Parameter(Mandatory = $true)]
    [string]$ZipPath,
    [string]$VoiceId = "en_US-hfc_female-medium"
)

# Runtime smoke gate per directive 2026-04-22c: the packaged zip is extracted,
# a real Piper voice is dropped next to the extracted binary, and an actual
# `op:"synthesize"` request is driven end-to-end to confirm the sidecar does
# not panic on the ort/onnxruntime.dll pairing.

$ErrorActionPreference = "Stop"

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

$resolvedZipPath = (Resolve-Path $ZipPath).Path
$extractRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("lingopilot-tts-piper-runtime-smoke-" + [System.Guid]::NewGuid().ToString("N"))

try {
    New-Item -ItemType Directory -Force -Path $extractRoot | Out-Null
    Expand-Archive -LiteralPath $resolvedZipPath -DestinationPath $extractRoot -Force

    $packageRoot = Join-Path $extractRoot ([System.IO.Path]::GetFileNameWithoutExtension($resolvedZipPath))
    if (-not (Test-Path $packageRoot)) {
        $directories = Get-ChildItem -LiteralPath $extractRoot -Directory
        if ($directories.Count -ne 1) {
            throw "Could not determine the extracted package root in $extractRoot."
        }
        $packageRoot = $directories[0].FullName
    }

    $binaryPath = Join-Path $packageRoot "lingopilot-tts-piper.exe"
    $onnxruntimeDll = Join-Path $packageRoot "onnxruntime.dll"
    if (-not (Test-Path $binaryPath)) { throw "Packaged binary missing: $binaryPath" }
    if (-not (Test-Path $onnxruntimeDll)) { throw "Packaged onnxruntime.dll missing: $onnxruntimeDll" }

    # Pin voice fixture to the extracted package dir so voice files live next
    # to the binary, mirroring how a real host call would look.
    $voiceDir = Join-Path $packageRoot ("voice-fixtures\" + $VoiceId)
    $modelPath = Join-Path $voiceDir "$VoiceId.onnx"
    $configPath = Join-Path $voiceDir "$VoiceId.onnx.json"
    New-Item -ItemType Directory -Force -Path $voiceDir | Out-Null

    $baseUrl = "https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/hfc_female/medium"
    if (-not (Test-Path $modelPath)) {
        Write-Host "Downloading $baseUrl/$VoiceId.onnx"
        Invoke-WebRequest -Uri "$baseUrl/$VoiceId.onnx" -OutFile $modelPath
    }
    if (-not (Test-Path $configPath)) {
        Write-Host "Downloading $baseUrl/$VoiceId.onnx.json"
        Invoke-WebRequest -Uri "$baseUrl/$VoiceId.onnx.json" -OutFile $configPath
    }

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $binaryPath
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardInput = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    $null = $process.Start()

    try {
        $stdout = $process.StandardOutput.BaseStream
        $readyLine = Read-LineBytes -Stream $stdout
        $ready = $readyLine | ConvertFrom-Json
        if ($ready.op -ne "ready") {
            throw "Runtime smoke failed: expected ready, got '$readyLine'."
        }

        $request = @{
            op                = "synthesize"
            id                = "runtime-smoke-1"
            text              = "Runtime smoke test for ORT one point two four."
            voice_model_path  = $modelPath
            voice_config_path = $configPath
            speed             = 1.0
        } | ConvertTo-Json -Compress

        $process.StandardInput.WriteLine($request)
        $process.StandardInput.Flush()

        $audioLine = Read-LineBytes -Stream $stdout
        $audio = $audioLine | ConvertFrom-Json
        if ($audio.op -ne "audio") {
            $stderrText = $process.StandardError.ReadToEnd()
            throw "Runtime smoke failed: expected audio, got '$audioLine'. stderr: $stderrText"
        }
        if ($audio.sample_rate -ne 22050 -or $audio.channels -ne 1) {
            throw "Runtime smoke failed: audio header does not match directive. Got '$audioLine'."
        }
        if ([int]$audio.bytes -le 0) {
            throw "Runtime smoke failed: audio.bytes must be positive. Got '$audioLine'."
        }

        $null = Read-ExactBytes -Stream $stdout -Count ([int]$audio.bytes)

        $doneLine = Read-LineBytes -Stream $stdout
        $done = $doneLine | ConvertFrom-Json
        if ($done.op -ne "done" -or $done.id -ne "runtime-smoke-1") {
            throw "Runtime smoke failed: expected done envelope echoing id, got '$doneLine'."
        }

        $process.StandardInput.Close()
        $null = $process.WaitForExit(10000)
        if (-not $process.HasExited) {
            throw "Runtime smoke failed: sidecar did not exit after stdin close."
        }
        if ($process.ExitCode -ne 0) {
            $stderrText = $process.StandardError.ReadToEnd()
            throw "Runtime smoke failed: sidecar exited with code $($process.ExitCode). stderr: $stderrText"
        }

        Write-Host "Runtime smoke passed for $resolvedZipPath" -ForegroundColor Green
    }
    finally {
        if (-not $process.HasExited) {
            $process.Kill()
            $null = $process.WaitForExit(2000)
        }
    }
}
finally {
    if (Test-Path $extractRoot) {
        Remove-Item -LiteralPath $extractRoot -Recurse -Force
    }
}
