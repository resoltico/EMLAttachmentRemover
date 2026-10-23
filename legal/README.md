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
manifest component identities. Native Windows qualification requires
selected-input/final-link checks and unknown-input rejection controls; see
[verification](../docs/verification.md). These legal files and a Cargo-only
generator do not establish that boundary. Qualification
must still be renewed for the delivered source and any changed native inputs.

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

The material review distinguishes the standalone CLI, macOS GUI executable and
private macOS worker by component/target scope. Common package facts are shared;
GUI and worker membership is checked separately rather than inferred from CLI
coverage. `xtask/app-notices.toml` selects the enabled GUI licenses. Both native
macOS target graphs also receive explicit cargo-deny policy checks.

Some published crates omit their repository's legal files. The retained inputs
under upstream/ are exact files from the package-declared VCS revision, with
per-package revision/path checks. Complete combined files retain notices such as
libm's musl and CORE-MATH attributions. Slint helper source declarations retain
actual copyright/SPDX lines; a standard MIT template is identified as a template,
never filled with an invented year or owner. dispatch0.2.0 supplies an MIT package
declaration and author metadata but no separate license file at its declared
revision; its declaration accompanies the identified standard text.

cargo-about0.9.2 omits the selected LicenseRef text groups for Slint1.18.1 despite
including those packages in its graph. The generator requires an explicit
reviewed supplement from every exact package's complete royalty-free license.
An unreviewed omission fails. Help → About Slint and Licenses… displays the actual AboutSlint widget.
This supplements the selected source materials; generator success, checksums and
license policy do not establish legal assurance or native product qualification.
