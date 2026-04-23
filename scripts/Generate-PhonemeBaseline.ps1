param(
    [Parameter(Mandatory = $true)]
    [string]$BinaryPath,
    [Parameter(Mandatory = $true)]
    [string]$OutputPath
)

$ErrorActionPreference = "Stop"

$corpus = @(
    "hello",
    "I would like a cup of coffee",
    "The quick brown fox jumps over the lazy dog",
    "She sells seashells by the seashore",
    "How much wood would a woodchuck chuck",
    "I'd like to know",
    "Peter Piper picked a peck of pickled peppers",
    "Good morning, everyone",
    "Release readiness validation.",
    "Can you hear me now?",
    "This is a test of the emergency broadcast system",
    "Please remain calm and stay seated",
    "We apologize for the inconvenience",
    "Thank you for your patience and understanding",
    "All systems are functioning normally"
)

$resolvedBinary = (Resolve-Path $BinaryPath).Path

$startInfo = [System.Diagnostics.ProcessStartInfo]::new()
$startInfo.FileName = $resolvedBinary
$startInfo.UseShellExecute = $false
$startInfo.RedirectStandardInput = $true
$startInfo.RedirectStandardOutput = $true
$startInfo.RedirectStandardError = $true

$process = [System.Diagnostics.Process]::new()
$process.StartInfo = $startInfo
$null = $process.Start()

$readyLine = $process.StandardOutput.ReadLine()
$ready = $readyLine | ConvertFrom-Json
if ($ready.op -ne "ready") { throw "did not receive ready: $readyLine" }

$entries = New-Object System.Collections.Generic.List[object]
$i = 0
foreach ($text in $corpus) {
    $i += 1
    $id = "baseline-$i"
    $payload = @{ op = "phonemize"; id = $id; text = $text; language = "en-us" } | ConvertTo-Json -Compress
    $process.StandardInput.WriteLine($payload)
    $process.StandardInput.Flush()
    $line = $process.StandardOutput.ReadLine()
    $response = $line | ConvertFrom-Json
    if ($response.op -ne "phonemes") { throw "expected phonemes for '$text', got: $line" }
    $entries.Add([ordered]@{ text = $text; phonemes = $response.phonemes })
}

$process.StandardInput.Close()
$process.WaitForExit()

$output = [ordered]@{
    generated_with_version = $ready.version
    language = "en-us"
    entries = $entries
}

$json = $output | ConvertTo-Json -Depth 6
Set-Content -LiteralPath $OutputPath -Value $json -Encoding UTF8
Write-Host "Wrote $($entries.Count) baseline entries to $OutputPath" -ForegroundColor Green
