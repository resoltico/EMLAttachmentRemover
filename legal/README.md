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

`windows-runtime/` retains the six complete upstream runtime legal materials and
source-comment notices. Its source record gives the four official distribution
manifest component identities; those component archives have not yet been
downloaded/verified for EML's final link. Actual selected native-input and final
link review remain pending. These legal inputs and a Cargo-only generator do not
qualify that boundary.

`crate-materials.json` retains the independent review's per-package legal-file
and selected-text observations. It distinguishes generator text from actual
copyright-bearing package material, including the lowercase Microsoft license
files missed by cargo-about. Its input hashes and observed head describe that
review's basis; the record explicitly supplies no generation execution
attestation. The generator must separately bind its actual target, configuration
and feature graph, and compare current locked identities and exact known legal
bytes with these observations. A changed package, binding or legal file requires
renewed review; grouping equal text cannot supply another package's provenance.
The generator must not manufacture a new review record from its own output.
