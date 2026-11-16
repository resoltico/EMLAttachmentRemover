# SPDX-License-Identifier: AGPL-3.0-only
# Native exact-release installer checks. All installs use temporary directories.
# Registry cases run only in CI and restore raw value, kind and absence exactly.
param([Parameter(Mandatory = $true)][string]$Dist, [switch]$RunInCurrentShell)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3
$Dist = [IO.Path]::GetFullPath($Dist)
$installer = Join-Path $Dist 'install.ps1'
$work = Join-Path ([IO.Path]::GetTempPath()) ('cli-install-test-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($work)
$script:failures = 0
$command = 'emlattachmentremover'
$prefix = 'EMLATTACHMENTREMOVER'
$checksumName = 'SHA256SUMS'
$testVersion = ''
$reviewedInstaller = $null

function Check([string]$Why, [bool]$Condition) {
    if ($Condition) { Write-Host "    ok: $Why" }
    else {
        Write-Host "    FAIL: $Why"
        $script:failures++
        $out = Join-Path $work 'out.txt'
        if (Test-Path -LiteralPath $out) { Write-Host ([IO.File]::ReadAllText($out)) }
    }
}
function Get-Output { return [IO.File]::ReadAllText((Join-Path $work 'out.txt')) }
function Get-RecoveryCommand([string]$Text) { return [regex]::Match($Text, '(?m)^  (& [^\r\n]+)\r?$') }
function Invoke-NativeCase([string]$Shell, [string[]]$Arguments, [switch]$StreamOutput) {
    # PS 5.1 turns redirected native stderr into NativeCommandError records.
    # Collect them without terminating the harness, then assert the actual exit.
    $previousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $global:LASTEXITCODE = $null
        if ($StreamOutput) {
            & $Shell @Arguments 2>&1 | ForEach-Object { Write-Host $_ }
        } else {
            & $Shell @Arguments *>&1 | Out-File -LiteralPath (Join-Path $work 'out.txt') -Encoding utf8 -Width 32767
        }
        if ($global:LASTEXITCODE -isnot [int]) { throw "Native process did not produce an exit code: $Shell" }
        return $global:LASTEXITCODE
    } finally { $ErrorActionPreference = $previousPreference }
}
function Get-InstallerOptionMap([string[]]$Arguments) {
    $options = @{}
    for ($i = 0; $i -lt $Arguments.Count; $i++) {
        $argument = $Arguments[$i]
        switch ($argument) {
            { $_ -in @('-Version', '-InstallDir') } {
                if ($i + 1 -ge $Arguments.Count -or $options.ContainsKey($argument.Substring(1))) { return $null }
                $i++
                $options[$argument.Substring(1)] = $Arguments[$i]
            }
            { $_ -in @('-NoModifyPath', '-Uninstall', '-ReplaceUnmanaged', '-Help') } {
                if ($options.ContainsKey($argument.Substring(1))) { return $null }
                $options[$argument.Substring(1)] = $true
            }
            default { return $null }
        }
    }
    return $options
}
function Invoke-Installer([string]$Shell, [string[]]$Arguments, [string]$Base = $Dist, [switch]$AsFile) {
    $saved = @{}
    $sessionPath = $env:Path
    foreach ($suffix in @('VERSION', 'INSTALL_DIR', 'NO_MODIFY_PATH', 'UNINSTALL', 'DOWNLOAD_BASE')) {
        $name = $prefix + '_' + $suffix
        $saved[$name] = [Environment]::GetEnvironmentVariable($name)
        [Environment]::SetEnvironmentVariable($name, $null)
    }
    $options = Get-InstallerOptionMap $Arguments
    $fileMode = $AsFile -or -not $RunInCurrentShell -or $null -eq $options
    $mode = if ($fileMode) { '-File' } else { 'reviewed scriptblock' }
    Write-Host ("    invoking {0}: {1}" -f $mode, ($Arguments -join ' '))
    $watch = [Diagnostics.Stopwatch]::StartNew()
    try {
        [Environment]::SetEnvironmentVariable(($prefix + '_VERSION'), $testVersion)
        [Environment]::SetEnvironmentVariable(($prefix + '_DOWNLOAD_BASE'), $Base)
        if ($fileMode) {
            return (Invoke-NativeCase $Shell (@('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $installer) + $Arguments))
        }
        try {
            # Keep PS5.1 host output unwrapped and UTF-8, matching catch append.
            # Reuse the exact public scriptblock, not an internal installer function.
            # Hashtable splatting preserves named binding; string arrays do not.
            & $reviewedInstaller @options *>&1 | Out-File -LiteralPath (Join-Path $work 'out.txt') -Encoding utf8 -Width 32767
            return 0
        } catch {
            [IO.File]::AppendAllText((Join-Path $work 'out.txt'), ($_ | Out-String))
            return 1
        }
    } finally {
        $watch.Stop()
        Write-Host ('    invocation finished in {0:N2}s' -f $watch.Elapsed.TotalSeconds)
        foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name]) }
        $env:Path = $sessionPath
    }
}
function Get-RegistryPath {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
    try {
        if (-not $key -or $key.GetValueNames() -notcontains 'Path') { return @{ Exists = $false; Value = ''; Kind = [Microsoft.Win32.RegistryValueKind]::ExpandString } }
        return @{ Exists = $true; Value = $key.GetValue('Path', '', 'DoNotExpandEnvironmentNames'); Kind = $key.GetValueKind('Path') }
    } finally { if ($key) { $key.Close() } }
}
function Save-RegistryPath([hashtable]$Record) {
    $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
    try {
        if ($Record.Exists) { $key.SetValue('Path', $Record.Value, $Record.Kind) }
        else { $key.DeleteValue('Path', $false) }
    } finally { $key.Close() }
}
function Test-ArchiveMemberSelection {
    # Exercise the production ZIP extraction block without executing a fixture binary.
    $node = $ast.FindAll({ param($n) $n -is [Management.Automation.Language.TryStatementAst] -and $null -ne $n.Finally -and $n.Finally.Extent.Text.Trim() -ceq '{ $zip.Dispose() }' }, $true)[0]
    $extract = [scriptblock]::Create($node.Extent.Text)
    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $asset = $Product.ArchivePrefix
    if ($Product.VersionedArchive) { $asset += '1.2.3' + $Product.Separator }
    $asset += $Product.WindowsX64 + '.zip'
    $root = if ($Product.RootedArchive) { $command + '-1.2.3-windows-x86_64/' } else { '' }
    $cases = @('valid', 'duplicate', 'reparse')
    if ($Product.RootedArchive) { $cases += @('wrong-root', 'flat') }
    foreach ($case in $cases) {
        $folder = Join-Path $work ('archive-member-' + $case)
        $stage = Join-Path $folder 'stage'
        $nextState = Join-Path $stage 'state'
        [void][IO.Directory]::CreateDirectory((Join-Path $nextState 'legal'))
        $archive = Join-Path $folder $asset
        $zip = [IO.Compression.ZipFile]::Open($archive, [IO.Compression.ZipArchiveMode]::Create)
        try {
            foreach ($name in @($command + '.exe') + @($Product.LegalFiles)) {
                $member = $root + $name
                if ($name -ceq ($command + '.exe')) {
                    if ($case -eq 'wrong-root') { $member = 'wrong-root/' + $name }
                    if ($case -eq 'flat') { $member = $name }
                }
                $entry = $zip.CreateEntry($member)
                $stream = [IO.StreamWriter]::new($entry.Open())
                try { $stream.Write('fixture:' + $name) } finally { $stream.Dispose() }
                if ($name -ceq ($command + '.exe')) {
                    if ($case -eq 'duplicate') { [void]$zip.CreateEntry($member) }
                    if ($case -eq 'reparse') { $entry.ExternalAttributes = 0x400 }
                }
            }
        } finally { $zip.Dispose() }
        $zip = [IO.Compression.ZipFile]::OpenRead($archive)
        $failure = ''
        try { & $extract } catch { $failure = $_.Exception.Message }
        if ($case -eq 'valid') {
            Check 'archive extraction accepts exact release members' ($failure -eq '')
            foreach ($name in @($command + '.exe') + @($Product.LegalFiles)) {
                $path = if ($name -ceq ($command + '.exe')) { Join-Path $stage $name } else { Join-Path (Join-Path $nextState 'legal') $name }
                Check ('extracted bytes match release member ' + $name) ((Test-Path -LiteralPath $path) -and [IO.File]::ReadAllText($path) -ceq ('fixture:' + $name))
            }
        } else {
            $cause = if ($case -eq 'reparse') { 'Archive member is not regular' } else { 'Archive has no unique regular' }
            Check ("$case member rejects for the intended cause before activation") ($failure.Contains($cause) -and -not (Test-Path -LiteralPath (Join-Path $stage ($command + '.exe'))))
        }
    }
}

