# SPDX-License-Identifier: MPL-2.0
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
            & $Shell @Arguments *> (Join-Path $work 'out.txt')
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
            { $_ -in @('-NoModifyPath', '-Uninstall', '-Help') } {
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
        [Environment]::SetEnvironmentVariable(($prefix + '_DOWNLOAD_BASE'), $Base)
        [Environment]::SetEnvironmentVariable(($prefix + '_VERSION'), $testVersion)
        if ($fileMode) {
            return (Invoke-NativeCase $Shell (@('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $installer) + $Arguments))
        }
        try {
            # Reuse the exact public scriptblock, not an internal installer function.
            # Hashtable splatting preserves named binding; string arrays do not.
            & $reviewedInstaller @options *> (Join-Path $work 'out.txt')
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
        $sourceMail = Join-Path $work "$shellName-source.eml"
        [IO.File]::WriteAllBytes($sourceMail, [Text.Encoding]::ASCII.GetBytes("Subject: Installer smoke`r`n`r`nBody retained`r`n"))
        & $exe -- $sourceMail | Out-Null
        $copyMail = Join-Path $work "$shellName-source.no-attachments.eml"
        Check 'installed CLI creates the exact body copy' ($LASTEXITCODE -eq 0 -and (Test-Path -LiteralPath $copyMail) -and [Convert]::ToBase64String([IO.File]::ReadAllBytes($copyMail)) -ceq [Convert]::ToBase64String([IO.File]::ReadAllBytes($sourceMail)))
        foreach ($notice in @('LICENSE', 'THIRD-PARTY-NOTICES.txt')) { Check ('installed legal file retained: ' + $notice) ((Get-Item -LiteralPath (Join-Path (Join-Path $state 'legal') $notice)).Length -gt 0) }
        Check 'installation reported' ((Get-Output) -match 'is installed')
        Check 'custom uninstall path is quoted' ((Get-Output).Contains("-InstallDir '" + $dir.Replace("'", "''") + "'"))
        Check 'reinstall succeeds' ((Invoke-Installer $shell @('-InstallDir', $dir, '-NoModifyPath')) -eq 0)
        Check 'reinstall reported' ((Get-Output) -match 'reinstalled')
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
        $lines = @([IO.File]::ReadAllLines((Join-Path $Dist $checksumName)) | ForEach-Object { ('0' * 64) + '  ' + ($_ -split '\s+')[1] })
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
        Write-Host '--- malformed and ambiguous release data'
        $duplicate = Join-Path $work "$shellName-duplicate-checksum"
        [void][IO.Directory]::CreateDirectory($duplicate)
        foreach ($file in Get-ChildItem -LiteralPath $Dist -Filter '*.zip') { [IO.File]::Copy($file.FullName, (Join-Path $duplicate $file.Name)) }
        $manifest = [IO.File]::ReadAllText((Join-Path $Dist $checksumName))
        [IO.File]::WriteAllText((Join-Path $duplicate $checksumName), $manifest + "`n" + $manifest)
        Check 'duplicate checksums fail without replacement' ((Invoke-Installer $shell @('-InstallDir', $dir, '-NoModifyPath') $duplicate) -ne 0 -and (Get-FileHash -LiteralPath $exe).Hash -eq $before)
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        foreach ($kind in @('duplicate', 'reparse')) {
            $badZip = Join-Path $work ("$shellName-$kind-zip")
            [void][IO.Directory]::CreateDirectory($badZip)
            foreach ($file in Get-ChildItem -LiteralPath $Dist -Filter '*.zip') {
                $path = Join-Path $badZip $file.Name
                [IO.File]::Copy($file.FullName, $path)
                $zip = [IO.Compression.ZipFile]::Open($path, [IO.Compression.ZipArchiveMode]::Update)
                try {
                    if ($kind -eq 'duplicate') { [void]$zip.CreateEntry($command + '.exe') }
                    else { $zip.GetEntry($command + '.exe').ExternalAttributes = 0x400 }
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
        $stream = [IO.File]::Open($exe, [IO.FileMode]::Open, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try { $stream.SetLength($originalLength) } finally { $stream.Dispose() }
        [IO.File]::WriteAllText($receiptFile, '{broken')
        Check 'corrupt receipt refuses update' ((Invoke-Installer $shell @('-InstallDir', $dir, '-NoModifyPath')) -ne 0)
        Check 'corrupt state did not delete executable' (Test-Path -LiteralPath $exe)
        [IO.File]::WriteAllText($receiptFile, $beforeReceipt)
        [IO.File]::WriteAllText($receiptSeal, $beforeSeal)
        Check 'uninstall succeeds' ((Invoke-Installer $shell @('-InstallDir', $dir, '-Uninstall', '-NoModifyPath')) -eq 0)
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
        Check 'unrelated files and directory remain' ([IO.File]::ReadAllText($exe) -ceq 'unrelated executable' -and [IO.File]::ReadAllText("$exe.old") -ceq 'unrelated backup')
        Remove-Item -LiteralPath $exe, "$exe.old"
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
                Save-RegistryPath @{ Exists = $false; Value = ''; Kind = [Microsoft.Win32.RegistryValueKind]::ExpandString }
                Check 'absent PATH install succeeds' ((Invoke-Installer $shell @('-InstallDir', $dir)) -eq 0)
                Check 'absent PATH restored as absent' ((Invoke-Installer $shell @('-InstallDir', $dir, '-Uninstall')) -eq 0 -and -not (Get-RegistryPath).Exists)
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
