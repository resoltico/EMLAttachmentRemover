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
require Ubuntu 26.04 or newer. Headless GUI development needs `pkg-config`,
`libfontconfig1-dev` and `libwayland-dev`.
Windows CLI development uses Visual Studio C++ Build Tools with the native x64
or ARM64 MSVC tools. CI builds a static CRT.

On macOS and Linux:

```sh
cargo build --locked -p emlattachmentremover
cargo test --locked --workspace
```

On Windows, select your native MSVC target explicitly and exclude the unsupported
GUI package from workspace commands:

```powershell
$target = 'x86_64-pc-windows-msvc' # use aarch64-pc-windows-msvc on ARM64
cargo build --locked -p emlattachmentremover --target $target
cargo test --locked --workspace --exclude eml-gui --target $target
```

Install the tools listed under Checks first. For a native GUI on macOS, build
and open the actual app bundle for native identity and resources:

```sh
cargo xtask dist --app --out target/desktop
mkdir -p target/desktop/app
ditto -x -k target/desktop/emlattachmentremover-gui-*-macos-*.zip target/desktop/app
open "target/desktop/app/EML Attachment Remover.app"
```

The default is the Rust host architecture. To build for the other Mac CPU, add
`--target aarch64-apple-darwin` or `--target x86_64-apple-darwin` and install that
Rust target first. Each command creates a single-architecture app archive.
Use a fresh output directory on subsequent distributions. The app calls core in
process and includes no CLI. Its bundle supplies native identity and resources.
The GUI library and testing backend support macOS and Linux. Windows supports
core and the CLI only. The native GUI uses Slint's Winit backend on macOS only.
On Windows, exclude `eml-gui` from workspace commands and use `--target` with your native MSVC triple so host procedural macros
are built independently of static-CRT target flags. Windows CI copies its freshly
built xtask executable to the runner's temporary directory
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
```

Installer source checks also need Python 3 (standard library only) and PowerShell 7
(`pwsh`) on PATH. Install PSScriptAnalyzer at the version declared in tools.toml;
from PowerShell in the repository:

```powershell
$toolLines = cargo xtask tool-versions
if ($LASTEXITCODE -ne 0) { throw 'Could not read the declared tool versions.' }
$toolPins = ($toolLines -join "`n") | ConvertFrom-StringData
Install-Module PSScriptAnalyzer -RequiredVersion $toolPins.psscriptanalyzer -Scope CurrentUser -Force
```

Obtain actionlint and shellcheck from their upstream releases or a package manager,
using the versions in tools.toml. On macOS the full gate builds both app targets,
so install both Rust targets:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

On macOS and Linux, once the prerequisites are installed, run:

```sh
cargo xtask ci
```

`cargo xtask ci` checks tool versions, formatting,
dependency policy for the headless graph, macOS native renderers alone/together, the separate fuzz lockfile, workflows, shell scripts and the
working-tree verification budget, installer body parity and installer source regressions;
then runs Clippy and tests. On macOS it also builds separate signed Apple Silicon
and Intel apps in a fresh temporary directory. Structural complexity thresholds
warn; other Clippy findings fail. Inline exceptions use `#[expect(..., reason = "...")]`.

`cargo xtask ci --rust-only` runs the native Clippy/test portion used by matrix
jobs. On Windows CI it runs through an explicitly targeted MSVC xtask executable.
Windows checks run on both native CPU architectures. GUI tests use Slint's testing
backend, with no OS UI automation. CLI tests cross the real subprocess boundary. GUI tests use real core and held typed-event callbacks through one runner seam;
receipt projection units retain exhaustive wording/capability coverage. The native
document probe uses a separate test binary that is never packaged or released.