function Test-UninstallPathFailure {
    # Execute the staged production uninstall branch and file/PATH helpers.
    # Only registry access and the OS mutex are replaced; user settings are untouched.
    foreach ($name in @('Get-ItemOrNull', 'Assert-Regular', 'Get-Digest', 'Test-SamePath', 'Invoke-OwnedPathRemoval', 'Write-Receipt', 'Read-Receipt', 'Invoke-StateRemoval', 'Invoke-RetiredRemoval', 'Write-Step', 'Get-AbsoluteDirectory', 'ConvertTo-QuotedCommand')) {
        $node = $ast.FindAll({ param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq $name }, $true)[0]
        . ([scriptblock]::Create($node.Extent.Text))
    }
    $node = $ast.FindAll({ param($n) $n -is [Management.Automation.Language.IfStatementAst] -and $n.Clauses[0].Item1.Extent.Text -ceq '$Uninstall' -and $n.Clauses[0].Item2.Extent.Text.Contains('Invoke-StateRemoval') }, $true)[0]
    $body = $node.Clauses[0].Item2.Extent.Text
    $uninstallBranch = [scriptblock]::Create($body.Substring(1, $body.Length - 2))
    function Enter-PathLock { return $null }
    function Get-UserPath { return $script:fixturePath.Clone() }
    function Save-UserPath($record) {
        if ($script:denyPathWrite) { throw 'fixture registry write denied' }
        $script:fixturePath = $record
    }
    # Production AST helpers read these variables from this dynamic scope.
    Set-Variable -Name utf8 -Value ([Text.UTF8Encoding]::new($false))
    $sessionPath = $env:Path
    try {
        foreach ($case in @('failure', 'shared', 'no-modify', 'changed-path', 'changed-kind', 'locked')) {
            if ($case -eq 'locked' -and $PSVersionTable.PSEdition -eq 'Core' -and -not $IsWindows) { continue }
            $InstallDir = Join-Path $work ('path-uninstall-' + $case)
            $exe = Join-Path $InstallDir ($Product.Command + '.exe')
            $stateDir = Join-Path $InstallDir ('.' + $Product.Command + '-install')
            $lockPath = Join-Path $InstallDir ('.' + $Product.Command + '-install.lock')
            [void][IO.Directory]::CreateDirectory($stateDir)
            [IO.File]::WriteAllText($exe, 'owned executable fixture')
            [IO.File]::WriteAllText($lockPath, 'owned lock fixture')
            $receipt = @{ Schema = 1; Directory = $InstallDir; Command = $Product.Command; Version = '1.2.3'; BinaryHash = Get-Digest $exe; DirectoryCreated = $true; PathEntry = $InstallDir; PathWasAbsent = $false; Legal = @(); Retired = $null }
            $receipt.PathWasAbsent = $case -eq 'changed-kind'
            foreach ($name in @($Product.LegalFiles)) {
                $legalDir = Join-Path $stateDir 'legal'
                [void][IO.Directory]::CreateDirectory($legalDir)
                $path = Join-Path $legalDir $name
                [IO.File]::WriteAllText($path, 'owned legal fixture')
                $receipt.Legal += @{ Name = $name; Hash = Get-Digest $path }
            }
            $retired = Join-Path $InstallDir ('.' + $Product.Command + '-retired-' + ('a' * 32) + '.exe')
            [IO.File]::WriteAllText($retired, 'owned retired fixture')
            $receipt.Retired = @{ Name = [IO.Path]::GetFileName($retired); Hash = Get-Digest $retired }
            Write-Receipt $stateDir $receipt
            $receiptFile = Join-Path $stateDir 'receipt.json'
            $sealFile = Join-Path $stateDir 'receipt.sha256'
            $before = @((Get-Digest $exe), (Get-Digest $receiptFile), (Get-Digest $sealFile))
            Set-Variable -Name ModifyPath -Value ($case -ne 'no-modify')
            $raw = if ($case -eq 'changed-path') { 'C:\Keep;;' } elseif ($case -eq 'changed-kind') { $InstallDir } else { $InstallDir + ';C:\Keep;;' }
            $env:Path = $raw
            $script:fixturePath = @{ Exists = $true; Value = $raw; Kind = [Microsoft.Win32.RegistryValueKind]::String }
            $script:denyPathWrite = $case -ne 'locked' -and $case -ne 'changed-kind'
            if ($case -eq 'shared') { [IO.File]::WriteAllText((Join-Path $InstallDir 'unrelated.old'), 'keep') }
            $held = $null
            if ($case -eq 'locked') { $held = [IO.File]::Open($exe, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read) }
            $failure = ''
            try { & $uninstallBranch } catch { $failure = $_.Exception.Message }
            finally { if ($held) { $held.Dispose() } }
            if ($case -eq 'failure' -or $case -eq 'locked') {
                Check ("$case uninstall reports its cause") ($failure -match $(if ($case -eq 'failure') { 'fixture registry write denied' } else { '.' }))
                $intact = (Test-Path -LiteralPath $exe) -and (Test-Path -LiteralPath $receiptFile) -and (Test-Path -LiteralPath $sealFile)
                if ($intact) { $intact = (Get-Digest $exe) -ceq $before[0] -and (Get-Digest $receiptFile) -ceq $before[1] -and (Get-Digest $sealFile) -ceq $before[2] }
                Check ("$case uninstall preserves executable and both ownership files") $intact
                Check ("$case uninstall leaves the expected persistent PATH") ($script:fixturePath.Value -ceq $(if ($case -eq 'failure') { $raw } else { 'C:\Keep;;' }))
                # A failed preservation assertion must not cause an unrelated missing-file error.
                if ($intact) {
                    if ($PSVersionTable.PSEdition -ne 'Core' -or $IsWindows) {
                        $verified = $null
                        try { $verified = Read-Receipt $true } catch { Write-Host ("Receipt verification failed: " + $_.Exception.Message) }
                        Check ("$case uninstall leaves a receipt accepted for retry") ($null -ne $verified)
                    }
                    $script:denyPathWrite = $false
                    $failure = ''
                    try { & $uninstallBranch } catch { $failure = $_.Exception.Message }
                    Check ("$case uninstall retry removes managed files") ($failure -eq '' -and -not (Test-Path -LiteralPath $exe) -and -not (Test-Path -LiteralPath $stateDir))
                    Check ("$case uninstall retry removes only its PATH entry") ($script:fixturePath.Value -ceq 'C:\Keep;;')
                }
            } else {
                Check ("$case uninstall removes managed files") ($failure -eq '' -and -not (Test-Path -LiteralPath $exe) -and -not (Test-Path -LiteralPath $stateDir))
                if ($case -eq 'changed-kind') {
                    Check 'uninstall preserves a user-edited empty String PATH container' ($script:fixturePath.Exists -and $script:fixturePath.Value -ceq '' -and $script:fixturePath.Kind -eq [Microsoft.Win32.RegistryValueKind]::String)
                } else { Check ("$case uninstall preserves PATH") ($script:fixturePath.Value -ceq $raw -and $env:Path -ceq $raw) }
                if ($case -eq 'shared') { Check 'shared uninstall preserves unrelated file' ([IO.File]::ReadAllText((Join-Path $InstallDir 'unrelated.old')) -ceq 'keep') }
            }
            $receipt = $null
            $failure = ''
            try { & $uninstallBranch } catch { $failure = $_.Exception.Message }
            Check ("$case repeat without receipt is a no-op") ($failure -eq '')
        }
    } finally {
        $env:Path = $sessionPath
        Remove-Variable -Scope Script -Name fixturePath, denyPathWrite -ErrorAction SilentlyContinue
    }
}

function Test-ActivationRecovery([string]$Group = 'configured', [string[]]$Legal = $Product.LegalFiles) {
    $Product = $Product.Clone()
    $Product.LegalFiles = @($Legal)
    # Execute the production activation transaction with real files. Only PATH
    # persistence is mocked; injected failures occur at an actual write boundary.
    foreach ($name in @('Get-ItemOrNull', 'Assert-Regular', 'Get-Digest', 'Test-SamePath', 'Get-AbsoluteDirectory', 'ConvertTo-QuotedCommand', 'Write-Receipt', 'Read-Receipt', 'Get-InstallationFingerprint', 'Invoke-StateRemoval', 'Invoke-RetiredRemoval', 'Restore-Activation', 'Write-Step')) {
        $node = $ast.FindAll({ param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq $name }, $true)[0]
        . ([scriptblock]::Create($node.Extent.Text))
    }
    $writer = ${function:Write-Receipt}
    $remover = ${function:Invoke-StateRemoval}
    $node = $ast.FindAll({ param($n) $n -is [Management.Automation.Language.TryStatementAst] -and $n.CatchClauses.Count -gt 0 -and $n.Body.Extent.Text.Contains('Write-Receipt $nextState $newReceipt') }, $true)[-1]
    $outer = $ast.FindAll({ param($n) $n -is [Management.Automation.Language.TryStatementAst] -and $null -ne $n.Finally -and $n.Finally.Extent.Text.Contains('$pathMutex.ReleaseMutex()') }, $true)[0]
    $activateScript = [scriptblock]::Create('try {' + $node.Extent.Text + '} finally ' + $outer.Finally.Extent.Text)
    Set-Variable -Name utf8 -Value ([Text.UTF8Encoding]::new($false))
    foreach ($case in @('unmanaged-restore', 'missing-restore', 'changed-after-preflight', 'unmanaged-restore-failure')) {
        $InstallDir = Join-Path $work ($Group + '-' + $case)
        $exe = Join-Path $InstallDir ($Product.Command + '.exe')
        $stateDir = Join-Path $InstallDir ('.' + $Product.Command + '-install')
        $stage = Join-Path $InstallDir 'fixture-stage'
        $nextState = Join-Path $stage 'state'
        $candidate = Join-Path $stage ($Product.Command + '.exe')
        [void][IO.Directory]::CreateDirectory($nextState)
        [IO.File]::WriteAllText($candidate, 'candidate fixture')
        $unmanaged = $case -ne 'missing-restore'
        Set-Variable -Name ReplaceUnmanaged -Value $true
        Set-Variable -Name Uninstall -Value $false
        $receipt = $null
        Set-Variable -Name ModifyPath -Value $false
        Set-Variable -Name hadBinary -Value $unmanaged
        Set-Variable -Name retired -Value $null
        $activation = $null
        Set-Variable -Name backupPath -Value $(if ($unmanaged) { Join-Path $work ('.' + $Product.Command + '-unmanaged-' + [guid]::NewGuid().ToString('N') + '.exe') } else { $null })
        Set-Variable -Name pathMutex -Value $null
        Set-Variable -Name tmp -Value $null
        Set-Variable -Name lock -Value $null
        Set-Variable -Name created -Value $false
        Set-Variable -Name pathRoot -Value ([IO.Path]::GetPathRoot($InstallDir))
        Set-Variable -Name preserveStage -Value $false
        $r = @{ Schema = 1; Directory = $InstallDir; Command = $Product.Command; Version = '1.2.3'; BinaryHash = Get-Digest $candidate; DirectoryCreated = $false; PathEntry = $null; PathWasAbsent = $false; Legal = @(); Retired = $null }
        if ($unmanaged) { [IO.File]::WriteAllText($exe, 'unowned original') }
        else {
            [void][IO.Directory]::CreateDirectory($stateDir)
            foreach ($name in @($Product.LegalFiles)) {
                $legalDir = Join-Path $stateDir 'legal'
                [void][IO.Directory]::CreateDirectory($legalDir)
                [IO.File]::WriteAllText((Join-Path $legalDir $name), 'legal fixture')
                $r.Legal += @{ Name = $name; Hash = Get-Digest (Join-Path $legalDir $name) }
            }
            & $writer $stateDir $r
            $receipt = Read-Receipt $true
        }
        $preflight = Get-InstallationFingerprint
        $nextReceipt = $r.Clone()
        $nextReceipt.Legal = @()
        foreach ($name in @($Product.LegalFiles)) {
            $legalDir = Join-Path $nextState 'legal'
            [void][IO.Directory]::CreateDirectory($legalDir)
            [IO.File]::WriteAllText((Join-Path $legalDir $name), 'next legal fixture')
            $nextReceipt.Legal += @{ Name = $name; Hash = Get-Digest (Join-Path $legalDir $name) }
        }
        Set-Variable -Name newReceipt -Value $nextReceipt
        # Deleting the candidate forces activation failure after old-file/state
        # moves, rather than failing before the restoration path is exercised.
        function Write-Receipt([string]$Directory, [hashtable]$Receipt) {
            & $writer $Directory $Receipt
            Remove-Item -LiteralPath $candidate
        }
        function Invoke-StateRemoval([string]$Directory, [object[]]$Legal) {
            & $remover $Directory $Legal
            # The actual new-state removal succeeds, then a concurrent directory
            # blocks the subsequent exclusive restoration of the unmanaged file.
            if ($case -eq 'unmanaged-restore-failure') { [void][IO.Directory]::CreateDirectory($exe) }
        }
        if ($case -eq 'changed-after-preflight') { [IO.File]::WriteAllText($exe, 'concurrent replacement') }
        $failure = ''
        try { . $activateScript } catch { $failure = $_.Exception.Message }
        if ($case -eq 'changed-after-preflight') {
            Check 'locked revalidation refuses changed receiptless target' ($failure.Contains('changed after preflight') -and [IO.File]::ReadAllText($exe) -ceq 'concurrent replacement' -and -not (Test-Path -LiteralPath $stateDir))
        } elseif ($case -eq 'unmanaged-restore-failure') {
            $recoveryFile = Join-Path $stage 'recovery.txt'
            $recoveryText = if (Test-Path -LiteralPath $recoveryFile) { [IO.File]::ReadAllText($recoveryFile) } else { '' }
            Check 'failed unmanaged restore preserves transaction stage' ($preserveStage -and (Test-Path -LiteralPath $stage) -and ($failure -match 'restoration is incomplete'))
            Check 'failed unmanaged restore names exact retained original backup' ($activation -and (Test-Path -LiteralPath $activation.OldBinarySlot) -and [IO.File]::ReadAllText($activation.OldBinarySlot) -ceq 'unowned original' -and $recoveryText.Contains($activation.OldBinarySlot))
            Check 'failed restore retains concurrent directory and no active receipt' ((Get-Item -LiteralPath $exe).PSIsContainer -and -not (Test-Path -LiteralPath $stateDir))
        } elseif ($unmanaged) {
            Check 'activation failure restores original unmanaged bytes' ($failure.Contains("this transaction's changes were rolled back") -and [IO.File]::ReadAllText($exe) -ceq 'unowned original' -and -not (Test-Path -LiteralPath $stateDir) -and -not (Test-Path -LiteralPath $activation.OldBinarySlot))
        } else {
            Check 'repair activation rollback preserves prior payload absence' ($failure.Contains("this transaction's changes were rolled back") -and -not (Test-Path -LiteralPath $exe) -and (Get-InstallationFingerprint) -ceq $preflight)
        }
    }
}

