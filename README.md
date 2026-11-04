# EML Attachment Remover

Create verified body-only copies of EML messages while keeping originals unchanged.
Copies retain supported plain-text and HTML bodies, including their encoded bytes;
attachments and other non-body MIME parts are removed. Retained HTML can still
contain remote references or unsafe content. See the [MIME policy](docs/mime.md).

This documentation describes the source in this checkout. For downloads, use the
documentation at the matching release tag.

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
**Start check** is pressed. Exact duplicate paths appear once.
Collection coalesces proven native aliases of the same directory entry; distinct
hard-link names remain separate. Folders are scanned only after Start, so selection counts are not message counts.
Remove individual selections or Clear selection before starting.

Choose Create copies or Check only and the destination in the request draft. Check
also verifies the chosen destination without writing copies. **Change…** offers
Beside originals or an existing output folder. Start captures the current paths,
mode and destination; changes afterwards do not alter active or queued requests.
Up to eight requests wait behind the active run. **Tasks** opens waiting requests
and session results; **Remove from queue** cancels only the chosen waiting request.
Queue refusals retain the draft. By default, two retained results pause further
work until a result is closed. **Allow temporary storage…** offers an explicit
choice for this session: exact diagnostic reports are backed by private, unnamed
temporary files so accepted queued work can continue without closing results.
There are at most ten admitted tasks/results, with less than 256 MiB per report
stream (at most 2.5 GiB of raw backing). Files disappear when their results close
or the session exits; there is no persistent history. Storage failure preserves
available reports and pauses new launches with **Retry storage** guidance.
**Review inputs again** restores original selected file/folder roots under current
settings for correction; a completed Check offers **Review for copy creation**.
Both require explicit Start. Finish or clear an existing draft first; reviewing
a previous request never silently merges or replaces its selected paths.

The empty window offers file/folder selection without inactive report controls.
The compact task drawer keeps navigation separate from the workspace. Choosing a
result keeps that view when queued work starts. A result shows its captured
settings, counts and message outcomes. Initial completion selects a message that
needs attention unless the user already selected one. Errors and recovery guidance
appear in content-sized message details before optional technical provenance.
Paths, removal metadata and byte counts expand under **Paths, parts and technical details**.
**Needs attention only** filters the table without changing the full report or its export;
toggle it off to restore the list. Up/Down/Home/End move through visible messages
and reveal the selected row. **File → Open files…** uses Cmd-O on macOS (Ctrl-O
on other desktops); **File → Add folder…** opens folder selection. Long filenames retain their distinguishing suffixes, with
full paths in tooltips and accessibility text. Expanded metadata offers **Copy path**
for the source and intended destination; it copies the native path, not escaped display text. Report-export feedback is separate
from retained run warnings. **Close result** removes the report from this
session, never the original messages or copies.

The active-task strip names the run addressed by **Stop**, independently of the
inspected result or draft. **Force Stop…** requires confirmation. Completed copies remain after either Stop.
Busy Quit offers Stop and wait, confirmed Force Stop and quit, or Cancel;
Quit waits for worker and report-export settlement, including after a confirmed
Force Stop. When only an export is active, Finish export and quit waits for it;
Force Stop is unavailable.
Waiting requests also require a Quit decision; they are not silently discarded.
Decision dialogs start on Cancel and Escape cancels the decision. Cancelling a
Force Stop decision does not undo a cooperative Stop already requested.
Once shutdown is confirmed, further Quit requests do not reopen the decision,
and new selections or late file-dialog submissions cannot start more work. **Save JSONL report…** writes
the run's original protocol-1 stream captured when Save is clicked, including
partial results after failure. **Export new report** captures that run, offers a native
folder chooser and a file name, then **Create report** writes exclusively. Existing
names are refused inline so you can correct the name without reopening the flow;
there is no Replace action. Navigating between reports does not retarget this draft.
Failed physical writes leave no partial final report; complete bytes are synchronized
before publication. A crash can leave a private staging file, and directory-entry
survival across power loss is not promised. A saved result retains its filename and
location with **Show report**. Refusals before any protocol bytes have no JSONL to save;
their available diagnostics remain under **Failure details**.
**Show copy** is available only for a verified copy receipt.

On macOS, open **EML Attachment Remover → About EML Attachment Remover** in the
system menu bar. Other desktops use **Help → About**. The main window contains
no duplicate product/version banner or About button. About uses a centered
Slint layout with the application icon, release version and optional 12-character
source revision, two purpose lines, copyright, Project website and Licenses, then
AboutSlint. CLI and GUI `--version` (or `-V`) retain the full source revision.
Project website receives keyboard focus when About opens. Licenses shows read-only
software/artwork terms with a separate third-party notices action; reopening it
focuses the text and resets selection and scrolling to the start.
Escape closes either auxiliary window.

A persistent **Settings unavailable** summary means destination changes are session-only.
**Details…** opens the preserved cause, path and recovery in a bounded popup.
The summary remains visible in review and report views.

Saved destinations use the platform configuration folder's
`EMLAttachmentRemover/preferences.json`. Unreadable or corrupt preferences are
preserved. The app starts with defaults and shows the exact path and recovery guidance. Destination changes remain session-only
until the settings problem is corrected and the app restarted.

The app accepts launch arguments, for example:

```sh
open -n -a '/Applications/EML Attachment Remover.app' --args "$PWD/message.eml"
```

Dock-icon drop, Finder Services and Finder Open With are unsupported. File and
folder selection uses parented native dialogs.

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
[release guide](docs/release.md) for packaging and release checks. MPL-2.0-covered
source is available in this repository; redistributors must preserve notices and corresponding source
availability. Slint's desktop license and AboutSlint requirements also apply;
see [legal materials](legal/README.md). Editable artwork, construction references
and platform exports are documented in the [artwork guide](artwork/README.md).
