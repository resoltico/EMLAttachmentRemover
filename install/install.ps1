# SPDX-License-Identifier: MPL-2.0
# Self-contained user-scope CLI release installer. Keep this file ASCII for PS 5.1.
[CmdletBinding(PositionalBinding = $false)]
param([string]$Version, [string]$InstallDir, [switch]$NoModifyPath, [switch]$Uninstall, [switch]$Help)

# BEGIN PRODUCT SETTINGS
$Product = @{
    Name = 'EML Attachment Remover'
    Command = 'emlattachmentremover'
    Repository = 'https://github.com/resoltico/EMLAttachmentRemover'
    Environment = 'EMLATTACHMENTREMOVER'
    ArchivePrefix = 'emlattachmentremover-'
    Separator = '-'
    VersionedArchive = $true
    RootedArchive = $false
    MacOS = 'macos'
    X64 = 'x86_64'
    Arm64 = 'aarch64'
    Checksums = 'SHA256SUMS'
    WindowsFolder = 'EMLAttachmentRemover'
    MinimumMacOS = 15
    LegalFiles = @('LICENSE', 'THIRD-PARTY-NOTICES.txt')
}
# END PRODUCT SETTINGS

function Invoke-CliInstaller {
    param([string]$Version, [string]$InstallDir, [bool]$ModifyPath, [bool]$Uninstall, [hashtable]$Product)
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'
    Set-StrictMode -Version 3
    $stable = '(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)'
    $identifier = '(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)'
    $semver = $stable + '(?:-' + $identifier + '(?:\.' + $identifier + ')*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?'
    $utf8 = [Text.UTF8Encoding]::new($false)
    function Exit-Installer([string]$Text) { throw [InvalidOperationException]::new($Text) }
    function Write-Step([string]$Label, [string]$Text) { Write-Host ('  {0,-11} {1}' -f $Label, $Text) }
    function ConvertTo-QuotedCommand([string]$Text) { return "'" + $Text.Replace("'", "''") + "'" }
    function Get-Digest([string]$Path) { return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
    function Get-ItemOrNull([string]$Path) {
        try { return Get-Item -LiteralPath $Path -Force }
        catch [Management.Automation.ItemNotFoundException] { return $null }
    }
    function Assert-Regular([string]$Path, [bool]$Missing = $false) {
        $item = Get-ItemOrNull $Path
        if (-not $item) { if ($Missing) { return $false }; Exit-Installer "Required managed file is missing: $Path" }
        if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { Exit-Installer "Not a regular managed file; left untouched: $Path" }
        return $true
    }
    function Test-SamePath([string]$A, [string]$B) {
        $a2 = [Environment]::ExpandEnvironmentVariables($A).TrimEnd('\', '/')
        $b2 = [Environment]::ExpandEnvironmentVariables($B).TrimEnd('\', '/')
        return [string]::Equals($a2, $b2, [StringComparison]::OrdinalIgnoreCase)
    }
    function Enter-PathLock {
        # A named mutex coordinates CLI installers across sessions of this user.
        # Windows requires no administrator privilege for a global mutex.
        $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
        $mutex = [Threading.Mutex]::new($false, ('Global\CliReleaseInstaller.Path.' + $sid))
        try {
            try { $held = $mutex.WaitOne(0) }
            catch [Threading.AbandonedMutexException] { $held = $true }
            if (-not $held) { Exit-Installer 'Another installer is changing user PATH. Nothing was changed; run this command after it finishes.' }
            return $mutex
        } catch { $mutex.Dispose(); throw }
    }
    function Get-AbsoluteDirectory([string]$Directory) {
        $root = [IO.Path]::GetPathRoot($Directory)
        if (-not $root -or $root -eq '\' -or $root -match '\A[A-Za-z]:\z') { Exit-Installer 'A fully qualified absolute directory is required.' }
        $full = [IO.Path]::GetFullPath($Directory)
        if ($full.Length -gt ([IO.Path]::GetPathRoot($full)).Length) { $full = $full.TrimEnd('\', '/') }
        return $full
    }
    function Get-UserPath {
        $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
        try {
            if (-not $key -or $key.GetValueNames() -notcontains 'Path') { return @{ Exists = $false; Value = ''; Kind = [Microsoft.Win32.RegistryValueKind]::ExpandString } }
            $kind = $key.GetValueKind('Path')
            if ($kind -ne [Microsoft.Win32.RegistryValueKind]::String -and $kind -ne [Microsoft.Win32.RegistryValueKind]::ExpandString) { Exit-Installer 'User PATH is not a string value; it was left untouched.' }
            return @{ Exists = $true; Value = [string]$key.GetValue('Path', '', 'DoNotExpandEnvironmentNames'); Kind = $kind }
        } finally { if ($key) { $key.Close() } }
    }
    function Save-UserPath([hashtable]$Record) {
        $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
        try {
            if ($Record.Exists) { $key.SetValue('Path', $Record.Value, $Record.Kind) }
            else { $key.DeleteValue('Path', $false) }
        } finally { $key.Close() }
        # Notify Explorer directly; do not create/delete an unrelated user variable.
        try {
            if (-not ('CliInstaller.EnvironmentNotice' -as [type])) {
            Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
namespace CliInstaller {
    public static class EnvironmentNotice {
        [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        public static extern IntPtr SendMessageTimeout(IntPtr h, uint m, UIntPtr w, string l, uint f, uint t, out UIntPtr r);
    }
}
'@
        }
        $result = [UIntPtr]::Zero
        $sent = [CliInstaller.EnvironmentNotice]::SendMessageTimeout([IntPtr]0xffff, 0x001a, [UIntPtr]::Zero, 'Environment', 2, 1000, [ref]$result)
        if ($sent -eq [IntPtr]::Zero) { Write-Step 'Note' 'PATH was saved; Explorer notification did not complete. Open a new terminal.' }
        } catch { Write-Step 'Note' ("PATH was saved, but Explorer notification failed: " + $_.Exception.Message) }
    }
    function Test-PathEntry([hashtable]$Record) {
        foreach ($entry in $Record.Value.Split([char]';')) { if ($entry -and (Test-SamePath $entry $InstallDir)) { return $true } }
        return $false
    }
    function Invoke-OwnedPathRemoval([string]$Entry, [bool]$WasAbsent) {
        $record = Get-UserPath
        $parts = [Collections.Generic.List[string]]::new()
        foreach ($part in $record.Value.Split([char]';')) { $parts.Add($part) }
        $index = $parts.IndexOf($Entry)
        if ($index -lt 0) { return }
        $parts.RemoveAt($index)
        $record.Value = [string]::Join(';', $parts.ToArray())
        if ($parts.Count -eq 0 -and $WasAbsent) { $record.Exists = $false }
        Save-UserPath $record
    }
    function Get-RemoteFile([uri]$Uri, [string]$Destination) {
        Add-Type -AssemblyName System.Net.Http
        $previousTLS = [Net.ServicePointManager]::SecurityProtocol
        $handler = [Net.Http.HttpClientHandler]::new()
        $handler.AllowAutoRedirect = $false
        $client = [Net.Http.HttpClient]::new($handler)
        $client.DefaultRequestHeaders.UserAgent.ParseAdd('cli-release-installer')
        $timeout = [Threading.CancellationTokenSource]::new()
        $timeout.CancelAfter(100000)
        try {
            if ($PSVersionTable.PSEdition -eq 'Desktop') { [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 }
            for ($hop = 0; $hop -le 10; $hop++) {
                if ($Uri.Scheme -ne 'https' -or $Uri.UserInfo) { Exit-Installer 'Downloads and every redirect must use HTTPS without embedded credentials.' }
                $response = $client.GetAsync($Uri, [Net.Http.HttpCompletionOption]::ResponseHeadersRead, $timeout.Token).GetAwaiter().GetResult()
                try {
                    if ([int]$response.StatusCode -ge 300 -and [int]$response.StatusCode -lt 400) {
                        if (-not $response.Headers.Location) { Exit-Installer 'Download redirect has no Location.' }
                        $Uri = [uri]::new($Uri, $response.Headers.Location)
                        continue
                    }
                    if (-not $response.IsSuccessStatusCode) { Exit-Installer "Download failed: HTTP $([int]$response.StatusCode)." }
                    $inputStream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
                    $outputStream = [IO.File]::Open($Destination, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
                    try { [void]$inputStream.CopyToAsync($outputStream, 81920, $timeout.Token).GetAwaiter().GetResult() } finally { $outputStream.Dispose(); $inputStream.Dispose() }
                    return
                } finally { $response.Dispose() }
            }
            Exit-Installer 'Too many download redirects.'
        } finally { $client.Dispose(); $timeout.Dispose(); [Net.ServicePointManager]::SecurityProtocol = $previousTLS }
    }
    function Get-ReleaseFile([string]$Name, [string]$Destination) {
        if ($base.StartsWith('https://', [StringComparison]::OrdinalIgnoreCase)) { Get-RemoteFile ([uri]("$base/$Name")) $Destination }
        else { [IO.File]::Copy((Join-Path $base $Name), $Destination, $false) }
    }
    function Get-ProgramVersion([string]$Program, [string]$Diagnostics) {
        $output = @()
        $failure = $null
        try { $output = @(& $Program --version 2>$Diagnostics); $code = $LASTEXITCODE }
        catch { $code = -1; $failure = $_.Exception.Message }
        $detail = if (Test-Path -LiteralPath $Diagnostics) { [IO.File]::ReadAllText($Diagnostics) } else { '' }
        $pattern = '\A' + [regex]::Escape($Product.Command) + ' (' + $semver + ')(?: \([^()\r\n]+\))?\z'
        if ($code -ne 0 -or $output.Count -ne 1 -or [string]$output[0] -cnotmatch $pattern) {
            Exit-Installer ("The downloaded program did not report a valid version (exit $code).`n$failure`n$detail`n" + ($output -join "`n"))
        }
        return $Matches[1]
    }
    function Write-Receipt([string]$Directory, [hashtable]$Receipt) {
        $receiptPath = Join-Path $Directory 'receipt.json'
        [IO.File]::WriteAllText($receiptPath, ($Receipt | ConvertTo-Json -Depth 8), $utf8)
        [IO.File]::WriteAllText((Join-Path $Directory 'receipt.sha256'), ((Get-Digest $receiptPath) + "`n"), $utf8)
    }
    function Read-Receipt([bool]$AllowMissing) {
        $bin = Get-ItemOrNull $exe
        $state = Get-ItemOrNull $stateDir
        if (-not $state) {
            if ($bin) { Exit-Installer "Existing $exe is not managed by this installer. Remove it manually or choose a new install directory." }
            return $null
        }
        if (-not $state.PSIsContainer -or ($state.Attributes -band [IO.FileAttributes]::ReparsePoint)) { Exit-Installer "Unmanaged installer state; left untouched: $stateDir" }
        try {
            $receiptPath = Join-Path $stateDir 'receipt.json'
            $sealPath = Join-Path $stateDir 'receipt.sha256'
            [void](Assert-Regular $receiptPath)
            [void](Assert-Regular $sealPath)
            $seal = [IO.File]::ReadAllText($sealPath)
            if ($seal -cnotmatch '\A[0-9a-f]{64}(?:\r?\n)?\z' -or (Get-Digest $receiptPath) -cne $seal.TrimEnd("`r", "`n")) { throw 'installer receipt changed or its seal is invalid' }
            $r = [IO.File]::ReadAllText($receiptPath) | ConvertFrom-Json
            if ($r.Directory -isnot [string] -or -not [string]::Equals($r.Directory, $InstallDir, [StringComparison]::OrdinalIgnoreCase)) { throw 'receipt belongs to another install directory' }
            if ($r.Schema -ne 1 -or $r.Command -cne $Product.Command -or $r.DirectoryCreated -isnot [bool] -or $r.PathWasAbsent -isnot [bool]) { throw 'invalid receipt identity or flags' }
            if ([string]$r.Version -cnotmatch ('\A' + $semver + '\z')) { throw 'invalid receipt version' }
            if ($null -ne $r.PathEntry -and ($r.PathEntry -isnot [string] -or -not (Test-SamePath $r.PathEntry $InstallDir))) { throw 'invalid owned PATH entry' }
            $expected = @($Product.LegalFiles)
            if (@($r.Legal).Count -ne $expected.Count) { throw 'invalid legal-file list' }
            if ([string]$r.BinaryHash -notmatch '\A[0-9a-f]{64}\z') { throw 'invalid binary digest' }
            $known = @('receipt.json', 'receipt.sha256')
            for ($i = 0; $i -lt $expected.Count; $i++) {
                if ($r.Legal[$i].Name -cne $expected[$i] -or [string]$r.Legal[$i].Hash -notmatch '\A[0-9a-f]{64}\z') { throw 'invalid legal digest' }
                $known += 'legal\' + $expected[$i]
            }
            foreach ($item in Get-ChildItem -LiteralPath $stateDir -Recurse -Force) {
                if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'reparse point in installer state' }
                $relative = $item.FullName.Substring($stateDir.Length + 1)
                if ($item.PSIsContainer) { if ($expected.Count -eq 0 -or $relative -cne 'legal') { throw 'unowned state directory' } }
                elseif ($known -cnotcontains $relative) { throw 'unowned file in installer state' }
            }
            if (Assert-Regular $exe $AllowMissing) { if ((Get-Digest $exe) -cne $r.BinaryHash) { throw 'managed executable has changed' } }
            foreach ($legal in @($r.Legal)) {
                $path = Join-Path (Join-Path $stateDir 'legal') $legal.Name
                if (Assert-Regular $path $AllowMissing) { if ((Get-Digest $path) -cne $legal.Hash) { throw 'managed legal file has changed' } }
            }
            if ($null -ne $r.Retired) {
                if ([string]$r.Retired.Name -cnotmatch ('\A\.' + [regex]::Escape($Product.Command) + '-retired-[0-9a-f]{32}\.exe\z') -or [string]$r.Retired.Hash -notmatch '\A[0-9a-f]{64}\z') { throw 'invalid retired-file ownership' }
                $path = Join-Path $InstallDir $r.Retired.Name
                if (Assert-Regular $path $true) { if ((Get-Digest $path) -cne $r.Retired.Hash) { throw 'retired executable has changed' } }
            }
            return @{ Schema = 1; Directory = $r.Directory; Command = $r.Command; Version = $r.Version; BinaryHash = $r.BinaryHash; Legal = @($r.Legal); DirectoryCreated = $r.DirectoryCreated; PathEntry = $r.PathEntry; PathWasAbsent = $r.PathWasAbsent; Retired = $r.Retired }
        } catch { Exit-Installer "Installer ownership could not be verified; nothing was replaced or removed: $($_.Exception.Message). Use manual cleanup or a new install directory." }
    }
    function Invoke-StateRemoval([string]$Directory, [object[]]$Legal) {
        foreach ($file in $Legal) {
            $path = Join-Path (Join-Path $Directory 'legal') $file.Name
            if (Assert-Regular $path $true) {
                if ((Get-Digest $path) -cne $file.Hash) { Exit-Installer "Managed legal file changed; it was left untouched: $path" }
                Remove-Item -LiteralPath $path -Force
            }
        }
        $legalDir = Join-Path $Directory 'legal'
        if (Test-Path -LiteralPath $legalDir) { Remove-Item -LiteralPath $legalDir -Force }
        Remove-Item -LiteralPath (Join-Path $Directory 'receipt.json') -Force
        Remove-Item -LiteralPath (Join-Path $Directory 'receipt.sha256') -Force
        Remove-Item -LiteralPath $Directory -Force
    }
    function Invoke-RetiredRemoval([hashtable]$Receipt) {
        if ($Receipt -and $null -ne $Receipt.Retired) {
            $path = Join-Path $InstallDir $Receipt.Retired.Name
            if (Assert-Regular $path $true) {
                if ((Get-Digest $path) -cne $Receipt.Retired.Hash) { Exit-Installer "Retired executable changed; it was left untouched: $path" }
                Remove-Item -LiteralPath $path -Force
            }
        }
    }

    if ($PSVersionTable.PSEdition -eq 'Core' -and -not $IsWindows) { Exit-Installer 'This installer is for Windows; use install.sh on macOS or Linux.' }
    if (-not $InstallDir) { $InstallDir = Join-Path $env:LOCALAPPDATA ('Programs\' + $Product.WindowsFolder) }
    $InstallDir = Get-AbsoluteDirectory $InstallDir
    $pathRoot = [IO.Path]::GetPathRoot($InstallDir)
    if ($InstallDir.Contains(';') -and $ModifyPath) { Write-Step 'Note' 'PATH cannot represent a semicolon in this directory; PATH will be left unchanged.'; $ModifyPath = $false }
    $exe = Join-Path $InstallDir ($Product.Command + '.exe')
    $stateDir = Join-Path $InstallDir ('.' + $Product.Command + '-install')
    $lockPath = Join-Path $InstallDir ('.' + $Product.Command + '-install.lock')
    $existingDir = Get-ItemOrNull $InstallDir
    if ($existingDir -and (-not $existingDir.PSIsContainer -or ($existingDir.Attributes -band [IO.FileAttributes]::ReparsePoint))) { Exit-Installer "The install directory is not an ordinary directory: $InstallDir" }
    if ($Uninstall -and -not $existingDir) { Write-Host "$($Product.Name) is not installed there. Nothing was changed."; return }
    $created = -not $existingDir
    if ($created) { [void][IO.Directory]::CreateDirectory($InstallDir) }
    $lock = $null
    $tmp = $null
    $stage = $null
    $backupState = $null
    $pathMutex = $null
    try {
        try { $lock = [IO.File]::Open($lockPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None) }
        catch { Exit-Installer "Another installer may be using $InstallDir, or its lock remains after interruption. Nothing was changed. Close it or inspect $lockPath before retrying." }
        $receipt = Read-Receipt $Uninstall
        if ($Uninstall) {
            if (-not $receipt) { Write-Host "$($Product.Name) is not managed here. Nothing was changed."; return }
            if ($ModifyPath -and $receipt.PathEntry) { $pathMutex = Enter-PathLock }
            # Determine sharing while ownership is intact; only validated owned
            # entries may be excluded. PATH failure must leave a retryable install.
            $owned = @($exe, $stateDir, $lockPath)
            if ($receipt.Retired) { $owned += Join-Path $InstallDir $receipt.Retired.Name }
            $others = @(Get-ChildItem -LiteralPath $InstallDir -Force | Where-Object { $owned -notcontains $_.FullName })
            if ($receipt.PathEntry) {
                if (-not $ModifyPath -or $others.Count -gt 0) { Write-Step 'Note' 'Owned PATH entry was retained (NoModifyPath or a shared folder); remove it manually when no longer needed.' }
                else {
                    Invoke-OwnedPathRemoval $receipt.PathEntry $receipt.PathWasAbsent
                    $parts = [Collections.Generic.List[string]]::new()
                    foreach ($part in $env:Path.Split([char]';')) { $parts.Add($part) }
                    $index = $parts.IndexOf($receipt.PathEntry)
                    if ($index -ge 0) { $parts.RemoveAt($index); $env:Path = [string]::Join(';', $parts.ToArray()) }
                }
            }
            Invoke-RetiredRemoval $receipt
            if (Test-Path -LiteralPath $exe) { Remove-Item -LiteralPath $exe -Force }
            Invoke-StateRemoval $stateDir $receipt.Legal
            $created = $receipt.DirectoryCreated
            Write-Host "$($Product.Name) has been uninstalled. Your documents and unrelated files were not touched."
            return
        }
        if ($Version.StartsWith('v')) { $Version = $Version.Substring(1) }
        if ($Version -and $Version -cnotmatch ('\A' + $stable + '\z')) { Exit-Installer "Not a stable release version: $Version" }
        $native = (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Environment').PROCESSOR_ARCHITECTURE
        $arch = switch ($native) { 'AMD64' { $Product.X64 } 'ARM64' { $Product.Arm64 } default { $null } }
        if (-not $arch) { Exit-Installer "No Windows release for the $native processor." }
        Write-Step 'System' "Windows - $native"
        $tmp = Join-Path ([IO.Path]::GetTempPath()) ('cli-install-' + [guid]::NewGuid().ToString('N'))
        [void][IO.Directory]::CreateDirectory($tmp)
        $mirror = [Environment]::GetEnvironmentVariable($Product.Environment + '_DOWNLOAD_BASE')
        if ($mirror) {
            if ($mirror.StartsWith('https://', [StringComparison]::OrdinalIgnoreCase)) {
                try { $mirrorUri = [uri]$mirror } catch { Exit-Installer 'DOWNLOAD_BASE is not a valid HTTPS directory URL.' }
                if ($mirrorUri.UserInfo -or $mirrorUri.Query -or $mirrorUri.Fragment) { Exit-Installer 'DOWNLOAD_BASE cannot contain credentials, a query or a fragment.' }
                $base = $mirrorUri.AbsoluteUri.TrimEnd('/')
            } else { $base = Get-AbsoluteDirectory $mirror }
            if ($Product.VersionedArchive -and -not $Version) { Exit-Installer 'A versioned release mirror requires -Version X.Y.Z.' }
        } else {
            if (-not $Version) {
                $repoPath = ([uri]$Product.Repository).AbsolutePath.Trim('/')
                $latest = Join-Path $tmp 'latest.json'
                Get-RemoteFile ([uri]("https://api.github.com/repos/$repoPath/releases/latest")) $latest
                $tag = ([IO.File]::ReadAllText($latest) | ConvertFrom-Json).tag_name
                if ([string]$tag -cnotmatch ('\Av(' + $stable + ')\z')) { Exit-Installer 'Latest release has no stable vX.Y.Z tag.' }
                $Version = $Matches[1]
            }
            $base = "$($Product.Repository)/releases/download/v$Version"
        }
        $asset = $Product.ArchivePrefix
        if ($Product.VersionedArchive) { $asset += $Version + $Product.Separator }
        $asset += 'windows' + $Product.Separator + $arch + '.zip'
        $archive = Join-Path $tmp $asset
        $checksums = Join-Path $tmp $Product.Checksums
        Get-ReleaseFile $asset $archive
        Get-ReleaseFile $Product.Checksums $checksums
        Write-Step 'Downloaded' $asset
        $selected = @()
        foreach ($line in [IO.File]::ReadAllLines($checksums, $utf8)) {
            if ($line -cmatch ('(?:^|\s)\*?' + [regex]::Escape($asset) + '\z')) { $selected += $line }
        }
        if ($selected.Count -ne 1 -or $selected[0] -cnotmatch ('\A([0-9a-fA-F]{64}) [ *]' + [regex]::Escape($asset) + '\z')) { Exit-Installer "No unique valid checksum for $asset." }
        $expected = $Matches[1].ToLowerInvariant()
        if ((Get-Digest $archive) -cne $expected) { Exit-Installer 'The download does not match its published SHA-256 checksum. Nothing was installed.' }
        Write-Step 'Verified' 'SHA-256 matches the selected immutable release'
        $stage = Join-Path $InstallDir ('.' + $Product.Command + '-stage-' + [guid]::NewGuid().ToString('N'))
        [void][IO.Directory]::CreateDirectory($stage)
        $nextState = Join-Path $stage 'state'
        [void][IO.Directory]::CreateDirectory($nextState)
        if ($Product.LegalFiles.Count -gt 0) { [void][IO.Directory]::CreateDirectory((Join-Path $nextState 'legal')) }
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $zip = [IO.Compression.ZipFile]::OpenRead($archive)
        try {
            $memberPrefix = if ($Product.RootedArchive) { $asset.Substring(0, $asset.Length - 4) + '/' } else { '' }
            foreach ($name in @($Product.Command + '.exe') + @($Product.LegalFiles)) {
                $member = $memberPrefix + $name
                $entries = @($zip.Entries | Where-Object { $_.FullName -ceq $member })
                if ($entries.Count -ne 1) { Exit-Installer "Archive has no unique regular $name." }
                $kind = ($entries[0].ExternalAttributes -shr 16) -band 0xf000
                if (($kind -ne 0 -and $kind -ne 0x8000) -or ($entries[0].ExternalAttributes -band 0x410)) { Exit-Installer "Archive member is not regular: $name" }
                $destination = if ($name -ceq ($Product.Command + '.exe')) { Join-Path $stage $name } else { Join-Path (Join-Path $nextState 'legal') $name }
                [IO.Compression.ZipFileExtensions]::ExtractToFile($entries[0], $destination)
            }
        } finally { $zip.Dispose() }
        $candidate = Join-Path $stage ($Product.Command + '.exe')
        $newVersion = Get-ProgramVersion $candidate (Join-Path $tmp 'version-error.txt')
        if ($Version -and $newVersion -cne $Version) { Exit-Installer "The downloaded program reports version $newVersion, not $Version. Nothing was installed." }
        Write-Step 'Tested' "$($Product.Command) $newVersion runs on this system"
        # Downloads/probing can take time; validate the state used for mutation.
        $receipt = Read-Receipt $false
        Invoke-RetiredRemoval $receipt
        $newReceipt = @{ Schema = 1; Directory = $InstallDir; Command = $Product.Command; Version = $newVersion; BinaryHash = Get-Digest $candidate; Legal = @(); DirectoryCreated = $created; PathEntry = $null; PathWasAbsent = $false; Retired = $null }
        if ($receipt) { $newReceipt.DirectoryCreated = $receipt.DirectoryCreated; $newReceipt.PathEntry = $receipt.PathEntry; $newReceipt.PathWasAbsent = $receipt.PathWasAbsent }
        foreach ($name in @($Product.LegalFiles)) { $newReceipt.Legal += @{ Name = $name; Hash = Get-Digest (Join-Path (Join-Path $nextState 'legal') $name) } }
        $addedPath = $false
        $retired = $null
        if ($receipt) {
            $retired = Join-Path $InstallDir ('.' + $Product.Command + '-retired-' + [guid]::NewGuid().ToString('N') + '.exe')
            # Record ownership before activating. If deletion later fails, the
            # running old image is already owned; no in-place receipt rewrite.
            $newReceipt.Retired = @{ Name = [IO.Path]::GetFileName($retired); Hash = $receipt.BinaryHash }
        }
        $movedOldBinary = $false
        $movedOldState = $false
        $newStateActive = $false
        $newBinaryActive = $false
        try {
            if ($ModifyPath) {
                $pathMutex = Enter-PathLock
                $pathRecord = Get-UserPath
                if (-not (Test-PathEntry $pathRecord)) {
                    $newReceipt.PathEntry = $InstallDir
                    $newReceipt.PathWasAbsent = -not $pathRecord.Exists
                    $pathRecord.Value = if ($pathRecord.Exists) { $InstallDir + ';' + $pathRecord.Value } else { $InstallDir }
                    $pathRecord.Exists = $true
                    Save-UserPath $pathRecord
                    $addedPath = $true
                }
            }
            Write-Receipt $nextState $newReceipt
            if ($receipt) {
                [IO.File]::Move($exe, $retired)
                $movedOldBinary = $true
                $previousState = Join-Path $stage 'previous-state'
                [IO.Directory]::Move($stateDir, $previousState)
                $backupState = $previousState
                $movedOldState = $true
            }
            [IO.Directory]::Move($nextState, $stateDir)
            $newStateActive = $true
            [IO.File]::Move($candidate, $exe)
            $newBinaryActive = $true
        } catch {
            $primary = $_.Exception.Message
            try {
                if ($newBinaryActive) { Remove-Item -LiteralPath $exe -Force }
                if ($newStateActive) { Invoke-StateRemoval $stateDir $newReceipt.Legal }
                if ($movedOldState) { [IO.Directory]::Move($backupState, $stateDir); $backupState = $null }
                if ($movedOldBinary) { [IO.File]::Move($retired, $exe) }
                if ($addedPath) { Invoke-OwnedPathRemoval $InstallDir $newReceipt.PathWasAbsent }
            } catch { Exit-Installer "Activation failed: $primary. Rollback also failed: $($_.Exception.Message). Inspect $InstallDir before retrying." }
            Exit-Installer "Activation failed; the prior installation was restored: $primary"
        }
        if ($retired) {
            try { Invoke-RetiredRemoval $newReceipt }
            catch { Write-Step 'Note' ("The prior executable remains recorded because cleanup failed: " + $_.Exception.Message) }
        }
        if ($backupState) {
            try { Invoke-StateRemoval $backupState $receipt.Legal; $backupState = $null }
            catch { Exit-Installer "The new version is active, but previous installer state could not be removed at $backupState`: $($_.Exception.Message)" }
        }
        if ($ModifyPath) {
            $onSessionPath = @($env:Path.Split([char]';') | Where-Object { $_ -and (Test-SamePath $_ $InstallDir) }).Count -gt 0
            if (-not $onSessionPath) { $env:Path = $InstallDir + ';' + $env:Path }
            Write-Step 'PATH' 'Available in this session; open a new terminal for the saved user PATH.'
        } else { Write-Step 'PATH' 'Left unchanged.' }
        $verb = if (-not $receipt) { 'installed' } elseif ($receipt.Version -ceq $newVersion) { 'reinstalled' } else { 'updated from ' + $receipt.Version }
        Write-Host "$($Product.Name) $newVersion is $verb in $InstallDir."
        Write-Host ('  Get started: & ' + (ConvertTo-QuotedCommand $exe) + ' --help')
        Write-Host '  Save this reviewed installer as install.ps1 and reuse it to uninstall:'
        Write-Host ('      & .\install.ps1 -Uninstall -InstallDir ' + (ConvertTo-QuotedCommand $InstallDir))
    } finally {
        if ($pathMutex) { $pathMutex.ReleaseMutex(); $pathMutex.Dispose() }
        if ($backupState) { Write-Step 'Note' "Previous state remains at $backupState; it was not discarded."; $stage = $null }
        if ($stage -and (Test-Path -LiteralPath $stage)) { Remove-Item -LiteralPath $stage -Recurse -Force }
        if ($tmp -and (Test-Path -LiteralPath $tmp)) { Remove-Item -LiteralPath $tmp -Recurse -Force }
        if ($lock) { $lock.Dispose(); Remove-Item -LiteralPath $lockPath -Force }
        if ($created -and $InstallDir -cne $pathRoot -and (Test-Path -LiteralPath $InstallDir) -and @(Get-ChildItem -LiteralPath $InstallDir -Force).Count -eq 0) { Remove-Item -LiteralPath $InstallDir -Force }
    }
}

# Validate bound empty values before resolving environment/default locations.
if ($PSBoundParameters.ContainsKey('Version') -and [string]::IsNullOrEmpty($Version)) { throw [ArgumentException]::new('-Version requires a nonempty release version.') }
if ($PSBoundParameters.ContainsKey('InstallDir') -and [string]::IsNullOrEmpty($InstallDir)) { throw [ArgumentException]::new('-InstallDir requires a nonempty absolute directory.') }
if (-not $PSBoundParameters.ContainsKey('Version')) { $Version = [Environment]::GetEnvironmentVariable($Product.Environment + '_VERSION') }
if (-not $PSBoundParameters.ContainsKey('InstallDir')) { $InstallDir = [Environment]::GetEnvironmentVariable($Product.Environment + '_INSTALL_DIR') }
if ($Help) {
    Write-Host "Install, update or uninstall $($Product.Name) for the current Windows user."
    Write-Host '-Version X.Y.Z  -InstallDir DIR  -NoModifyPath  -Uninstall  -Help'
    $names = @('VERSION', 'INSTALL_DIR', 'NO_MODIFY_PATH=1', 'UNINSTALL=1', 'DOWNLOAD_BASE') | ForEach-Object { $Product.Environment + '_' + $_ }
    Write-Host ('Environment: ' + ($names -join ', '))
    return
}
try {
    Invoke-CliInstaller -Version $Version -InstallDir $InstallDir -Product $Product -ModifyPath (-not ($NoModifyPath -or [Environment]::GetEnvironmentVariable($Product.Environment + '_NO_MODIFY_PATH') -eq '1')) -Uninstall ($Uninstall -or [Environment]::GetEnvironmentVariable($Product.Environment + '_UNINSTALL') -eq '1')
} catch {
    Write-Host ("Installer failed: " + $_.Exception.Message)
    Write-Host "Help: $($Product.Repository)/blob/main/docs/INSTALL.md"
    if ($MyInvocation.MyCommand.Path) { exit 1 }
    throw 'CLI installation did not complete.'
}
