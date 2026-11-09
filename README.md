# EML Attachment Remover

Create verified body-only copies of EML messages while keeping originals unchanged.
Copies retain supported plain-text and HTML bodies, including their encoded bytes;
attachments and other non-body MIME parts are removed. Retained HTML can still
contain remote references or unsafe content. See the [MIME policy](docs/mime.md).

This documentation describes the source in this checkout. For downloads, use the
documentation at the matching release tag.

## Install

Download from [Releases](https://github.com/resoltico/EMLAttachmentRemover/releases).
Choose the archive matching your platform and CPU:

| Product | Archive | Minimum platform |
|---|---|---|
| CLI, macOS Apple Silicon | `emlattachmentremover-VERSION-macos-aarch64.tar.gz` | macOS 15 |
| CLI, macOS Intel | `emlattachmentremover-VERSION-macos-x86_64.tar.gz` | macOS 15 |
| Desktop app, macOS Apple Silicon | `emlattachmentremover-gui-VERSION-macos-aarch64.zip` | macOS 15 |
| Desktop app, macOS Intel | `emlattachmentremover-gui-VERSION-macos-x86_64.zip` | macOS 15 |
| CLI, Linux x86_64 or aarch64 | `emlattachmentremover-VERSION-linux-ARCH.tar.gz` | Ubuntu 26.04 |
| CLI, Windows x86_64 or aarch64 | `emlattachmentremover-VERSION-windows-ARCH.zip` | Windows 10 |

On a Mac, open Apple menu → About This Mac: a **Chip** entry identifies Apple
Silicon (`aarch64`); an Intel **Processor** entry uses `x86_64`. Each Mac archive
contains only its named architecture. Choose the native build for your Mac.

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

Human output uses the app’s core outcome labels and batch headline (JSON is unchanged):

```text
Ready                    message.eml → message.no-attachments.eml · 1 attachment · 392 B → 183 B
1 email · 1 attachment · 209 B would be removed. Exit 0
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

The Mac app runs core in process and needs no CLI installation. The app no longer
contains a CLI executable; install the separate macOS CLI archive for Terminal use.

Add `.eml` files or folders with **Add Files…**, **Add Folder…**, a window drop,
launch arguments, Finder **Open With**, or a drop on the Dock icon. Folder contents
are discovered immediately by a read-only check. Nothing is written until
**Remove Attachments**. Changing the inputs or save folder refreshes the preview;
you can press Remove Attachments while the check is still running.

One batch stays in one list in worker discovery order. Rows preview attachment names and
source/copy sizes, then show the result in place. The problems pill filters to
emails needing attention. Select a row for its explanation and attachment details;
**Show Copy** requires a verified copy receipt, **Show Original** reveals its source,
and **Copy Details** copies native paths and diagnostic facts. File-manager actions
are advisory and do not reverify a file's current identity.

Copies go **Next to originals** or to a chosen folder. They use the
`.no-attachments.eml` suffix and never overwrite existing entries. Emails with
nothing to remove are still copied, labelled **Copied as is**. Plain-text and HTML
message bodies are kept; HTML is not sanitized or rewritten. Attachments, inline
images, calendar parts and other non-body content are removed. Copies may therefore
have missing inline images. Signed or encrypted structures that cannot be changed
safely are refused; see the processing limits below.

**Stop** cooperatively interrupts copying. **Continue** repeats the same batch,
verifying and skipping identical copies already saved. Adding more while copying
is refused. After Done or Stopped, adding files starts a new batch immediately;
**Start Over** clears the list. There is no batch history or waiting queue.

**Save Report…** in File saves the exact latest settled JSONL stream: the create
stream when available, otherwise the check stream, including incomplete reports.
The native panel owns Replace approval; failed writes preserve prior targets.
The brief success notice offers **Show**. Reports stay in memory until the batch
changes or the app exits; an outstanding save keeps its captured bytes until it
settles. Save a report before starting another batch if you need it later.

Quit while copying offers **Cancel** or **Stop and Quit**. After five seconds of
waiting, **Quit Now** exits the app without waiting for the create thread;
report exports already in progress still settle first. Read-only checks receive
cooperative stop automatically, without a dialog.
The app remembers three recent save folders, the last destination and the last
report folder. Invalid or unreadable settings silently use defaults and are
replaced on the next successful save. Folder settings now use native encoding;
earlier string settings reset to defaults. About and Licenses are in the menus.
Finder Services remain unsupported. Open With uses rank Alternate, keeping the
existing default mail application.

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

The native desktop GUI and its archives target macOS only, using Slint's Winit
backend. Linux supports core, the CLI and headless GUI development
through Slint's testing backend; Windows supports core and the CLI.

See [CONTRIBUTING](CONTRIBUTING.md) for the shortest build and check commands,
[architecture](docs/architecture.md) for crate boundaries, and the
[release guide](docs/release.md) for packaging and release checks. MPL-2.0-covered
source is available in this repository; redistributors must preserve notices and corresponding source
availability. Slint's desktop license and AboutSlint requirements also apply;
see [legal materials](legal/README.md). Editable artwork, construction references
and platform exports are documented in the [artwork guide](artwork/README.md).
