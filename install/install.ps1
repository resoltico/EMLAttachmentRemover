# SPDX-License-Identifier: AGPL-3.0-only
# Self-contained user-scope CLI release installer. Keep this file ASCII for PS 5.1.
[CmdletBinding(PositionalBinding = $false)]
param([string]$Version, [string]$InstallDir, [switch]$NoModifyPath, [switch]$Uninstall, [switch]$ReplaceUnmanaged, [switch]$Help)

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
    LegalFiles = @('LICENSE', 'THIRD-PARTY-NOTICES.txt', 'PROJECT-NOTICE.md', 'LICENSE-COMMERCIAL.md', 'SOURCE.txt')
}
# END PRODUCT SETTINGS

function Invoke-CliInstaller {
    param([string]$Version, [string]$InstallDir, [bool]$ModifyPath, [bool]$Uninstall, [bool]$ReplaceUnmanaged, [hashtable]$Product)
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
        # An absent PATH is created as ExpandString. A later String container
        # belongs to the user's edit, even when removing our last entry empties it.
        if ($parts.Count -eq 0 -and $WasAbsent -and $record.Kind -eq [Microsoft.Win32.RegistryValueKind]::ExpandString) { $record.Exists = $false }
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
    function Read-Receipt([bool]$AllowMissing, [string]$Directory = $stateDir, [string]$Program = $exe, [string]$ExpectedHash = '') {
        # Recovery validates the same sealed inventory at its private staged path.
        $stateDir = $Directory
        $exe = $Program
        $bin = Get-ItemOrNull $exe
        $state = Get-ItemOrNull $stateDir
        if (-not $state -and $ExpectedHash) { Exit-Installer "Expected owned installer state is missing: $Directory. Preserve the transaction and its original executable backup for inspection." }
        if (-not $state) {
            if ($bin) {
                [void](Assert-Regular $exe)
                if (-not $ReplaceUnmanaged -or $Uninstall) {
                    Exit-Installer "No ownership receipt exists for the regular executable at $exe. Nothing was replaced or removed. To preserve it in a recovery backup and install a verified release, rerun this installer with -ReplaceUnmanaged -InstallDir $(ConvertTo-QuotedCommand $InstallDir). Existing PATH entries will not be adopted."
                }
            }
            return $null
        }
        if (-not $state.PSIsContainer -or ($state.Attributes -band [IO.FileAttributes]::ReparsePoint)) { Exit-Installer "Unmanaged installer state; left untouched: $stateDir" }
        try {
            $receiptPath = Join-Path $stateDir 'receipt.json'
            $sealPath = Join-Path $stateDir 'receipt.sha256'
            [void](Assert-Regular $receiptPath)
            [void](Assert-Regular $sealPath)
            if ($ExpectedHash -and (Get-Digest $receiptPath) -cne $ExpectedHash) { throw 'receipt differs from the verified transaction input' }
            $seal = [IO.File]::ReadAllText($sealPath)
            if ($seal -cnotmatch '\A[0-9a-f]{64}(?:\r?\n)?\z' -or (Get-Digest $receiptPath) -cne $seal.TrimEnd("`r", "`n")) { throw 'installer receipt changed or its seal is invalid' }
            $r = [IO.File]::ReadAllText($receiptPath) | ConvertFrom-Json
            if ($r.Directory -isnot [string]) { throw 'receipt has no valid install directory' }
            if (-not [string]::Equals((Get-AbsoluteDirectory $r.Directory), $InstallDir, [StringComparison]::OrdinalIgnoreCase)) { throw "receipt records a different directory: $($r.Directory). If that original ordinary directory still exists, rerun with -InstallDir $(ConvertTo-QuotedCommand $r.Directory); moved receipts and reparse aliases are not adopted" }
            if ($r.Schema -ne 1 -or $r.Command -cne $Product.Command -or $r.DirectoryCreated -isnot [bool] -or $r.PathWasAbsent -isnot [bool]) { throw 'invalid receipt identity or flags' }
            if ([string]$r.Version -cnotmatch ('\A' + $semver + '\z')) { throw 'invalid receipt version' }
            if ($null -ne $r.PathEntry -and ($r.PathEntry -isnot [string] -or -not (Test-SamePath $r.PathEntry $InstallDir))) { throw 'invalid owned PATH entry' }
            # Existing ownership comes from the sealed receipt inventory;
            # current settings define the complete NEW package requirements.
            $expected = @()
            if ([string]$r.BinaryHash -notmatch '\A[0-9a-f]{64}\z') { throw 'invalid binary digest' }
            $known = @('receipt.json', 'receipt.sha256')
            foreach ($legal in @($r.Legal)) {
                if ($legal.Name -isnot [string] -or @($Product.LegalFiles) -cnotcontains $legal.Name -or $expected -ccontains $legal.Name -or [string]$legal.Hash -cnotmatch '\A[0-9a-f]{64}\z') { throw 'invalid or duplicate owned legal file' }
                $expected += $legal.Name
                $known += Join-Path 'legal' $legal.Name
            }
            foreach ($item in Get-ChildItem -LiteralPath $stateDir -Recurse -Force) {
                if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'reparse point in installer state' }
                $relative = $item.FullName.Substring($stateDir.Length + 1)
                if ($item.PSIsContainer) { if ($expected.Count -eq 0 -or $relative -cne 'legal') { throw 'unowned state directory' } }
                elseif ($known -cnotcontains $relative) { throw 'unowned file in installer state' }
            }
            if (Assert-Regular $exe $AllowMissing) { if ((Get-Digest $exe) -cne $r.BinaryHash) { throw "managed executable has changed: $exe" } }
            foreach ($legal in @($r.Legal)) {
                $path = Join-Path (Join-Path $stateDir 'legal') $legal.Name
                if (Assert-Regular $path) { if ((Get-Digest $path) -cne $legal.Hash) { throw "managed legal file has changed: $path" } }
            }
            if ($null -ne $r.Retired) {
                if ([string]$r.Retired.Name -cnotmatch ('\A\.' + [regex]::Escape($Product.Command) + '-retired-[0-9a-f]{32}\.exe\z') -or [string]$r.Retired.Hash -notmatch '\A[0-9a-f]{64}\z') { throw 'invalid retired-file ownership' }
                $path = Join-Path $InstallDir $r.Retired.Name
                if (Assert-Regular $path $true) { if ((Get-Digest $path) -cne $r.Retired.Hash) { throw "retired executable has changed: $path" } }
            }
            return @{ Schema = 1; Directory = $r.Directory; Command = $r.Command; Version = $r.Version; BinaryHash = $r.BinaryHash; Legal = @($r.Legal); DirectoryCreated = $r.DirectoryCreated; PathEntry = $r.PathEntry; PathWasAbsent = $r.PathWasAbsent; Retired = $r.Retired }
        } catch { Exit-Installer "Cannot verify installer state at $stateDir`: $($_.Exception.Message). Preserve and inspect this exact state and its files before retrying; -ReplaceUnmanaged cannot bypass managed or corrupt state." }
    }
    function Get-InstallationFingerprint {
        $directory = Get-ItemOrNull $InstallDir
        if ($directory -and (-not $directory.PSIsContainer -or ($directory.Attributes -band [IO.FileAttributes]::ReparsePoint))) { Exit-Installer "Install directory changed or is no longer an ordinary directory: $InstallDir. Nothing was replaced." }
        # Include payload presence and each verified state byte; staging and
        # unrelated directory contents do not become replacement authority.
        $parts = [Collections.Generic.List[string]]::new()
        if (Assert-Regular $exe $true) { $parts.Add('binary:' + (Get-Digest $exe)) }
        else { $parts.Add('binary:absent') }
        $state = Get-ItemOrNull $stateDir
        if ($state) {
            foreach ($file in @(Get-ChildItem -LiteralPath $stateDir -Recurse -Force | Where-Object { -not $_.PSIsContainer } | Sort-Object FullName)) {
                $parts.Add($file.FullName.Substring($stateDir.Length) + ':' + (Get-Digest $file.FullName))
            }
        } else { $parts.Add('state:absent') }
        return [string]::Join("`n", $parts.ToArray())
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
    function Restore-Activation([hashtable]$Record) {
        # Inspect actual consumed sources and unique backup slots. PowerShell can
        # cancel between a completed .NET move and the next script statement.
        $failures = [Collections.Generic.List[string]]::new()
        try {
            $previous = Get-ItemOrNull $Record.PreviousState
            $next = Get-ItemOrNull $Record.NextState
            $current = Get-ItemOrNull $stateDir
            if ($previous) {
                if (-not $Record.OldReceiptHash) { throw "Unexpected prior state slot: $($Record.PreviousState)" }
                $prior = Read-Receipt $true $Record.PreviousState $Record.OldBinarySlot $Record.OldReceiptHash
                if (-not $prior) { throw "Prior receipt is missing: $($Record.PreviousState)" }
                if ($current) {
                    if ($next) { throw "Both active and staged ownership exist: $stateDir and $($Record.NextState)" }
                    $active = Read-Receipt $true $stateDir $Record.Candidate $Record.NewReceiptHash
                    if (-not $active) { throw "Activated receipt is missing: $stateDir" }
                    Invoke-StateRemoval $stateDir $active.Legal
                }
                [IO.Directory]::Move($Record.PreviousState, $stateDir)
            } elseif (-not $next -and -not $Record.OldReceiptHash) {
                if ($current) {
                    $active = Read-Receipt $true $stateDir $Record.Candidate $Record.NewReceiptHash
                    if (-not $active) { throw "Activated receipt is missing: $stateDir" }
                    Invoke-StateRemoval $stateDir $active.Legal
                }
            } elseif ($Record.OldReceiptHash) {
                $prior = Read-Receipt $true $stateDir $Record.OldBinarySlot $Record.OldReceiptHash
                if (-not $prior) { throw "Prior ownership cannot be restored from $($Record.PreviousState)" }
            } elseif ($current) { throw "Unexpected installer state appeared: $stateDir" }
        } catch { $failures.Add("Ownership restoration: $($_.Exception.Message)") }
        try {
            $slot = if ($Record.OldBinarySlot) { Get-ItemOrNull $Record.OldBinarySlot } else { $null }
            $current = Get-ItemOrNull $exe
            $candidate = Get-ItemOrNull $Record.Candidate
            $restore = $true
            if ($current) {
                [void](Assert-Regular $exe)
                $digest = Get-Digest $exe
                if ($Record.HadBinary -and $digest -ceq $Record.OldBinaryHash -and ($candidate -or -not $slot -or $digest -cne $Record.NewBinaryHash)) {
                    $restore = $false
                } elseif (-not $candidate -and $digest -ceq $Record.NewBinaryHash) {
                    if ($Record.HadBinary) {
                        [void](Assert-Regular $Record.OldBinarySlot)
                        if ((Get-Digest $Record.OldBinarySlot) -cne $Record.OldBinaryHash) { throw "Original executable backup changed: $($Record.OldBinarySlot)" }
                    }
                    Remove-Item -LiteralPath $exe -Force
                } else { throw "Executable changed or appeared during activation; preserved: $exe" }
            }
            if ($restore -and $Record.HadBinary) {
                [void](Assert-Regular $Record.OldBinarySlot)
                if ((Get-Digest $Record.OldBinarySlot) -cne $Record.OldBinaryHash) { throw "Original executable backup changed: $($Record.OldBinarySlot)" }
                [IO.File]::Move($Record.OldBinarySlot, $exe)
            }
        } catch { $failures.Add("Executable restoration: $($_.Exception.Message)") }
        if ($Record.PathBefore) {
            try {
                $current = Get-UserPath
                $before = $Record.PathBefore
                $expected = $Record.PathExpected
                $unchanged = $current.Exists -eq $before.Exists -and $current.Value -ceq $before.Value -and $current.Kind -eq $before.Kind
                if (-not $unchanged) {
                    $parts = [Collections.Generic.List[string]]::new()
                    foreach ($part in $current.Value.Split([char]';')) { $parts.Add($part) }
                    if ($parts.Contains($InstallDir)) {
                        # The original raw record, or appended concurrent content,
                        # must still follow our exact first entry. Never restore a
                        # stale PATH snapshot or remove a reordered ambiguous entry.
                        if (-not $current.Exists -or ($current.Value -cne $expected.Value -and -not $current.Value.StartsWith($expected.Value + ';', [StringComparison]::Ordinal))) { throw 'User PATH changed ambiguously; the planned entry was retained for inspection' }
                        $parts.RemoveAt(0)
                        $current.Value = [string]::Join(';', $parts.ToArray())
                        if ($parts.Count -eq 0 -and -not $before.Exists -and $current.Kind -eq $expected.Kind) { $current.Exists = $false }
                        Save-UserPath $current
                    }
                }
            } catch { $failures.Add("PATH restoration (exact entry $InstallDir): $($_.Exception.Message)") }
        }
        return [string]::Join('; ', $failures.ToArray())
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

    if ($Uninstall -and $ReplaceUnmanaged) { Exit-Installer '-ReplaceUnmanaged is install-only; it cannot authorize uninstall.' }
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
    # Inspect known local conflicts without creating a lock or doing network work.
    if (Get-ItemOrNull $lockPath) { Exit-Installer "Installer lock exists at $lockPath. Nothing was changed. Wait for its owner, or inspect the exact lock after confirming no installer is running." }
    if ($existingDir) {
        foreach ($oldStage in @(Get-ChildItem -LiteralPath $InstallDir -Force -Filter ('.' + $Product.Command + '-stage-*'))) {
            Exit-Installer "Incomplete earlier transaction evidence remains at $($oldStage.FullName). Nothing was changed. Preserve that directory and inspect recovery.txt, previous-state and any reported executable recovery paths before retrying."
        }
    }
    if ($ModifyPath) {
        $preflightPathMutex = Enter-PathLock
        $preflightPathMutex.ReleaseMutex(); $preflightPathMutex.Dispose()
    }
    $receipt = Read-Receipt $true
    $preflight = Get-InstallationFingerprint
    $unmanaged = -not $receipt -and $null -ne (Get-ItemOrNull $exe)
    if ($unmanaged -and -not [IO.Directory]::GetParent($InstallDir)) { Exit-Installer "Cannot preserve a recovery backup outside the filesystem-root install directory $InstallDir. Nothing was changed. Preserve the unowned executable manually before choosing a non-root directory." }
    if ($unmanaged) { Write-Step 'Replace' "Only $exe will be replaced after verification; its original bytes will be retained outside $InstallDir. Existing PATH entries and directory ownership will not be adopted." }
    $created = -not $existingDir
    if ($created) { [void][IO.Directory]::CreateDirectory($InstallDir) }
    $lock = $null
    $tmp = $null
    $stage = $null
    $activation = $null
    $preserveStage = $false
    $pathMutex = $null
    try {
        try { $lock = [IO.File]::Open($lockPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None) }
        catch { Exit-Installer "Another installer may be using $InstallDir, or its lock remains after interruption. Nothing was changed. Close it or inspect $lockPath before retrying." }
        $receipt = Read-Receipt $true
        if ((Get-InstallationFingerprint) -cne $preflight) { Exit-Installer "Installation changed after preflight at $exe or $stateDir. Nothing was replaced or removed; inspect the current files and rerun." }
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
        $receipt = Read-Receipt $true
        if ((Get-InstallationFingerprint) -cne $preflight) { Exit-Installer "Installation changed after preflight at $exe or $stateDir. Nothing was replaced or removed; inspect the current files and rerun." }
        Invoke-RetiredRemoval $receipt
        $newReceipt = @{ Schema = 1; Directory = $InstallDir; Command = $Product.Command; Version = $newVersion; BinaryHash = Get-Digest $candidate; Legal = @(); DirectoryCreated = $created; PathEntry = $null; PathWasAbsent = $false; Retired = $null }
        if ($receipt) { $newReceipt.DirectoryCreated = $receipt.DirectoryCreated; $newReceipt.PathEntry = $receipt.PathEntry; $newReceipt.PathWasAbsent = $receipt.PathWasAbsent }
        foreach ($name in @($Product.LegalFiles)) { $newReceipt.Legal += @{ Name = $name; Hash = Get-Digest (Join-Path (Join-Path $nextState 'legal') $name) } }
        $retired = $null
        $hadBinary = $null -ne (Get-ItemOrNull $exe)
        if ($receipt -and $hadBinary) {
            $retired = Join-Path $InstallDir ('.' + $Product.Command + '-retired-' + [guid]::NewGuid().ToString('N') + '.exe')
            $newReceipt.Retired = @{ Name = [IO.Path]::GetFileName($retired); Hash = $receipt.BinaryHash }
        }
        $backupPath = if ($unmanaged) { Join-Path ([IO.Directory]::GetParent($InstallDir).FullName) ('.' + $Product.Command + '-unmanaged-' + [guid]::NewGuid().ToString('N') + '.exe') } else { $retired }
        try {
            $pathBefore = $null; $pathExpected = $null
            if ($ModifyPath) {
                $pathMutex = Enter-PathLock
                $pathRecord = Get-UserPath
                if (-not (Test-PathEntry $pathRecord)) {
                    $pathBefore = $pathRecord.Clone()
                    $newReceipt.PathEntry = $InstallDir
                    $newReceipt.PathWasAbsent = -not $pathRecord.Exists
                    $pathRecord.Value = if ($pathRecord.Exists) { $InstallDir + ';' + $pathRecord.Value } else { $InstallDir }
                    $pathRecord.Exists = $true
                    $pathExpected = $pathRecord
                }
            }
            $receipt = Read-Receipt $true
            if ((Get-InstallationFingerprint) -cne $preflight) { Exit-Installer "Installation changed after preflight at $exe or $stateDir. Nothing was replaced or removed; inspect the current files and rerun." }
            Write-Receipt $nextState $newReceipt
            $prepared = @{
                PreviousState = Join-Path $stage 'previous-state'; NextState = $nextState; Candidate = $candidate
                OldReceiptHash = if ($receipt) { Get-Digest (Join-Path $stateDir 'receipt.json') } else { '' }
                NewReceiptHash = Get-Digest (Join-Path $nextState 'receipt.json')
                HadBinary = $hadBinary; OldBinarySlot = if ($hadBinary) { $backupPath } else { Join-Path $stage 'prior-payload-absent' }
                OldBinaryHash = if ($hadBinary) { Get-Digest $exe } else { '' }; NewBinaryHash = $newReceipt.BinaryHash
                PathBefore = $pathBefore; PathExpected = $pathExpected; Committed = $false; Failure = ''
            }
            $recovery = "Activation pending for $exe. Preserve this transaction if interruption or restoration fails. Original executable recovery: $backupPath. Prior ownership recovery: $($prepared.PreviousState). New ownership: $nextState. New executable: $candidate. Exact planned PATH entry: $($newReceipt.PathEntry). No whole PATH value will be restored. Transaction facts:`n" + ($prepared | ConvertTo-Json -Depth 8)
            [IO.File]::WriteAllText((Join-Path $stage 'recovery.txt'), $recovery, $utf8)
            if ($unmanaged) { Write-Step 'Recovery' "Planned original executable backup: $backupPath. It will be retained there after successful replacement." }
            $activation = $prepared
            if ($pathExpected) { Save-UserPath $pathExpected }
            $receipt = Read-Receipt $true
            if ((Get-InstallationFingerprint) -cne $preflight) { Exit-Installer "Installation changed after preflight at $exe or $stateDir. Nothing was replaced or removed; inspect the current files and rerun." }
            if ($receipt) {
                if ($hadBinary) { [IO.File]::Move($exe, $retired) }
                [IO.Directory]::Move($stateDir, $activation.PreviousState)
            }
            if ($unmanaged) { [IO.File]::Move($exe, $backupPath) }
            [IO.Directory]::Move($nextState, $stateDir)
            [IO.File]::Move($candidate, $exe)
            [void](Read-Receipt $false $stateDir $exe $activation.NewReceiptHash)
            $activation.Committed = $true
        } catch {
            if ($activation) { $activation.Failure = $_.Exception.Message }
            throw
        }
        if ($retired) {
            try { Invoke-RetiredRemoval $newReceipt }
            catch { Write-Step 'Note' ("The prior executable remains recorded because cleanup failed: " + $_.Exception.Message) }
        }
        if ($receipt) {
            try { Invoke-StateRemoval $activation.PreviousState $receipt.Legal }
            catch { $preserveStage = $true; Exit-Installer "The new version is active, but previous installer state could not be removed at $($activation.PreviousState): $($_.Exception.Message)" }
        }
        if ($unmanaged) { Write-Step 'Recovery' "Original unmanaged executable retained at $backupPath. Uninstall will not remove it. To recover it, first uninstall the new managed executable, then move this exact backup to $exe." }
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
        $restoreFailure = ''
        if ($activation -and -not $activation.Committed) {
            $preserveStage = $true
            $restoreFailure = Restore-Activation $activation
            if (-not $restoreFailure) { $preserveStage = $false }
            else {
                $detail = "`nRestoration is incomplete: $restoreFailure. Preserve transaction evidence at $stage and the exact recovery paths above."
                try { [IO.File]::AppendAllText((Join-Path $stage 'recovery.txt'), $detail, $utf8) }
                catch { Write-Step 'Note' "Could not update recovery.txt; preserve transaction evidence at $stage." }
            }
        }
        if ($pathMutex) { $pathMutex.ReleaseMutex(); $pathMutex.Dispose() }
        if ($stage -and -not $preserveStage -and (Test-Path -LiteralPath $stage)) { Remove-Item -LiteralPath $stage -Recurse -Force }
        if ($tmp -and (Test-Path -LiteralPath $tmp)) { Remove-Item -LiteralPath $tmp -Recurse -Force }
        if ($lock) { $lock.Dispose(); Remove-Item -LiteralPath $lockPath -Force }
        if ($created -and $InstallDir -cne $pathRoot -and (Test-Path -LiteralPath $InstallDir) -and @(Get-ChildItem -LiteralPath $InstallDir -Force).Count -eq 0) { Remove-Item -LiteralPath $InstallDir -Force }
        if ($activation -and -not $activation.Committed -and $activation.Failure) {
            if ($restoreFailure) { Exit-Installer "Activation failed: $($activation.Failure). Restoration is incomplete: $restoreFailure. Preserve transaction evidence at $stage." }
            Exit-Installer "Activation failed; this transaction's changes were rolled back: $($activation.Failure)"
        }
    }
}

# Validate bound empty values before resolving environment/default locations.
if ($PSBoundParameters.ContainsKey('Version') -and [string]::IsNullOrEmpty($Version)) { throw [ArgumentException]::new('-Version requires a nonempty release version.') }
if ($PSBoundParameters.ContainsKey('InstallDir') -and [string]::IsNullOrEmpty($InstallDir)) { throw [ArgumentException]::new('-InstallDir requires a nonempty absolute directory.') }
if (-not $PSBoundParameters.ContainsKey('Version')) { $Version = [Environment]::GetEnvironmentVariable($Product.Environment + '_VERSION') }
if (-not $PSBoundParameters.ContainsKey('InstallDir')) { $InstallDir = [Environment]::GetEnvironmentVariable($Product.Environment + '_INSTALL_DIR') }
if ($Help) {
    Write-Host "Install, update or uninstall $($Product.Name) for the current Windows user."
    Write-Host '-Version X.Y.Z  -InstallDir DIR  -NoModifyPath  -Uninstall  -ReplaceUnmanaged  -Help'
    $names = @('VERSION', 'INSTALL_DIR', 'NO_MODIFY_PATH=1', 'UNINSTALL=1', 'DOWNLOAD_BASE') | ForEach-Object { $Product.Environment + '_' + $_ }
    Write-Host ('Environment: ' + ($names -join ', '))
    return
}
try {
    Invoke-CliInstaller -Version $Version -InstallDir $InstallDir -Product $Product -ReplaceUnmanaged ([bool]$ReplaceUnmanaged) -ModifyPath (-not ($NoModifyPath -or [Environment]::GetEnvironmentVariable($Product.Environment + '_NO_MODIFY_PATH') -eq '1')) -Uninstall ($Uninstall -or [Environment]::GetEnvironmentVariable($Product.Environment + '_UNINSTALL') -eq '1')
} catch {
    Write-Host ("Installer failed: " + $_.Exception.Message)
    Write-Host "Help: $($Product.Repository)/blob/main/docs/INSTALL.md"
    if ($MyInvocation.MyCommand -is [Management.Automation.ExternalScriptInfo]) { exit 1 }
    throw 'CLI installation did not complete.'
}