function Test-LegalInventory {
    # Receipt ownership is the actual sealed set; current settings define NEW
    # package requirements. Exercise the staged reader and producer statements.
    foreach ($name in @('Get-ItemOrNull', 'Assert-Regular', 'Get-Digest', 'Test-SamePath', 'Get-AbsoluteDirectory', 'ConvertTo-QuotedCommand', 'Write-Receipt', 'Read-Receipt', 'Invoke-StateRemoval')) {
        $node = $ast.FindAll({ param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq $name }, $true)[0]
        . ([scriptblock]::Create($node.Extent.Text))
    }
    $Product = $Product.Clone()
    $Product.LegalFiles = @('LICENSE', 'EXTRA-NOTICE')
    Set-Variable -Name Uninstall -Value $false
    Set-Variable -Name ReplaceUnmanaged -Value $false
    Set-Variable -Name utf8 -Value ([Text.UTF8Encoding]::new($false))
    $InstallDir = Join-Path $work 'legal-inventory'
    $exe = Join-Path $InstallDir ($Product.Command + '.exe')
    $stateDir = Join-Path $InstallDir ('.' + $Product.Command + '-install')
    $legalDir = Join-Path $stateDir 'legal'
    [void][IO.Directory]::CreateDirectory($legalDir)
    [IO.File]::WriteAllText($exe, 'owned executable')
    $licenseFile = Join-Path $legalDir 'LICENSE'
    [IO.File]::WriteAllText($licenseFile, 'original owned license')
    $r = @{ Schema = 1; Directory = $InstallDir; Command = $Product.Command; Version = '1.2.3'; BinaryHash = Get-Digest $exe; DirectoryCreated = $false; PathEntry = $null; PathWasAbsent = $false; Legal = @(@{ Name = 'LICENSE'; Hash = Get-Digest $licenseFile }); Retired = $null }
    Write-Receipt $stateDir $r
    $verified = Read-Receipt $true
    Check 'expanded legal requirements accept smaller verified receipt inventory' ($verified.Legal.Count -eq 1 -and $verified.Legal[0].Name -ceq 'LICENSE')
    $missingExpected = Join-Path $InstallDir 'missing-staged-ownership'
    $failure = ''
    try { [void](Read-Receipt $true $missingExpected $exe ('a' * 64)) } catch { $failure = $_.Exception.Message }
    Check 'missing expected ownership names its exact state without suggesting unmanaged replacement' ($failure.Contains('Expected owned installer state is missing') -and $failure.Contains($missingExpected) -and -not $failure.Contains('ReplaceUnmanaged') -and [IO.File]::ReadAllText($exe) -ceq 'owned executable')
    foreach ($defect in @('duplicate', 'unallowed', 'missing', 'changed', 'extra')) {
        [IO.File]::WriteAllText($licenseFile, 'original owned license')
        $bad = $r.Clone()
        if ($defect -eq 'duplicate') { $bad.Legal = @($r.Legal[0], $r.Legal[0]) }
        if ($defect -eq 'unallowed') { $bad.Legal = @(@{ Name = 'UNALLOWED'; Hash = $r.Legal[0].Hash }) }
        Write-Receipt $stateDir $bad
        if ($defect -eq 'missing') { Remove-Item -LiteralPath $licenseFile }
        if ($defect -eq 'changed') { [IO.File]::WriteAllText($licenseFile, 'changed owned license') }
        $extra = Join-Path $legalDir 'UNOWNED-NOTE'
        if ($defect -eq 'extra') { [IO.File]::WriteAllText($extra, 'user note') }
        $receiptBefore = Get-Digest (Join-Path $stateDir 'receipt.json')
        $binaryBefore = Get-Digest $exe
        $failure = ''
        try { [void](Read-Receipt $true) } catch { $failure = $_.Exception.Message }
        Check ("$defect legal inventory refuses without changing binary or receipt") ($failure.Length -gt 0 -and (Get-Digest $exe) -ceq $binaryBefore -and (Get-Digest (Join-Path $stateDir 'receipt.json')) -ceq $receiptBefore)
        if ($defect -eq 'extra') {
            Check 'unowned legal sidecar survives refusal' ([IO.File]::ReadAllText($extra) -ceq 'user note')
            Remove-Item -LiteralPath $extra
        }
    }
    [IO.File]::WriteAllText($licenseFile, 'original owned license')
    Write-Receipt $stateDir $r
    $verified = Read-Receipt $true
    Invoke-StateRemoval $stateDir $verified.Legal
    Check 'expanded settings remove only actual smaller receipt inventory' (-not (Test-Path -LiteralPath $stateDir) -and [IO.File]::ReadAllText($exe) -ceq 'owned executable')

    $nextState = Join-Path $InstallDir 'new-state'
    [void][IO.Directory]::CreateDirectory((Join-Path $nextState 'legal'))
    [IO.File]::WriteAllText((Join-Path (Join-Path $nextState 'legal') 'LICENSE'), 'new license')
    [IO.File]::WriteAllText((Join-Path (Join-Path $nextState 'legal') 'EXTRA-NOTICE'), 'new notice')
    Set-Variable -Name candidate -Value $exe
    Set-Variable -Name newVersion -Value '1.2.4'
    Set-Variable -Name created -Value $false
    $node = $ast.FindAll({ param($n) $n -is [Management.Automation.Language.AssignmentStatementAst] -and $n.Left.Extent.Text -ceq '$newReceipt' }, $true)[0]
    . ([scriptblock]::Create($node.Extent.Text))
    $node = $ast.FindAll({ param($n) $n -is [Management.Automation.Language.ForEachStatementAst] -and $n.Body.Extent.Text.Contains('$newReceipt.Legal +=') }, $true)[0]
    . ([scriptblock]::Create($node.Extent.Text))
    Check 'new receipt producer records every current legal requirement' ($newReceipt.Legal.Count -eq 2 -and $newReceipt.Legal[0].Name -ceq 'LICENSE' -and $newReceipt.Legal[1].Name -ceq 'EXTRA-NOTICE' -and $newReceipt.Legal[0].Hash -ceq (Get-Digest (Join-Path (Join-Path $nextState 'legal') 'LICENSE')) -and $newReceipt.Legal[1].Hash -ceq (Get-Digest (Join-Path (Join-Path $nextState 'legal') 'EXTRA-NOTICE')))
    Write-Receipt $nextState $newReceipt
    $stateDir = $nextState
    $verified = Read-Receipt $true
    Check 'new expanded legal inventory is accepted by the actual receipt reader' ($verified.Legal.Count -eq 2)
}

