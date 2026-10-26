# Contributing

Read [AGENTS.md](AGENTS.md), [AGENTS_EXTRA.md](AGENTS_EXTRA.md) and
[project rules](docs/project-rules.md). Source behavior, public contracts and tests
must agree. Keep product code separate from transport and platform presentation.

## Build from a clean checkout

Install Rust with rustup and a native C/C++ linker. The compiler and components
come from [rust-toolchain.toml](rust-toolchain.toml); rustup selects them when Cargo
runs here. On macOS, install Command Line Tools (`xcode-select --install`); full
Xcode is unnecessary for builds. Ubuntu desktop development needs `pkg-config`
and `libfontconfig1-dev`. Windows CLI development uses Visual Studio C++ Build
Tools with the native x64 or ARM64 MSVC tools. CI builds a static CRT.

```sh
cargo build --locked -p emlattachmentremover
cargo test --locked --workspace
cargo build --locked -p eml-gui --features desktop --bin emlattachmentremover-gui
```

The desktop compile command applies to macOS and Linux. To run the desktop app
on macOS, build and extract the bundle using the distribution commands below,
then open `target/smoke/EML Attachment Remover.app`. The executable resolves its
CLI helper and notices relative to the bundle; `cargo run` alone does not supply
that layout. Distribution packages desktop apps only for macOS.

On Windows, exclude `eml-app` and `eml-gui` from workspace commands and use `--target` with your native MSVC triple so host
procedural macros are built independently of static-CRT target flags.

## Checks

Developer-tool versions live in [xtask/tools.toml](xtask/tools.toml). Install those
versions of cargo-deny, cargo-about, actionlint and shellcheck on PATH. For example,
from the repository on Unix:

```sh
cargo install --locked cargo-deny --version "$(awk -F '"' '/^cargo_deny = / {print $2}' xtask/tools.toml)"
cargo install --locked cargo-about --version "$(awk -F '"' '/^cargo_about = / {print $2}' xtask/tools.toml)"
rustup target add aarch64-apple-darwin x86_64-apple-darwin # macOS distribution
cargo xtask ci
```

Obtain actionlint and shellcheck from their upstream releases or a package manager,
using the versions in tools.toml. `cargo xtask ci` checks tool versions, formatting,
dependency policy, workflows, shell scripts and the committed verification budget;
then runs Clippy and tests. On macOS it also builds the signed universal app in a
fresh temporary directory. Structural complexity thresholds warn; other Clippy
findings fail. Inline exceptions use `#[expect(..., reason = "...")]`.

`cargo xtask ci --rust-only` runs the native Clippy/test portion used by matrix
jobs. On Windows CI it runs through an explicitly targeted MSVC xtask executable.
Windows checks run on both native CPU architectures. GUI tests use Slint's testing
backend, with no OS UI automation. CLI tests cross the real subprocess boundary. Supervisor/GUI process fixtures use
a Cargo-built launcher and per-test script data; distribution selects only the
public product binaries.

Merge CI restores caches on every producer and saves them only on main. The
[CI workflow](.github/workflows/ci.yml) and `cargo xtask ci` own the commands;
[project rules](docs/project-rules.md) define the verification budget. The budget
script reads `HEAD` by default, or the Git ref supplied as its argument. Staging
files does not change its inputs; use a commit containing the intended inputs
when checking the delivered state.

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

Fuzz with the compiler and cargo-fuzz versions in tools.toml, using
`cargo +<fuzz-compiler> fuzz run preparation`. To investigate a MIME change with
mutation testing, install the pinned cargo-mutants and run
`cargo mutants -p eml-message`. These are focused/manual tools; nightly fuzzing
runs for 15 minutes. Avoid adding campaign, evidence or native UI qualification
frameworks. Add tests for a concrete required behavior or regression.
