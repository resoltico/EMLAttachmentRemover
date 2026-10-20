# Typography dependency patches

The vendored Slint core and Parley retain their published licenses and source headers.
Their Cargo manifests and lockfile entries identify the selected versions. Upstream
inputs are Slint core 1.18.1, archive SHA-256
`ed2ea15059d70bc31b1a8b1eb4c34f2c5df04cb7e53f36179b43c0261231d646`,
and Parley 0.11.1, archive SHA-256
`22d2ff88bd3f7d68d1d9b09c7e6209f9a8e8c05088295140a2bcf2e9b17038c5`.
These identities describe the upstream basis, not the modified source or its QA.

On macOS, Slint core supplies `opsz` from the logical font size for ordinary text,
inline code, line-height calculations, direct character metrics and font metrics.
Font normalization clamps to each font's supported optical range and ignores an
absent axis. Character advances use shaping so native tracking is included.
The affected source files are `i-slint-core/textlayout/sharedparley.rs` and
`i-slint-core/textlayout/sharedparley/shaping.rs`.

Parley's `src/shape/mod.rs` derives HarfRust's typographic point size from the
resolved pixel size divided by the current layout scale on macOS. This keeps
tracking independent of display density and optical size. Invalid scales retain
upstream handling; this patch does not make invalid layouts meaningful. Other
platforms keep their existing variation and point-size behavior.

Parley's `src/bidi.rs` maps maintained named Unicode `BidiClass` values to its private bit masks, avoiding a dependency on ICU4X's deprecated ICU4C numeric conversion. Unsupported future classes fail explicitly. Direction and embedding-level regressions cover mixed LTR/RTL text and numbers.

The software renderer already consumes the shaped normalized coordinates in its
variation-aware glyph cache and rasterizer. No separate drawing adjustment or
font-size multiplier is applied. Typographic point size is derived from scale and
remains independent of the optical-axis value.

`crates/eml-gui/tests/native_typography.rs` compares the actual software layout and
rasterizer with the independent CoreText oracle in
`tests/macos/NativeFontObservation.swift`. Run it with the normal native macOS GUI
feature under the selected Xcode; results retain metrics and rendered samples
under `target/native-font-checks/`. Point-size and density controls do not imply
pixel-identical antialiasing across rasterizers or qualification on another OS.