function Test-PipelineCancellation {
    # Stop the actual transaction at debugger sequence points after real file or
    # PATH writes. This is pipeline cancellation, not process termination.
    if (-not ('CliInstaller.CancellationProbe' -as [type])) {
        Add-Type @'
using System.Threading;
namespace CliInstaller {
    public static class CancellationProbe {
        public static ManualResetEvent Ready = new ManualResetEvent(false);
        public static ManualResetEvent Resume = new ManualResetEvent(false);
    }
}
'@
    }
    $names = @('Get-ItemOrNull', 'Assert-Regular', 'Get-Digest', 'Test-SamePath', 'Get-AbsoluteDirectory', 'ConvertTo-QuotedCommand', 'Write-Receipt', 'Read-Receipt', 'Get-InstallationFingerprint', 'Invoke-StateRemoval', 'Invoke-RetiredRemoval', 'Restore-Activation', 'Exit-Installer', 'Write-Step', 'Test-PathEntry')
    $nativeRegistry = ($PSVersionTable.PSEdition -ne 'Core' -or $IsWindows) -and [bool]$env:CI
    if ($nativeRegistry) { $names += @('Get-UserPath', 'Save-UserPath', 'Enter-PathLock') }
    $definitions = foreach ($name in $names) {
        $wanted = $name
        $node = $ast.FindAll({ param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq $wanted }, $true)[0]
        . ([scriptblock]::Create($node.Extent.Text))
        $node.Extent.Text
    }
    $inputs = foreach ($name in @('Product', 'stable', 'identifier', 'semver')) {
        $wanted = '$' + $name
        $node = $ast.FindAll({ param($n) $n -is [Management.Automation.Language.AssignmentStatementAst] -and $n.Left.Extent.Text -ceq $wanted }, $true)[0]
        $node.Extent.Text
    }
    $node = $ast.FindAll({ param($n) $n -is [Management.Automation.Language.TryStatementAst] -and $n.CatchClauses.Count -gt 0 -and $n.Body.Extent.Text.Contains('Write-Receipt $nextState $newReceipt') }, $true)[-1]
    $outer = $ast.FindAll({ param($n) $n -is [Management.Automation.Language.TryStatementAst] -and $null -ne $n.Finally -and $n.Finally.Extent.Text.Contains('$pathMutex.ReleaseMutex()') }, $true)[0]
    $scriptText = [IO.File]::ReadAllText($installer)
    $afterActivation = $scriptText.Substring($node.Extent.EndOffset, $outer.Finally.Extent.StartOffset - $node.Extent.EndOffset)
    # The substring ends with the outer try's closing brace and finally keyword.
    $afterActivation = $afterActivation.Substring(0, $afterActivation.LastIndexOf('}'))
    $transaction = "try {`n" + $node.Extent.Text + $afterActivation + "} finally " + $outer.Finally.Extent.Text
    $cases = @()
    foreach ($kind in @('managed', 'missing', 'fresh', 'unmanaged')) {
        $phases = @('path', 'new-state', 'new-binary')
        if ($kind -eq 'managed') { $phases += @('old-binary', 'old-state', 'validated') }
        if ($kind -eq 'missing') { $phases += 'old-state' }
        if ($kind -eq 'unmanaged') { $phases += 'unmanaged-binary' }
        foreach ($phase in $phases) { $cases += @{ Kind = $kind; Phase = $phase; Stop = $true } }
        $cases += @{ Kind = $kind; Phase = 'new-binary'; Stop = $false }
    }
    $cases += @{ Kind = 'managed'; Phase = 'old-binary'; Stop = $true; Conflict = $true }
    $cases += @{ Kind = 'managed'; Phase = 'path'; Stop = $true; AmbiguousPath = $true }
    $cases += @{ Kind = 'managed'; Phase = 'path'; Stop = $true; AppendPath = $true }
    $cases += @{ Kind = 'fresh'; Phase = 'path'; Stop = $true; PathKindEdit = $true }
    $savedRegistry = if ($nativeRegistry) { Get-RegistryPath } else { $null }
    $savedSessionPath = $env:Path
    try {
        foreach ($case in $cases) {
            [void][CliInstaller.CancellationProbe]::Ready.Reset()
            [void][CliInstaller.CancellationProbe]::Resume.Reset()
            $folder = Join-Path $work ('pipeline-' + [guid]::NewGuid().ToString('N'))
            [void][IO.Directory]::CreateDirectory($folder)
            $InstallDir = Join-Path $folder 'bin'
            $exe = Join-Path $InstallDir ($Product.Command + '.exe')
            $stateDir = Join-Path $InstallDir ('.' + $Product.Command + '-install')
            $pathBefore = @{ Exists = $true; Value = 'C:\Keep;;'; Kind = [Microsoft.Win32.RegistryValueKind]::String }
            if ($case.Kind -eq 'fresh') { $pathBefore.Exists = $false; $pathBefore.Value = ''; $pathBefore.Kind = [Microsoft.Win32.RegistryValueKind]::ExpandString }
            if ($nativeRegistry) { Save-RegistryPath $pathBefore }
            else { [IO.File]::WriteAllText((Join-Path $folder 'path.json'), ($pathBefore | ConvertTo-Json)) }
            $quotedFolder = "'" + $folder.Replace("'", "''") + "'"
            $prefix = "`$ErrorActionPreference='Stop'`nSet-StrictMode -Version 3`n" + ($inputs -join "`n") + "`n" + ($definitions -join "`n") + "`n"
            $prefix += '$fixtureFolder=' + $quotedFolder + "`n" + '$kind=' + "'" + $case.Kind + "'`n"
            if (-not $nativeRegistry) {
                $prefix += @'
function Enter-PathLock { return $null }
function Get-UserPath {
    $r = [IO.File]::ReadAllText((Join-Path $fixtureFolder 'path.json')) | ConvertFrom-Json
    return @{ Exists = [bool]$r.Exists; Value = [string]$r.Value; Kind = [Microsoft.Win32.RegistryValueKind][int]$r.Kind }
}
function Save-UserPath([hashtable]$Record) { [IO.File]::WriteAllText((Join-Path $fixtureFolder 'path.json'), ($Record | ConvertTo-Json)) }
'@
            }
            $prefix += @'
$utf8=[Text.UTF8Encoding]::new($false)
$env:Path='C:\Session'
$InstallDir=Join-Path $fixtureFolder 'bin'
$exe=Join-Path $InstallDir ($Product.Command+'.exe')
$stateDir=Join-Path $InstallDir ('.'+$Product.Command+'-install')
$stage=Join-Path $InstallDir ('.'+$Product.Command+'-stage-fixture')
$nextState=Join-Path $stage 'state'
$candidate=Join-Path $stage ($Product.Command+'.exe')
[void][IO.Directory]::CreateDirectory($nextState)
[IO.File]::WriteAllText($candidate,'new candidate bytes')
$receipt=$null
if($kind -ne 'fresh' -and $kind -ne 'missing') { [IO.File]::WriteAllText($exe,'original executable bytes') }
if($kind -eq 'managed' -or $kind -eq 'missing') {
    [void][IO.Directory]::CreateDirectory($stateDir)
    $receipt=@{Schema=1;Directory=$InstallDir;Command=$Product.Command;Version='1.2.3';BinaryHash='aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';DirectoryCreated=$false;PathEntry=$null;PathWasAbsent=$false;Legal=@();Retired=$null}
    if($kind -eq 'managed') { $receipt.BinaryHash=Get-Digest $exe }
    foreach($name in @($Product.LegalFiles)) {
        [void][IO.Directory]::CreateDirectory((Join-Path $stateDir 'legal'))
        $file=Join-Path (Join-Path $stateDir 'legal') $name
        [IO.File]::WriteAllText($file,('original legal '+$name))
        $receipt.Legal+=@{Name=$name;Hash=Get-Digest $file}
    }
    Write-Receipt $stateDir $receipt
}
foreach($name in @($Product.LegalFiles)) {
    [void][IO.Directory]::CreateDirectory((Join-Path $nextState 'legal'))
    [IO.File]::WriteAllText((Join-Path (Join-Path $nextState 'legal') $name),('new legal '+$name))
}
$before=@{}
if(Test-Path -LiteralPath $exe) { $before[$Product.Command+'.exe']=Get-Digest $exe }
if(Test-Path -LiteralPath $stateDir) { foreach($f in Get-ChildItem -LiteralPath $stateDir -File -Force -Recurse) { $before[$f.FullName.Substring($InstallDir.Length+1)]=Get-Digest $f.FullName } }
[IO.File]::WriteAllText((Join-Path $fixtureFolder 'before.json'),($before|ConvertTo-Json))
$ReplaceUnmanaged=$true;$Uninstall=$false;$ModifyPath=$true
$preflight=Get-InstallationFingerprint
$newVersion='1.2.4';$created=$kind -eq 'fresh'
$newReceipt=@{Schema=1;Directory=$InstallDir;Command=$Product.Command;Version=$newVersion;BinaryHash=Get-Digest $candidate;DirectoryCreated=$created;PathEntry=$null;PathWasAbsent=$false;Legal=@();Retired=$null}
foreach($name in @($Product.LegalFiles)) { $newReceipt.Legal+=@{Name=$name;Hash=Get-Digest (Join-Path (Join-Path $nextState 'legal') $name)} }
$retired=$null;$hadBinary=Test-Path -LiteralPath $exe
if($receipt -and $hadBinary) {
    $retired=Join-Path $InstallDir ('.'+$Product.Command+'-retired-'+[guid]::NewGuid().ToString('N')+'.exe')
    $newReceipt.Retired=@{Name=[IO.Path]::GetFileName($retired);Hash=$receipt.BinaryHash}
}
$unmanaged=$kind -eq 'unmanaged'
$backupPath=if($unmanaged){Join-Path $fixtureFolder ('.'+$Product.Command+'-unmanaged-'+[guid]::NewGuid().ToString('N')+'.exe')}else{$retired}
$activation=$null;$preserveStage=$false;$pathMutex=$null;$tmp=$null;$lock=$null;$pathRoot=[IO.Path]::GetPathRoot($InstallDir)
'@
            $body = $prefix + "`n" + $transaction
            $childFile = Join-Path $folder 'transaction.ps1'
            [IO.File]::WriteAllText($childFile, $body, [Text.UTF8Encoding]::new($true))
            $lines = [IO.File]::ReadAllLines($childFile)
            $needle = switch ($case.Phase) {
                'path' { 'if ($pathExpected) { Save-UserPath $pathExpected }' }
                'old-binary' { '[IO.Directory]::Move($stateDir, $activation.PreviousState)' }
                'old-state' { 'if ($unmanaged) { [IO.File]::Move($exe, $backupPath) }' }
                'unmanaged-binary' { '[IO.Directory]::Move($nextState, $stateDir)' }
                'new-state' { '[IO.File]::Move($candidate, $exe)' }
                'new-binary' { '[void](Read-Receipt $false $stateDir $exe $activation.NewReceiptHash)' }
                'validated' { '$activation.Committed = $true' }
            }
            $line = 0
            for ($i = 0; $i -lt $lines.Length; $i++) { if ($lines[$i].Trim() -ceq $needle) { $line = $i + 1; break } }
            # PATH stops at the following statement, after the real save returns.
            if ($case.Phase -eq 'path') { $line++ }
            if ($line -le 0) { throw "Missing cancellation sequence point: $needle" }
            $ps = [powershell]::Create()
            $quotedChild = "'" + $childFile.Replace("'", "''") + "'"
            [void]$ps.AddScript(('Set-PSBreakpoint -Script ' + $quotedChild + ' -Line ' + $line + ' -Action { [void][CliInstaller.CancellationProbe]::Ready.Set(); [void][CliInstaller.CancellationProbe]::Resume.WaitOne() } | Out-Null; & ' + $quotedChild))
            try {
                $running = $ps.BeginInvoke()
                if (-not [CliInstaller.CancellationProbe]::Ready.WaitOne(10000)) { throw "Cancellation boundary was not reached: $($case.Kind)/$($case.Phase): $($ps.Streams.Error | Out-String)" }
                $editedPath = $null
                if ($case.ContainsKey('Conflict')) {
                    [void][IO.Directory]::CreateDirectory($exe)
                    [IO.File]::WriteAllText((Join-Path $exe 'keep.txt'), 'concurrent user content')
                }
                if ($case.ContainsKey('AmbiguousPath') -or $case.ContainsKey('AppendPath') -or $case.ContainsKey('PathKindEdit')) {
                    $value = $InstallDir + $(if ($pathBefore.Exists) { ';' + $pathBefore.Value } else { '' })
                    if ($case.ContainsKey('AppendPath')) { $value += ';C:\Added' }
                    if ($case.ContainsKey('AmbiguousPath')) { $value = 'C:\User;' + $value }
                    $kind = if ($case.ContainsKey('PathKindEdit')) { [Microsoft.Win32.RegistryValueKind]::String } else { [Microsoft.Win32.RegistryValueKind]::ExpandString }
                    $editedPath = @{ Exists = $true; Value = $value; Kind = $kind }
                    if ($nativeRegistry) { Save-RegistryPath $editedPath }
                    else { [IO.File]::WriteAllText((Join-Path $folder 'path.json'), ($editedPath | ConvertTo-Json)) }
                }
                if ($case.Stop) {
                    $stopping = $ps.BeginStop($null, $null)
                    Start-Sleep -Milliseconds 50
                    [void][CliInstaller.CancellationProbe]::Resume.Set()
                    $ps.EndStop($stopping)
                } else { [void][CliInstaller.CancellationProbe]::Resume.Set() }
                $stopped = $false; $invokeFailure = ''
                try { [void]$ps.EndInvoke($running) } catch { $stopped = $ps.InvocationStateInfo.State -eq 'Stopped'; $invokeFailure = $_.Exception.Message }
                $label = "$($case.Kind)/$($case.Phase)/stop=$($case.Stop)"
                if ($case.ContainsKey('Conflict')) { $label += '/concurrent-directory' }
                if ($case.ContainsKey('AmbiguousPath')) { $label += '/reordered-PATH' }
                if ($case.ContainsKey('AppendPath')) { $label += '/appended-PATH' }
                if ($case.ContainsKey('PathKindEdit')) { $label += '/PATH-kind-only' }
                if ($invokeFailure -and -not $stopped) { Write-Host ("Child failure: " + $invokeFailure + "; " + ($ps.Streams.Error | Out-String)) }
                Check ("$label has the expected pipeline outcome") ($stopped -eq $case.Stop -and $ps.Streams.Error.Count -eq 0 -and ($case.Stop -or $ps.InvocationStateInfo.State -eq 'Completed'))
                $committedAtStop = $false
                if ($case.Stop -and $case.Phase -eq 'validated') {
                    # A scalar commit assignment can finish before Stop is
                    # observed. Accept only a fully validated committed outcome.
                    $observed = Read-Receipt $false $stateDir $exe
                    $committedAtStop = $observed.Version -ceq '1.2.4'
                }
                if ($case.Stop -and -not $committedAtStop) {
                    $before = [IO.File]::ReadAllText((Join-Path $folder 'before.json')) | ConvertFrom-Json
                    $after = @{}
                    if (Test-Path -LiteralPath $InstallDir) { foreach ($f in Get-ChildItem -LiteralPath $InstallDir -File -Force -Recurse) { $after[$f.FullName.Substring($InstallDir.Length + 1)] = Get-Digest $f.FullName } }
                    $same = @($before.PSObject.Properties).Count -eq $after.Count
                    foreach ($property in $before.PSObject.Properties) { $same = $same -and $after.ContainsKey($property.Name) -and $after[$property.Name] -ceq $property.Value }
                    if ($case.ContainsKey('Conflict') -or $case.ContainsKey('AmbiguousPath')) {
                        $stagePath = Join-Path $InstallDir ('.' + $Product.Command + '-stage-fixture')
                        $note = [IO.File]::ReadAllText((Join-Path $stagePath 'recovery.txt'))
                        Check "$label retains durable incomplete-restoration facts" ($note -match 'restoration is incomplete')
                        if ($case.ContainsKey('Conflict')) {
                            $backups = @(Get-ChildItem -LiteralPath $InstallDir -File -Force | Where-Object { $_.Name -like ('.' + $Product.Command + '-retired-*') })
                            Check 'failed restore preserves original bytes and concurrent directory' ($backups.Count -eq 1 -and [IO.File]::ReadAllText($backups[0].FullName) -ceq 'original executable bytes' -and [IO.File]::ReadAllText((Join-Path $exe 'keep.txt')) -ceq 'concurrent user content' -and $note.Contains($backups[0].FullName))
                        } else {
                            $preserved = $true
                            foreach ($property in $before.PSObject.Properties) { $preserved = $preserved -and $after.ContainsKey($property.Name) -and $after[$property.Name] -ceq $property.Value }
                            Check 'ambiguous PATH edit preserves every original executable and receipt byte' $preserved
                        }
                    } else { Check ("$label restores exact prior executable and receipt bytes") $same }
                    if ($case.Kind -eq 'fresh') { Check 'cancelled fresh install removes its created empty directory' (-not (Test-Path -LiteralPath $InstallDir)) }
                    $afterPath = if ($nativeRegistry) { Get-RegistryPath } else { [IO.File]::ReadAllText((Join-Path $folder 'path.json')) | ConvertFrom-Json }
                    $expectedPath = if ($case.ContainsKey('AmbiguousPath')) { $editedPath } else { $pathBefore.Clone() }
                    if ($case.ContainsKey('AppendPath')) { $expectedPath.Value += ';C:\Added'; $expectedPath.Kind = [Microsoft.Win32.RegistryValueKind]::ExpandString }
                    if ($case.ContainsKey('PathKindEdit')) { $expectedPath.Exists = $true; $expectedPath.Value = ''; $expectedPath.Kind = [Microsoft.Win32.RegistryValueKind]::String }
                    Check ("$label preserves the required raw PATH value, kind and absence") ($afterPath.Exists -eq $expectedPath.Exists -and $afterPath.Value -ceq $expectedPath.Value -and [int]$afterPath.Kind -eq [int]$expectedPath.Kind)
                } else {
                    $verified = Read-Receipt $false $stateDir $exe
                    Check ("$label commits a fully verifiable new installation") (($committedAtStop -or $ps.InvocationStateInfo.State -eq 'Completed') -and $verified.Version -ceq '1.2.4' -and [IO.File]::ReadAllText($exe) -ceq 'new candidate bytes')
                    $afterPath = if ($nativeRegistry) { Get-RegistryPath } else { [IO.File]::ReadAllText((Join-Path $folder 'path.json')) | ConvertFrom-Json }
                    Check "$label owns its actual saved PATH entry" ($verified.PathEntry -ceq $InstallDir -and $afterPath.Value -ceq ($InstallDir + $(if ($pathBefore.Exists) { ';' + $pathBefore.Value } else { '' })))
                    if ($case.Kind -eq 'unmanaged') {
                        $backups = @(Get-ChildItem -LiteralPath $folder -File -Force | Where-Object { $_.Name -like ('.' + $Product.Command + '-unmanaged-*') })
                        Check 'uninterrupted unmanaged replacement retains exact external backup' ($backups.Count -eq 1 -and [IO.File]::ReadAllText($backups[0].FullName) -ceq 'original executable bytes')
                    }
                }
            } finally {
                [void][CliInstaller.CancellationProbe]::Resume.Set()
                if ($ps.InvocationStateInfo.State -eq 'Running') { $ps.Stop() }
                $ps.Dispose()
                $env:Path = $savedSessionPath
                if ($nativeRegistry) { Save-RegistryPath $savedRegistry }
            }
        }
    } finally {
        $env:Path = $savedSessionPath
        if ($nativeRegistry) { Save-RegistryPath $savedRegistry }
    }
}

