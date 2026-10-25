# Third-party notices

`cargo xtask notices --target <triple> --out <file>` generates CLI notices from
cargo-about's locked normal/build graph. Add `--app` for the union of the CLI and
software-rendered desktop graphs; repeat `--target` for universal binaries.
The output includes the selected compiler's complete Rust standard-library
copyright HTML from its sysroot. `legal/about.toml` selects accepted licenses;
`deny.toml` checks workspace dependency policy, including development inputs.

Slint's royalty-free LicenseRef text is omitted by cargo-about. The generator
requires and appends the complete upstream text in
`slint/LicenseRef-Slint-Royalty-free-2.0.md` for app notices. The desktop About
screen displays AboutSlint. Preserve upstream license text when updating inputs.

The retained `upstream/` and `objc2/LICENSE.md` files supply full AccessKit,
objc2, Lyon, rspolib and taffy legal material omitted by the registry packages or
cargo-about's selected text. App notices append these unchanged texts. Their
filenames identify retained source material; check the upstream licensing when
updating these dependencies.

Build archives with `cargo xtask dist`; each archive includes its generated
notices and the project's LICENSE. Native toolchain and SDK licensing remains
subject to the platform's terms.
