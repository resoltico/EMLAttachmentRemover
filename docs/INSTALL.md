# Install, update and uninstall the CLI

The installer downloads EML Attachment Remover for the current user, verifies the archive's
SHA-256 checksum and test-runs the program before replacing an owned installation.
No administrator account, background updater, scheduled task or file association
is needed. The CLI's inputs and outputs are outside the installer's scope.

The Windows examples that save a copy put `install-emlattachmentremover.ps1` in
your home directory (`$HOME`). If your reviewed copy is elsewhere, use its full
path for `-File` in the saved-installer examples.

## Existing executable without an installer receipt

`Existing executable has no installer receipt` means a file already exists at
the printed path, but this installer has no record of managing it. This does not
identify how the file was installed. The refusal changes no program or PATH entry.

Choose whether to replace that executable or keep it:

- **Replace it:** follow the recovery instructions printed by the installer. They keep
  the selected directory, version pin and choice to leave PATH unchanged. The
  replacement backs up only the named executable after the new candidate verifies.
- **Keep it:** install into another directory and use that installation's full
  executable path. Avoid adding a second command with the same name to PATH
  until you have chosen which installation should be used.

For the default directory and a saved installer, explicit replacement is:

```sh
sh ./install-emlattachmentremover.sh --install-dir "$HOME/.local/bin" --replace-unmanaged
```

```powershell
powershell -NoProfile -ExecutionPolicy ByPass -File (Join-Path $HOME 'install-emlattachmentremover.ps1') -InstallDir "$env:LOCALAPPDATA\Programs\EMLAttachmentRemover" -ReplaceUnmanaged
if ($LASTEXITCODE -ne 0) { throw "Installer failed ($LASTEXITCODE)." }
```

