#!/usr/bin/env bash
set -euo pipefail

cargo deny --locked --config xtask/dependencies.toml check advisories sources licenses
cargo deny --locked --manifest-path fuzz/Cargo.toml --config xtask/dependencies.toml check advisories sources licenses
cargo deny --locked --manifest-path tests/native-filesystem/Cargo.toml --config xtask/dependencies.toml check advisories sources licenses
for target in aarch64-apple-darwin x86_64-apple-darwin; do
  cargo deny --locked --target "${target}" --features eml-gui/macos-gui --config xtask/dependencies.toml check advisories sources licenses
done
