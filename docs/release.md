# Distribution and release

The workspace manifest owns the only maintained release version;
`eml_core::product` owns names and shared source-identity formatting.
The CLI and GUI `--version` retain a supplied full source revision; About uses
its first 12 characters. There is no separate manually maintained build counter.
The compiler comes from rust-toolchain.toml and developer tools from xtask/tools.toml. Distribution requires native platform
linkers; macOS uses Command Line Tools rather than a full Xcode installation.

When upgrading pinned Slint, re-verify native Edit → Copy and Select All routing in
[`text_commands.rs`](../crates/eml-gui/src/text_commands.rs), including its private
`i_slint_core` focus and popup access.

## Build and inspect locally

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
for mac_arch in aarch64 x86_64; do
  cargo xtask dist --cli --app --target "$mac_arch-apple-darwin" --out target/dist
done
```

Run the exact-archive checks on the matching Mac. The example selects Apple
Silicon. On Intel set `mac_arch=x86_64` and `mac_macho_arch=x86_64`; each
architecture needs its own native run. The checks reject wrong-CPU or
multi-architecture binaries and a Rosetta-translated shell before execution.

```sh
set -eu
mac_arch=aarch64
mac_macho_arch=arm64
mac_target="$mac_arch-apple-darwin"
mkdir -p "target/smoke/$mac_arch"
tar -xzf target/dist/emlattachmentremover-*-macos-"$mac_arch".tar.gz -C "target/smoke/$mac_arch"
ditto -x -k target/dist/emlattachmentremover-gui-*-macos-"$mac_arch".zip "target/smoke/$mac_arch"
host_arch=$(uname -m)
test "$host_arch" = "$mac_macho_arch"
translated=$(sysctl -in sysctl.proc_translated)
test "$translated" != "1"
cli_arch=$(lipo -archs "target/smoke/$mac_arch/emlattachmentremover")
test "$cli_arch" = "$mac_macho_arch"
revision=$(git rev-parse HEAD)
app_version=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "target/smoke/$mac_arch/EML Attachment Remover.app/Contents/Info.plist")
cli_version=$("target/smoke/$mac_arch/emlattachmentremover" --version)
test "$cli_version" = "emlattachmentremover $app_version ($revision)"
scripts/smoke-cli.sh "target/smoke/$mac_arch/emlattachmentremover" tests/contract/mime
EML_SOURCE_REVISION="$revision" MACOSX_DEPLOYMENT_TARGET=15.0 cargo build --locked --profile dist --target "$mac_target" -p eml-gui --bin eml-gui-document-probe --features desktop
cargo_artifacts=$(cargo metadata --locked --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
EML_SOURCE_REVISION="$revision" scripts/smoke-macos-app.sh "target/smoke/$mac_arch/EML Attachment Remover.app" tests/contract/mime/inputs/01-gmail-attachment.eml "$cargo_artifacts/$mac_target/dist/eml-gui-document-probe" "$mac_macho_arch"
```

Distribution uses the `dist` Cargo profile: checked release arithmetic, Thin LTO,
one codegen unit and stripped symbols. Ordinary development builds keep their
existing debug/incremental settings. Cargo metadata supplies the actual target
directory, including Cargo configuration overrides.
Commands resolve the invoking workspace with Cargo at runtime, including when
called from a member directory. A shared target cache must not select the
worktree in which an earlier `xtask` executable happened to be compiled.

Use a fresh output folder. The CLI archive is flat: executable, LICENSE and
THIRD-PARTY-NOTICES.txt. The app ZIP contains `EML Attachment Remover.app`, with
GUI executable `Contents/MacOS/EMLAttachmentRemover` and icon/notices/LICENSE in
Resources. It contains no CLI; Terminal users install the separate CLI archive.
The GUI Cargo target is `emlattachmentremover-gui`; the bundle identifier is
`io.github.resoltico.emlattachmentremover`. The bundle declares email and folder
document types as Viewer/Alternate, with no Services or Assets.car. The icon is the committed complete ICNS; re-exporting its editable
source is an artwork-maintenance task, described in
[artwork guide](../artwork/README.md).

Each macOS archive contains exactly one architecture: `macos-aarch64` contains
arm64 Mach-O code; `macos-x86_64` contains x86_64 code. Both have deployment minimum
15.0. Distribution defaults to the Rust host target; `--target` selects a different
supported architecture. The app requires a macOS build host and a Darwin target.
The app bundle is ad hoc signed, followed by deep strict signature verification.
Each distribution command resolves `HEAD` once, embeds that full revision in both
binaries for its CPU target, and uses its UTC committer time for archives.
A conflicting `EML_SOURCE_REVISION` is rejected; empty or absent values are omitted
from ordinary Cargo builds. Supplied revisions must contain 40 or 64 ASCII
hexadecimal characters. ZIP timestamps round down to two-second precision.
Commit reviewed inputs before distributing: packages built from a dirty checkout
carry the base revision and are development evidence, not exact source snapshots.

`CFBundleShortVersionString` is the workspace version. `CFBundleVersion` is derived
as `major * 10000 + minor * 100 + patch` (7.0.0 becomes 70000, greater than the old
Build 44). App packaging requires X.Y.Z without suffixes, minor/patch at most 99,
and a result representable as a u32. These bounds keep the mapping ordered and
collision-free. Increase the release version before distributing changed builds;
changing only the source hash does not increase the native bundle version.

Linux x86_64/aarch64 CLI builds use Ubuntu 26.04 and tar.gz. Windows x86_64/aarch64
CLI builds use native MSVC with static CRT and ZIP. On Linux run
`cargo xtask dist --cli --target <native-triple> --out <fresh-folder>`. On Windows
run `cargo run --locked -p xtask --target <native-triple> -- dist --cli --target <native-triple> --out <fresh-folder>`
so host procedural macros are built separately from static-CRT target flags.
The CLI and GUI are built in separate Cargo invocations so their dependency
features remain independent. Each archive includes notices for its selected normal/build dependency graph and
Rust standard-library material. App notices cover its selected architecture’s GUI
graph, with required upstream supplements. See [legal materials](../legal/README.md).

## CI and owner release steps

The [release workflow](../.github/workflows/release.yml) builds archives once,
smokes those exact bytes, and runs the Apple Silicon CLI/app on macos-15 and the
Intel CLI/app on macos-15-intel. CLI smoke compares every MIME fixture with its
golden bytes or expected refusal and checks original bytes. App smoke verifies
signatures and the exact single CPU architecture, launches with two selected paths
and requests idle Quit without writing copies, and rejects a Helpers directory.
CLI archives own their independent golden copy check. It does not automate Finder, Dock, file dialogs or other OS UI.

The floor smoke also runs [the native document-event probe](../scripts/smoke-macos-document-event.sh),
which requires Python 3 (standard library only; tested with 3.9 and 3.15).
After the exact archive checks, it makes a separate ad hoc signed copy with an
isolated bundle identifier and a recording runner that executes real core in
process and refuses create requests. The separate `eml-gui-document-probe` test
binary uses the same native startup and the controller's single runner seam. Its
source identity must match the production app. It is never packaged or released. The first check establishes startup;
one `open -a` delivery must then produce a request containing both native paths.
The probe checks source preservation and unregisters its temporary bundle. It
stays outside the merge gate and does not replace manual Dock acceptance.
Pass the matching probe binary as its third argument. The release workflow builds
a separate matching probe for each Mac target, deriving its minimum OS from the
app plist. Product artifact selection explicitly excludes this fixture. Passing
`--omit-delivery` to the probe is a negative control: it must fail because no
second request arrives. There are no delivery retries.

A branch `workflow_dispatch` builds, smokes and uploads archives and SHA256SUMS.
The checksum job requires exactly eight product archives: six CLI archives
(macOS, Linux and Windows, each for both CPUs) and two macOS app ZIPs. The
architecture-matched document probes are separate test artifacts and are excluded
from this set. After downloading the complete product set again, the tag-only
draft job checks every archive against SHA256SUMS before attestation and drafting.
Producer archives and CLI extraction folders use runner-temporary directories
outside restored Cargo caches. A branch run skips attestation and draft-release
creation. On an existing `v*` tag, successful
build/floor jobs also attest artifacts and create a draft GitHub release using
`--verify-tag`. Draft notes come from the dated `CHANGELOG.md` entry matching the
tag version in the checked-out release commit, passed through `--notes-file`.
A missing, duplicate, undated or whitespace-only entry fails before attestation
or draft creation. The workflow never creates tags. Publishing the draft is an
owner action. Caches restore on producers; only main can save caches.

Before the owner tags or publishes:

1. Confirm the intended source revision, version and release notes.
   Move the reviewed `Unreleased` bullets into `## [VERSION] - YYYY-MM-DD`, leaving
   an empty `Unreleased` section above it. Commit that entry before creating the
   matching `vVERSION` tag; tag annotations do not supply the release notes.
   Review the final PR's native CI and both macOS floor-smoke results.
2. On Intel and Apple Silicon, manually check real window drag delivery, native
   file/folder dialogs, automatic preview, Remove Attachments and destination selection,
   keyboard navigation, appearance,
   AboutSlint, Stop/Continue, busy Quit/Quit Now and exact report export. Check
   Finder Open With for an email and a folder, Dock drops and launch-by-document. Check
   non-Unicode/long paths and a realistic mailbox selection where practical.
3. Review the software-renderer screenshots and retained upstream license texts.
   Check the full icon at normal/high display scale. Real OS interaction and
   visual review belong in this manual checklist, outside the merge gate.
4. Verify downloaded archive checksums against SHA256SUMS. On macOS use
   `shasum -a 256 -c SHA256SUMS`; on Linux use `sha256sum -c SHA256SUMS`.
   GitHub artifact attestations can be checked with
   `gh attestation verify <archive> --repo resoltico/EMLAttachmentRemover`.
5. Test first launch of the downloaded ad hoc app using the
   [README opening steps](../README.md#install). Preserve corresponding MPL source
   availability and notices when redistributing.

The scheduled Weekly workflow runs full fuzz and mutation campaigns once a week,
separately from the merge gate. Only the owner may dispatch it manually. Development
is limited to fuzz micro runs of at most 120 seconds (10 minutes total) and mutation
runs of at most 60 selected mutants. See [CONTRIBUTING](../CONTRIBUTING.md) for commands.
Intel Macs retain their own builds and macOS 15 Intel archive smoke coverage; the
separate weekly Intel full-suite job is retired. The first scheduled Weekly run
establishes the survivor baseline and is not part of the gates follow-up PR.