function Test-WindowsArchiveName {
    $Product = $Product.Clone()
    foreach ($name in @('Exit-Installer', 'Get-WindowsArchiveSuffix')) {
        $node = $ast.FindAll({ param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq $name }, $true)[0]
        . ([scriptblock]::Create($node.Extent.Text))
    }
    $first = $ast.FindAll({ param($n) $n -is [Management.Automation.Language.AssignmentStatementAst] -and $n.Extent.Text -ceq '$asset = $Product.ArchivePrefix' }, $true)[0]
    $statements = @($first.Parent.Statements)
    $index = [array]::IndexOf($statements, $first)
    $nameBody = ($statements[$index..($index + 2)] | ForEach-Object { $_.Extent.Text }) -join "`n"
    $compose = [scriptblock]::Create('param([string]$Version, [string]$archiveSuffix, [hashtable]$Product)' + "`n" + $nameBody + "`nreturn " + '$asset')
    $archiveSuffix = Get-WindowsArchiveSuffix 'AMD64'
    Check 'native AMD64 selects its declared Windows archive suffix' ($archiveSuffix -ceq $Product.WindowsX64)
    $Product.ArchivePrefix = 'example_'; $Product.VersionedArchive = $false
    $Product.WindowsX64 = 'windows_amd64'
    $archiveSuffix = Get-WindowsArchiveSuffix 'AMD64'
    Check 'unversioned Windows archive naming uses the declared full suffix' ((& $compose -Version '1.2.3' -archiveSuffix $archiveSuffix -Product $Product) -ceq 'example_windows_amd64.zip')
    $Product.ArchivePrefix = 'example-'; $Product.Separator = '-'; $Product.VersionedArchive = $true
    $Product.WindowsX64 = 'x86_64-pc-windows-msvc'
    $archiveSuffix = Get-WindowsArchiveSuffix 'AMD64'
    Check 'versioned Windows archive naming permits architecture-first release triples' ((& $compose -Version '1.2.3' -archiveSuffix $archiveSuffix -Product $Product) -ceq 'example-1.2.3-x86_64-pc-windows-msvc.zip')
    $Product.WindowsArm64 = 'aarch64-pc-windows-msvc'
    Check 'native ARM64 selects its declared full suffix' ((Get-WindowsArchiveSuffix 'ARM64') -ceq 'aarch64-pc-windows-msvc')
    foreach ($case in @('ARM64', 'X86', 'amd64')) {
        $Product.WindowsArm64 = ''
        $failure = ''
        try { [void](Get-WindowsArchiveSuffix $case) } catch { $failure = $_.Exception.Message }
        Check ("unsupported Windows processor is refused: $case") ($failure -ceq "No Windows release for the $case processor.")
    }
}

