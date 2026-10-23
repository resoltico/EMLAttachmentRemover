#requires -Version 7.4
<##
Run only on an elevated, disposable GitHub-hosted Windows runner. All storage
changes address a newly created VHD held by this process. In particular this
does not change the host's global 8.3 policy or any existing volume's policy.
The attachment deliberately omits PERMANENT_LIFETIME: closing its kernel handle
also ends the attachment if this PowerShell process is terminated.
https://learn.microsoft.com/windows/win32/api/virtdisk/nf-virtdisk-attachvirtualdisk
##>
[CmdletBinding()]
param(
    [Parameter(Mandatory, Position = 0)][string] $Binary,
    [Parameter(Mandatory, Position = 1)][string] $Out
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$profileIds = @('ntfs-case-insensitive-8dot3', 'ntfs-case-sensitive-8dot3')
$Binary = [IO.Path]::GetFullPath($Binary)
$Out = [IO.Path]::GetFullPath($Out)
if (Test-Path -LiteralPath $Out) { throw "Evidence directory must not exist: $Out" }
if (!(Test-Path -LiteralPath ([IO.Path]::GetDirectoryName($Out)) -PathType Container)) {
    throw 'Evidence directory parent must already exist.'
}
[void][IO.Directory]::CreateDirectory($Out)
$result = [ordered]@{
    schema = 1; binary_sha256 = $null; passed = $false
    host = [ordered]@{ os = [Runtime.InteropServices.RuntimeInformation]::OSDescription; architecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString(); powershell = $PSVersionTable.PSVersion.ToString() }
    profiles = @($profileIds | ForEach-Object { [ordered]@{ profile = $_; status = 'failed_or_unavailable'; exit_code = 1; executed = $false; behavior = $null; failure = 'Profile was not executed.' } })
    storage = $null; cleanup = [ordered]@{ detached = $false; observation_complete = $false; forced_detach = $false }
    failure = $null
}
$volumeOwner = $null
$image = Join-Path $Out 'owned-name-volume.vhd'
$commandSequence = 0
$phase = 'prerequisites'
function Error-Facts([Management.Automation.ErrorRecord] $Record) {
    $fields = [ordered]@{
        message = $Record.Exception.Message; exception_type = $Record.Exception.GetType().FullName
        hresult = $Record.Exception.HResult; error_id = $Record.FullyQualifiedErrorId
        category = $Record.CategoryInfo.ToString(); position = $Record.InvocationInfo.PositionMessage
        script_stack = $Record.ScriptStackTrace; detail = $Record.ToString()
        native_error_code = $null; inner_exception_type = $null
    }
    $inner = $Record.Exception
    for ($depth = 0; $depth -lt 4 -and $null -ne $inner; $depth++) {
        if ($inner -is [ComponentModel.Win32Exception]) { $fields.native_error_code = $inner.NativeErrorCode; $fields.inner_exception_type = $inner.GetType().FullName; break }
        $inner = $inner.InnerException
    }
    foreach ($key in @($fields.Keys)) {
        if ($null -ne $fields[$key] -and $fields[$key] -is [string] -and $fields[$key].Length -gt 8192) { $fields[$key] = $fields[$key].Substring(0,8192) }
    }
    return $fields
}
function Invoke-Observed([string] $Program, [string[]] $Arguments, [string] $Directory, [string] $Name) {
    $script:commandSequence++
    $script:phase = "native-command:$Name"
    $stem = Join-Path $Directory ('{0:D3}-{1}' -f $script:commandSequence, $Name)
    $observation = [ordered]@{ program = $Program; arguments = $Arguments; phase = 'before-start'; started = $false; process_id = $null; settled = $false; exit_code = $null; stdout = $null; stderr = $null; error = $null; cleanup_error = $null }
    $process = [Diagnostics.Process]::new()
    $stdout = $null; $stderr = $null; $stdoutCopy = $null; $stderrCopy = $null
    try {
        # Literal streams avoid PowerShell provider wildcard expansion of \?\ paths.
        $stdout = [IO.File]::Open("$stem.stdout", [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
        $stderr = [IO.File]::Open("$stem.stderr", [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
        $process.StartInfo.FileName = $Program
        $process.StartInfo.UseShellExecute = $false
        $process.StartInfo.RedirectStandardOutput = $true
        $process.StartInfo.RedirectStandardError = $true
        foreach ($argument in $Arguments) { $process.StartInfo.ArgumentList.Add($argument) }
        $observation | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath "$stem.json" -Encoding utf8NoBOM
        if (!$process.Start()) { throw 'Native command did not start.' }
        $observation.started = $true; $observation.process_id = $process.Id; $observation.phase = 'draining'
        $stdoutCopy = $process.StandardOutput.BaseStream.CopyToAsync($stdout)
        $stderrCopy = $process.StandardError.BaseStream.CopyToAsync($stderr)
        $pending = [Collections.Generic.List[Threading.Tasks.Task]]::new()
        $pending.Add($stdoutCopy); $pending.Add($stderrCopy); $pending.Add($process.WaitForExitAsync())
        while ($pending.Count -gt 0) {
            $completed = [Threading.Tasks.Task]::WhenAny($pending.ToArray()).GetAwaiter().GetResult()
            [void]$completed.GetAwaiter().GetResult()
            [void]$pending.Remove($completed)
        }
        $process.WaitForExit()
        $observation.settled = $true; $observation.exit_code = $process.ExitCode
        $stdout.Dispose(); $stdout = $null; $stderr.Dispose(); $stderr = $null
        $observation.phase = 'decode'
        $observation.stdout = [IO.File]::ReadAllText("$stem.stdout", [Text.UTF8Encoding]::new($false,$true))
        $observation.stderr = [IO.File]::ReadAllText("$stem.stderr", [Text.UTF8Encoding]::new($false,$false))
        $observation.phase = 'complete'
        if ($observation.exit_code -ne 0) { throw "$Name failed with exit $($observation.exit_code); see $stem.json" }
        return $observation
    } catch {
        $observation.error = Error-Facts $_
        throw
    } finally {
        try {
            if ($observation.started) {
                # The enclosing Rust-owned Job enforces the campaign deadline and owns descendants.
                # This retained Process terminates only the exact child it started on local failure.
                if (!$process.HasExited) { $process.Kill() }
                $process.WaitForExit()
                $observation.settled = $true; $observation.exit_code = $process.ExitCode
                foreach ($copy in @($stdoutCopy,$stderrCopy)) {
                    if ($null -ne $copy) {
                        try { [void]$copy.GetAwaiter().GetResult() }
                        catch { if ($null -eq $observation.cleanup_error) { $observation.cleanup_error = Error-Facts $_ } }
                    }
                }
            }
        } catch { $observation.cleanup_error = Error-Facts $_ }
        finally {
            if ($null -ne $stdout) { $stdout.Dispose() }
            if ($null -ne $stderr) { $stderr.Dispose() }
            $process.Dispose()
            $observation | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath "$stem.json" -Encoding utf8NoBOM
        }
        if ($null -ne $observation.cleanup_error -and $null -eq $observation.error) { throw 'Native command cleanup failed; see invocation receipt.' }
    }
}
function Namespace-Facts([string] $Root) {
    $paths = @((Get-Item -LiteralPath (Verbatim $Root))) + @(Get-ChildItem -LiteralPath (Verbatim $Root) -Recurse -Force)
    return @($paths | Sort-Object FullName -CaseSensitive | ForEach-Object {
        # NTFS enumeration attributes may be stale. Identity and basic metadata
        # come from the same retained read-attributes handle instead.
        $native = [EmlOwnedNameVolume]::NamespaceMetadata((Verbatim $_.FullName))
        [ordered]@{ path = $_.FullName; attributes = $native.Attributes; last_write_ticks = $native.LastWriteTicks; identity = $native.Identity; bytes = if ($_.PSIsContainer) { $null } else { $_.Length }; sha256 = if ($_.PSIsContainer) { $null } else { (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() } }
    })
}
function Verbatim([string] $Path) {
    if ($Path.StartsWith('\\?\')) { return $Path }
    if ($Path.StartsWith('\\')) { throw 'Qualification paths must be local drive paths.' }
    return '\\?\' + [IO.Path]::GetFullPath($Path)
}
try {
    if (![Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([Runtime.InteropServices.OSPlatform]::Windows)) {
        throw 'Native Windows is required; no NTFS profile was executed.'
    }
    if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or $env:CI -ne 'true') {
        throw 'An exclusively owned disposable GitHub-hosted Windows runner is required.'
    }
    $principal = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
    if (!$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Elevated Windows storage privileges are required.' }
    if (!(Test-Path -LiteralPath $Binary -PathType Leaf)) { throw 'Frozen CLI binary is missing.' }
    $result.binary_sha256 = (Get-FileHash -LiteralPath $Binary -Algorithm SHA256).Hash.ToLowerInvariant()
    $oracle = Join-Path $PSScriptRoot 'check_name_profile.py'
    if (!(Test-Path -LiteralPath $oracle -PathType Leaf)) { throw 'Independent naming oracle is missing.' }
    $python = (Get-Command python.exe -CommandType Application -ErrorAction Stop).Source
    if ($python -like '*WindowsApps*') { throw 'A real Python installation is required, not a Store execution alias.' }
    $fsutil = Join-Path $env:SystemRoot 'System32\fsutil.exe'
    if (!(Test-Path -LiteralPath $fsutil -PathType Leaf)) { throw 'Native fsutil.exe is missing.' }
    $result.tools = [ordered]@{ fsutil = $fsutil; fsutil_sha256 = (Get-FileHash -LiteralPath $fsutil -Algorithm SHA256).Hash.ToLowerInvariant(); python = $python; python_sha256 = (Get-FileHash -LiteralPath $python -Algorithm SHA256).Hash.ToLowerInvariant(); process_architecture = [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString(); powershell_home = $PSHOME; current_directory = [IO.Directory]::GetCurrentDirectory() }
    if (![IO.Directory]::Exists($result.tools.current_directory)) { throw 'Native command current directory is missing.' }
    foreach ($command in @('Get-DiskImage', 'Get-Disk', 'Initialize-Disk', 'New-Partition', 'Format-Volume', 'Get-Volume')) {
        [void](Get-Command $command -ErrorAction Stop)
    }
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;
public sealed class EmlOwnedNameVolume : IDisposable {
    [StructLayout(LayoutKind.Sequential)] struct StorageType { public uint Device; public Guid Vendor; }
    [StructLayout(LayoutKind.Sequential)] struct CreateV1 { public Guid Id; public ulong MaximumSize; public uint BlockSize; public uint SectorSize; public IntPtr Parent; public IntPtr Source; }
    [StructLayout(LayoutKind.Sequential)] struct CreateParameters { public uint Version; public CreateV1 V1; }
    [StructLayout(LayoutKind.Sequential)] struct AttachParameters { public uint Version; public uint Reserved; }
    [StructLayout(LayoutKind.Sequential)] struct CaseInformation { public uint Flags; }
    [StructLayout(LayoutKind.Sequential)] struct FileIdentity { public ulong Volume; public ulong Low; public ulong High; }
    [StructLayout(LayoutKind.Sequential)] struct FileBasicInformation { public long CreationTime; public long LastAccessTime; public long LastWriteTime; public long ChangeTime; public uint Attributes; }
    public sealed class NamespaceEntryMetadata { public string Identity; public uint Attributes; public long LastWriteTicks; }
    [DllImport("virtdisk.dll", CharSet=CharSet.Unicode)] static extern uint CreateVirtualDisk(ref StorageType type, string path, uint access, IntPtr security, uint flags, uint providerFlags, ref CreateParameters parameters, IntPtr overlapped, out IntPtr handle);
    [DllImport("virtdisk.dll")] static extern uint AttachVirtualDisk(SafeFileHandle handle, IntPtr security, uint flags, uint providerFlags, ref AttachParameters parameters, IntPtr overlapped);
    [DllImport("virtdisk.dll")] static extern uint DetachVirtualDisk(SafeFileHandle handle, uint flags, uint providerFlags);
    [DllImport("virtdisk.dll", CharSet=CharSet.Unicode)] static extern uint GetVirtualDiskPhysicalPath(SafeFileHandle handle, ref uint bytes, StringBuilder path);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern SafeFileHandle CreateFileW(string path, uint access, uint share, IntPtr security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetFileInformationByHandleEx(SafeFileHandle file, int kind, out CaseInformation info, uint size);
    [DllImport("kernel32.dll", EntryPoint="GetFileInformationByHandleEx", SetLastError=true)] static extern bool GetFileIdentity(SafeFileHandle file, int kind, out FileIdentity info, uint size);
    [DllImport("kernel32.dll", EntryPoint="GetFileInformationByHandleEx", SetLastError=true)] static extern bool GetFileBasicInformation(SafeFileHandle file, int kind, out FileBasicInformation info, uint size);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern uint GetShortPathNameW(string path, StringBuilder output, uint size);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode)] static extern uint GetDriveTypeW(string root);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool GetVolumeInformationW(string root, StringBuilder label, uint labelSize, out uint serial, out uint component, out uint flags, StringBuilder fs, uint fsSize);
    SafeFileHandle owned;
    bool attached;
    static void Check(uint code) { if(code != 0) throw new Win32Exception((int)code); }
    public EmlOwnedNameVolume(string path) {
        if(File.Exists(path)) throw new IOException("Owned VHD path already exists");
        var type = new StorageType { Device=2, Vendor=new Guid("EC984AEC-A0F9-47e9-901F-71415A66345B") };
        var parameters = new CreateParameters { Version=1, V1=new CreateV1 { Id=Guid.NewGuid(), MaximumSize=1073741824, SectorSize=512 } };
        IntPtr raw;
        // On failure the API's output handle is undefined and must not be closed.
        Check(CreateVirtualDisk(ref type,path,0x003f0000,IntPtr.Zero,0,0,ref parameters,IntPtr.Zero,out raw));
        owned=new SafeFileHandle(raw,true);
        if(owned.IsInvalid) { owned.Dispose(); throw new IOException("Invalid created VHD handle"); }
        try { var ap=new AttachParameters { Version=1 }; Check(AttachVirtualDisk(owned,IntPtr.Zero,0,0,ref ap,IntPtr.Zero)); attached=true; }
        catch { owned.Dispose(); throw; }
    }
    public string PhysicalPath() { uint size=2048; var p=new StringBuilder(1024); Check(GetVirtualDiskPhysicalPath(owned,ref size,p)); return p.ToString(); }
    public void Detach() { if(attached) { Check(DetachVirtualDisk(owned,0,0)); attached=false; } }
    public void Dispose() { if(owned!=null) owned.Dispose(); }
    public static bool CaseSensitive(string path) {
        using(var file=CreateFileW(path,0x80,7,IntPtr.Zero,3,0x02000000,IntPtr.Zero)) {
            if(file.IsInvalid) throw new Win32Exception(Marshal.GetLastWin32Error());
            CaseInformation info;
            if(!GetFileInformationByHandleEx(file,23,out info,4)) throw new Win32Exception(Marshal.GetLastWin32Error());
            if((info.Flags & ~1u)!=0) throw new IOException("Unexpected directory case flags");
            return (info.Flags & 1)!=0;
        }
    }
    public static string Identity(string path) {
        using(var file=CreateFileW(path,0x80,7,IntPtr.Zero,3,0x02000000,IntPtr.Zero)) {
            if(file.IsInvalid) throw new Win32Exception(Marshal.GetLastWin32Error());
            FileIdentity info;
            if(!GetFileIdentity(file,18,out info,24)) throw new Win32Exception(Marshal.GetLastWin32Error());
            return info.Volume.ToString("x16")+":"+info.Low.ToString("x16")+info.High.ToString("x16");
        }
    }
    public static NamespaceEntryMetadata NamespaceMetadata(string path) {
        using(var file=CreateFileW(path,0x80,7,IntPtr.Zero,3,0x02000000,IntPtr.Zero)) {
            if(file.IsInvalid) throw new Win32Exception(Marshal.GetLastWin32Error());
            FileIdentity identity;
            if(!GetFileIdentity(file,18,out identity,24)) throw new Win32Exception(Marshal.GetLastWin32Error());
            FileBasicInformation basic;
            if(!GetFileBasicInformation(file,0,out basic,40)) throw new Win32Exception(Marshal.GetLastWin32Error());
            return new NamespaceEntryMetadata {
                Identity=identity.Volume.ToString("x16")+":"+identity.Low.ToString("x16")+identity.High.ToString("x16"),
                Attributes=basic.Attributes, LastWriteTicks=DateTime.FromFileTimeUtc(basic.LastWriteTime).Ticks
            };
        }
    }
    public static string ShortPath(string path) {
        var output=new StringBuilder(32768); uint n=GetShortPathNameW(path,output,(uint)output.Capacity);
        if(n==0) throw new Win32Exception(Marshal.GetLastWin32Error());
        if(n>=output.Capacity) throw new IOException("Short path exceeds bounded buffer");
        return output.ToString();
    }
    public static string[] Volume(string root) {
        var label=new StringBuilder(261); var fs=new StringBuilder(261); uint serial,component,flags;
        if(!GetVolumeInformationW(root,label,261,out serial,out component,out flags,fs,261)) throw new Win32Exception(Marshal.GetLastWin32Error());
        return new[]{fs.ToString(),serial.ToString(),component.ToString(),flags.ToString(),GetDriveTypeW(root).ToString(),label.ToString()};
    }
    public static ushort ImageMachine(string path) {
        using(var file=File.OpenRead(path)) using(var r=new BinaryReader(file)) {
            if(r.ReadUInt16()!=0x5a4d || file.Length<64) throw new IOException("Not a PE image");
            file.Position=60; uint offset=r.ReadUInt32();
            if(offset>1048576 || offset+6>file.Length) throw new IOException("Invalid PE header offset");
            file.Position=offset; if(r.ReadUInt32()!=0x00004550) throw new IOException("Invalid PE signature");
            return r.ReadUInt16();
        }
    }
}
'@
    $expectedMachine = switch ($result.host.architecture) { 'X64' { 0x8664 } 'Arm64' { 0xaa64 } default { throw 'Unsupported native Windows CPU.' } }
    $machine = [EmlOwnedNameVolume]::ImageMachine($Binary)
    if ($machine -ne $expectedMachine) { throw 'Frozen CLI image does not match actual native Windows CPU.' }
    $policy = (Get-ItemProperty -LiteralPath 'HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem' -Name NtfsDisable8dot3NameCreation).NtfsDisable8dot3NameCreation
    if ($policy -notin @(0, 2)) { throw "Host global 8.3 policy $policy cannot qualify both profiles without changing host policy." }
    $phase = 'create-owned-vhd'
    $volumeOwner = [EmlOwnedNameVolume]::new($image)
    $physical = $volumeOwner.PhysicalPath()
    if ($physical -notmatch '^\\\\\.\\PhysicalDrive([0-9]+)$') { throw 'Owned VHD physical path is malformed.' }
    $number = [uint32]$Matches[1]
    $disk = Get-DiskImage -ImagePath $image | Get-Disk
    if (@($disk).Count -ne 1 -or $disk.Number -ne $number -or $disk.IsBoot -or $disk.IsSystem -or $disk.Size -ne 1073741824 -or $disk.PartitionStyle -ne 'RAW') { throw 'Newly owned VHD disk identity is not exclusive and uninitialized.' }
    $uniqueId = $disk.UniqueId
    if ([string]::IsNullOrWhiteSpace($uniqueId) -or $uniqueId.Length -gt 1024) { throw 'New owned VHD has no bounded native disk identity.' }
    function Assert-OwnedDisk {
        if ($volumeOwner.PhysicalPath() -ne $physical) { throw 'Owned kernel VHD identity changed.' }
        $current = Get-DiskImage -ImagePath $image | Get-Disk
        if (@($current).Count -ne 1 -or $current.Number -ne $number -or $current.UniqueId -ne $uniqueId -or $current.IsBoot -or $current.IsSystem -or $current.Size -ne 1073741824) { throw 'Owned VHD disk binding changed.' }
    }
    Assert-OwnedDisk
    $initialized = Initialize-Disk -InputObject $disk -PartitionStyle GPT -PassThru
    # The exact initialized object is used, never a wildcard or ambient disk selection.
    $used = @(Get-Volume | Where-Object DriveLetter | ForEach-Object { $_.DriveLetter.ToString() })
    $letter = @('R','S','T','U','V','W','X','Y','Z' | Where-Object { $_ -notin $used -and !(Test-Path "${_}:\") }) | Select-Object -First 1
    if (!$letter) { throw 'No free owned-volume drive letter is available.' }
    Assert-OwnedDisk
    $partition = New-Partition -InputObject $initialized -UseMaximumSize -DriveLetter $letter
    Assert-OwnedDisk
    $label = 'EML-' + [Guid]::NewGuid().ToString('N').Substring(0,12)
    $formatted = Format-Volume -Partition $partition -FileSystem NTFS -NewFileSystemLabel $label -Confirm:$false
    $driveRoot = "${letter}:\"
    $nativeVolume = [EmlOwnedNameVolume]::Volume($driveRoot)
    if ($nativeVolume[0] -ne 'NTFS' -or $nativeVolume[2] -ne '255' -or $nativeVolume[4] -ne '3' -or $nativeVolume[5] -ne $label -or $formatted.FileSystem -ne 'NTFS') { throw 'Owned local NTFS volume observations do not match.' }
    $phase = 'observe-formatted-volume'
    $result.storage = [ordered]@{ image = $image; physical_path = $physical; disk_number = $number; disk_unique_id = $uniqueId; volume_root = $driveRoot; filesystem = $nativeVolume[0]; volume_serial = $nativeVolume[1]; max_component = 255; drive_type = 3; label = $label; global_8dot3_policy = $policy; native_cli_machine = $machine; lifetime = 'kernel-handle-owned' }
    $phase = 'revalidate-formatted-disk'
    Assert-OwnedDisk
    $phase = 'bind-formatted-partition'
    $driveBinding = Get-Partition -DriveLetter $letter
    if ($driveBinding.DiskNumber -ne $number -or $driveBinding.PartitionNumber -ne $partition.PartitionNumber) { throw 'Owned drive letter binding changed.' }
    if ($policy -eq 2) { [void](Invoke-Observed $fsutil @('8dot3name','set',"${letter}:",'0') $Out 'owned-volume-enable-8dot3') }
    [void](Invoke-Observed $fsutil @('8dot3name','query',"${letter}:") $Out 'owned-volume-query-8dot3')
    [void](Invoke-Observed $fsutil @('fsinfo','drivetype',"${letter}:") $Out 'owned-volume-drive-type')
    [void](Invoke-Observed $fsutil @('fsinfo','volumeinfo',"${letter}:") $Out 'owned-volume-info')
    # Prove the namespace oracle detects a deliberate metadata change even
    # while the enumerated FileSystemInfo object retains its cached timestamp.
    $phase = 'native-metadata-control'
    $metadataControlRoot = Join-Path $driveRoot 'EML metadata observation control'
    $controlDirectory = Join-Path $metadataControlRoot 'directory'
    if ([IO.Directory]::Exists($metadataControlRoot)) { throw 'Owned metadata-control path unexpectedly exists.' }
    $expectedBefore = [DateTime]::new(2021,2,3,4,5,6,[DateTimeKind]::Utc)
    $expectedAfter = [DateTime]::new(2022,3,4,5,6,7,[DateTimeKind]::Utc)
    $result.metadata_control = [ordered]@{ passed = $false; root = (Verbatim $metadataControlRoot); directory = (Verbatim $controlDirectory); expected_before_ticks = $expectedBefore.Ticks; expected_after_ticks = $expectedAfter.Ticks; cached_before_ticks = $null; cached_after_ticks = $null; native_before = $null; native_after = $null; namespace_before = $null; namespace_after = $null; namespace_change_detected = $false; cleanup_complete = $false }
    try {
        [void][IO.Directory]::CreateDirectory($controlDirectory)
        [IO.Directory]::SetLastWriteTimeUtc($controlDirectory,$expectedBefore)
        $enumerated = @(Get-ChildItem -LiteralPath (Verbatim $metadataControlRoot))
        if ($enumerated.Count -ne 1 -or !$enumerated[0].PSIsContainer) { throw 'Metadata control did not enumerate exactly its owned directory.' }
        $result.metadata_control.cached_before_ticks = $enumerated[0].LastWriteTimeUtc.Ticks
        $result.metadata_control.native_before = [EmlOwnedNameVolume]::NamespaceMetadata((Verbatim $controlDirectory))
        $controlBefore = Namespace-Facts $metadataControlRoot
        [IO.Directory]::SetLastWriteTimeUtc($controlDirectory,$expectedAfter)
        $result.metadata_control.cached_after_ticks = $enumerated[0].LastWriteTimeUtc.Ticks
        $result.metadata_control.native_after = [EmlOwnedNameVolume]::NamespaceMetadata((Verbatim $controlDirectory))
        $controlAfter = Namespace-Facts $metadataControlRoot
        $beforeChild = @($controlBefore | Where-Object { [IO.Path]::GetFileName($_.path) -ceq 'directory' })
        $afterChild = @($controlAfter | Where-Object { [IO.Path]::GetFileName($_.path) -ceq 'directory' })
        if ($beforeChild.Count -ne 1 -or $afterChild.Count -ne 1) { throw 'Native namespace control lost its exact directory.' }
        $result.metadata_control.namespace_before = @($controlBefore)
        $result.metadata_control.namespace_after = @($controlAfter)
        $result.metadata_control.namespace_change_detected = ($controlBefore | ConvertTo-Json -Depth 10 -Compress) -cne ($controlAfter | ConvertTo-Json -Depth 10 -Compress)
        $controlBefore | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $Out 'metadata-control-before.json') -Encoding utf8NoBOM
        $controlAfter | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $Out 'metadata-control-after.json') -Encoding utf8NoBOM
        $beforeMetadata = $result.metadata_control.native_before; $afterMetadata = $result.metadata_control.native_after
        if ($beforeMetadata.LastWriteTicks -ne $expectedBefore.Ticks -or $afterMetadata.LastWriteTicks -ne $expectedAfter.Ticks -or $beforeChild[0].last_write_ticks -ne $expectedBefore.Ticks -or $afterChild[0].last_write_ticks -ne $expectedAfter.Ticks -or $beforeMetadata.Identity -ne $afterMetadata.Identity -or $beforeMetadata.Attributes -ne $afterMetadata.Attributes -or $result.metadata_control.cached_before_ticks -le 0 -or $result.metadata_control.cached_before_ticks -ne $result.metadata_control.cached_after_ticks -or $result.metadata_control.cached_after_ticks -eq $expectedAfter.Ticks -or !$result.metadata_control.namespace_change_detected) { throw 'Native namespace metadata control did not detect the actual directory timestamp change.' }
        $result.metadata_control.passed = $true
    } finally {
        if ([IO.Directory]::Exists($metadataControlRoot)) { [IO.Directory]::Delete($metadataControlRoot,$true) }
        $result.metadata_control.cleanup_complete = ![IO.Directory]::Exists($metadataControlRoot)
        if (!$result.metadata_control.cleanup_complete) { throw 'Owned metadata-control cleanup is unconfirmed.' }
    }
    foreach ($profile in $result.profiles) {
        $directory = Join-Path $Out $profile.profile
        [void][IO.Directory]::CreateDirectory($directory)
        try {
            Assert-OwnedDisk
            $driveBinding = Get-Partition -DriveLetter $letter
            if ($driveBinding.DiskNumber -ne $number -or $driveBinding.PartitionNumber -ne $partition.PartitionNumber) { throw 'Owned drive letter binding changed.' }
            $profileVolume = [EmlOwnedNameVolume]::Volume($driveRoot)
            if ($profileVolume[0] -ne 'NTFS' -or $profileVolume[1] -ne $nativeVolume[1] -or $profileVolume[4] -ne '3' -or $profileVolume[5] -ne $label) { throw 'Owned volume identity changed before profile execution.' }
            $sensitive = $profile.profile -eq 'ntfs-case-sensitive-8dot3'
            $root = Join-Path $driveRoot ('EML qualification ' + $profile.profile)
            [void][IO.Directory]::CreateDirectory($root)
            # Generate and observe the root's 8.3 alias before enabling its case flag.
            $shortRoot = [EmlOwnedNameVolume]::ShortPath($root)
            if ($shortRoot -eq $root -or $shortRoot -notmatch '~') { throw 'Owned profile root has no actual 8.3 alias.' }
            $flag = if ($sensitive) { 'enable' } else { 'disable' }
            [void](Invoke-Observed $fsutil @('file','setCaseSensitiveInfo',$root,$flag) $directory 'set-case')
            $destination = Join-Path $root 'destination'
            [void][IO.Directory]::CreateDirectory($destination)
            [void](Invoke-Observed $fsutil @('file','setCaseSensitiveInfo',$destination,$flag) $directory 'set-destination-case')
            [void](Invoke-Observed $fsutil @('file','queryCaseSensitiveInfo',$root) $directory 'query-case')
            $rootCase = [EmlOwnedNameVolume]::CaseSensitive($root)
            $destinationCaseBefore = [EmlOwnedNameVolume]::CaseSensitive($destination)
            if ($rootCase -ne $sensitive -or $destinationCaseBefore -ne $sensitive) { throw 'Actual directory case mode is not the selected mode.' }
            $source = Join-Path $root 'sources'
            $behaviorOut = Join-Path $directory 'behavior'
            $profile.volume = [ordered]@{ filesystem_type = 'ntfs'; case = @{ case_sensitive = $rootCase; destination_before = $destinationCaseBefore }; root = $root; short_root = $shortRoot; serial = $profileVolume[1]; label = $profileVolume[5]; drive_type = 3; global_8dot3_policy = $policy }
            $profile.executed = $true
            $observed = Invoke-Observed $python @($oracle,'--binary',$Binary,'--source-root',(Verbatim $source),'--destination',(Verbatim $destination),'--out',(Verbatim $behaviorOut)) $directory 'independent-oracle'
            $profile.exit_code = $observed.exit_code
            $profile.behavior = Get-Content -LiteralPath (Join-Path $behaviorOut 'result.json') -Raw | ConvertFrom-Json -AsHashtable
            $mode = if ($sensitive) { 'case-sensitive' } else { 'case-insensitive' }
            $profile.source_case_sensitive = [EmlOwnedNameVolume]::CaseSensitive($source)
            $profile.volume.case.destination_after = [EmlOwnedNameVolume]::CaseSensitive($destination)
            if (!$profile.behavior.passed -or $profile.behavior.binary_sha256 -ne $result.binary_sha256 -or $profile.behavior.case_mode -ne $mode -or $profile.volume.case.destination_after -ne $sensitive) { throw 'Independent oracle or configured destination case mode failed.' }
            $actualEntries = @(Get-ChildItem -LiteralPath (Verbatim $destination) -Recurse -Force | ForEach-Object {
                if (($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Generated namespace contains a reparse entry.' }
                $longPath = Verbatim $_.FullName
                $shortPath = [EmlOwnedNameVolume]::ShortPath($longPath)
                $longIdentity = [EmlOwnedNameVolume]::Identity($longPath)
                $shortIdentity = [EmlOwnedNameVolume]::Identity($shortPath)
                $directoryCase = if ($_.PSIsContainer) { [EmlOwnedNameVolume]::CaseSensitive($longPath) } else { $null }
                [ordered]@{ relative = [IO.Path]::GetRelativePath((Verbatim $destination), $longPath); directory = $_.PSIsContainer; case_sensitive = $directoryCase; long_path = $longPath; short_path = $shortPath; long_identity = $longIdentity; short_identity = $shortIdentity }
            })
            $profile.native_entries = $actualEntries
            $entryIdentities = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
            foreach ($entry in $actualEntries) {
                if (!$entryIdentities.Add($entry.long_identity)) { throw 'Generated namespace contains a repeated native identity.' }
                if ($entry.long_identity -cne $entry.short_identity) { throw 'Observed generated short spelling identifies a different native entry.' }
            }
            $expectedEntries = @($profile.behavior.entries)
            $actualSelection = [Collections.Generic.Dictionary[string,bool]]::new([StringComparer]::Ordinal)
            $expectedSelection = [Collections.Generic.Dictionary[string,bool]]::new([StringComparer]::Ordinal)
            foreach ($entry in $actualEntries) { $actualSelection.Add($entry.relative, $entry.directory) }
            foreach ($entry in $expectedEntries) { $expectedSelection.Add($entry.relative, $entry.directory) }
            if ($actualSelection.Count -ne $expectedSelection.Count) { throw 'Native generated entry count differs from the independent common namespace.' }
            foreach ($entry in $expectedSelection.GetEnumerator()) {
                $observedType = $false
                if (!$actualSelection.TryGetValue($entry.Key, [ref]$observedType) -or $observedType -ne $entry.Value) { throw 'Native long/short entry observations differ from the independent common namespace.' }
            }
            # Invoke the frozen executable with the actual short root spelling. Python's
            # resolve() canonicalizes aliases, so the common oracle alone cannot prove it.
            $firstSource = Join-Path $source 'file-0\A.eml'
            $originalIdentity = [EmlOwnedNameVolume]::Identity($firstSource)
            $before = (Get-FileHash -LiteralPath $firstSource -Algorithm SHA256).Hash
            $namespaceBefore = Namespace-Facts $root
            $aliasCheck = Invoke-Observed $Binary @('--dry-run','--format','json','--output-dir',(Join-Path $shortRoot 'destination'),'--',(Join-Path $shortRoot 'sources\file-0\A.eml')) $directory 'short-root-check'
            $events = @($aliasCheck.stdout -split '\r?\n' | Where-Object { $_.Trim() } | ForEach-Object { $_ | ConvertFrom-Json -AsHashtable })
            $namespaceAfter = Namespace-Facts $root
            $namespaceBefore | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $directory 'short-root-before.json') -Encoding utf8NoBOM
            $namespaceAfter | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $directory 'short-root-after.json') -Encoding utf8NoBOM
            if (($namespaceBefore | ConvertTo-Json -Depth 10 -Compress) -cne ($namespaceAfter | ConvertTo-Json -Depth 10 -Compress)) { throw 'Short-root Check changed source/destination namespace, identity, metadata or bytes.' }
            if ($events.Count -ne 3 -or $events[0].type -ne 'start' -or $events[0].protocol -ne 1 -or $events[0].dry_run -ne $true -or $events[0].total -ne 1 -or $events[0].collection_complete -ne $true -or $events[1].type -ne 'item' -or $events[1].index -ne 0 -or $events[2].type -ne 'summary' -or $events[2].exit_code -ne 0 -or $events[2].collection_complete -ne $true -or $events[2].interrupted -ne $false -or $null -ne $events[2].run_error) { throw 'Short-root Check did not complete the exact one-item public protocol stream.' }
            $counts = $events[2].counts
            if ($counts.planned -ne 1 -or $counts.created -ne 0 -or $counts.up_to_date -ne 0 -or $counts.failed -ne 0 -or $counts.not_processed -ne 0) { throw 'Short-root Check summary contradicts its one planned result.' }
            $item = $events[1]
            if ($item.publication.state -ne 'not_published' -or $item.publication.file_sync.state -ne 'not_attempted' -or $item.publication.namespace_sync.state -ne 'not_attempted' -or $item.publication.namespace_sync.required_edges -ne 0 -or $item.publication.namespace_sync.completed_edges -ne 0 -or $item.publication.cleanup.state -ne 'not_needed' -or $item.existing_observation.file_sync.state -ne 'not_attempted' -or $item.existing_observation.namespace_sync.state -ne 'not_attempted' -or $item.existing_observation.namespace_sync.required_edges -ne 0 -or $item.existing_observation.namespace_sync.completed_edges -ne 0 -or $null -ne $item.error) { throw 'Short-root Check reported a publication, synchronization or error.' }
            $items = @($item)
            if ($items.Count -ne 1 -or $items[0].status -ne 'planned' -or $items[0].plan -ne 'verify_existing' -or $items[0].existing_observation.bytes_equal -ne $true -or $items[0].existing_observation.stable -ne $true -or (Get-FileHash -LiteralPath $firstSource -Algorithm SHA256).Hash -ne $before -or [EmlOwnedNameVolume]::Identity($firstSource) -ne $originalIdentity) { throw 'Actual short-root Check did not verify equal stable retained bytes.' }
            $profile.short_root_check = [ordered]@{ events = $events; namespace_unchanged = $true; namespace_before_sha256 = (Get-FileHash -LiteralPath (Join-Path $directory 'short-root-before.json') -Algorithm SHA256).Hash.ToLowerInvariant(); namespace_after_sha256 = (Get-FileHash -LiteralPath (Join-Path $directory 'short-root-after.json') -Algorithm SHA256).Hash.ToLowerInvariant(); original_identity_before = $originalIdentity; original_identity_after = [EmlOwnedNameVolume]::Identity($firstSource); original_sha256_before = $before.ToLowerInvariant(); original_sha256_after = (Get-FileHash -LiteralPath $firstSource -Algorithm SHA256).Hash.ToLowerInvariant() }
            $profile.status = 'passed'; $profile.failure = $null
        } catch {
            $profile.failure = $_.Exception.Message
            $profile.failure_phase = $phase; $profile.error = Error-Facts $_
            $profile.exit_code = 1
            # Preserve a failed oracle result rather than replacing it with absence.
            $failedResult = Join-Path $directory 'behavior\result.json'
            if ($null -eq $profile.behavior -and (Test-Path -LiteralPath $failedResult)) { $profile.behavior = Get-Content -LiteralPath $failedResult -Raw | ConvertFrom-Json -AsHashtable }
        }
    }
} catch { $result.failure = $_.Exception.Message; $result.failure_phase = $phase; $result.error = Error-Facts $_ }
finally {
    if ($null -ne $volumeOwner) {
        try { $volumeOwner.Detach() }
        catch { $result.cleanup.failure = $_.Exception.Message }
        finally { $volumeOwner.Dispose() }
        try {
            $remaining = Get-DiskImage -ImagePath $image
            $result.cleanup.detached = !$remaining.Attached
            $result.cleanup.observation_complete = $true
        } catch { $result.cleanup.observation_failure = $_.Exception.Message }
    }
    if ($null -ne $result.binary_sha256) {
        try { $result.binary_unchanged = (Get-FileHash -LiteralPath $Binary -Algorithm SHA256).Hash.ToLowerInvariant() -eq $result.binary_sha256 }
        catch { $result.binary_unchanged = $false; $result.failure = $_.Exception.Message }
    } else { $result.binary_unchanged = $false }
    $result.passed = $null -eq $result.failure -and $result.Contains('metadata_control') -and $result.metadata_control.passed -and $result.metadata_control.cleanup_complete -and $result.binary_unchanged -and $result.cleanup.detached -and $result.cleanup.observation_complete -and !($result.cleanup.Contains('failure')) -and !($result.cleanup.Contains('observation_failure')) -and @($result.profiles | Where-Object { $_.status -ne 'passed' }).Count -eq 0
    $json = $result | ConvertTo-Json -Depth 100
    [IO.File]::WriteAllText((Join-Path $Out 'result.json'), $json + "`n", [Text.UTF8Encoding]::new($false))
}
if (!$result.passed) { Write-Error -ErrorAction Continue 'Required native NTFS naming profiles failed or are unavailable; see result.json.'; exit 1 }
exit 0
