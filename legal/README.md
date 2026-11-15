# Third-party notices

`cargo xtask notices --target <triple> --out <file>` generates CLI notices from
cargo-about's locked normal/build graph. Add `--app` for the software-rendered
desktop graph. Generate notices for the single target included in each archive;
ARM and Intel macOS distributions are separate.
The output includes the selected compiler's complete Rust standard-library
copyright HTML from its sysroot. `legal/about.toml` selects accepted licenses;
`deny.toml` checks workspace dependency policy, including development inputs.

Slint 1.18.1 is selected under its upstream `GPL-3.0-only` option, compatible
with the original AGPL-3.0-only project modules under
[section 13 of GPLv3](https://www.gnu.org/licenses/gpl-3.0.html#section13) and
[AGPLv3](https://www.gnu.org/licenses/agpl-3.0.html#section13).
The [pinned Slint manifest](https://github.com/slint-ui/slint/blob/372cf0ee5577c3dfec309a45e7b778ba4e81b734/api/rs/slint/Cargo.toml)
identifies its alternatives; the generator includes its selected GPL text.
The existing AboutSlint widget remains. Its unchanged badge SVGs have separate
CC-BY-ND-4.0 terms in Slint 1.18.1's
[pinned upstream metadata](https://github.com/slint-ui/slint/blob/372cf0ee5577c3dfec309a45e7b778ba4e81b734/REUSE.toml).
App notices also include `slint/graphics-notices.txt` and the complete
`slint/CC-BY-ND-4.0.txt`: these identify the badges and the Apache-2.0 Material
Icons submenu arrow embedded by the Cupertino widgets. Preserve the branding,
asset-specific attribution and upstream license text when updating inputs.

The retained `upstream/` and `objc2/LICENSE.md` files supply full AccessKit,
objc2, Lyon, rspolib and taffy legal material omitted by the registry packages or
cargo-about's selected text. App notices append these unchanged texts. Their
filenames identify retained source material; check the upstream licensing when
updating these dependencies.

Build archives with `cargo xtask dist`; each archive includes its generated
notices, the project's LICENSE, PROJECT-NOTICE.md, LICENSE-COMMERCIAL.md,
and SOURCE.txt. The separately checksummed Corresponding Source
archive is produced by `cargo xtask source`. Native toolchain and SDK licensing remains
subject to the platform's terms.
