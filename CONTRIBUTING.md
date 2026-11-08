# Contributing

Read [AGENTS.md](AGENTS.md), [AGENTS_EXTRA.md](AGENTS_EXTRA.md) and
[project rules](docs/project-rules.md). Source behavior, public contracts and tests
must agree. Keep product code separate from transport and platform presentation.

## Build from a clean checkout

Install Rust with rustup and a native C/C++ linker. The compiler and components
come from [rust-toolchain.toml](rust-toolchain.toml); rustup selects them when Cargo
runs here. `EML_SOURCE_REVISION` optionally supplies a full 40/64-character commit
ID; CI sets it from its checked-out revision. Without it, ordinary builds display
only the workspace version. Distribution resolves and supplies HEAD for both
frontends, using the optimized `dist` profile (see [release identity](docs/release.md)).
On macOS, install Command Line Tools (`xcode-select --install`); full
Xcode is unnecessary for builds. Linux development and released CLI binaries
require Ubuntu 26.04 or newer. Native GUI development also needs `pkg-config`,
`libfontconfig1-dev` and `libwayland-dev`, plus the Qt SDK version in `xtask/tools.toml`. Install the
official Qt 6.12 SDK (Qt Base, Qt Wayland and bundled ICU) for your architecture and set
`QMAKE` to its `bin/qmake`. Distro Qt development packages may be older than the
supported baseline. Install the
[Qt Linux system prerequisites](https://doc.qt.io/qt-6/linux-requirements.html)
for your display server; the SDK does not supply all host libraries.
Windows CLI development uses Visual Studio C++ Build Tools with the native x64
or ARM64 MSVC tools. CI builds a static CRT.

```sh
cargo build --locked -p emlattachmentremover
cargo test --locked --workspace
```

Install the tools listed under Checks first. For a native GUI on macOS, build
and open the actual app bundle so OS identity,
resources and the helper CLI use their supported layout:

```sh
cargo xtask dist --app --out target/desktop
mkdir -p target/desktop/app
ditto -x -k target/desktop/emlattachmentremover-gui-*-macos-universal.zip target/desktop/app
open "target/desktop/app/EML Attachment Remover.app"
```

Use a fresh output directory on subsequent distributions. Apart from `--version`
and `-V`, the bare Cargo GUI executable requires its macOS bundle/helper context
before opening a window. The GUI library and testing backend support macOS and
Linux. Native Linux GUI builds use Slint's Qt backend, Qt 6 Widgets and Qt's
accessibility integration;
macOS uses Slint's Winit backend. CI installs the exact Qt SDK pin on Ubuntu 26.04 for both Linux
architectures; `"$QMAKE" -query QT_VERSION` identifies the selected SDK. Native distribution archives currently target
macOS. Linux development uses:

```sh
cargo build --locked -p emlattachmentremover
cargo build --locked -p eml-gui --bin emlattachmentremover-gui --no-default-features --features desktop-qt
cargo run --locked -p eml-gui --bin emlattachmentremover-gui --no-default-features --features desktop-qt
```

Keep the built CLI beside the GUI executable. Linux native operation requires
Qt 6.12 or newer within Qt 6, with the runtime version matching the build SDK
exactly, plus a Qt platform plugin for the session:
Wayland or X11. Install Qt Wayland from the same SDK for Wayland, and put the
SDK's `lib` directory on `LD_LIBRARY_PATH` when it is outside the system library
search path. Native file dialogs require XDG Desktop Portal and your desktop's file chooser
backend (for example, KDE or GTK). Wayland compositors must support xdg-foreign
window export for parented panels. Dialogs are modal and parented to the app;
portal setup or parent-export failures are shown as errors. There is no Zenity
fallback. Accessibility uses Qt's platform integration. Missing Qt or `SLINT_NO_QT` is an error for the Linux
native profile. On Windows, exclude `eml-app` and `eml-gui` from workspace commands
and use `--target` with your native MSVC triple so host
procedural macros are built independently of static-CRT target flags. Windows CI
copies its freshly built xtask executable to the runner's temporary directory
before running checks: Windows cannot replace that executable while Cargo tests
rebuild it. The native MSVC step in [CI](.github/workflows/ci.yml) uses Cargo's
reported artifact path, including configured target directories.

## Checks

Developer-tool versions live in [xtask/tools.toml](xtask/tools.toml). Install those
versions of cargo-deny, cargo-about, actionlint and shellcheck on PATH. For example,
from the repository on Unix:

```sh
cargo install --locked cargo-deny --version "$(awk -F '"' '/^cargo_deny = / {print $2}' xtask/tools.toml)"
cargo install --locked cargo-about --features cli --version "$(awk -F '"' '/^cargo_about = / {print $2}' xtask/tools.toml)"
rustup target add aarch64-apple-darwin x86_64-apple-darwin # macOS distribution
cargo xtask ci
```

Obtain actionlint and shellcheck from their upstream releases or a package manager,
using the versions in tools.toml. `cargo xtask ci` checks tool versions, formatting,
dependency policy for the headless graph, macOS native renderers alone/together, the Linux
Qt graph and the separate fuzz lockfile, workflows, shell scripts and the
working-tree verification budget;
then runs Clippy and tests. On macOS it also builds the signed universal app in a
fresh temporary directory. Structural complexity thresholds warn; other Clippy
findings fail. Inline exceptions use `#[expect(..., reason = "...")]`.

`cargo xtask ci --rust-only` runs the native Clippy/test portion used by matrix
jobs. On Windows CI it runs through an explicitly targeted MSVC xtask executable.
Windows checks run on both native CPU architectures. GUI tests use Slint's testing
backend, with no OS UI automation. CLI tests cross the real subprocess boundary. Supervisor/GUI process fixtures use
a Cargo-built launcher and per-test script data; distribution selects only the
public product binaries.

The desktop uses one batch with automatic read-only previews. See the
[desktop guide](README.md#desktop-app) and [review captures](docs/gui-screenshots/README.md).
Native Finder/Dock, file panels and appearance are manual checks.

Merge CI restores caches on every producer and saves them only on main. Release
builds read those trusted cache namespaces rather than keys with no producers.
Linux test and release builds use the same Ubuntu 26.04 baseline, so they can
reuse build outputs. Cache keys include the Qt/developer-tool pins.
`cargo xtask lint` owns source/configuration checks for both hosted lint and local
CI; `cargo xtask ci --rust-only` owns the native Clippy/test portion. Native
Clippy selects `desktop-winit` on macOS and `desktop-qt` on Linux. Linux also
runs native adapter tests against Slint's testing backend and pure portal
responses, plus the scoped Qt backend's boundary unit tests outside workspace
test discovery. They do not contact desktop services. These
platform profiles are mutually exclusive; `--all-features` is unsupported.
macOS defaults to `renderer-software`; `renderer-skia` is optional. Linux Qt
draws through Qt's painter rather than a Slint renderer selection. The full
`cargo xtask ci` composes them and checks macOS archives separately.

The budget reads tracked and non-ignored untracked working files, including
verification scripts; deleted files are omitted. To compare a committed state,
run `scripts/check-budget.sh <git-ref>`. Both modes reject an empty product set.
Shell lint uses the same working-file selection and fails if listing fails or
no shell files are found. `cargo xtask tool-versions` prints pins from the invoking
workspace, and does not rely on an executable's compiled-in configuration.

## Distribution and notices

On macOS:

```sh
cargo xtask dist --cli --app --universal --out target/dist
mkdir -p target/smoke
tar -xzf target/dist/emlattachmentremover-*-macos-universal.tar.gz -C target/smoke
ditto -x -k target/dist/emlattachmentremover-gui-*-macos-universal.zip target/smoke
scripts/smoke-cli.sh target/smoke/emlattachmentremover tests/contract/mime
```

Use a fresh output folder: distribution refuses existing archive files. Linux and
Windows use native target builds. On Linux, run
`cargo xtask dist --cli --target <native-triple> --out <fresh-folder>`. On Windows,
run `cargo run --locked -p xtask --target <native-triple> -- dist --cli --target <native-triple> --out <fresh-folder>`.
Windows triples end in `pc-windows-msvc`; `.cargo/config.toml` sets `+crt-static`.
See [release instructions](docs/release.md) for exact-archive floor smoke and the
manual checklist. Build requirements and actual release acceptance are separate:
run the floor checks before releasing.

`cargo xtask notices --target <triple> --out <new-file>` generates CLI notices.
Add `--app` and repeat `--target` for a universal app's CLI/GUI union. It requires
cargo-about, the selected Rust sysroot's copyright file and the retained upstream
legal texts. See [legal materials](legal/README.md).

## Focused investigation

Pure MIME code lives in `eml-message`. Its independent model and retained-span
verification check body retention and removals. Real contract fixtures live in
[tests/contract](tests/contract); the extracted CLI smoke uses their golden bytes.
Keep oracles independent of the parser under test.

Fuzz with the compiler and cargo-fuzz versions in tools.toml. Select the native
target explicitly because prebuilt cargo-fuzz binaries can default to their own
build target (for example, musl on a GNU Linux host). Include the retained seeds
alongside a writable corpus:

```sh
mkdir -p fuzz/corpus/preparation
cargo +<fuzz-compiler> fuzz run --target <native-triple> preparation fuzz/corpus/preparation fuzz/seeds -- -max_total_time=900
```

The nightly runs on `x86_64-unknown-linux-gnu` with AddressSanitizer and retains
crashing inputs as workflow artifacts. To investigate a MIME change with
mutation testing, install the pinned cargo-mutants and run
`cargo mutants -p eml-message`. These are focused/manual tools; nightly fuzzing
runs for 15 minutes. Avoid adding campaign, evidence or native UI qualification
frameworks. Add tests for a concrete required behavior or regression.

## Pinned Slint source maintenance

The [Qt backend patch](crates/slint-qt/SOURCE-PATCH.md) covers native path,
window, text and accessibility integration. Linux also uses the
[published Fluent widget closure](crates/slint-widgets/SOURCE-PATCH.md) with
menu accessibility properties. Its original visual and layout source is retained.
A [small build-helper extension](crates/slint-build-source/SOURCE-PATCH.md)
selects the compiler's existing broad experimental switch explicitly for this
Linux UI compilation: the copied published widgets use internal interfaces.
This also enables other experimental syntax/context constructors; application
UI code does not use them. No global environment flag is changed. macOS keeps
builtin Cupertino and native menus. Review these source patches and rerun their
native checks when changing the pinned toolkit; they are not a general-purpose
replacement toolkit distribution.
