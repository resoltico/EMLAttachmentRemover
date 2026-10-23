#requires -Version 7.4
# Exercise only the literal-output command boundary; this does not mount or qualify NTFS.
[CmdletBinding()]
param([Parameter(Mandatory)][string] $Python, [Parameter(Mandatory)][string] $Out)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ([IO.Directory]::Exists($Out)) { throw 'Test output directory must not exist.' }
[void][IO.Directory]::CreateDirectory($Out)
$tokens = $null; $parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'qualify_windows_name_profiles.ps1'), [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count -ne 0) { throw ($parseErrors | Out-String) }
foreach ($name in @('Error-Facts','Invoke-Observed')) {
    $definition = @($ast.FindAll({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $false))
    if ($definition.Count -ne 1) { throw "Missing exact function $name" }
    . ([scriptblock]::Create($definition[0].Extent.Text))
}
$commandSequence = 0; $phase = 'test'
$names = @('ordinary','literal[brackets]','literal`tick')
if (!$IsWindows) { $names += @('literal?mark','literal*star','combined[?*]`') }
$results = @()
foreach ($name in $names) {
    $directory = Join-Path $Out $name
    [void][IO.Directory]::CreateDirectory($directory)
    if ($IsWindows) {
        if ($name -eq 'ordinary' -and $directory.StartsWith('\\?\')) { $directory = $directory.Substring(4) }
        elseif ($name -ne 'ordinary' -and !$directory.StartsWith('\\?\')) { $directory = '\\?\' + [IO.Path]::GetFullPath($directory) }
    }
    $result = Invoke-Observed $Python @('-c','import sys; assert sys.argv[1:]==["space value","bracket[]?*`","é"]; sys.stdout.buffer.write(b"a"*1048576); sys.stderr.buffer.write(bytes([254,0,10])+b"b"*1048576)','space value','bracket[]?*`','é') $directory 'bytes'
    $stem = Join-Path $directory ('{0:D3}-bytes' -f $commandSequence)
    $stdout = [IO.File]::ReadAllBytes("$stem.stdout"); $stderr = [IO.File]::ReadAllBytes("$stem.stderr")
    if ($stdout.Length -ne 1048576 -or $stderr.Length -ne 1048579 -or $stderr[0] -ne 254 -or $stderr[1] -ne 0 -or $stderr[2] -ne 10 -or @($stdout | Where-Object { $_ -ne 97 }).Count -ne 0 -or @($stderr[3..($stderr.Length-1)] | Where-Object { $_ -ne 98 }).Count -ne 0) { throw 'Native byte streams changed.' }
    if ($result.process_id -le 0 -or !$result.started -or !$result.settled -or $result.exit_code -ne 0 -or $null -ne $result.cleanup_error) { throw 'Native owner did not settle.' }
    if ([IO.Directory]::GetFiles($directory).Count -ne 3) { throw 'Escaped or unexpected file was created.' }
    $results += [ordered]@{ case = $name; passed = $true; stdout_bytes = $stdout.Length; stderr_bytes = $stderr.Length }
}
foreach ($case in @('nonzero','invalid-stdout','missing-program')) {
    $directory = Join-Path $Out $case; [void][IO.Directory]::CreateDirectory($directory)
    $program = $Python; $arguments = @('-c','import sys;sys.stdout.write("valid");sys.exit(7)')
    if ($case -eq 'invalid-stdout') { $arguments = @('-c','import sys;sys.stdout.buffer.write(bytes([255]))') }
    if ($case -eq 'missing-program') { $program = Join-Path $directory 'absent-executable' }
    $rejected = $false
    try { [void](Invoke-Observed $program $arguments $directory $case) } catch { $rejected = $true }
    if (!$rejected) { throw "Expected rejection: $case" }
    $receipt = Get-Content -LiteralPath (Join-Path $directory ('{0:D3}-{1}.json' -f $commandSequence,$case)) -Raw | ConvertFrom-Json
    if ($null -eq $receipt.error -or $null -ne $receipt.cleanup_error) { throw 'Failure diagnostic/settlement missing.' }
    if ($case -eq 'missing-program') {
        if ($null -ne $receipt.process_id -or $receipt.started -or $receipt.settled -or $null -ne $receipt.exit_code) { throw 'Unstarted command fabricated a process.' }
    } elseif ($receipt.process_id -le 0 -or !$receipt.started -or !$receipt.settled -or $receipt.exit_code -ne $(if ($case -eq 'nonzero') { 7 } else { 0 })) { throw 'Failed operation lost actual process settlement.' }
    $results += [ordered]@{ case = $case; passed = $true; receipt = $receipt }
}
$results | ConvertTo-Json -Depth 20
