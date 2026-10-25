# Contributing

Read [AGENTS.md](AGENTS.md). Acceptance requires the complete [native, archive and GUI gates](docs/verification.md) for the delivered
source. Local checks establish only the properties they execute; consult the
source-specific CI run for platform qualification.

## Start with Rust

Install rustup. On Unix, the CI evidence-admission tests also require jq 1.6 or
later on PATH; it is tooling only, not an app/CLI runtime dependency. Then run
from a clean checkout:

```sh
rustup show
cargo test --locked --workspace
cargo build --locked --release -p emlattachmentremover
cargo xtask distribution
```

`rust-toolchain.toml` selects the compiler and components. Cargo.lock is committed;
do not regenerate it merely to work around a failed locked command. Normal product
business behavior belongs in eml-core/eml-fs, not CLI renderers or desktop views.
All test mail is synthetic. Put build output and owned fixtures under target/.
Never use owner mail, saved preferences or unrelated processes for verification.

Docker Linux product tests require a reviewed destination filesystem, such as an ext4-backed volume selected through `TMPDIR`; the usual OverlayFS container layer is deliberately refused for namespace creation. Protected archive tooling also admits every ancestor through `/`, so changing only its scratch directory cannot qualify an OverlayFS-root container; run the complete archive/native suite on the supported GitHub CI hosts.

Windows native builds use the matching `*-pc-windows-gnullvm` host with rust-mingw.
Install that host from the authoritative target table, select its
`lib/rustlib/TARGET/bin/rust-lld.exe` as `CARGO_TARGET_TARGET_LINKER` (uppercase,
hyphens replaced by underscores), and pass the explicit target to Cargo.
`.cargo/config.toml` owns linking flags. MSVC and cross-compilation do not qualify
this contract. The native Windows steps in
[native-qualification.yml](.github/workflows/native-qualification.yml) show the exact invocation.

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
This produces a `native_candidate` receipt after structure, signature, CPU,
minimum-OS and actual extracted-worker checks. On each matching macOS15 host,
`cargo xtask smoke-macos-app --archive ARCHIVE --reference REFERENCE` requires the
complete desktop case set against those frozen bytes. On macOS26 Intel,
`cargo xtask check-macos-picker --archive ARCHIVE --reference REFERENCE` separately
requires the dedicated picker case with the original startup/interaction
prelude, actual message/folder/keyboard pickers, independent copy checks,
scenario-bound preferences and normal Quit. Full Finder and broader desktop
integration remain required on both macOS15 consumers. Neither consumer rebuilds the
shipping app, and candidate success alone does not qualify the desktop product.
A settled actool failure may use the approved macOS15 Intel producer; timeout,
uncertain cleanup, missing pinned Xcode or invalid outputs do not permit fallback.
Keep each app ZIP's frozen `.reference.json` receipt for `smoke-macos-app` and
`check-macos-picker`; it binds producer bytes, not review quality. Ad hoc signing is
the only supported signing mode; no Apple developer account is required.

`ci.yml` sends exact PR head H, main source S or merge-queue source Q through
the reusable native workflow. PR Linux integration uses the exact merge M and
checks its ordered base/H parents. A final API observation rejects stale head/base
pairings. Native jobs retain compiler/host identities, strict Slint fixtures,
real different-UID controls, complete legal generation, native archives and
matching macOS15 desktop qualification and macOS26 Intel picker compatibility.
Intel26 uses one dedicated modified-host job. Preparation requires the exact
configured hosted image, a non-root console user with a graphical Security
session, the fixed Apple GUI-user agent and recognized current-boot crash
evidence. Only then may it boot out and disable that agent. The actual same
frozen product must pass with normal cleanup and preferences; read-only final
checks verify host/context identity and the absent, disabled service. See
[picker admission](docs/verification.md) for exact guards and limits. The private
`prepare-macos-picker-host` and `verify-macos-picker-host` commands are confined
to that hosted job and refuse owner-desktop execution. Unknown images or failure
signatures stop qualification. The standalone panel diagnostic remains manual
only.
`ci-ok` always runs and rejects failed, missing and
unexpectedly skipped jobs; only non-PR events omit merge integration/freshness.
Full product acceptance requires every registered native role and its matching
archive/desktop consumers. Icon and standalone panel diagnostics
are manual only. Partial reruns
that lack the exact current producer artifact fail; rerun the complete workflow.
Actionlint and cargo-fuzz use exact checksum-pinned upstream prebuilts because
install-action does not provide them. Other supported tools use SHA-pinned
install-action, checksums and `fallback:none`. CI requires the pinned prebuilt
downloads; it does not build tools from source.

