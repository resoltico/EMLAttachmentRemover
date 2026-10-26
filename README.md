# EML Attachment Remover

Create verified body-only copies of EML messages while keeping originals unchanged.
Copies retain supported plain-text and HTML bodies, including their encoded bytes;
attachments and other non-body MIME parts are removed. Retained HTML can still
contain remote references or unsafe content. See the [MIME policy](docs/mime.md).

This branch documents unreleased 7.0.0. For existing downloads, use the
documentation at their matching release tag.

## Install

Download from [Releases](https://github.com/resoltico/EMLAttachmentRemover/releases).
Choose the archive matching your platform:

| Product | Archive | Minimum platform |
|---|---|---|
| CLI, macOS Intel and Apple Silicon | `emlattachmentremover-VERSION-macos-universal.tar.gz` | macOS 15 |
| Desktop app, both Mac CPUs | `emlattachmentremover-gui-VERSION-macos-universal.zip` | macOS 15 |
| CLI, Linux x86_64 or aarch64 | `emlattachmentremover-VERSION-linux-ARCH.tar.gz` | Ubuntu 22.04 / glibc 2.35 |
| CLI, Windows x86_64 or aarch64 | `emlattachmentremover-VERSION-windows-ARCH.zip` | Windows 10 |

Extract into a new folder. On macOS and Linux, run `./emlattachmentremover --help`;
on Windows, run `.\emlattachmentremover.exe --help` in PowerShell. Add the extracted
executable to PATH if desired. CLI archives include LICENSE and third-party notices;
the app carries both under `Contents/Resources`. Release assets include SHA256SUMS
and GitHub artifact attestations. See [release verification](docs/release.md).

The Mac app is ad hoc signed, without Developer ID or notarization. Move it to
Applications and try opening it. If macOS blocks it, use System Settings → Privacy
& Security → Open Anyway for that app, then confirm Open. Apple's
[opening instructions](https://support.apple.com/en-us/102445) explain the exception.

## CLI

```sh
emlattachmentremover -- message.eml
emlattachmentremover --dry-run -- mail-folder
mkdir -p copies
emlattachmentremover --output-dir copies -- mail-folder
emlattachmentremover --format json -- message.eml
```

Copies use `<stem>.no-attachments.eml`. Explicit files go directly into the chosen
existing output folder; selected folders mirror their root names and relative
layout. Without `--output-dir`, copies stay beside their sources. Names that exceed
native component limits are shortened. Conflicts are refused without retry names.
Identical existing copies are verified and synchronized; differing files stay intact.
Directory walks skip hidden entries and generated copies. Explicit selections are
still checked. Overlapping input/output trees and source/destination aliases are refused.

`--dry-run` checks sources and destinations without writes. A later Create repeats
admission and verification. `--request FILE|-` accepts an EOF-terminated protocol-1
JSON request instead of positional paths, output and dry-run options. See the
[protocol](docs/protocol.md) for the request, JSONL events and native path encoding.

Exit codes: 0 completed successfully, 1 run/delivery failure, 2 invalid invocation
or pre-stream admission failure, 130 interrupted. Ctrl-C requests cooperative stop;
a second interrupt forces exit. Completed copies remain. Inspect partial results
and reported cleanup failures before retrying. Recognizable staging files use
`.emlattachmentremover-<random>.tmp`; abandoned files are never swept automatically.

## Desktop app

Add files or folders with the buttons, window drops or launch arguments. These
paths enter a review selection; no worker starts until **Create copies** or
**Check selection** is pressed in the footer. Exact duplicate paths appear once.
Folders are scanned only after Start, so selection counts are not message counts.
Remove individual selections or Clear selection before starting.

Choose Create copies or Check only and the destination in **Next request**. Check
also verifies the chosen destination without writing copies. **Change…** offers
Beside originals or an existing output folder. Start captures the current paths,
mode and destination; changes afterwards do not alter active or queued requests.
Up to eight requests wait behind the active run. Two retained reports pause the
queue until a completed report is dismissed. Queue refusals retain the reviewed
selection. A settled Check's **Create copies…** returns its paths to review in
Create mode; it does not write until Start is pressed again. Finish or clear any
other reviewed selection first; Check-to-Create never merges unrelated paths.

The empty window offers file/folder selection without inactive report controls.
Reports and waiting requests appear in the sidebar only when there is work to
navigate. Choosing a report keeps that view when queued work starts. A report shows its captured settings, counts, message results and selected
message details. Errors, recovery guidance and run-level discovery failures remain
visible in a separately bordered **Message details** inspector, before provenance.
Paths, removal metadata and byte counts expand under **Paths, parts and technical details**.
**Failed only** filters the table without changing the full report or its export;
Toggle **Failed only** off to restore the list. Up/Down/Home/End move through visible messages
and reveal the selected row. **File → Open files…** uses Cmd-O on macOS (Ctrl-O
on other desktops); **File → Add folder…** opens folder selection. Long filenames retain their distinguishing suffixes, with
full paths in tooltips and accessibility text. Report-export feedback is separate
from retained run warnings. **Dismiss report** removes the report from this
session, never the original messages or copies.

The full-width footer keeps actions on one aligned row. **Stop** addresses the
active run even while another report or selection is being reviewed. **More… →
Force Stop…** requires confirmation. Completed copies remain after either Stop.
Busy Quit offers Stop and wait, confirmed Force Stop and quit, or Keep running;
Quit waits for worker and report-export settlement, including after a confirmed
Force Stop. When only an export is active, Finish export and quit waits for it;
Force Stop is unavailable.
Waiting requests also require a Quit decision; they are not silently discarded.
Once shutdown is confirmed, further Quit requests do not reopen the decision,
and new selections or late file-dialog submissions cannot start more work. **Save JSONL report…** writes
the run's original protocol-1 stream captured when Save is clicked, including
partial results after failure. Navigating to another report while choosing the
filename does not change the captured export.
**Show copy** is available only for a verified copy receipt.

On macOS, open **EML Attachment Remover → About EML Attachment Remover** in the
system menu bar. Other desktops use **Help → About**. The main window contains
no duplicate product/version banner or About button. About uses the same centered
Slint layout as Auditory Lab: the application icon, version/build, two purpose
lines, copyright, Project website and Licenses, then AboutSlint. Licenses shows
read-only software/artwork terms with a separate third-party notices action.
Escape closes either auxiliary window.

Saved destinations use the platform configuration folder's
`EMLAttachmentRemover/preferences.json`. Earlier NSUserDefaults settings are not
imported; choose the destination again. Unreadable or corrupt preferences are preserved. The app starts with defaults and
shows the exact path and recovery guidance. Destination changes remain session-only
until the settings problem is corrected and the app restarted.

The app accepts launch arguments, for example:

```sh
open -n -a '/Applications/EML Attachment Remover.app' --args "$PWD/message.eml"
```

Dock-icon drop, Finder Services and Finder Open With integration are removed.
Native file dialogs use the toolkit/platform implementation; clipboard shortcuts
in their Go-To fields may differ from earlier releases.

## Limits and filesystem behavior

A source may contain at most 256 MiB. Requests admit 4,096 explicit paths and 4 MiB
of native input path bytes. Collection admits 4,096 work items, 100,000 visited
entries, 64 directory levels and 8 MiB of charged reported paths. Split large
selections or shorten paths when refused. These bounds govern inputs and reports;
peak memory also includes prepared copies and retained reports.

Sources are read-only. Copies are written to a fresh staging file, verified,
synchronized, then published without replacing an existing destination. Filesystem
synchronization and requested-address observations are reported separately from
publication. Ordinary owned folders are the supported environment; see the
[threat model](docs/threat-model.md) for guarantees and excluded actors.

## Develop and redistribute

See [CONTRIBUTING](CONTRIBUTING.md) for the shortest build and check commands,
[architecture](docs/architecture.md) for crate boundaries, and the
[changelog](CHANGELOG.md) for breaking changes. MPL-2.0-covered source is available
in this repository; redistributors must preserve notices and corresponding source
availability. Slint's desktop license and AboutSlint requirements also apply;
see [legal materials](legal/README.md).
