# Shared helpers for consuming `release-sources.toml` — the canonical,
# machine-enforced pin for the ONNX Runtime asset that ships next to
# `lingopilot-tts-piper.exe`.
#
# The parser is intentionally minimal (double-quoted string values, inline
# comments, single-level sections) and mirrors the semantics of the
# equivalent helpers in `lingopilot-tts-kokoro/scripts/ReleasePackaging.Common.ps1`.
# Keep the two implementations in sync — `Assert-OrtPinParity.ps1` relies on
# both reading the same dialect.

function ConvertFrom-ReleaseSourcesToml {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    $result = @{}
    $currentSection = $null

    foreach ($rawLine in Get-Content -LiteralPath $Path) {
        $line = $rawLine.Trim()
        if ([string]::IsNullOrEmpty($line)) { continue }
        if ($line.StartsWith('#')) { continue }

        if ($line.StartsWith('[') -and $line.EndsWith(']')) {
            $currentSection = $line.Substring(1, $line.Length - 2).Trim()
            if (-not $result.ContainsKey($currentSection)) {
                $result[$currentSection] = @{}
            }
            continue
        }

        if ($null -eq $currentSection) {
            throw "release-sources.toml: key '$line' is not under any [section]."
        }

        $eqIndex = $line.IndexOf('=')
        if ($eqIndex -lt 1) {
            throw "release-sources.toml: unrecognized line '$rawLine'."
        }

        $key = $line.Substring(0, $eqIndex).Trim()
        $value = $line.Substring($eqIndex + 1).Trim()

        $hashIndex = -1
        $inString = $false
        for ($i = 0; $i -lt $value.Length; $i++) {
            $ch = $value[$i]
            if ($ch -eq '"') { $inString = -not $inString }
            elseif ($ch -eq '#' -and -not $inString) { $hashIndex = $i; break }
        }
        if ($hashIndex -ge 0) {
            $value = $value.Substring(0, $hashIndex).Trim()
        }

        if ($value.StartsWith('"') -and $value.EndsWith('"') -and $value.Length -ge 2) {
            $value = $value.Substring(1, $value.Length - 2)
        } else {
            throw "release-sources.toml: value for '$key' must be a double-quoted string."
        }

        $result[$currentSection][$key] = $value
    }

    return $result
}

function ConvertFrom-ReleaseSourcesTomlText {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Text
    )

    $tempPath = Join-Path ([System.IO.Path]::GetTempPath()) ("lingopilot-release-sources-" + [System.Guid]::NewGuid().ToString("N") + ".toml")
    try {
        Set-Content -LiteralPath $tempPath -Value $Text -Encoding UTF8
        return ConvertFrom-ReleaseSourcesToml -Path $tempPath
    }
    finally {
        if (Test-Path -LiteralPath $tempPath) {
            Remove-Item -LiteralPath $tempPath -Force
        }
    }
}

function Get-ReleaseSourcesConfig {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RepoRoot
    )

    $path = Join-Path $RepoRoot "release-sources.toml"
    if (-not (Test-Path -LiteralPath $path)) {
        throw "release-sources.toml not found at '$path'. The pin manifest is required."
    }

    $parsed = ConvertFrom-ReleaseSourcesToml -Path $path
    foreach ($section in @('onnxruntime', 'onnxruntime_linux_x64', 'onnxruntime_macos_arm64')) {
        if (-not $parsed.ContainsKey($section)) {
            $parsed[$section] = @{}
        }
        foreach ($field in @('url', 'sha256')) {
            $value = $parsed[$section][$field]
            if ([string]::IsNullOrWhiteSpace($value)) {
                $parsed[$section][$field] = $null
            } else {
                $parsed[$section][$field] = $value.Trim()
            }
        }
    }

    return $parsed
}

function Assert-FileSha256 {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [Parameter(Mandatory = $true)]
        [string]$Expected
    )

    if ([string]::IsNullOrWhiteSpace($Expected)) {
        throw "SHA-256 pin is missing for '$([System.IO.Path]::GetFileName($Path))'. release-sources.toml must pin every asset."
    }

    $actual = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
    $expectedLower = $Expected.Trim().ToLowerInvariant()
    if ($actual -ne $expectedLower) {
        $fileName = Split-Path -Leaf $Path
        throw "SHA-256 mismatch for '$fileName': expected $expectedLower, got $actual."
    }
    return $actual
}
