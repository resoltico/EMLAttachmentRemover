# EML Attachment Remover

Create attachment-stripped EML copies while leaving original messages untouched.
The application uses reusable Rust libraries, a native CLI and a Slint macOS
frontend. Download the separate macOS, Linux or Windows CLI archive, or the
macOS app for your CPU, from [Releases](https://github.com/resoltico/EMLAttachmentRemover/releases).
See the [platform policy](#native-platform-policy) before installing. Public
release requires complete [native, archive and GUI qualification](docs/verification.md)
for the selected source and the [release procedure](docs/release.md). Check the
source-specific [CI runs](https://github.com/resoltico/EMLAttachmentRemover/actions/workflows/ci.yml)
for execution results.

## Run the CLI

Install rustup, then build from a checkout. `rust-toolchain.toml` selects Rust:

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
execution results are recorded in the source-specific CI run.

macOS requires 15.0 or later. Qualification selects the Xcode version/build in
`xtask/tools.toml`. Builds are ad hoc signed, without Developer ID or
notarization; first launch may require app-specific **Open Anyway** in macOS
Privacy & Security. No Gatekeeper configuration change is part of this project.
The desktop workflow has one main window with queued runs, source/destination
selection, per-file results and technical reports. [App details](docs/macos-app.md)
describe the interface and its native qualification requirements.
Intel macOS26 qualification includes a follow-up route after a narrowly
identified native-dialog failure. [Qualification records](docs/verification.md)
distinguish ordinary success, recovery without intervention and a bounded
service intervention. Every follow-up requires the actual same-archive product
to pass; it does not erase the earlier failure or establish general
unmodified-host compatibility.

Linux targets Ubuntu 22.04+ with GNU/glibc, with required GLIBC symbols capped at
2.35. Other distributions are unqualified. Windows targets system UCRT; native
x64 testing uses Server 2022 and ARM64 uses Windows 11. This does not establish
all Windows client patches. Hosted patch versions and compiled deployment minima
are different evidence.

## Develop and redistribute

See [CONTRIBUTING](CONTRIBUTING.md) for setup and checks, the
[architecture](docs/architecture.md) for library boundaries and
[verification](docs/verification.md) for bounded campaigns and required gates.
Editable artwork, geometry and licensing information live under [artwork](artwork).

Software uses MPL 2.0; owner artwork has separate terms in [LICENSE](LICENSE).
Reviewed upstream third-party materials live under [legal](legal). Builds generate
target-specific complete notices and accompanying license files inside each archive. Corresponding source must
be made available when distributing MPL-covered binaries; use the matching source
commit/tag and preserve all applicable notices. Each release identifies its
source revision and includes checksums for its downloadable archives.
