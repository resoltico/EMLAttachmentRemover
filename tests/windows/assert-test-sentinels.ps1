param(
    [Parameter(Mandatory = $true)][string] $Listing,
    [Parameter(Mandatory = $true)][string] $IgnoredListing,
    [string] $Sentinels = (Join-Path $PSScriptRoot 'test-sentinels.txt')
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-TestName([string] $Path) {
    $names = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::Ordinal)
    foreach ($line in [System.IO.File]::ReadLines($Path)) {
        $line = $line.Trim()
        if ($line.EndsWith(': test', [System.StringComparison]::Ordinal)) {
            [void] $names.Add($line.Substring(0, $line.Length - 6))
        }
    }
    return ,$names
}

$required = @([System.IO.File]::ReadAllLines($Sentinels) | ForEach-Object { $_.Trim() } | Where-Object { $_ -and !$_.StartsWith('#') })
if ($required.Length -eq 0) { throw 'The required Windows test sentinel list is empty' }
$available = Get-TestName $Listing
$ignored = Get-TestName $IgnoredListing
foreach ($name in $required) {
    if (!$available.Contains($name)) { throw "Missing required Windows test: $name" }
    if ($ignored.Contains($name)) { throw "Ignored required Windows test: $name" }
}
Write-Output 'Required Windows tests are present and enabled'