These examples explicitly select the usual default directories. If
`XDG_BIN_HOME`, another directory, a version pin or disabled PATH changes were
selected, use the printed command instead. If the installer ran through stdin or
an expression and cannot identify its saved file, save and inspect the installer using [Install](#install), then
apply the options printed by the refusal to that saved script. Keep mirror
configuration in the environment; recovery commands do not print it.

To keep the existing executable, choose a fresh directory, for example:

```sh
sh ./install-emlattachmentremover.sh --install-dir "$HOME/emlattachmentremover-cli" --no-modify-path
```

```powershell
powershell -NoProfile -ExecutionPolicy ByPass -File (Join-Path $HOME 'install-emlattachmentremover.ps1') -InstallDir "$env:LOCALAPPDATA\EMLAttachmentRemover-CLI" -NoModifyPath
if ($LASTEXITCODE -ne 0) { throw "Installer failed ($LASTEXITCODE)." }
```

Retain any selected version option when changing directories. After a successful
replacement, keep the original backup at its exact reported path until you have
verified the new executable. Uninstall does not remove this backup. Run the
printed full executable path with `--version` and `--help`; this checks the
installation you just selected rather than another command on PATH. For the
default directories:

```sh
"$HOME/.local/bin/emlattachmentremover" --version
"$HOME/.local/bin/emlattachmentremover" --help
```

```powershell
& "$env:LOCALAPPDATA\Programs\EMLAttachmentRemover\emlattachmentremover.exe" --version
& "$env:LOCALAPPDATA\Programs\EMLAttachmentRemover\emlattachmentremover.exe" --help
```

To restore the original after a successful replacement, first [uninstall](#uninstall)
the new managed copy from the same directory. Continue only after uninstall
succeeds; then move the exact reported backup to the original executable path.
Review PATH separately: pre-existing entries were not adopted, and entries added
by the new installation may have been removed. If replacement or restoration
failed, preserve the reported recovery files and follow
[ownership and unmanaged installations](#ownership-and-unmanaged-installations)
before retrying. The replacement option cannot bypass changed managed files,
corrupt state, symlinks, nonregular targets or access failures.

If this error occurs during **uninstall**, the installer cannot remove that
untracked executable or its PATH entries. Use the method that installed it, or
inspect and preserve the file before deciding on manual removal. Replacement is
an installation action, not a way to uninstall an untracked executable.

Published installers link Help to their source commit, so it stays matched to
that installer. Scripts run from a source checkout link to the current `main`
guide. Keep the saved installer for later update or offline removal.

## Install

The [README quick start](../README.md#cli-installer) leaves no installer script
after a successful install. Save a copy using the commands below if you want to
inspect it before running it, pass installer options or uninstall offline later.

On macOS or Linux, save the installer, inspect it, then run it:

```sh
curl --proto '=https' --proto-redir '=https' -fL -o ./install-emlattachmentremover.sh https://github.com/resoltico/EMLAttachmentRemover/releases/latest/download/install.sh &&
  less ./install-emlattachmentremover.sh &&
  sh ./install-emlattachmentremover.sh
```

If curl is unavailable, use `wget --https-only -O ./install-emlattachmentremover.sh URL`
with the same URL and `&&` chain. `less` displays the saved script for review.
The installer itself requires standard shell tools, tar and a
SHA-256 tool (`sha256sum`, `shasum` or `openssl`).

On Windows, use PowerShell to download and display the saved installer:

```powershell
& {
    $ErrorActionPreference = 'Stop'
    $installer = Join-Path $HOME 'install-emlattachmentremover.ps1'
    Invoke-WebRequest -UseBasicParsing -Uri https://github.com/resoltico/EMLAttachmentRemover/releases/latest/download/install.ps1 -OutFile $installer
    Get-Content -LiteralPath $installer
}
```

After a successful download and review, run the saved installer in a child session:

```powershell
powershell -NoProfile -ExecutionPolicy ByPass -File (Join-Path $HOME 'install-emlattachmentremover.ps1')
if ($LASTEXITCODE -ne 0) { throw "Installer failed ($LASTEXITCODE)." }
```

Keep the Unix `&&` chain or the equivalent download-success check in automation,
and propagate a nonzero installer exit. A bare `curl | sh` pipeline can hide a
failed fetch behind a successful empty shell. Retain the saved script for
offline uninstall.

Windows PowerShell 5.1 and PowerShell 7 are supported. `ByPass` applies only to the
launched PowerShell session; it does not change persistent policy or override
Group Policy. Managed systems can still refuse the program.

Supported CLI targets are macOS 15 or later on Apple Silicon and Intel, Linux
x86-64/ARM64 built on the Ubuntu 26.04 baseline, and Windows 10 or later on
x64/ARM64. Linux compatibility is checked by running the verified candidate;
the installer does not reject a distribution by its name. Native CPU selection
also handles translated shells.

The default location is `$XDG_BIN_HOME/emlattachmentremover` when `XDG_BIN_HOME` is set,
otherwise `~/.local/bin/emlattachmentremover`. On Windows it is
`%LOCALAPPDATA%\Programs\EMLAttachmentRemover\emlattachmentremover.exe`.
Choose another absolute folder with `--install-dir DIR` or `-InstallDir DIR`.

Open a new terminal after PATH changes, then verify:

```sh
emlattachmentremover --version
emlattachmentremover --help
```

The CLI and Mac desktop app are independent. Installing or removing the CLI
does not install or remove the app. See [CLI usage](../README.md#cli).

The documented Windows command runs in a child process, so its PATH changes do
not reach an already-open parent terminal. Running the script directly in the
current PowerShell session may also update that session's PATH. On macOS/Linux,
use the quoted full executable path printed by the installer while this terminal
keeps its current PATH, or open a new terminal after successful persistence. `--no-modify-path` / `-NoModifyPath` leaves persistent
and current-session PATH unchanged.

## Update and choose a release

Run the same install command again. The installer resolves latest once to a stable
`vX.Y.Z` tag, then downloads both archive and `SHA256SUMS` from that release.
A failed download, checksum or candidate test happens before replacing the active
program. An update requires intact ownership state and every remaining managed
file to match its receipt. The receipt must list exactly the current installer's
legal-file names; see [ownership requirements](#ownership-and-unmanaged-installations).

Select a release explicitly, for example:

```sh
curl --proto '=https' --proto-redir '=https' -fL -o ./install-emlattachmentremover.sh https://github.com/resoltico/EMLAttachmentRemover/releases/download/v10.0.0/install.sh &&
  sh ./install-emlattachmentremover.sh --version 10.0.0
```

```powershell
& {
    $ErrorActionPreference = 'Stop'
    $startingLocation = Get-Location
    Set-Location -LiteralPath $HOME
    try {
        Invoke-WebRequest -UseBasicParsing -Uri https://github.com/resoltico/EMLAttachmentRemover/releases/download/v10.0.0/install.ps1 -OutFile .\install-emlattachmentremover.ps1 -ErrorAction Stop
        powershell -NoProfile -ExecutionPolicy ByPass -File .\install-emlattachmentremover.ps1 -Version 10.0.0
        if ($LASTEXITCODE -ne 0) { throw "Installer failed ($LASTEXITCODE)." }
    } finally {
        Set-Location -LiteralPath $startingLocation.Path
    }
}
```

These commands pin installer and payload to 10.0.0. Replace both version values
together to select another stable release. Alternatively,
`EMLATTACHMENTREMOVER_VERSION` supplies the payload version when no option is
given. That environment pin remains set in the session; clear it to return to latest:

```sh
unset EMLATTACHMENTREMOVER_VERSION
```

```powershell
Remove-Item Env:EMLATTACHMENTREMOVER_VERSION -ErrorAction SilentlyContinue
```

Keep a copy of the reviewed installer you used, from the selected release tag or
reviewed source checkout. The commands above save one as
`install-emlattachmentremover.sh` or `install-emlattachmentremover.ps1`. Do not assume that
fetching the current latest script later gives the same ownership protocol.

The installer stages files privately on the destination filesystem and uses a
per-product, per-directory exclusive lock. A held lock fails promptly; there are
no automatic lock retries. It attempts to restore the prior owned state if
activation fails. Errors describe any remaining state; an interruption or failed
PATH write is not a promise that nothing was installed. Inspect the reported
paths before trying again.

On Windows an update can retain one uniquely named, receipt-owned running
executable. Close processes using it before another update or uninstall if
cleanup is refused. The installer never adopts or deletes arbitrary `.old` files.

## Ownership and unmanaged installations

The installer uses `.emlattachmentremover-install` beside the executable
for ownership receipts and managed data. It verifies managed file hashes before
update or removal and refuses unowned files by default and changed managed files.
A matching filename, version output or PATH entry does not establish ownership.

Update and uninstall require exactly the legal-file names declared by the current
installer. Receipts with a different inventory are refused without conversion.
Remove an affected managed copy using the matching installer that created it,
or install into a fresh directory and use the printed full executable path.
`--replace-unmanaged` / `-ReplaceUnmanaged` cannot bypass this managed-state refusal.

Use the installer and payload from the same qualified release. The installer
verifies ownership before replacement and preserves recoverable state on failure.

Unix uninstall and rollback preserve private recovery images before rewriting
startup files in place, retaining their symlink and hardlink identities. A failed
write or handled interruption attempts to restore only attributable changes. If
restoration is unsafe or fails, retain the reported staging directory and
privately inspect its `rewrite.N` paths, identities, exact `preimage`/`postimage`
and available `observed`/`restore-image` files against the current startup file.
These files may contain private shell configuration. Retry refuses retained
staging until recovery is reviewed; do not remove evidence or overwrite later
user edits blindly.

- Local ownership and lock conflicts are diagnosed before downloading or running
  a candidate. The target is rechecked under lock immediately before mutation.
- A receiptless ordinary executable, with private state absent, is refused by
  default. Install-only `--replace-unmanaged` / `-ReplaceUnmanaged` explicitly
  authorizes that one replacement. No version-string adoption or stdin prompt is used.
- The original executable is kept in a unique recovery location in the install
  directory's parent. The exact path is printed; later uninstall does not delete
  it or mistake it for an active tool requiring shared-directory PATH retention.
  Pre-existing directories and PATH entries remain unowned.
- An intact receipt whose executable alone is missing can be repaired by
  reinstalling, or cleaned up with uninstall. Every remaining owned file must
  verify. Changed files, corrupt/incomplete state, nonregular targets and access
  failures remain precise conservative refusals; the replacement flag cannot bypass them.
- Unix accepts aliases only when requested and recorded directories still
  resolve to the same physical directory, preserving exact recorded PATH text.
  Windows normalizes ordinary spellings but refuses a reparse-point install directory.
  Moved receipts are not adopted; use the recorded-path command in the diagnostic.

For a receiptless executable, follow the [replace or keep decision and commands](#existing-executable-without-an-installer-receipt).

Activation failure attempts to restore the original executable, receipt and prior
absence. Keep reported recovery paths if restoration is incomplete; inspect them
before retrying rather than deleting valuable evidence. Unix PATH receipts record
only completed writes. Transaction rollback removes only its verified additions,
preserving later user content; partial writes or intervening edits retain named
staging evidence instead of claiming complete restoration. Deliberate later edits
or removals of owned startup entries are preserved.

To manage an installation manually, preserve valuable executable/state files,
inspect exact diagnostics and choose a fresh directory if needed. Never remove an
entire shared bin directory, copy a receipt from another directory or edit hashes
to bypass a refusal. A receiptless executable requires explicit backed-up
replacement; its version output is not ownership proof.

PowerShell pipeline cancellation uses the same restoration path as ordinary
activation failures. Recovery checks actual file slots and verified receipt and
payload hashes. A retained `recovery.txt` records planned paths, not proof that
every planned write completed; inspect the named files before manual recovery.
After the new executable and receipt are validated, installation is committed;
later retired-file cleanup can be retried without undoing that valid installation.
An abrupt process kill or power loss cannot run script cleanup.

Pre-existing directories may be shared with other tools. They are never removed
merely because they became empty. Unrelated files are preserved.

## Uninstall

Use the same reviewed installer, saved as `install-emlattachmentremover.sh` / `install-emlattachmentremover.ps1`, to remove
an unchanged installation and only the PATH entries it owns:

```sh
sh ./install-emlattachmentremover.sh --uninstall
```

```powershell
powershell -ExecutionPolicy ByPass -File (Join-Path $HOME 'install-emlattachmentremover.ps1') -Uninstall
```

Pass the same custom install directory used for installation, for example
`sh ./install-emlattachmentremover.sh --uninstall --install-dir '/absolute/my tools'` or
`powershell -ExecutionPolicy ByPass -File (Join-Path $HOME 'install-emlattachmentremover.ps1') -Uninstall -InstallDir 'C:\My Tools'`.
Removal does not
need a download of the program. An offline saved copy of the installer can be
run with the same uninstall option.

Changed or unowned managed files are refused rather than silently deleted. For
manual cleanup, inspect the executable and its private `.emlattachmentremover-install`
directory, then remove only the files you intend to remove. An installer-created
directory is removed only if empty and never if it is a filesystem root.

Unix PATH cleanup uses the exact recorded startup path and generated line or Fish
entry for this install directory. It preserves other installations and user edits;
symlinked startup files remain symlinks. Windows removes only its exact added user
PATH entry, preserving raw text, empty segments, environment references and value
kind. A PATH entry can be conservatively retained with a warning if unrelated
files remain in the shared install directory; removing it could break other tools.
`NoModifyPath` also suppresses PATH cleanup during
uninstall and reports the entry left for manual removal.

Windows uninstall removes an owned PATH entry before deleting
the executable or receipt. A failed PATH write leaves those files intact for
retry. A later file-removal failure can leave partial progress; inspect the
reported paths before retrying or using the manual cleanup above.

Your emails, copies, reports and desktop app preferences are not installer-managed files.

## Options and mirrors

| macOS/Linux | Windows | Environment | Effect |
|---|---|---|---|
| `--version X.Y.Z` | `-Version X.Y.Z` | `EMLATTACHMENTREMOVER_VERSION` | Choose a stable release; one leading `v` is allowed. |
| `--install-dir DIR` | `-InstallDir DIR` | `EMLATTACHMENTREMOVER_INSTALL_DIR` | Choose an absolute directory. |
| `--no-modify-path` | `-NoModifyPath` | `EMLATTACHMENTREMOVER_NO_MODIFY_PATH=1` | Do not change PATH or shell startup files. |
| `--replace-unmanaged` | `-ReplaceUnmanaged` | | Back up and replace a receiptless ordinary executable; install only. |
| `--uninstall` | `-Uninstall` | `EMLATTACHMENTREMOVER_UNINSTALL=1` (Windows) | Remove unchanged owned files. |
| `--help` | `-Help` | | Show usage. |

Pass options to the saved, reviewed script shown above. Set environment
variables before launching that script.

`EMLATTACHMENTREMOVER_DOWNLOAD_BASE` selects an HTTPS release mirror or an absolute local
directory containing the release's archive and `SHA256SUMS`. HTTPS is required
at every redirect hop. A mirror is explicitly trusted as the release source;
its checksums do not authenticate its publisher. Mirror URL bases must be directory
URLs without credentials, queries or fragments; use an absolute local release
directory if authenticated downloads are handled separately. Versioned EML mirror assets require an explicit stable version through --version or EMLATTACHMENTREMOVER_VERSION.

A PATH entry cannot represent a Unix colon or a Windows semicolon in a directory
name. The installer reports unsupported persistence rather than claiming PATH
was configured. Use the printed, correctly quoted full executable path or choose
another directory. Startup/registry write failures are reported.

## What the installer changes

- The executable at the selected install directory.
- The private `.emlattachmentremover-install` ownership directory. Its receipt is data, not
  an executable shell configuration file.
- EML keeps `LICENSE` and `THIRD-PARTY-NOTICES.txt` under the private directory
  in `legal/`. They are retained for local reference and are not a runtime
  dependency. The installer does not install the desktop GUI.
- Only the exact startup entry or user PATH entry added by this installation.

The Unix lock is the sibling `.emlattachmentremover-install.lock`; a normal run removes its
own lock. Locks serialize cooperating installer operations, not hostile processes.
Do not remove a lock while another installer is running. The CLI does not require
installer receipts to run.

## Trust and verification

Both installer and program are code from this repository's release. SHA-256
verification detects damaged or incomplete downloads, not a compromised repository
or mirror. A candidate is executed as the current user only after checksum
verification. A receipt prevents accidental replacement/removal of other files;
it is not protection from a hostile process with your account's permissions.

Published release provenance can be checked with `gh attestation verify <asset>
--repo resoltico/EMLAttachmentRemover`. That is a separate optional
verification step; the installer does not claim to authenticate attestations.

The scripts do not disable Gatekeeper, SmartScreen or managed-device policy.
Mac executables are ad hoc signed, without Developer ID or notarization; Windows
executables are unsigned. Browser downloads may carry internet markings and
system policy may block execution even after a matching checksum. Inspect the
candidate diagnostic shown by the installer before its temporary files are cleaned.

## Manual installation

For manual downloads, open [Releases](https://github.com/resoltico/EMLAttachmentRemover/releases),
choose the native CPU archive on the matching release tag,
verify its entry in `SHA256SUMS`, then extract into a fresh directory. Keep the
archive's legal material with any copy you redistribute.

## Troubleshooting

If an ownership check fails, inspect the selected path and private directory.
Choose a fresh directory or remove the prior files manually; a version string
is not proof that this installer owns a file. If a lock remains after a crash,
first establish that no installer is using it before manual cleanup.

If the verified candidate cannot run, use its reported diagnostic to check the
CPU, minimum OS/runtime and device policy. Do not bypass checksum verification.
Open a new terminal after a saved PATH change.

When another executable comes first on the installer's PATH, the installer
reports its path. Use the quoted full path printed by the installer to run the
new copy. To check your own shell, use `command -v emlattachmentremover` on macOS
or Linux, or `Get-Command emlattachmentremover.exe` in PowerShell. Remove a
conflicting installation using the method that installed it.

## Archive names

Replace `VERSION` with the selected stable release. These are CLI archives only:

| System | Archive |
|---|---|
| macOS Apple Silicon | `emlattachmentremover-VERSION-macos-aarch64.tar.gz` |
| macOS Intel | `emlattachmentremover-VERSION-macos-x86_64.tar.gz` |
| Linux ARM64 | `emlattachmentremover-VERSION-linux-aarch64.tar.gz` |
| Linux x86-64 | `emlattachmentremover-VERSION-linux-x86_64.tar.gz` |
| Windows ARM64 | `emlattachmentremover-VERSION-windows-aarch64.zip` |
| Windows x64 | `emlattachmentremover-VERSION-windows-x86_64.zip` |

The checksum manifest also lists desktop archives. The CLI installer selects only
its exact CLI filename and requires one valid checksum for it.