function Test-UnmanagedGuidance {
    $quote = $ast.FindAll({ param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq 'ConvertTo-QuotedCommand' }, $true)[0]
    . ([scriptblock]::Create($quote.Extent.Text))
    $node = $ast.FindAll({ param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq 'Get-UnmanagedRefusal' }, $true)[0]
    $body = $node.Body.Extent.Text
    # Pass the production helper's enclosing installer inputs explicitly.
    $refusal = [scriptblock]::Create('param([string]$Program, [string]$SavedScriptPath, [string]$Version, [string]$InstallDir, [bool]$ModifyPath, [bool]$Uninstall, [hashtable]$Product)' + "`n" + $body.Substring(1, $body.Length - 2))
    $InstallDir = Join-Path $work "a user's tools"
    $context = @{ Version = '1.2.3'; InstallDir = $InstallDir; ModifyPath = $false; Uninstall = $false; Product = $Product }
    $fixtureScript = Join-Path $work "reviewed installer's script.ps1"
    [IO.File]::WriteAllText($fixtureScript, @'
param([string]$Version, [string]$InstallDir, [switch]$NoModifyPath, [switch]$ReplaceUnmanaged)
[IO.File]::WriteAllText($env:CLI_INSTALLER_UX_OPTIONS, (@{ Version=$Version; Directory=$InstallDir; NoModify=[bool]$NoModifyPath; Replace=[bool]$ReplaceUnmanaged } | ConvertTo-Json))
'@)
    $failure = & $refusal @context -Program (Join-Path $InstallDir ($Product.Command + '.exe')) -SavedScriptPath $fixtureScript
    Check 'receiptless guidance explains ownership and unchanged files' ($failure.Message.Contains('no record of managing that file') -and $failure.Message.Contains('No program or PATH entry was changed'))
    Check 'receiptless guidance links directly to its Help section' ($failure.Data['HelpAnchor'] -ceq '#existing-executable-without-an-installer-receipt')
    $match = Get-RecoveryCommand $failure.Message
    Check 'saved-script guidance contains a complete recovery command' $match.Success
    $crlfMatch = Get-RecoveryCommand ($failure.Message.Replace("`n", "`r`n"))
    Check 'recovery extraction handles Windows line endings without capturing them' ($match.Success -and $crlfMatch.Success -and $crlfMatch.Groups[1].Value -ceq $match.Groups[1].Value)
    Check 'recovery extraction rejects diagnostics without a command' (-not (Get-RecoveryCommand 'No program or PATH entry was changed.').Success)
    $savedOptions = $env:CLI_INSTALLER_UX_OPTIONS
    try {
        $env:CLI_INSTALLER_UX_OPTIONS = Join-Path $work 'recovery-options.json'
        if ($match.Success) { & ([scriptblock]::Create($match.Groups[1].Value)) }
        Check 'copied command launches the saved script successfully' ($match.Success -and $LASTEXITCODE -eq 0 -and (Test-Path -LiteralPath $env:CLI_INSTALLER_UX_OPTIONS))
        if (Test-Path -LiteralPath $env:CLI_INSTALLER_UX_OPTIONS) {
            $actual = [IO.File]::ReadAllText($env:CLI_INSTALLER_UX_OPTIONS) | ConvertFrom-Json
            Check 'copied command preserves release, quoted directory and PATH choice' ($actual.Version -ceq '1.2.3' -and $actual.Directory -ceq $InstallDir -and $actual.NoModify -and $actual.Replace)
        }
    } finally { $env:CLI_INSTALLER_UX_OPTIONS = $savedOptions }
    $failure = & $refusal @context -Program 'existing.exe' -SavedScriptPath ''
    Check 'scriptblock guidance asks to save the installer without inventing a file path' ($failure.Message.Contains('Save and review this installer as a .ps1 file first') -and $failure.Message -notmatch '(?m)^  & ')
    $context.Version = ''; $context.ModifyPath = $true
    $failure = & $refusal @context -Program 'existing.exe' -SavedScriptPath $fixtureScript
    Check 'latest and normal PATH choices do not gain a pin or PATH restriction' ($failure.Message -notmatch ' -Version ' -and $failure.Message -notmatch ' -NoModifyPath')
    $context.Uninstall = $true
    $failure = & $refusal @context -Program 'existing.exe' -SavedScriptPath $fixtureScript
    Check 'unmanaged uninstall offers removal guidance instead of replacement' ($failure.Message.Contains('Use the method that installed this executable') -and $failure.Message -notmatch 'ReplaceUnmanaged|recovery backup| -File ')
}

function Save-Manifest([string]$Directory) {
    $lines = @()
    foreach ($file in Get-ChildItem -LiteralPath $Directory -Filter '*.zip') { $lines += (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant() + '  ' + $file.Name }
    [IO.File]::WriteAllLines((Join-Path $Directory $checksumName), $lines, [Text.UTF8Encoding]::new($false))
}

try {
    # Bootstrap qualification uses the staged published script, never a checkout fallback.
    if (-not (Test-Path -LiteralPath $installer -PathType Leaf)) { throw "Stage the published install.ps1 beside its archives and $checksumName before running this harness." }
    $selected = @([IO.File]::ReadAllLines((Join-Path $Dist $checksumName)) | Where-Object { $_ -cmatch '(?:^|\s)\*?install\.ps1\z' })
    if ($selected.Count -ne 1 -or $selected[0] -cnotmatch '\A([0-9a-fA-F]{64}) [ *]install\.ps1\z') { throw 'No unique valid installer checksum.' }
    if ((Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant() -cne $Matches[1].ToLowerInvariant()) { throw 'Staged installer bytes differ from the release manifest.' }
    $tokens = $null; $errors = $null
    $ast = [Management.Automation.Language.Parser]::ParseFile($installer, [ref]$tokens, [ref]$errors)
    $reviewedInstaller = [scriptblock]::Create([IO.File]::ReadAllText($installer))
    Check 'published script parses' ($errors.Count -eq 0)
    $probeArguments = Get-InstallerOptionMap @('-Version', '1.2.3', '-InstallDir', "C:\a user's tools", '-NoModifyPath')
    $bound = & {
        [CmdletBinding(PositionalBinding = $false)]
        param([string]$Version, [string]$InstallDir, [switch]$NoModifyPath)
        return @{ Version = $Version; Directory = $InstallDir; NoModify = [bool]$NoModifyPath }
    } @probeArguments
    Check 'cached invocation preserves named values and switch binding' ($bound.Version -ceq '1.2.3' -and $bound.Directory -ceq "C:\a user's tools" -and $bound.NoModify)

    $currentShell = [Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
    $preferenceBeforeNative = $ErrorActionPreference
    $exitCode = Invoke-NativeCase $currentShell @('-NoProfile', '-Command', "[Console]::Error.WriteLine('native stderr diagnostic'); exit 17")
    Check 'native stderr retains the real nonzero process exit' ($exitCode -eq 17 -and (Get-Output).Contains('native stderr diagnostic'))
    Check 'native invocation restores strict harness error handling' ($ErrorActionPreference -eq $preferenceBeforeNative)
    $global:LASTEXITCODE = 0
    $startFailureRefused = $false
    try { [void](Invoke-NativeCase -Shell (Join-Path $work 'no-such-shell.exe') -Arguments @()) }
    catch { $startFailureRefused = $true }
    Check 'failed native launch cannot reuse a prior successful exit code' $startFailureRefused


    Check 'unknown arguments fail before platform or filesystem work' ((Invoke-Installer $currentShell @('-Unexpected', '-Help')) -ne 0)
    Check 'positional arguments fail before platform or filesystem work' ((Invoke-Installer $currentShell @('unexpected', '-Help')) -ne 0)
    Check 'help remains available' ((Invoke-Installer $currentShell @('-Help')) -eq 0)
    $reviewedBlock = "& ([scriptblock]::Create([IO.File]::ReadAllText('" + $installer.Replace("'", "''") + "')))"
    foreach ($parameter in @('Version', 'InstallDir')) {
        $exitCode = Invoke-NativeCase $currentShell @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command', ($reviewedBlock + " -$parameter '' -Help"))
        Check ("explicit empty $parameter refuses before defaults") ($exitCode -ne 0)
    }
    if ($errors.Count) { throw ($errors | Out-String) }
    # Exercise the actual version/quoting helpers without running installation
    # or touching any user settings. Only trusted staged definitions are loaded.
    foreach ($name in @('Product', 'stable', 'identifier', 'semver')) {
        $node = $ast.FindAll({ param($n) $n -is [Management.Automation.Language.AssignmentStatementAst] -and $n.Left.Extent.Text -ceq ('$' + $name) }, $true)[0]
        . ([scriptblock]::Create($node.Extent.Text))
    }
    foreach ($name in @('Exit-Installer', 'Get-ProgramVersion', 'ConvertTo-QuotedCommand', 'Get-AbsoluteDirectory')) {
        $node = $ast.FindAll({ param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq $name }, $true)[0]
        . ([scriptblock]::Create($node.Extent.Text))
    }
    function Invoke-VersionFixture {
        $global:LASTEXITCODE = $script:fixtureExit
        if ($script:fixtureError) { Write-Error $script:fixtureError -ErrorAction Continue }
        return $script:fixtureOutput
    }
    foreach ($case in @(
        @{ Text = @('emlattachmentremover 1.2.3 (abc, Go, windows/amd64)'); Code = 0; Version = '1.2.3' },
        @{ Text = @('emlattachmentremover 1.2.3-SNAPSHOT-a1+build.2'); Code = 0; Version = '1.2.3-SNAPSHOT-a1+build.2' },
        @{ Text = @('emlattachmentremover not-a-version'); Code = 0; Version = $null },
        @{ Text = @('emlattachmentremover 1.2.3 extra'); Code = 0; Version = $null },
        @{ Text = @('emlattachmentremover 01.2.3'); Code = 0; Version = $null },
        @{ Text = @('emlattachmentremover 1.2.3-01'); Code = 0; Version = $null },
        @{ Text = @('emlattachmentremover 1.2.3', 'extra line'); Code = 0; Version = $null },
        @{ Text = @('another 1.2.3'); Code = 0; Version = $null },
        @{ Text = @('emlattachmentremover 1.2.3'); Code = 17; Version = $null }
    )) {
        $script:fixtureExit = $case.Code; $script:fixtureOutput = $case.Text; $script:fixtureError = if ($case.Code) { 'native version failure detail' } else { '' }
        $result = $null; $failure = ''
        try { $result = Get-ProgramVersion 'Invoke-VersionFixture' (Join-Path $work 'version-error.txt') }
        catch { $failure = $_.Exception.Message }
        if ($null -eq $case.Version) {
            Check ('version rejects ' + ($case.Text -join ' / ')) ($failure.Length -gt 0)
            if ($case.Code) { Check 'native failure detail is retained' ($failure.Contains('native version failure detail')) }
        } else { Check ('version accepts ' + $case.Version) ($failure -eq '' -and $result -ceq $case.Version) }
    }
    $root = [IO.Path]::GetPathRoot($work)
    Check 'filesystem root remains a root' ((Get-AbsoluteDirectory $root) -ceq ([IO.Path]::GetFullPath($root)))
    foreach ($relative in @('C:relative', '\relative')) {
        $refused = $false
        try { [void](Get-AbsoluteDirectory $relative) } catch { $refused = $true }
        Check ('non-absolute directory refused: ' + $relative) $refused
    }
    Check 'PowerShell command quoting preserves apostrophes' ((ConvertTo-QuotedCommand "C:\user's tools\program.exe") -ceq "'C:\user''s tools\program.exe'")
    Test-WindowsArchiveName
    Test-UnmanagedGuidance
    Test-UninstallPathFailure
    Test-ArchiveMemberSelection
    Test-ActivationRecovery
    Test-ActivationRecovery -Group 'nonempty-legal' -Legal @('LICENSE', 'EXTRA-NOTICE')
    Test-LegalInventory
    Test-PipelineCancellation
    if ($PSVersionTable.PSEdition -eq 'Core' -and -not $IsWindows) {
        Write-Host 'Native Windows integration not run on this host; published script helper checks completed.'
        if ($script:failures -gt 0) { throw "$($script:failures) helper checks failed." }
        return
    }
    $versions = @(Get-ChildItem -LiteralPath $Dist -Filter 'emlattachmentremover-*-windows-*.zip' | ForEach-Object {
        if ($_.Name -cmatch '^emlattachmentremover-([0-9]+\.[0-9]+\.[0-9]+)-windows-(x86_64|aarch64)\.zip$') { $Matches[1] }
    } | Select-Object -Unique)
    if ($versions.Count -ne 1) { throw 'Expected one release version in the native Windows archives.' }
    $testVersion = $versions[0]
    $shells = @('powershell', 'pwsh') | Where-Object { Get-Command $_ -ErrorAction SilentlyContinue }
    if ($env:CI) {
        Check 'CI requires Windows PowerShell and PowerShell Core' (@($shells).Count -eq 2)
        if (@($shells).Count -ne 2) { throw 'Both supported Windows shells are required for CI qualification.' }
    }
    Check 'a native PowerShell is available' (@($shells).Count -gt 0)
    if (-not $RunInCurrentShell) {
        # One worker session per supported PowerShell retains the same lifecycle
        # cases without recompiling the large installer in dozens of new processes.
        foreach ($shell in $shells) {
            Write-Host "=== worker: $shell"
            $exitCode = Invoke-NativeCase $shell @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $PSCommandPath, '-Dist', $Dist, '-RunInCurrentShell') -StreamOutput
            Check ("$shell complete suite") ($exitCode -eq 0)
        }
        if ($script:failures -gt 0) { throw "$($script:failures) installer checks failed." }
        return
    }
    $shells = @($currentShell)
    foreach ($shell in $shells) {
        $shellName = [IO.Path]::GetFileNameWithoutExtension($shell)
        Write-Host "=== $shell"
        Check 'NoModifyPath typo is rejected' ((Invoke-Installer $shell @('-NoModifyPah', '-Help')) -ne 0)
        Check 'unexpected positional argument is rejected' ((Invoke-Installer $shell @('unexpected', '-Help')) -ne 0)
        $dir = Join-Path $work ("$shellName-app's folder")
        $exe = Join-Path $dir ($command + '.exe')
        $state = Join-Path $dir ('.' + $command + '-install')
        $receiptFile = Join-Path $state 'receipt.json'
        $receiptSeal = Join-Path $state 'receipt.sha256'
        Check 'fresh -File install succeeds' ((Invoke-Installer $shell @('-InstallDir', $dir, '-NoModifyPath') -AsFile) -eq 0)
        Check 'executable installed' (Test-Path -LiteralPath $exe)
        if (-not (Test-Path -LiteralPath $exe)) { continue }
        Check 'receipt installed' (Test-Path -LiteralPath $receiptFile)
        Check 'receipt integrity seal installed' (Test-Path -LiteralPath $receiptSeal)
        Check 'installed executable runs' ((& $exe --version) -match '^emlattachmentremover ')
        # Exercise EML's native product contract and retain all legal material.
        $sourceMail = Join-Path $work "$shellName-source.eml"
        [IO.File]::WriteAllBytes($sourceMail, [Text.Encoding]::ASCII.GetBytes("Subject: Installer smoke`r`n`r`nBody retained`r`n"))
        & $exe -- $sourceMail | Out-Null
        $copyMail = Join-Path $work "$shellName-source.no-attachments.eml"
        Check 'installed CLI creates the exact body copy' ($LASTEXITCODE -eq 0 -and (Test-Path -LiteralPath $copyMail) -and [Convert]::ToBase64String([IO.File]::ReadAllBytes($copyMail)) -ceq [Convert]::ToBase64String([IO.File]::ReadAllBytes($sourceMail)))
        foreach ($notice in @($Product.LegalFiles)) { Check ('installed legal file retained: ' + $notice) ((Get-Item -LiteralPath (Join-Path (Join-Path $state 'legal') $notice)).Length -gt 0) }
        Check 'installation reported' ((Get-Output) -match 'is installed')
        Check 'custom uninstall path is quoted' ((Get-Output).Contains("-InstallDir '" + $dir.Replace("'", "''") + "'"))
        Check 'reinstall succeeds' ((Invoke-Installer $shell @('-InstallDir', $dir, '-NoModifyPath')) -eq 0)
        Check 'reinstall reported' ((Get-Output) -match 'reinstalled')
        Remove-Item -LiteralPath $exe
        Check 'intact receipt repairs an absent payload' ((Invoke-Installer $shell @('-InstallDir', $dir, '-NoModifyPath')) -eq 0 -and (Test-Path -LiteralPath $exe))
        $repaired = [IO.File]::ReadAllText($receiptFile) | ConvertFrom-Json
        Check 'missing-payload repair creates no fictitious retired ownership' ($null -eq $repaired.Retired)
        Check 'equivalent dot directory spelling remains manageable' ((Invoke-Installer $shell @('-InstallDir', ($dir + '\.'), '-NoModifyPath')) -eq 0)
        $moved = Join-Path $work "$shellName-copied-install"
        [void][IO.Directory]::CreateDirectory($moved)
        [IO.File]::Copy($exe, (Join-Path $moved ($command + '.exe')))
        Copy-Item -LiteralPath $state -Destination $moved -Recurse
        Check 'copied receipt cannot claim another directory' ((Invoke-Installer $shell @('-InstallDir', $moved, '-Uninstall', '-NoModifyPath')) -ne 0)
        Check 'copied installation and unowned directory remain' ((Test-Path -LiteralPath (Join-Path $moved ($command + '.exe'))) -and (Test-Path -LiteralPath $moved))

        Check 'same named .old is preserved' (-not (Test-Path -LiteralPath "$exe.old"))

        Write-Host '--- update a running executable'
        $start = [Diagnostics.ProcessStartInfo]::new($exe, '--request -')
        $start.UseShellExecute = $false
        $start.RedirectStandardInput = $true; $start.RedirectStandardOutput = $true; $start.RedirectStandardError = $true
        $running = [Diagnostics.Process]::Start($start)
        try {
            Check 'update while running succeeds' ((Invoke-Installer $shell @('-InstallDir', $dir, '-NoModifyPath')) -eq 0)
            Check 'new executable runs' ((& $exe --version) -match '^emlattachmentremover ')
            $owned = [IO.File]::ReadAllText($receiptFile) | ConvertFrom-Json
            Check 'one retired path is explicitly owned' ($null -ne $owned.Retired -and $owned.Retired.Name -match ('^\.' + $command + '-retired-[0-9a-f]{32}\.exe$'))
            $retiredPath = Join-Path $dir $owned.Retired.Name
            if (Test-Path -LiteralPath $retiredPath) {
                $before = (Get-FileHash -LiteralPath $exe).Hash
                Check 'another update refuses a still-running retired image' ((Invoke-Installer $shell @('-InstallDir', $dir, '-NoModifyPath')) -ne 0)
                Check 'current executable is unchanged' ((Get-FileHash -LiteralPath $exe).Hash -eq $before)
            }
        } finally { $running.StandardInput.Close(); [void]$running.WaitForExit() }
        Check 'owned retired image is cleaned on next install' ((Invoke-Installer $shell @('-InstallDir', $dir, '-NoModifyPath')) -eq 0)

        Write-Host '--- validation failures preserve the installation'
        $before = (Get-FileHash -LiteralPath $exe).Hash
        $beforeReceipt = [IO.File]::ReadAllText($receiptFile)
        $beforeSeal = [IO.File]::ReadAllText($receiptSeal)
        $tampered = Join-Path $work "$shellName-tampered"
        [void][IO.Directory]::CreateDirectory($tampered)
        foreach ($file in Get-ChildItem -LiteralPath $Dist -Filter '*.zip') { [IO.File]::Copy($file.FullName, (Join-Path $tampered $file.Name)) }
        # Corrupt only the digest, preserving GNU markers, separators and filenames.
        $lines = @([IO.File]::ReadAllLines((Join-Path $Dist $checksumName)) | ForEach-Object { $_ -replace '\A[0-9a-fA-F]{64}', ('0' * 64) })
        [IO.File]::WriteAllLines((Join-Path $tampered $checksumName), $lines)
        Check 'bad checksum fails through -File' ((Invoke-Installer $shell @('-InstallDir', $dir, '-NoModifyPath') $tampered -AsFile) -ne 0)
        Check 'checksum cause is shown' ((Get-Output) -match 'does not match its published SHA-256')
        Check 'binary and receipt unchanged' ((Get-FileHash -LiteralPath $exe).Hash -eq $before -and [IO.File]::ReadAllText($receiptFile) -ceq $beforeReceipt -and [IO.File]::ReadAllText($receiptSeal) -ceq $beforeSeal)
        $wrongVersion = Join-Path $work "$shellName-wrong-version"
        [void][IO.Directory]::CreateDirectory($wrongVersion)
        foreach ($file in Get-ChildItem -LiteralPath $Dist -Filter 'emlattachmentremover-*-windows-*.zip') {
            $renamed = $file.Name -creplace '^emlattachmentremover-[0-9]+\.[0-9]+\.[0-9]+-', 'emlattachmentremover-999.0.0-'
            [IO.File]::Copy($file.FullName, (Join-Path $wrongVersion $renamed))
        }
        Save-Manifest $wrongVersion
        Check 'version mismatch fails' ((Invoke-Installer $shell @('-InstallDir', $dir, '-NoModifyPath', '-Version', '999.0.0') $wrongVersion) -ne 0)
        Check 'version mismatch reported' ((Get-Output) -match 'not 999.0.0')
        Write-Host '--- native access-denied ownership preflight'
        $originalAcl = Get-Acl -LiteralPath $exe
        try {
            $deniedAcl = Get-Acl -LiteralPath $exe
            $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User
            $deny = [Security.AccessControl.FileSystemAccessRule]::new($sid, [Security.AccessControl.FileSystemRights]::ReadData, [Security.AccessControl.AccessControlType]::Deny)
            [void]$deniedAcl.AddAccessRule($deny)
            Set-Acl -LiteralPath $exe -AclObject $deniedAcl
            $exitCode = Invoke-Installer $shell @('-InstallDir', $dir, '-ReplaceUnmanaged', '-NoModifyPath')
            Check 'native ACL read denial is refused before download' ($exitCode -ne 0 -and (Get-Output).Contains($exe) -and -not (Get-Output).Contains('Downloaded'))
        } finally { Set-Acl -LiteralPath $exe -AclObject $originalAcl }
        Check 'ACL denial preserved payload and receipt bytes' ((Get-FileHash -LiteralPath $exe).Hash -ceq $before -and [IO.File]::ReadAllText($receiptFile) -ceq $beforeReceipt -and [IO.File]::ReadAllText($receiptSeal) -ceq $beforeSeal)
        Write-Host '--- malformed and ambiguous release data'
        $duplicate = Join-Path $work "$shellName-duplicate-checksum"
        [void][IO.Directory]::CreateDirectory($duplicate)
        foreach ($file in Get-ChildItem -LiteralPath $Dist -Filter '*.zip') { [IO.File]::Copy($file.FullName, (Join-Path $duplicate $file.Name)) }
        $manifest = [IO.File]::ReadAllText((Join-Path $Dist $checksumName))
        [IO.File]::WriteAllText((Join-Path $duplicate $checksumName), $manifest + "`n" + $manifest)
        Check 'duplicate checksums fail without replacement' ((Invoke-Installer $shell @('-InstallDir', $dir, '-NoModifyPath') $duplicate) -ne 0 -and (Get-FileHash -LiteralPath $exe).Hash -eq $before)
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $badKinds = @('duplicate', 'reparse')
        if ($Product.RootedArchive) { $badKinds += @('wrong-root', 'flat') }
        foreach ($kind in $badKinds) {
            $badZip = Join-Path $work ("$shellName-$kind-zip")
            [void][IO.Directory]::CreateDirectory($badZip)
            foreach ($file in Get-ChildItem -LiteralPath $Dist -Filter '*.zip') {
                $path = Join-Path $badZip $file.Name
                [IO.File]::Copy($file.FullName, $path)
                $zip = [IO.Compression.ZipFile]::Open($path, [IO.Compression.ZipArchiveMode]::Update)
                try {
                    $member = $command + '.exe'
                    if ($Product.RootedArchive) { $member = [IO.Path]::GetFileNameWithoutExtension($file.Name) + '/' + $member }
                    if ($kind -eq 'duplicate') { [void]$zip.CreateEntry($member) }
                    elseif ($kind -eq 'reparse') { $zip.GetEntry($member).ExternalAttributes = 0x400 }
                    else {
                        $entry = $zip.GetEntry($member)
                        $destination = if ($kind -eq 'flat') { $command + '.exe' } else { 'wrong-root/' + $command + '.exe' }
                        $inputStream = $entry.Open()
                        $bytes = [IO.MemoryStream]::new()
                        try { $inputStream.CopyTo($bytes) } finally { $inputStream.Dispose() }
                        $entry.Delete()
                        $replacement = $zip.CreateEntry($destination).Open()
                        try { $bytes.Position = 0; $bytes.CopyTo($replacement) } finally { $replacement.Dispose(); $bytes.Dispose() }
                    }
                } finally { $zip.Dispose() }
            }
            Save-Manifest $badZip
            Check ("$kind archive member rejected") ((Invoke-Installer $shell @('-InstallDir', $dir, '-NoModifyPath') $badZip) -ne 0 -and (Get-FileHash -LiteralPath $exe).Hash -eq $before)
        }
        $empty = Join-Path $work "$shellName-empty"
        [void][IO.Directory]::CreateDirectory($empty)
        Check 'missing download fails' ((Invoke-Installer $shell @('-InstallDir', $dir, '-NoModifyPath') $empty) -ne 0)
        $lockPath = Join-Path $dir ('.' + $command + '-install.lock')
        $held = [IO.File]::Open($lockPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
        try { Check 'cooperating lock refuses promptly' ((Invoke-Installer $shell @('-InstallDir', $dir, '-NoModifyPath')) -ne 0) }
        finally { $held.Dispose(); Remove-Item -LiteralPath $lockPath }
        [IO.File]::WriteAllText("$exe.old", 'unrelated backup')
        Check 'unrelated .old does not block or disappear on reinstall' ((Invoke-Installer $shell @('-InstallDir', $dir, '-NoModifyPath')) -eq 0 -and [IO.File]::ReadAllText("$exe.old") -ceq 'unrelated backup')
        Remove-Item -LiteralPath "$exe.old"
        # Positive reinstall above changed metadata; save its current matching pair.
        $beforeReceipt = [IO.File]::ReadAllText($receiptFile)
        $beforeSeal = [IO.File]::ReadAllText($receiptSeal)
        $originalLength = (Get-Item -LiteralPath $exe).Length
        $stream = [IO.File]::Open($exe, [IO.FileMode]::Open, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try { [void]$stream.Seek(0, [IO.SeekOrigin]::End); $stream.WriteByte(0) } finally { $stream.Dispose() }
        Check 'changed managed executable refuses uninstall' ((Invoke-Installer $shell @('-InstallDir', $dir, '-Uninstall', '-NoModifyPath')) -ne 0)
        Check 'changed managed executable remains' ((Get-Item -LiteralPath $exe).Length -eq $originalLength + 1)
        Check 'ReplaceUnmanaged cannot bypass changed managed payload' ((Invoke-Installer $shell @('-InstallDir', $dir, '-ReplaceUnmanaged', '-NoModifyPath')) -ne 0 -and (Get-Item -LiteralPath $exe).Length -eq $originalLength + 1)
        $stream = [IO.File]::Open($exe, [IO.FileMode]::Open, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try { $stream.SetLength($originalLength) } finally { $stream.Dispose() }
        [IO.File]::WriteAllText($receiptFile, '{broken')
        Check 'corrupt receipt refuses update' ((Invoke-Installer $shell @('-InstallDir', $dir, '-NoModifyPath')) -ne 0)
        Check 'corrupt state did not delete executable' (Test-Path -LiteralPath $exe)
        Check 'ReplaceUnmanaged cannot bypass corrupt receipt' ((Invoke-Installer $shell @('-InstallDir', $dir, '-ReplaceUnmanaged', '-NoModifyPath')) -ne 0 -and [IO.File]::ReadAllText($receiptFile) -ceq '{broken')
        [IO.File]::WriteAllText($receiptFile, $beforeReceipt)
        [IO.File]::WriteAllText($receiptSeal, $beforeSeal)
        Remove-Item -LiteralPath $exe
        Check 'missing-payload uninstall succeeds' ((Invoke-Installer $shell @('-InstallDir', $dir, '-Uninstall', '-NoModifyPath')) -eq 0 -and -not (Test-Path -LiteralPath $state))
        Check 'installer-created folder removed' (-not (Test-Path -LiteralPath $dir))
        Check 'second uninstall is a no-op' ((Invoke-Installer $shell @('-InstallDir', $dir, '-Uninstall', '-NoModifyPath')) -eq 0 -and (Get-Output) -match 'Nothing was changed')

        Write-Host '--- empty explicit uninstall cannot select a different default installation'
        $savedLocalAppData = $env:LOCALAPPDATA
        $env:LOCALAPPDATA = Join-Path $work "$shellName-isolated-localappdata"
        try {
            Check 'isolated default installation succeeds' ((Invoke-Installer $shell @('-NoModifyPath')) -eq 0)
            $defaultExe = Join-Path $env:LOCALAPPDATA ('Programs\' + $Product.WindowsFolder + '\' + $command + '.exe')
            Check 'isolated default executable exists' (Test-Path -LiteralPath $defaultExe)
            $defaultHash = (Get-FileHash -LiteralPath $defaultExe).Hash
            $exitCode = Invoke-NativeCase $shell @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command', ($reviewedBlock + " -Uninstall -InstallDir '' -NoModifyPath"))
            Check 'empty explicit uninstall fails and default stays untouched' ($exitCode -ne 0 -and (Test-Path -LiteralPath $defaultExe) -and (Get-FileHash -LiteralPath $defaultExe).Hash -ceq $defaultHash)
            Check 'normal isolated default uninstall succeeds' ((Invoke-Installer $shell @('-Uninstall', '-NoModifyPath')) -eq 0)
        } finally { $env:LOCALAPPDATA = $savedLocalAppData }

        Write-Host '--- unowned targets stay untouched'
        [void][IO.Directory]::CreateDirectory($dir)
        [IO.File]::WriteAllText($exe, 'unrelated executable')
        [IO.File]::WriteAllText("$exe.old", 'unrelated backup')
        Check 'unmanaged executable refuses install' ((Invoke-Installer $shell @('-InstallDir', $dir, '-NoModifyPath')) -ne 0)
        Check 'unmanaged executable refuses uninstall' ((Invoke-Installer $shell @('-InstallDir', $dir, '-Uninstall', '-NoModifyPath')) -ne 0)
        Check 'unmanaged uninstall does not suggest installation' ((Get-Output).Contains('Use the method that installed this executable') -and -not (Get-Output).Contains('ReplaceUnmanaged'))
        Check 'unrelated files and directory remain' ([IO.File]::ReadAllText($exe) -ceq 'unrelated executable' -and [IO.File]::ReadAllText("$exe.old") -ceq 'unrelated backup')
        $recoveryBefore = @(Get-ChildItem -LiteralPath $work -Filter ('.' + $command + '-unmanaged-*.exe') -Force | ForEach-Object { $_.FullName })
        Check 'ReplaceUnmanaged is refused for uninstall' ((Invoke-Installer $shell @('-InstallDir', $dir, '-Uninstall', '-ReplaceUnmanaged', '-NoModifyPath')) -ne 0 -and [IO.File]::ReadAllText($exe) -ceq 'unrelated executable')
        $emptyMirror = Join-Path $work ($shellName + '-missing-unmanaged-release')
        [void][IO.Directory]::CreateDirectory($emptyMirror)
        Check 'default unmanaged refusal precedes download' ((Invoke-Installer $shell @('-InstallDir', $dir, '-NoModifyPath') $emptyMirror) -ne 0 -and (Get-Output).Contains('Existing executable has no installer receipt') -and -not (Get-Output).Contains('Downloaded'))
        Check 'bad candidate checksum leaves unmanaged bytes and state untouched' ((Invoke-Installer $shell @('-InstallDir', $dir, '-ReplaceUnmanaged', '-NoModifyPath') $tampered) -ne 0 -and [IO.File]::ReadAllText($exe) -ceq 'unrelated executable' -and -not (Test-Path -LiteralPath $state))
        Check 'saved-file unmanaged refusal supplies a recovery command' ((Invoke-Installer $shell @('-InstallDir', $dir, '-NoModifyPath') -AsFile) -ne 0)
        $recoveryMatch = Get-RecoveryCommand (Get-Output)
        Check 'recovery command is present in native refusal' $recoveryMatch.Success
        $recoveryEnvironment = @{}
        foreach ($suffix in @('VERSION', 'INSTALL_DIR', 'NO_MODIFY_PATH', 'UNINSTALL', 'DOWNLOAD_BASE')) {
            $name = $prefix + '_' + $suffix
            $recoveryEnvironment[$name] = [Environment]::GetEnvironmentVariable($name)
            [Environment]::SetEnvironmentVariable($name, $null)
        }
        try {
            [Environment]::SetEnvironmentVariable(($prefix + '_DOWNLOAD_BASE'), $Dist)
            $recoveryExit = if ($recoveryMatch.Success) { Invoke-NativeCase $shell @('-NoProfile', '-Command', $recoveryMatch.Groups[1].Value) } else { -1 }
            Check 'copied recovery command installs the real release' ($recoveryExit -eq 0 -and (Test-Path -LiteralPath $receiptFile))
        } finally { foreach ($name in $recoveryEnvironment.Keys) { [Environment]::SetEnvironmentVariable($name, $recoveryEnvironment[$name]) } }
        if (Test-Path -LiteralPath $receiptFile) { Check 'copied command preserves NoModifyPath' ($null -eq ([IO.File]::ReadAllText($receiptFile) | ConvertFrom-Json).PathEntry) }
        $recovery = @(Get-ChildItem -LiteralPath $work -Filter ('.' + $command + '-unmanaged-*.exe') -Force | Where-Object { $recoveryBefore -notcontains $_.FullName })
        Check 'one recovery backup outside shared bin retains original bytes' ($recovery.Count -eq 1 -and [IO.File]::ReadAllText($recovery[0].FullName) -ceq 'unrelated executable')
        if ($recovery.Count -eq 1) { Check 'backup exact path is reported' ((Get-Output).Contains($recovery[0].FullName)) }
        Check 'replacement does not adopt existing directory ownership' (-not ([IO.File]::ReadAllText($receiptFile) | ConvertFrom-Json).DirectoryCreated)
        Check 'replacement uninstall leaves backup and unrelated files' ((Invoke-Installer $shell @('-InstallDir', $dir, '-Uninstall', '-NoModifyPath')) -eq 0 -and $recovery.Count -eq 1 -and (Test-Path -LiteralPath $recovery[0].FullName) -and [IO.File]::ReadAllText("$exe.old") -ceq 'unrelated backup')
        Remove-Item -LiteralPath "$exe.old"
        [void][IO.Directory]::CreateDirectory($exe)
        Check 'ReplaceUnmanaged cannot replace a directory target' ((Invoke-Installer $shell @('-InstallDir', $dir, '-ReplaceUnmanaged', '-NoModifyPath')) -ne 0 -and (Get-Item -LiteralPath $exe).PSIsContainer -and -not (Test-Path -LiteralPath $state))
        Remove-Item -LiteralPath $exe
        Check 'preexisting empty directory can be used' ((Invoke-Installer $shell @('-InstallDir', $dir, '-NoModifyPath')) -eq 0)
        $preexistingReceipt = [IO.File]::ReadAllText($receiptFile)
        $preexistingSeal = [IO.File]::ReadAllText($receiptSeal)
        $edited = $preexistingReceipt | ConvertFrom-Json
        Check 'receipt records that directory was preexisting' (-not $edited.DirectoryCreated)
        $edited.DirectoryCreated = $true
        $editedText = $edited | ConvertTo-Json -Depth 8
        [IO.File]::WriteAllText($receiptFile, $editedText)
        Check 'valid JSON ownership flag edit refuses uninstall' ((Invoke-Installer $shell @('-InstallDir', $dir, '-Uninstall', '-NoModifyPath')) -ne 0)
        Check 'edited receipt, binary and preexisting directory remain' ((Test-Path -LiteralPath $exe) -and (Test-Path -LiteralPath $dir) -and [IO.File]::ReadAllText($receiptFile) -ceq $editedText)
        [IO.File]::WriteAllText($receiptFile, $preexistingReceipt)
        [IO.File]::WriteAllText($receiptSeal, $preexistingSeal)
        Check 'preexisting empty directory survives uninstall' ((Invoke-Installer $shell @('-InstallDir', $dir, '-Uninstall', '-NoModifyPath')) -eq 0 -and (Test-Path -LiteralPath $dir))

        Write-Host '--- piped Invoke-Expression contract'
        $saved = @{}
        foreach ($suffix in @('VERSION','INSTALL_DIR','NO_MODIFY_PATH','UNINSTALL','DOWNLOAD_BASE')) { $name = $prefix + '_' + $suffix; $saved[$name] = [Environment]::GetEnvironmentVariable($name) }
        try {
            [Environment]::SetEnvironmentVariable(($prefix + '_VERSION'), $testVersion)
            [Environment]::SetEnvironmentVariable(($prefix + '_INSTALL_DIR'), $dir)
            [Environment]::SetEnvironmentVariable(($prefix + '_NO_MODIFY_PATH'), '1')
            [Environment]::SetEnvironmentVariable(($prefix + '_DOWNLOAD_BASE'), $Dist)
            $exitCode = Invoke-NativeCase $shell @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command', ("Get-Content -Raw '" + $installer.Replace("'", "''") + "' | Invoke-Expression"))
            Check 'iex install succeeds' ($exitCode -eq 0 -and (Test-Path -LiteralPath $exe))
            [Environment]::SetEnvironmentVariable(($prefix + '_UNINSTALL'), '1')
            $exitCode = Invoke-NativeCase $shell @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command', ("Get-Content -Raw '" + $installer.Replace("'", "''") + "' | Invoke-Expression"))
            Check 'iex uninstall succeeds' ($exitCode -eq 0 -and -not (Test-Path -LiteralPath $exe))
            [Environment]::SetEnvironmentVariable(($prefix + '_UNINSTALL'), $null)
            [Environment]::SetEnvironmentVariable(($prefix + '_VERSION'), '999.0.0')
            $exitCode = Invoke-NativeCase $shell @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command', ("Get-Content -Raw '" + $installer.Replace("'", "''") + "' | Invoke-Expression"))
            Check 'iex failure is nonzero' ($exitCode -ne 0)
        } finally { foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name]) } }

        if ($env:CI) {
            Write-Host '--- user PATH ownership and exact registry restoration'
            $original = Get-RegistryPath
            try {
                foreach ($kind in @([Microsoft.Win32.RegistryValueKind]::String, [Microsoft.Win32.RegistryValueKind]::ExpandString)) {
                    $raw = '%USERPROFILE%\custom;;C:\Keep;'
                    Save-RegistryPath @{ Exists = $true; Value = $raw; Kind = $kind }
                    Check 'PATH install succeeds' ((Invoke-Installer $shell @('-InstallDir', $dir)) -eq 0)
                    $current = Get-RegistryPath
                    Check 'raw PATH and registry kind preserved' ($current.Value -ceq ($dir + ';' + $raw) -and $current.Kind -eq $kind)
                    Check 'reinstall does not duplicate PATH' ((Invoke-Installer $shell @('-InstallDir', $dir)) -eq 0 -and (Get-RegistryPath).Value -ceq ($dir + ';' + $raw))
                    Check 'owned PATH removed exactly' ((Invoke-Installer $shell @('-InstallDir', $dir, '-Uninstall')) -eq 0 -and (Get-RegistryPath).Value -ceq $raw -and (Get-RegistryPath).Kind -eq $kind)
                    Save-RegistryPath @{ Exists = $true; Value = $dir + ';' + $raw; Kind = $kind }
                    Check 'preexisting PATH install succeeds' ((Invoke-Installer $shell @('-InstallDir', $dir)) -eq 0)
                    Check 'preexisting PATH survives uninstall' ((Invoke-Installer $shell @('-InstallDir', $dir, '-Uninstall')) -eq 0 -and (Get-RegistryPath).Value -ceq ($dir + ';' + $raw))
                }
                Save-RegistryPath @{ Exists = $true; Value = 'C:\Keep;;'; Kind = [Microsoft.Win32.RegistryValueKind]::String }
                [IO.File]::WriteAllText($exe, 'unmanaged PATH fixture')
                $beforeRecovery = @(Get-ChildItem -LiteralPath $work -Force -Filter ('.' + $command + '-unmanaged-*.exe') | ForEach-Object { $_.FullName })
                Check 'explicit replacement owns only newly added PATH' ((Invoke-Installer $shell @('-InstallDir', $dir, '-ReplaceUnmanaged')) -eq 0 -and (Get-RegistryPath).Value -ceq ($dir + ';C:\Keep;;'))
                $retainedRecovery = @(Get-ChildItem -LiteralPath $work -Force -Filter ('.' + $command + '-unmanaged-*.exe') | Where-Object { $beforeRecovery -notcontains $_.FullName })
                Check 'outside-bin backup does not block owned PATH cleanup' ((Invoke-Installer $shell @('-InstallDir', $dir, '-Uninstall')) -eq 0 -and (Get-RegistryPath).Value -ceq 'C:\Keep;;' -and $retainedRecovery.Count -eq 1 -and [IO.File]::ReadAllText($retainedRecovery[0].FullName) -ceq 'unmanaged PATH fixture')
                Save-RegistryPath @{ Exists = $false; Value = ''; Kind = [Microsoft.Win32.RegistryValueKind]::ExpandString }
                Check 'absent PATH install succeeds' ((Invoke-Installer $shell @('-InstallDir', $dir)) -eq 0)
                Check 'absent PATH restored as absent' ((Invoke-Installer $shell @('-InstallDir', $dir, '-Uninstall')) -eq 0 -and -not (Get-RegistryPath).Exists)
                Save-RegistryPath @{ Exists = $false; Value = ''; Kind = [Microsoft.Win32.RegistryValueKind]::ExpandString }
                Check 'absent PATH install for later kind edit succeeds' ((Invoke-Installer $shell @('-InstallDir', $dir)) -eq 0)
                Save-RegistryPath @{ Exists = $true; Value = $dir; Kind = [Microsoft.Win32.RegistryValueKind]::String }
                Check 'uninstall removes owned entry but retains edited empty String container' ((Invoke-Installer $shell @('-InstallDir', $dir, '-Uninstall')) -eq 0 -and (Get-RegistryPath).Exists -and (Get-RegistryPath).Value -ceq '' -and (Get-RegistryPath).Kind -eq [Microsoft.Win32.RegistryValueKind]::String)
                Save-RegistryPath @{ Exists = $true; Value = 'C:\Keep;;'; Kind = [Microsoft.Win32.RegistryValueKind]::String }
                Check 'NoModifyPath install leaves raw PATH' ((Invoke-Installer $shell @('-InstallDir', $dir, '-NoModifyPath')) -eq 0 -and (Get-RegistryPath).Value -ceq 'C:\Keep;;')
                Check 'NoModifyPath uninstall leaves raw PATH' ((Invoke-Installer $shell @('-InstallDir', $dir, '-Uninstall', '-NoModifyPath')) -eq 0 -and (Get-RegistryPath).Value -ceq 'C:\Keep;;')
                Check 'shared folder installation succeeds' ((Invoke-Installer $shell @('-InstallDir', $dir)) -eq 0)
                [IO.File]::WriteAllText((Join-Path $dir 'other-program.txt'), 'user file')
                Check 'shared folder PATH is conservatively retained' ((Invoke-Installer $shell @('-InstallDir', $dir, '-Uninstall')) -eq 0 -and (Get-RegistryPath).Value.StartsWith($dir + ';') -and (Test-Path -LiteralPath (Join-Path $dir 'other-program.txt')))
                Remove-Item -LiteralPath (Join-Path $dir 'other-program.txt')
            } finally { Save-RegistryPath $original }
        }
    }
} finally { Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue }
if ($script:failures -gt 0) { throw "$($script:failures) installer checks failed." }
Write-Host 'All installer checks passed.'
