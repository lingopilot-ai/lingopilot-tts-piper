param(
    [switch]$Packaged
)

$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

Push-Location $repoRoot
try {
    cargo check --locked
    cargo test --locked

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
