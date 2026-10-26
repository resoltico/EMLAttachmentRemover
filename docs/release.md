# Distribution and release

The workspace manifest owns the release version; `eml_core::product` owns the
monotonic build number and names. The compiler comes from rust-toolchain.toml and
developer tools from xtask/tools.toml. Distribution requires native platform
linkers; macOS uses Command Line Tools rather than a full Xcode installation.

## Build and inspect locally

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo xtask dist --cli --app --universal --out target/dist
mkdir -p target/smoke
tar -xzf target/dist/emlattachmentremover-*-macos-universal.tar.gz -C target/smoke
ditto -x -k target/dist/emlattachmentremover-gui-*-macos-universal.zip target/smoke
scripts/smoke-cli.sh target/smoke/emlattachmentremover tests/contract/mime
scripts/smoke-macos-app.sh 'target/smoke/EML Attachment Remover.app' tests/contract/mime/inputs/01-gmail-attachment.eml
```

Use a fresh output folder. The CLI archive is flat: executable, LICENSE and
THIRD-PARTY-NOTICES.txt. The app ZIP contains `EML Attachment Remover.app`, with
GUI executable `Contents/MacOS/EMLAttachmentRemover`, public CLI helper
`Contents/Helpers/emlattachmentremover`, and icon/notices/LICENSE in Resources.
The GUI Cargo target is `emlattachmentremover-gui`; the bundle identifier is
`io.github.resoltico.emlattachmentremover`. The bundle has no Services/document
declarations or Assets.car. The icon is the committed complete ICNS; re-exporting its editable
source is an artwork-maintenance task, described in
[artwork guide](../artwork/README.md).

Both Mac executables contain arm64 and x86_64 slices with deployment minimum 15.0.
The helper is ad hoc signed first, then the bundle, followed by deep strict
signature verification. Archive timestamps use the `HEAD` commit's UTC committer
time from Git; ZIP timestamps round down to two-second precision. Commit intended
release inputs before building so that timestamp and source revision agree.

Linux x86_64/aarch64 CLI builds use Ubuntu 22.04 and tar.gz. Windows x86_64/aarch64
CLI builds use native MSVC with static CRT and ZIP. On Linux run
`cargo xtask dist --cli --target <native-triple> --out <fresh-folder>`. On Windows
run `cargo run --locked -p xtask --target <native-triple> -- dist --cli --target <native-triple> --out <fresh-folder>`
so host procedural macros are built separately from static-CRT target flags.
Each archive includes notices for its selected normal/build dependency graph and
Rust standard-library material. App notices cover both executables and
both CPU slices, with required upstream supplements. See
[legal materials](../legal/README.md).

## CI and owner release steps

The [release workflow](../.github/workflows/release.yml) builds archives once,
smokes those exact bytes, and runs the universal CLI/app on macos-15 and
macos-15-intel. CLI smoke compares every MIME fixture with its golden bytes or
expected refusal and checks original bytes. App smoke verifies signatures and CPU
slices, launches with two selected paths and requests idle Quit without writing
copies. It then invokes the bundled CLI helper and compares copies with a golden. It does not automate Finder, Dock, file dialogs or other OS UI.

A branch `workflow_dispatch` builds, smokes and uploads archives and SHA256SUMS.
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

1. Confirm the intended source, version, build number and release notes.
   Move the reviewed `Unreleased` bullets into `## [VERSION] - YYYY-MM-DD`, leaving
   an empty `Unreleased` section above it. Commit that entry before creating the
   matching `vVERSION` tag; tag annotations do not supply the release notes.
   Review the final PR's native CI and both macOS floor-smoke results.
2. On Intel and Apple Silicon, manually check real window drag delivery, native
   file/folder dialogs, selection review, explicit Start and destination selection,
   keyboard navigation, appearance,
   AboutSlint, Stop/Force Stop, busy Quit and technical report export. Check
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

Nightly CI runs Intel macOS tests and 15 minutes of pure MIME fuzzing. Its failures
are investigated separately from the merge gate.
