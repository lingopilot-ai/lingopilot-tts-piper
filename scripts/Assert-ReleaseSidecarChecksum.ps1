param(
    [Parameter(Mandatory = $true)][string]$ZipPath
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path -LiteralPath $ZipPath)) {
    throw "Release zip not found: $ZipPath"
}

$sidecarPath = "$ZipPath.sha256"
if (-not (Test-Path -LiteralPath $sidecarPath)) {
    throw "Missing sidecar checksum file: $sidecarPath"
}

$expected = (Get-Content -LiteralPath $sidecarPath -Raw).Trim().ToLowerInvariant()
if ($expected -notmatch '^[0-9a-f]{64}$') {
    throw "Sidecar '$sidecarPath' does not contain a bare lowercase SHA-256 digest."
}

$actual = (Get-FileHash -LiteralPath $ZipPath -Algorithm SHA256).Hash.ToLowerInvariant()
if ($expected -ne $actual) {
    throw "Sidecar hash mismatch for $ZipPath. Expected=$expected Actual=$actual"
}

Write-Host "Sidecar checksum verified: $sidecarPath ($actual)" -ForegroundColor Green
