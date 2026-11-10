# Install, update and uninstall the CLI

The installer downloads EML Attachment Remover for the current user, verifies the archive's
SHA-256 checksum and test-runs the program before replacing an owned installation.
No administrator account, background updater, scheduled task or file association
is needed. The CLI's inputs and outputs are outside the installer's scope.

## Install

On macOS or Linux:

```sh
curl -fsSL https://github.com/resoltico/EMLAttachmentRemover/releases/latest/download/install.sh | sh
```

If curl is unavailable, use
`wget -qO- https://github.com/resoltico/EMLAttachmentRemover/releases/latest/download/install.sh | sh`.
The installer itself requires standard shell tools, tar and a SHA-256 tool
(`sha256sum`, `shasum` or `openssl`).

On Windows, use PowerShell:

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://github.com/resoltico/EMLAttachmentRemover/releases/latest/download/install.ps1 | iex"
```

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
program. An update requires the existing binary, legal files and ownership state
to match their receipt.

Select a release explicitly, for example:

```sh
curl -fsSL https://github.com/resoltico/EMLAttachmentRemover/releases/latest/download/install.sh | sh -s -- --version 8.0.0
```

```powershell
$env:EMLATTACHMENTREMOVER_VERSION = '8.0.0'
irm https://github.com/resoltico/EMLAttachmentRemover/releases/latest/download/install.ps1 | iex
```

The environment pin remains set in that session. Clear it to return to latest:

```sh
unset EMLATTACHMENTREMOVER_VERSION
```

```powershell
Remove-Item Env:EMLATTACHMENTREMOVER_VERSION -ErrorAction SilentlyContinue
```

Keep a copy of the reviewed installer you used, from the selected release tag or
reviewed source checkout. The pipe commands do not save one. Do not assume that
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
update or removal and refuses existing unowned or changed files. A matching
filename, version output or PATH entry does not establish ownership.

For any installation without a valid receipt, remove the executable yourself or
select a fresh directory. Keep any files you need and review PATH settings before
removing them by hand. Do not copy a receipt from another directory or edit hashes
to bypass a refusal.
A non-directory target or non-regular managed entry is also left alone.

Pre-existing directories may be shared with other tools. They are never removed
merely because they became empty. Unrelated files are preserved.

## Uninstall

Use the same reviewed installer, saved as `install.sh` / `install.ps1`, to remove
an unchanged installation and only the PATH entries it owns:

```sh
sh ./install.sh --uninstall
```

```powershell
powershell -ExecutionPolicy ByPass -File .\install.ps1 -Uninstall
```

Pass the same custom install directory used for installation, for example
`sh ./install.sh --uninstall --install-dir '/absolute/my tools'` or
`powershell -ExecutionPolicy ByPass -File .\install.ps1 -Uninstall -InstallDir 'C:\My Tools'`.
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

Windows uninstall in this checkout removes an owned PATH entry before deleting
the executable or receipt. A failed PATH write leaves those files intact for
retry. A later file-removal failure can leave partial progress; inspect the
reported paths before retrying or using the manual cleanup above. Published
installer scripts receive this fix only in a release containing it.

Your emails, copies, reports and desktop app preferences are not installer-managed files.

## Options and mirrors

| macOS/Linux | Windows | Environment | Effect |
|---|---|---|---|
| `--version X.Y.Z` | `-Version X.Y.Z` | `EMLATTACHMENTREMOVER_VERSION` | Choose a stable release; one leading `v` is allowed. |
| `--install-dir DIR` | `-InstallDir DIR` | `EMLATTACHMENTREMOVER_INSTALL_DIR` | Choose an absolute directory. |
| `--no-modify-path` | `-NoModifyPath` | `EMLATTACHMENTREMOVER_NO_MODIFY_PATH=1` | Do not change PATH or shell startup files. |
| `--uninstall` | `-Uninstall` | `EMLATTACHMENTREMOVER_UNINSTALL=1` (Windows) | Remove unchanged owned files. |
| `--help` | `-Help` | | Show usage. |

Pass Unix options after `sh -s --`. In PowerShell use environment variables before
`irm ... | iex`, or parameters on the saved, reviewed script shown above.

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
If PATH was not changed, use the quoted full path printed by the installer or
open a new terminal after successful persistence. For shell shadowing use
`command -v emlattachmentremover`; in PowerShell use `Get-Command emlattachmentremover` and remove a
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
