param(
    [string]$Version,
    [string]$OutputDir = (Join-Path (Join-Path $PSScriptRoot "..") "dist")
)

$ErrorActionPreference = "Stop"

function Get-PackageVersion {
    param([string]$CargoTomlPath)

    $versionMatch = Select-String -Path $CargoTomlPath -Pattern '^\s*version\s*=\s*"([^"]+)"' | Select-Object -First 1
    if (-not $versionMatch) {
        throw "Could not determine the package version from Cargo.toml."
    }

    return $versionMatch.Matches[0].Groups[1].Value
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$cargoTomlPath = Join-Path $repoRoot "Cargo.toml"
$normalizedVersion = if ($Version) { $Version.Trim() } else { Get-PackageVersion -CargoTomlPath $cargoTomlPath }

if ($normalizedVersion.StartsWith("v")) {
    $normalizedVersion = $normalizedVersion.Substring(1)
}

$versionTag = "v$normalizedVersion"
$assetBase = "lingopilot-tts-piper-$versionTag-windows-x86_64"
$binaryPath = Join-Path $repoRoot "target\release\lingopilot-tts-piper.exe"
$runtimeDir = Join-Path $repoRoot "target\release\espeak-runtime"
$readmePath = Join-Path $repoRoot "README.md"
$licensePath = Join-Path $repoRoot "LICENSE"

foreach ($requiredPath in @($binaryPath, $runtimeDir, $readmePath, $licensePath)) {
    if (-not (Test-Path $requiredPath)) {
        throw "Required release input is missing: $requiredPath"
    }
}

if (-not (Test-Path (Join-Path $runtimeDir "espeak-ng-data"))) {
    throw "The packaged runtime is incomplete: '$runtimeDir\espeak-ng-data' is missing."
}

$outputRoot = New-Item -ItemType Directory -Force -Path $OutputDir
$packageRoot = Join-Path $outputRoot.FullName $assetBase
$zipPath = Join-Path $outputRoot.FullName "$assetBase.zip"
$checksumPath = Join-Path $outputRoot.FullName "lingopilot-tts-piper-$versionTag-sha256.txt"

if (Test-Path $packageRoot) {
    Remove-Item -LiteralPath $packageRoot -Recurse -Force
}

if (Test-Path $zipPath) {
    Remove-Item -LiteralPath $zipPath -Force
}

New-Item -ItemType Directory -Force -Path $packageRoot | Out-Null

Copy-Item -LiteralPath $binaryPath -Destination (Join-Path $packageRoot "lingopilot-tts-piper.exe")
Copy-Item -LiteralPath $runtimeDir -Destination (Join-Path $packageRoot "espeak-runtime") -Recurse
Copy-Item -LiteralPath $readmePath -Destination (Join-Path $packageRoot "README.md")
Copy-Item -LiteralPath $licensePath -Destination (Join-Path $packageRoot "LICENSE")

Compress-Archive -LiteralPath $packageRoot -DestinationPath $zipPath -Force

$hash = (Get-FileHash -LiteralPath $zipPath -Algorithm SHA256).Hash.ToLowerInvariant()
$checksumLine = "{0}  {1}" -f $hash, (Split-Path -Leaf $zipPath)
Set-Content -LiteralPath $checksumPath -Value $checksumLine -NoNewline

Write-Host "Created release archive: $zipPath" -ForegroundColor Green
Write-Host "Created checksum manifest: $checksumPath" -ForegroundColor Green
