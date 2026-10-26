# Interface artwork geometry

The [SVGs](svg/) are the editable drawing authority. They are design references;
the Slint application does not load them. Preserve their view boxes and aspect
ratio. The coordinate system has y increasing downward.

`regular/` supplies weights intended for 24 pt and up; `compact/` supplies heavier
weights below 24 pt. Each contains `light`, `dark`, `hc-light` and `hc-dark`
appearances. Identity, processing, success, attention and stopped use distinct
marks; `attention-failure.svg` supplies the failure-colour attention variant.
The progress ring is static.

## Construction helpers

[source/Artwork.swift](source/Artwork.swift) draws the envelope and badge with
AppKit in a 100 × 100 square with a bottom-left origin.
[source/ArtworkPreview.swift](source/ArtworkPreview.swift) exports PNG previews
using that drawing code. These are optional reference tools, outside Cargo builds
and merge checks.

[source/generate_svgs.py](source/generate_svgs.py) expresses the geometry in SVG
coordinates and writes variants beside itself. It requires Python 3 and only the
standard library. To inspect generated output without overwriting the editable
SVGs, copy the generator into a fresh review directory:

```sh
mkdir -p target/interface-artwork-review
cp artwork/interface/source/generate_svgs.py target/interface-artwork-review/
python3 target/interface-artwork-review/generate_svgs.py
```

Compare generated variants with the editable SVGs before adopting changes. Review
rendered geometry, contrast, clipping and scale at the intended display size and
background. File hashes and palette calculations alone do not establish visual
quality or application accessibility. The Slint interface uses its own palette
and accessible text and widget roles.

The root [LICENSE](../../LICENSE) specifies the proprietary terms for these
SVGs, drawing-generation material, geometry guide and generated images.
