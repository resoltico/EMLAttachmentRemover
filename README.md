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

Add email files or folders with **Add Files…**, **Add Folder…**, a window drop, or
launch arguments. Choose **Create copies** or **Check only** and a destination. Nothing runs until
**Create Copies**, **Start Check**, or **Add to Queue** is pressed. Folder counts
are discovered after Start. Each run keeps its captured inputs and settings.

The session sidebar appears after the first run. Up to eight requests wait behind
one active run; the session holds up to 20 runs and 2.5 GiB of stored report bytes.
Reports are stored automatically in private unnamed files and disappear on close
or exit. There is no persistent history. Storage failure keeps the report in
memory; close finished results if asked for space. **Close Result** removes only session information. **Run Again…** restores the captured draft; a finished
Check offers **Create Copies…**. Replacing a non-empty draft requires confirmation.

Results show progress, outcome filters, naturally sorted messages and an inspector.
Explicitly interrupted messages count as **Not processed** in the GUI and human CLI summary. Genuine processing failures still count as failures. JSON reports retain raw started-failure accounting. Unknown results stay
unknown. **Technical Details** exposes paths and processor diagnostics. **Show Copy**
and output-folder actions require earlier verified copy receipts; opening a file
manager is advisory. **Copy Path** copies the native original path when representable.
**Stop** requests a cooperative stop; **Force Stop…** appears after five seconds and
requires confirmation. Busy Quit offers cancellation or waits for owned work to settle.

**Save Report…** uses a native save panel, including its Replace confirmation, and
saves the exact captured JSONL stream, including partial reports. Failed writes leave
no partial target. The brief success banner offers **Show** for the saved report.
The app remembers three destination folders and the last report folder. Unreadable,
old-format or corrupt settings are preserved; defaults and **Settings unavailable**
are shown. About and Licenses are available in the menus. Dock drops, Finder Services
and Finder Open With are unsupported; file and folder panels are parented to the app.

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