The desktop uses one batch with automatic read-only previews. See the
[desktop guide](README.md#desktop-app) and [review captures](docs/gui-screenshots/README.md).
Native Finder/Dock, file panels and appearance are manual checks.

Merge CI restores caches on every producer and saves them only on main. Release
builds read those trusted cache namespaces rather than keys with no producers.
Linux test and release builds use the same Ubuntu 26.04 baseline, so they can
reuse build outputs. Cache keys include the developer-tool pins.
`cargo xtask lint` owns source/configuration checks for both hosted lint and local
CI; `cargo xtask ci --rust-only` owns the native Clippy/test portion. Native
Clippy selects `desktop` on macOS and headless elsewhere.
macOS defaults to `renderer-software`; `renderer-skia` is optional. The full
`cargo xtask ci` composes them and checks macOS archives separately.

The budget reads tracked and non-ignored untracked working files, including
verification scripts; deleted files are omitted. To compare a committed state,
run `scripts/check-budget.sh <git-ref>`. Both modes reject an empty product set.
Shell lint uses the same working-file selection and fails if listing fails or
no shell files are found. `cargo xtask tool-versions` prints pins from the invoking
workspace, and does not rely on an executable's compiled-in configuration.

## Distribution and notices

On macOS, build separate archives for both CPUs:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
for mac_arch in aarch64 x86_64; do
  cargo xtask dist --cli --app --target "$mac_arch-apple-darwin" --out target/dist
done
```

Smoke the archive matching the current Mac. The example selects Apple Silicon;
on Intel set both `mac_arch` and `mac_macho_arch` to `x86_64`. The checks reject a
wrong-CPU or multi-architecture CLI and a Rosetta-translated shell before execution:

```sh
set -eu
mac_arch=aarch64
mac_macho_arch=arm64
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
Add `--app` for the GUI graph and select the same target as the app archive.
It requires cargo-about, the selected Rust sysroot's copyright file and the retained upstream
legal texts. See [legal materials](legal/README.md).

## CLI installer verification

The user contract is [install, update and uninstall](docs/INSTALL.md). The installers
manage user-local CLI files and their own PATH entries, not the desktop app or
email data. Keep the Unix installer POSIX `sh` and the Windows installer compatible
with Windows PowerShell 5.1 and PowerShell 7. The PowerShell script stays ASCII.

The common engine bodies are owned by PDFConcat. Only literal product settings
may differ inside `BEGIN PRODUCT SETTINGS` / `END PRODUCT SETTINGS`. Changes to
behavior must be synchronized in both repositories, with EML's reviewed upstream
pin updated. The published scripts are self-contained; neither installer fetches
code or configuration from the other repository at runtime. `install/engine-source.txt`
contains the full reviewed PDF commit. Check body parity locally against that Git
object, or fetch its files over HTTPS for development/CI:

```sh
python3 install/check-sync.py /absolute/path/to/PDFConcat
python3 install/check-sync.py
```

Preserve EML's literal product block, copy both canonical engine bodies and update
the pin together. Commit the PDF engine first; a dirty working tree is not what
the offline check reads.

Use only isolated HOME/config/install paths for tests. Native Windows registry
PATH tests run only in CI and restore the original raw value, kind and existence.
The real archive checks must cover each of the six shipped CLI targets on its
native host, including PowerShell 5.1 and 7 when both exist. They exercise install,
update, uninstall, ownership refusal, changed data, lock refusal, PATH handling,
checksum/version rejection and regular-file extraction. Preserve original files
and arbitrary pre-existing directory contents. No test may adopt or remove an
unowned program just because it prints the expected name/version.

A local EML mirror contains versioned archives, so supply a stable version along
with `EMLATTACHMENTREMOVER_DOWNLOAD_BASE`. Use fresh output directories and the
actual generated `SHA256SUMS`. See [release checks](docs/release.md) for archive
production and native verification. During development, select the version in
the actual locally built archives. Installer testing does not publish releases
or modify published assets.

## Focused investigation

Pure MIME code lives in `eml-message`. Its independent model and retained-span
verification check body retention and removals. Real contract fixtures live in
[tests/contract](tests/contract); the extracted CLI smoke uses their golden bytes.
Keep oracles independent of the parser under test.

Full fuzz and mutation campaigns run only in the scheduled [Weekly workflow](.github/workflows/weekly.yml),
once a week. Manual dispatch is reserved for the owner. During development run
micro campaigns only: fuzz at most 120 seconds per run and 10 minutes total;
mutation at most 60 mutants per run, selected with `--in-diff`, `--file` or `-F`.
Never dispatch or re-run Weekly to verify a PR.

Use the compiler and cargo-fuzz versions in `xtask/tools.toml`. Select the native
target explicitly (prebuilt cargo-fuzz may otherwise default to musl on GNU Linux).
Use the corpora read-only after the writable directory; do not copy the seeds:

```sh
mkdir -p fuzz/corpus/preparation
cargo +<fuzz-compiler> fuzz run --target <native-triple> preparation fuzz/corpus/preparation fuzz/seeds tests/contract/mime/inputs tests/contract/mime/golden -- -dict=fuzz/mime.dict -max_len=65536 -max_total_time=60
```

The weekly ASan campaign runs for two hours on Ubuntu 26.04 using the runner's CPUs.
Crashes become contract inputs with expected outcomes, serving as regression tests
and seeds. Findings are uploaded as artifacts; the grown corpus is not persisted.
`cargo xtask lint` compiles and lints the standalone fuzz crate with the product
toolchain, without exploration.

Mutation scope, product test packages, timeout multiplier and justified equivalent
exclusions live in [.cargo/mutants.toml](.cargo/mutants.toml). All product tests run
for the baseline and each mutant; xtask is excluded because its archive test needs
a git checkout. Use the pinned cargo-mutants and list the selection first:

```sh
git diff main -- crates > /absolute/path/product.diff
cargo mutants --in-diff /absolute/path/product.diff --list
# If the list exceeds 60, narrow it with --file or -F before running.
cargo mutants --in-diff /absolute/path/product.diff
```

Weekly mutation uses 16 round-robin shards, each with runner CPUs minus one workers
(at least one). A baseline failure, missed mutant or timeout fails the shard;
unviable mutants do not. Every shard uploads `mutants.out` for review. Campaign
cost and surviving mutants must be assessed from the actual scheduled results.
Keep oracles independent and add tests for concrete behavior rather than campaign
or evidence frameworks.
