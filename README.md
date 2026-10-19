# EML Attachment Remover

Create attachment-stripped EML copies while leaving original messages untouched.
The v6 source uses reusable Rust libraries, a native CLI and a Slint macOS
frontend following the [owner's scope decision](docs/desktop-frontend-design.md).
Version 6 remains **Unreleased**. Required native, archive and GUI qualification
outcomes, including failures and unrun gates, are recorded in the
[implementation evidence](docs/v6-implementation-evidence.md). Published v5
releases and their source history remain unchanged.

## Run the CLI

Build from a checkout with Rust 1.99.0:

```sh
cargo build --locked --release -p remove-eml-attachments
./target/release/remove-eml-attachments --help
./target/release/remove-eml-attachments --dry-run -- message.eml
./target/release/remove-eml-attachments -- message.eml
mkdir copies
./target/release/remove-eml-attachments --output-dir copies -- selected-folder
./target/release/remove-eml-attachments --format json -- message.eml
```

On Windows the executable ends in `.exe`; native GNU LLVM builds also require
its selected Rust sysroot linker and rust-mingw component. See
[contributing](CONTRIBUTING.md). These examples require your own input file; the
program never modifies it. Paths are literal native arguments. Let the shell
expand home-directory shorthand; display text is not an operational filename.
Use `--` before filenames that begin with a dash.
The selected output root must already exist; the program creates only the
required mirrored directories beneath it.

A copy of `message.eml` is named `message.no-attachments.eml`. The final `.eml`
extension is recognized without regard to case. A long stem is shortened to the
actual destination component limit; this can cause a refused collision. There
is no numeric collision search or name digest. Directory walks skip recognized
copy suffixes; an explicitly selected file is still an input.

Without `--output-dir`, copies go beside each source. With it, explicitly selected
files go directly into that folder and selected directories mirror their relative
tree under a root label. Multiple inputs that reserve the same destination fail;
choose distinct destinations rather than expecting automatic renaming. An
identical existing copy is checked and resynchronized as up to date. A different
existing file remains untouched. Output overlapping selected source trees and
source/destination aliases are refused.

`--dry-run` checks available preparation and destination facts without creating
folders, stages or copies. It cannot establish that a later create will succeed.
The first interrupt requests cooperative stop; a second forces exit 130 and may
leave a recognized staging directory. Completed copies remain. An OS or storage
call can block beyond cooperative cancellation; no universal stop deadline is
promised. Inspect retained residue only after all processing using that destination
has settled.

JSON mode emits protocol 1 start/item/summary records, each terminated by LF.
`--request FILE` or `--request -` accepts one bounded EOF-terminated native-path
request instead of positional inputs. See the [protocol contract](docs/protocol.md)
for lossless path encoding, required nullable fields, counts, failures and limits.
Exit 0 means completed results, exit 1 means failure, exit 2 means pre-stream
usage/request admission failure, and exit 130 means cooperative or forced interruption.
A missing summary or truncated stream is incomplete evidence.

## Message and storage policy

Preparation removes authorized source spans; it does not decode and regenerate
retained message content. It keeps supported text body alternatives, selects the
related body root and removes non-root related resources, including inline images.
Other non-body leaves, including inline images outside related containers,
calendar payloads and opaque encoded forwarded messages, are removed even without
an attachment disposition. At least one supported text body must remain. It
handles embedded messages conservatively and refuses protected or structurally
unusable messages. Attached files, including mail containers, are
not exported elsewhere. Source bytes, line endings and retained structure are
independently checked. See [MIME behavior](docs/mime.md).

This is not a malware scanner or an HTML sanitizer. Retained HTML can contain
remote content, tracking and active rendering behavior; use a viewer configured
for untrusted mail. An attachment-free copy is not a safe-to-open certification.

Publication uses private staging and atomic no-replace operations with independent
source, candidate, address, synchronization and cleanup observations. A file can
be published even when a later check fails; inspect the receipt rather than
assuming every failed result means no copy exists. OS flush success is not a
promise of hardware durability, network-server persistence or immunity to later
changes by an equally privileged actor. Unsupported permission/volume models
are refused. See [filesystem boundaries](docs/filesystem.md) and
[Windows publication](docs/windows-publication.md). Windows copies inherit the
destination folder's ACL. Another writer may make an item fail by redirecting an
empty destination, but the app checks containment before writing message bytes;
it does not promise that other writers cannot later change a completed copy.

## Native platform policy

The authoritative [distribution table](xtask/distribution.toml) declares six
separate CLI builds: macOS Intel/Apple Silicon, Linux x64/ARM64 and Windows
x64/ARM64. Two separate macOS apps use their same-CPU private core-backed worker. No
universal binary or other-platform GUI is provided. These are target policies;
actual completed and failed gates are recorded separately in the evidence log.

macOS requires 15.0 or later. Qualification selects Xcode 26.3 build 17C529, not the
machine's default Xcode. Builds are ad hoc signed, without Developer ID or
notarization; first launch may require app-specific **Open Anyway** in macOS
Privacy & Security. No Gatekeeper configuration change is part of this project.
The desktop workflow has one main window with queued runs, source/destination
selection, per-file results and technical reports. [App details](docs/macos-app.md)
and the evidence log distinguish implemented behavior from native qualification.
The hosted Intel macOS26 image has shown intermittent native-dialog failures.
[Qualification records](docs/verification.md) distinguish ordinary runs from the
narrowly controlled-host route; controlled-host success does not establish
healthy unmodified-host compatibility.

Linux targets Ubuntu 22.04+ with GNU/glibc, with required GLIBC symbols capped at
2.35. Other distributions are unqualified. Windows targets system UCRT; native
x64 testing uses Server 2022 and ARM64 uses Windows 11. This does not establish
all Windows client patches. Hosted patch versions and compiled deployment minima
are different evidence.

## Develop and redistribute

See [CONTRIBUTING](CONTRIBUTING.md) for setup and checks, the
[architecture](docs/architecture.md) for library boundaries and
[verification](docs/verification.md) for bounded campaigns and remaining gates.
The repository uses Rust tooling; it contains no Python product or build system.
The editable owner artwork and its historical construction records remain under
[artwork](artwork).

Software uses MPL 2.0; owner artwork has separate terms in [LICENSE](LICENSE).
Reviewed upstream third-party materials live under [legal](legal). Builds generate
target-specific complete notices and accompanying license files inside each archive. Corresponding source must
be made available when distributing MPL-covered binaries; use the matching source
commit/tag and preserve all applicable notices. No v6 download, tag or published
source archive is claimed here.