If a native run disproves a premise relied on by product safety, stop that design
work and obtain the owner decision after that run. Do not leave an impossible
required assertion red while proceeding. Correct an invalid check when the
established product contract does not rely on it, preserving meaningful controls
and the original failure evidence.

## Checks and evidence

```sh
# Install exact tool versions from xtask/tools.toml on PATH
cargo xtask check
cargo xtask check-quality
cargo xtask check-shell
cargo xtask notices --check
cargo xtask notices --write --out target/legal-materials
cargo xtask qualify --target TARGET --out target/native-qualification
cargo xtask verify-archive --archive ARCHIVE --target TARGET
```

On macOS, `qualify` additionally requires `--icons target/icon-input`, produced
by the same-source icon command above, and runs only on a fresh hosted Mac.

Supervised commands select the native Unix or Windows process backend. Bounded
fuzz/mutation campaigns require Unix. Install the exact Cargo tools,
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

Without a producer input, `notices --check` regenerates all current selected graphs
and validates reviewed source/material bindings without maintaining a tracked
generated tree. `--check --out DIR` also
compares an existing tree exactly, including its source receipt. `--write` uses
the default new `target/generated-legal-materials` directory unless `--out` is
given, and refuses an existing address. The two operations are mutually exclusive.
On Intel macOS, set `EML_GENERATED_LEGAL_MATERIALS` to the complete legal artifact
from a successful same-source Linux producer, with both producer and consumer at
the same clean commit. Check evidence then says `consumer_identity`: it checks
inventory, source and every required component identity; it does not claim local
generator execution or independently prove the producer ran. Dirty Intel work
needs a new committed producer run before that route can pass. There is no
source-build fallback for an unavailable prebuilt tool.

The macOS normal/native checks also strictly lint every discovered retained Swift
bridge/test source using the selected Xcode's default formatter configuration.
`cargo xtask check-macos-swift --out NEW_DIR` runs that check separately. Defaults
are captured from the pinned tool and passed explicitly, so ancestor/owner
formatter settings are not inputs. A newer local formatter is supplementary.

Rust compiler, rustdoc and Clippy levels are authoritative in [Cargo.toml](Cargo.toml); all authored members inherit them. Excluded Rust workspaces carry checked-equivalent rules and run Clippy too. [clippy.toml](clippy.toml) defines structural review limits. These tripwires support responsibility-based design review; a small file or function alone does not establish good architecture.

[quality.toml](quality.toml) centrally records every necessary authored lint exception by stable identifier, exact file, named syntax scope, lint set and rationale. Source attributes reference `reason = "quality:IDENTIFIER"`; do not copy rationales into independent inventories. `check-quality` rejects unregistered, stale, duplicated or widened exceptions, conditional and opaque macro suppressions, formatter skips, divergent package rules and undocumented literal lint-weakening options. Safe crates retain their stronger unsafe-code prohibition. Mutation lint capping is confined to the explicitly registered experiment command; it does not qualify product sources.

Source discovery includes tracked and pending nonignored files, extensionless shell shebangs and referenced Rust sources. Vendored upstream packages are explicitly classified ownership boundaries, rather than authored style exemptions. The physical source-file limit is a navigation/review tripwire and applies to tests and checking tools too; split responsibilities rather than arbitrary text chunks.

Native Swift checks compile the syntax checker using the selected Xcode's matching SwiftParser/SwiftSyntax host modules. Missing modules or parse errors fail visibly. Callable code-line and parameter limits follow the corresponding Clippy thresholds; Swift branch complexity uses the same numeric threshold but a distinct metric: one plus conditionals, loops, catches, nondefault switch cases, ternaries, additional comma-separated conditions and boolean conjunction/disjunction. Nested callable bodies, closures and accessors are measured separately. Negative controls accompany both Rust policy checks and Swift structure checks.

macOS `qualify` produces a native app candidate; separate same-byte desktop and
picker consumers establish GUI qualification. Final acceptance on every platform
requires the delivered source's complete native/archive/GUI matrix. TARGET and ARCHIVE must match actual native
host and declared artifact identities. Reusing a populated output directory is
refused. See [verification](docs/verification.md) for the exact implemented scope.
Release admission, qualification, attestations and draft recovery are described
in the [release procedure](docs/release.md). Publication requires an explicit
owner release decision.

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
