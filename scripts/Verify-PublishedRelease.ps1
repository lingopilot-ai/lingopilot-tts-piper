param(
    [Parameter(Mandatory = $true)]
    [string]$Version,
    [string]$Repo = "lingopilot-ai/lingopilot-tts-piper"
)

$ErrorActionPreference = "Stop"

$normalizedVersion = $Version.Trim()
if (-not $normalizedVersion.StartsWith("v")) {
    $normalizedVersion = "v$normalizedVersion"
}

$assetBase = "lingopilot-tts-piper-$normalizedVersion-windows-x86_64"
$zipName = "$assetBase.zip"
$checksumName = "lingopilot-tts-piper-$normalizedVersion-sha256.txt"
$baseUrl = "https://github.com/$Repo/releases/download/$normalizedVersion"
$downloadRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("lingopilot-tts-piper-release-download-" + [System.Guid]::NewGuid().ToString("N"))
$zipPath = Join-Path $downloadRoot $zipName
$checksumPath = Join-Path $downloadRoot $checksumName
$smokeTestScript = Join-Path $PSScriptRoot "Test-WindowsReleaseArchive.ps1"

try {
    New-Item -ItemType Directory -Force -Path $downloadRoot | Out-Null

    Invoke-WebRequest -Uri "$baseUrl/$zipName" -OutFile $zipPath
    Invoke-WebRequest -Uri "$baseUrl/$checksumName" -OutFile $checksumPath

    $checksumLine = (Get-Content $checksumPath -Raw).Trim()
    if ([string]::IsNullOrWhiteSpace($checksumLine)) {
        throw "Checksum manifest is empty."
    }

    $expectedHash = ($checksumLine -split '\s+')[0].ToLowerInvariant()
    $actualHash = (Get-FileHash -LiteralPath $zipPath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $expectedHash) {
        throw "Checksum mismatch for $zipName."
    }

    & $smokeTestScript -ZipPath $zipPath

    Write-Host "PASS: published release $normalizedVersion verified for $Repo."
}
catch {
    Write-Error "FAIL: $($_.Exception.Message)"
    exit 1
}
finally {
    if (Test-Path $downloadRoot) {
        Remove-Item -LiteralPath $downloadRoot -Recurse -Force
    }
}
