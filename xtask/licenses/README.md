# Retained upstream license materials

These are source inputs for archive notices, not evidence that any archive is
qualified. Keep upstream legal files unchanged. Material digests establish byte
identity; they do not establish legal completeness or native link provenance.

`rust/source.json` identifies the official dated Rust distribution manifest and
the downloaded compiler component containing the complete library copyright
HTML and associated license texts. The component digest was checked against
that manifest before extracting these members. These are standard-library
materials, including nested notices and exceptions, rather than a custom
rendering of the upstream HTML. The version is a material identity; the compiler
selection remains in `rust-toolchain.toml`.

`objc2/LICENSE.md` is the unchanged upstream repository licensing explanation
shared by the four exact revisions in `objc2/source.json`. It supplements the
registry packages that omit this file. Its Apple SDK discussion is retained.
The selected MIT license text must also accompany it; a standard MIT template
must not be represented as an upstream copyright attribution.

Windows runtime materials and actual selected native-input review are still
pending. Neither these files nor a Cargo-only notice generator qualifies that
boundary.
