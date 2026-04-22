param(
    [switch]$Packaged,
    [switch]$RequireRealVoice
)

$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$realVoiceDir = $env:PIPER_TTS_REAL_VOICE_DIR
$realVoiceId = $env:PIPER_TTS_REAL_VOICE_ID
$ortDylibPath = $env:ORT_DYLIB_PATH
$debugBinaryPath = Join-Path $repoRoot "target\debug\lingopilot-tts-piper.exe"

Push-Location $repoRoot
try {
    cargo check --locked
    cargo test --locked

    if ($RequireRealVoice) {
        if ([string]::IsNullOrWhiteSpace($realVoiceDir) -or [string]::IsNullOrWhiteSpace($realVoiceId)) {
            throw "Set PIPER_TTS_REAL_VOICE_DIR and PIPER_TTS_REAL_VOICE_ID before using -RequireRealVoice."
        }

        if ([string]::IsNullOrWhiteSpace($ortDylibPath)) {
            $canonicalOrtPath = Join-Path $env:LOCALAPPDATA "LingoPilot\OnnxRuntime\1.24.4\onnxruntime.dll"
            if (Test-Path -LiteralPath $canonicalOrtPath) {
                $ortDylibPath = (Resolve-Path $canonicalOrtPath).Path
                $env:ORT_DYLIB_PATH = $ortDylibPath
            } else {
                throw "Set ORT_DYLIB_PATH or run .\scripts\Download-RealVoiceFixture.ps1 before using -RequireRealVoice."
            }
        }

        cargo test --locked synthesize_with_real_voice_emits_audio_payload_and_done_when_configured -- --exact --nocapture

        .\scripts\Test-RealVoiceFixture.ps1 `
            -BinaryPath $debugBinaryPath `
            -ModelDir $realVoiceDir `
            -VoiceId $realVoiceId `
            -OrtDylibPath $ortDylibPath
    }

    if ($Packaged) {
        $zip = Get-ChildItem -LiteralPath (Join-Path $repoRoot "dist") -Filter "*.zip" |
            Sort-Object -Property LastWriteTimeUtc -Descending |
            Select-Object -First 1

        if (-not $zip) {
            throw "No packaged archive was found under dist\."
        }

        .\scripts\Test-WindowsReleaseArchive.ps1 -ZipPath $zip.FullName
    }
}
finally {
    Pop-Location
}
