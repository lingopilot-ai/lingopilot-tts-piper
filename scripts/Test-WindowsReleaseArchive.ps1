param(
    [Parameter(Mandatory = $true)]
    [string]$ZipPath
)

$ErrorActionPreference = "Stop"

$resolvedZipPath = (Resolve-Path $ZipPath).Path
$extractRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("lingopilot-tts-piper-release-smoke-" + [System.Guid]::NewGuid().ToString("N"))

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
    $runtimeDir = Join-Path $packageRoot "espeak-runtime"
    $onnxruntimeDll = Join-Path $packageRoot "onnxruntime.dll"
    $thirdPartyLicensesPath = Join-Path $packageRoot "THIRD_PARTY_LICENSES.txt"

    foreach ($requiredPath in @($binaryPath, $runtimeDir, (Join-Path $runtimeDir "espeak-ng-data"), $onnxruntimeDll, $thirdPartyLicensesPath)) {
        if (-not (Test-Path $requiredPath)) {
            throw "Smoke test input is missing: $requiredPath"
        }
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

    $readyLine = $process.StandardOutput.ReadLine()
    if ([string]::IsNullOrWhiteSpace($readyLine)) {
        throw "Smoke test failed: the packaged binary did not emit a ready line."
    }

    $ready = $readyLine | ConvertFrom-Json
    if ($ready.op -ne "ready") {
        throw "Smoke test failed: expected a ready response, got '$readyLine'."
    }
    foreach ($required in @("version", "sample_rate", "channels", "encoding", "ops")) {
        if (-not ($ready.PSObject.Properties.Name -contains $required)) {
            throw "Smoke test failed: ready line missing required field '$required'. Got '$readyLine'."
        }
    }
    if ($ready.sample_rate -ne 22050 -or $ready.channels -ne 1 -or $ready.encoding -ne "pcm16le") {
        throw "Smoke test failed: ready line values do not match directive. Got '$readyLine'."
    }
    $expectedOps = @("synthesize", "phonemize")
    $actualOps = @($ready.ops)
    if (($actualOps.Count -ne $expectedOps.Count) -or
        ($actualOps[0] -ne $expectedOps[0]) -or ($actualOps[1] -ne $expectedOps[1])) {
        throw "Smoke test failed: ready.ops must be ['synthesize','phonemize']. Got '$readyLine'."
    }

    # Phonemize contract gate (directive 2026-04-22e §P0 smoke list).
    # 1. Happy path: simple English text emits words array with non-empty phonemes
    #    and the top-level string matches the v0.1.5 baseline byte-for-byte.
    $happyRequest = @{
        op = "phonemize"
        id = "smoke-phonemize-1"
        text = "I would like a cup of coffee"
        language = "en-US"
    } | ConvertTo-Json -Compress
    $process.StandardInput.WriteLine($happyRequest)
    $process.StandardInput.Flush()
    $happyLine = $process.StandardOutput.ReadLine()
    $happy = $happyLine | ConvertFrom-Json
    if ($happy.op -ne "phonemes") {
        throw "Smoke test failed: expected phonemes response, got '$happyLine'."
    }
    $expectedBaseline = "aɪ wʊd lˈaɪk ɐ kˈʌp ʌv kˈɔfi"
    if ($happy.phonemes -ne $expectedBaseline) {
        throw "Smoke test failed: top-level phonemes drift from v0.1.5 baseline. Expected '$expectedBaseline', got '$($happy.phonemes)'."
    }
    if (-not $happy.PSObject.Properties.Name.Contains("words")) {
        throw "Smoke test failed: phonemes response missing 'words' field."
    }
    $happyWords = @($happy.words)
    if ($happyWords.Count -lt 1) {
        throw "Smoke test failed: words array must have at least one entry for a non-empty request."
    }
    foreach ($w in $happyWords) {
        if ([string]::IsNullOrEmpty($w.text)) {
            throw "Smoke test failed: word entry has empty text."
        }
        if ([string]::IsNullOrEmpty($w.phonemes)) {
            throw "Smoke test failed: word entry has empty phonemes for text '$($w.text)'."
        }
    }

    # 2. Empty text: returns {phonemes:"", words:[]} with no error.
    $emptyRequest = @{ op = "phonemize"; id = "smoke-phonemize-2"; text = ""; language = "en-US" } | ConvertTo-Json -Compress
    $process.StandardInput.WriteLine($emptyRequest)
    $process.StandardInput.Flush()
    $emptyLine = $process.StandardOutput.ReadLine()
    $empty = $emptyLine | ConvertFrom-Json
    if ($empty.op -ne "phonemes" -or $empty.phonemes -ne "" -or @($empty.words).Count -ne 0) {
        throw "Smoke test failed: empty-text request did not return {phonemes:'',words:[]}. Got '$emptyLine'."
    }

    # 3. Unknown BCP-47 language: returns kind=unsupported_language, process stays alive.
    $unsupportedRequest = @{ op = "phonemize"; id = "smoke-phonemize-3"; text = "hello"; language = "zz-ZZ" } | ConvertTo-Json -Compress
    $process.StandardInput.WriteLine($unsupportedRequest)
    $process.StandardInput.Flush()
    $errLine = $process.StandardOutput.ReadLine()
    $err = $errLine | ConvertFrom-Json
    if ($err.op -ne "error" -or $err.kind -ne "unsupported_language") {
        throw "Smoke test failed: unknown BCP-47 must return kind='unsupported_language'. Got '$errLine'."
    }

    # 4. Process must still be alive after the unsupported_language error.
    $livenessRequest = @{ op = "phonemize"; id = "smoke-phonemize-4"; text = "hello"; language = "en-US" } | ConvertTo-Json -Compress
    $process.StandardInput.WriteLine($livenessRequest)
    $process.StandardInput.Flush()
    $livenessLine = $process.StandardOutput.ReadLine()
    $liveness = $livenessLine | ConvertFrom-Json
    if ($liveness.op -ne "phonemes") {
        throw "Smoke test failed: sidecar did not survive unsupported_language error. Got '$livenessLine'."
    }

    $process.StandardInput.Close()
    $remainingStdout = $process.StandardOutput.ReadToEnd()
    $stderrText = $process.StandardError.ReadToEnd()
    $process.WaitForExit()

    if ($process.ExitCode -ne 0) {
        throw "Smoke test failed: packaged binary exited with code $($process.ExitCode). stderr: $stderrText"
    }

    if (-not [string]::IsNullOrEmpty($remainingStdout)) {
        throw "Smoke test failed: stdout contained extra output after the phonemize gate."
    }

    Write-Host "Smoke test passed for $resolvedZipPath" -ForegroundColor Green
}
finally {
    if (Test-Path $extractRoot) {
        Remove-Item -LiteralPath $extractRoot -Recurse -Force
    }
}
