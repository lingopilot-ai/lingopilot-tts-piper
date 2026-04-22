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
    $thirdPartyLicensesPath = Join-Path $packageRoot "THIRD_PARTY_LICENSES.txt"

    foreach ($requiredPath in @($binaryPath, $runtimeDir, (Join-Path $runtimeDir "espeak-ng-data"), $thirdPartyLicensesPath)) {
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

    $process.StandardInput.Close()
    $remainingStdout = $process.StandardOutput.ReadToEnd()
    $stderrText = $process.StandardError.ReadToEnd()
    $process.WaitForExit()

    if ($process.ExitCode -ne 0) {
        throw "Smoke test failed: packaged binary exited with code $($process.ExitCode). stderr: $stderrText"
    }

    if (-not [string]::IsNullOrEmpty($remainingStdout)) {
        throw "Smoke test failed: stdout contained extra output after the ready line."
    }

    Write-Host "Smoke test passed for $resolvedZipPath" -ForegroundColor Green
}
finally {
    if (Test-Path $extractRoot) {
        Remove-Item -LiteralPath $extractRoot -Recurse -Force
    }
}
