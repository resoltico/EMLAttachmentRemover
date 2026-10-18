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
this contract. The native Windows steps in .github/workflows/ci.yml show the exact invocation.

macOS app checks require the Xcode version/build in xtask/tools.toml. Set
DEVELOPER_DIR only for each invocation to that Xcode's Contents/Developer path;
do not change xcode-select or saved owner preferences. The builder verifies the
compiler, SDK, CPU and deployment policy. A newer local Xcode is supplementary
evidence. Build the native GUI/worker executables with
`cargo xtask build-macos-sources --out target/desktop-sources`, using a
new output directory. Actual GUI automation is confined to owned hosted runners;
local tests must not change Accessibility permissions or preference domains.

Build complete app archives on the native macOS26 hosts and execute the same bytes
on matching macOS15 hosts. The minimum remains 15.0. Compile the owner icon once
with `cargo xtask compile-macos-icon --out target/icon-input`; pass that validated
same-source directory to both native builders with
`cargo xtask build-macos-app --icons target/icon-input --out target/app-build`.
A settled actool failure may use the approved macOS15 Intel producer; timeout,
uncertain cleanup, missing pinned Xcode or invalid outputs do not permit fallback.
Keep each app ZIP's frozen `.reference.json` receipt for native floor smoke and
`verify-archive`; it binds producer bytes, not review quality. Ad hoc signing is
the only supported signing mode; no Apple developer account is required.

One `ci.yml` handles branch heads, standard PR merge checkouts, main and merge
queues. It retains actual checkout identities, six native test jobs, strict Slint
fixtures, real different-UID controls, complete legal generation, native app/CLI
archives and matching macOS15 smoke. `ci-ok` always runs and accepts only success
for every required job/matrix. Icon diagnostics are manual only. Partial reruns
that lack the exact current producer artifact fail; rerun the complete workflow.
Actionlint uses its exact checksum-pinned upstream prebuilt because install-action
does not provide it; supported tools use SHA-pinned install-action, checksums and
`fallback:none`. No tool source-build path remains.

If a native run disproves a premise relied on by product safety, stop that design
work and obtain the owner decision after that run. Do not leave an impossible
required assertion red while proceeding. Correct an invalid check when the
established product contract does not rely on it, preserving meaningful controls
and the original failure evidence.

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
GUI/worker closures have separate component/target material bindings and enabled-feature policy checks. Native Intel macOS builds consume
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

On a fresh owned hosted Mac, `cargo xtask check-macos-volume --out NEW_DIRECTORY`
creates and tests only its own exFAT image/mount, records actual native capability
and Core outcomes, and detaches only its verified device. Native privilege or
cleanup failure keeps this required gate failed/unrun. Local owner preferences
and unrelated disks are never setup resources.
