param(
    [Parameter(Mandatory = $true)][ValidateSet('x86_64-pc-windows-gnullvm','aarch64-pc-windows-gnullvm')][string] $Target,
    [Parameter(Mandatory = $true)][string] $Evidence
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$root = (Get-Location).Path
$Evidence = [IO.Path]::GetFullPath($Evidence)
New-Item -ItemType Directory -Path $Evidence | Out-Null
$head = & git rev-parse HEAD
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$dirty = & git status --porcelain --untracked-files=no
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
if ($dirty) { throw 'Controls require their own clean source checkout' }
$encoding = [Text.UTF8Encoding]::new($false)
$arguments = @('test','--workspace','--locked','--features','eml-core/native-fixtures','--target',$Target)
$name = 'windows::file::tests::native_file_identity_hardlinks_and_excluded_writer_are_observed'
$sourcePath = Join-Path $root 'crates/eml-fs/src/windows/file.rs'
$original = [IO.File]::ReadAllText($sourcePath)
$needle = '    fn native_file_identity_hardlinks_and_excluded_writer_are_observed() {'
if ($original.IndexOf($needle) -lt 0 -or $original.IndexOf($needle) -ne $original.LastIndexOf($needle)) {throw 'Sentinel mutation needs exactly one declared function'}
$controls = @()
foreach ($control in @(@{label='compiled-out';attribute='#[cfg(any())]';prefix='Missing'},@{label='ignored';attribute='#[ignore]';prefix='Ignored'})) {
    $folder = Join-Path $Evidence $control.label
    New-Item -ItemType Directory -Path $folder | Out-Null
    try {
        [IO.File]::WriteAllText($sourcePath,$original.Replace($needle,"    $($control.attribute)`n$needle"),$encoding)
        $listing = & cargo "+1.99.0-$Target" @arguments -- --list --color never 2>&1
        $code = $LASTEXITCODE
        $listing | Set-Content -Encoding utf8 (Join-Path $folder 'tests.list')
        if ($code -ne 0) {throw "Native list failed before sentinel check: $code"}
        $ignored = & cargo "+1.99.0-$Target" @arguments -- --list --ignored --color never 2>&1
        $code = $LASTEXITCODE
        $ignored | Set-Content -Encoding utf8 (Join-Path $folder 'ignored.list')
        if ($code -ne 0) {throw "Native ignored-list failed before sentinel check: $code"}
        $expected = "$($control.prefix) required Windows test: $name"
        $rejected = $false
        try { & ./tests/windows/assert-test-sentinels.ps1 -Listing (Join-Path $folder 'tests.list') -IgnoredListing (Join-Path $folder 'ignored.list') }
        catch {if ($_.Exception.Message -cne $expected) {throw}; $rejected=$true}
        if (!$rejected) {throw "Required sentinel was accepted: $($control.label)"}
        $controls += @{kind=$control.label;rejected=$true;name=$name;actual_error=$expected}
    } finally {[IO.File]::WriteAllText($sourcePath,$original,$encoding)}
}
$containment = Join-Path $root 'crates/eml-fs/src/windows/containment.rs'
$originalContainment = [IO.File]::ReadAllText($containment)
$start = $originalContainment.IndexOf('    if live != captured')
if ($start -lt 0 -or $start -ne $originalContainment.LastIndexOf('    if live != captured')) {throw 'Containment mutation point is not unique'}
$end = $originalContainment.IndexOf(' {',$start)
if ($end -lt 0) {throw 'Containment predicate boundary is absent'}
$predicate = $originalContainment.Substring($start+7,$end-$start-7).Trim()
$mutant = $originalContainment.Substring(0,$start) + '    if std::hint::black_box(false) && (' + $predicate + ')' + $originalContainment.Substring($end)
$oldEvidence = $env:EML_WINDOWS_EVIDENCE_DIR
try {
    [IO.File]::WriteAllText($containment,$mutant,$encoding)
    $env:EML_WINDOWS_EVIDENCE_DIR = Join-Path $Evidence 'containment-disabled'
    & cargo "+1.99.0-$Target" test --locked --package eml-core --features native-fixtures --test windows_creation --target $Target redirected_destination_receives_no_bytes -- --color never
    $negativeExit = $LASTEXITCODE
    if ($negativeExit -eq 0) {throw 'Containment-disabled product gate was incorrectly accepted'}
    $metric = Join-Path $env:EML_WINDOWS_EVIDENCE_DIR 'P-W-file.json'
    if (!(Test-Path $metric)) {throw 'No independent byte-effect evidence: compilation/setup failure is not detection'}
    $effect = Get-Content -Raw $metric | ConvertFrom-Json
    if ($effect.outside_bytes -le 0 -or !$effect.source_unchanged -or !$effect.redirected -or !$effect.restored) {throw 'The same P-W byte-effect oracle did not detect the mutant'}
    $controls += @{kind='containment-disabled';native_test_exit=$negativeExit;outside_bytes=$effect.outside_bytes;source_unchanged=$effect.source_unchanged;detected=$true}
} finally {
    [IO.File]::WriteAllText($containment,$originalContainment,$encoding)
    $env:EML_WINDOWS_EVIDENCE_DIR = $oldEvidence
}
$dirty = & git status --porcelain --untracked-files=no
if ($LASTEXITCODE -ne 0) {exit $LASTEXITCODE}
if ($dirty) {throw 'Control mutations were not completely restored'}
@{source=$head;target=$Target;role='native rejection controls; mutable working tree is not a qualifying product artifact';controls=$controls} | ConvertTo-Json -Depth 6 | Set-Content -Encoding utf8 (Join-Path $Evidence 'result.json')
