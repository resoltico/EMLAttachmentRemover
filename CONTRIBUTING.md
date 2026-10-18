# Contributing

Use a separate checkout and read [AGENTS.md](AGENTS.md). The v6 implementation is
Unreleased and its complete acceptance gates remain open; consult the
[implementation evidence](docs/v6-implementation-evidence.md) before interpreting
a local pass as native platform qualification.

## Start with Rust

Install rustup, then run from a clean checkout:

```sh
rustup show
cargo test --locked --workspace
cargo build --locked --release -p remove-eml-attachments
cargo xtask distribution
```

`rust-toolchain.toml` selects the compiler and components. Cargo.lock is committed;
do not regenerate it merely to work around a failed locked command. Normal product
business behavior belongs in eml-core/eml-fs, not CLI renderers or desktop views.
All test mail is synthetic. Put build output and owned fixtures under target/.
Never use owner mail, saved preferences or unrelated processes for verification.

Windows native builds use the matching `*-pc-windows-gnullvm` host with rust-mingw.
Install that host from the authoritative target table, select its
`lib/rustlib/TARGET/bin/rust-lld.exe` as `CARGO_TARGET_TARGET_LINKER` (uppercase,
hyphens replaced by underscores), and pass the explicit target to Cargo.
`.cargo/config.toml` owns linking flags. MSVC and cross-compilation do not qualify
this contract. The hosted Windows workflow shows the exact native invocation.

macOS app checks require the Xcode version/build in xtask/tools.toml. Set
DEVELOPER_DIR only for each invocation to that Xcode's Contents/Developer path;
do not change xcode-select or saved owner preferences. The builder verifies the
compiler, SDK, CPU and deployment policy. A newer local Xcode is supplementary
evidence. Build the native GUI/worker executables with
`cargo xtask build-macos-sources --out target/desktop-sources`, using a
new output directory. Actual GUI automation is confined to owned hosted runners;
local tests must not change Accessibility permissions or preference domains.

## Checks and evidence

```sh
# Install exact tool versions from xtask/tools.toml on PATH
cargo xtask check
cargo xtask notices --out target/legal-materials
cargo xtask qualify --target TARGET --out target/native-qualification
cargo xtask verify-archive --archive ARCHIVE --target TARGET
```

These supervised check commands currently require Unix; their Windows process
backend remains an open distribution gate. Install the exact Cargo tools,
actionlint and shellcheck declared in xtask/tools.toml on PATH. Every Cargo-tool
consumer checks the selected executable's version, including notices and isolated
campaigns. CI uses checksum-verified prebuilt Cargo tools with source-build
fallback disabled. No repository task installs or replaces global tools.

The complete check command retains independent failed sections under
target/repository-checks. Required local Xcode absence remains explicit. Legal
materials are generated into a new output directory at build time from the
reviewed inputs in legal/; there is no tracked generated tree to refresh. The
GUI/worker closure requires its own review. Native Intel macOS builds consume
same-source Linux-generated legal inputs because cargo-about has no Intel macOS
prebuilt download at the selected version; this does not establish native
execution or legal review.

`qualify` currently fails at remaining required platform/app gates even after a
bounded CLI package check succeeds. TARGET and ARCHIVE must match actual native
host and declared artifact identities. Reusing a populated output directory is
refused. See [verification](docs/verification.md) for the exact implemented scope.

From a clean committed source, run the bounded campaigns separately:

```sh
cargo fetch --locked --manifest-path fuzz/Cargo.toml
cargo xtask fuzz --seconds 60
cargo xtask mutants --initial
cargo xtask mutants --diff origin/main
```

Use the pinned fuzz compiler and tools. These campaigns require Unix and retain
all admitted mutants/outcomes; do not expand them into a full workspace campaign.
Dirty/new files must not disappear from mutation selection. Failed, inconclusive,
filtered or unrun required checks remain visible in the PR.

## Changes and redistribution

Keep source goldens independent of the implementation. Verify actual extracted
binaries and real native boundaries when making integration claims. Report exact
source SHA, compiler/host/OS, commands, discovered tests and every required gate.
Do not substitute a hash, mock or successful build for execution evidence.

Update affected contracts, callers, packaging and documentation together. Record
net user/operator outcomes under Unreleased; preserve published changelog history.
Software and artwork have distinct terms in LICENSE. Keep editable artwork and
its grants intact. No agent task authorizes tagging, publishing, merging, enabling
auto-merge or changing repository settings. Release remains a later owner action.
