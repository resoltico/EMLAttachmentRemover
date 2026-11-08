# Slint build configuration source maintenance patch

Published `slint-build` 1.18.1, under the original licenses in `LICENSES/`.
Source: <https://crates.io/crates/slint-build/1.18.1>.
Published `.crate` SHA-256: `3ea28d4e1ad47d829b91e227f17037fac07a8001668a93fee47ac04f88718ebc`.

The published normalized dependency manifest is retained with publication disabled
and its header referring to the omitted `Cargo.toml.orig` removed. Cargo
publication markers, the original workspace manifest and package lockfile are
omitted. The only Rust change adds `CompilerConfiguration::with_experimental_features(bool)`,
a typed setter for the existing compiler configuration field. It changes no
compiler implementation or default. The application calls it only for Linux's
published widget source bundle; macOS retains builtin Cupertino and native menus.

The flag is broad: it permits the compiler's experimental interfaces, internal
widget APIs, closures, named slots and match syntax and emits additional context
constructors. The application uses it to compile Slint's own published Fluent
widgets, which already use internal interfaces and properties. It does not add
those experimental constructs to application UI code or make them product APIs.
The setter avoids global environment changes, unsafe environment mutation and
compiler-process wrappers. Remove this package patch when the maintained upstream
builder exposes the required configuration.
