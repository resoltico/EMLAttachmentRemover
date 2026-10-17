# Interface artwork geometry

The editable SVGs in `artwork-svg/` are the authoritative drawing inputs. They
retain the supplied coordinates, strokes, view boxes and appearance palettes.
Use them unchanged at the intended aspect ratio; do not redraw their shapes in
frontend code. `ARTWORK.md` describes visual roles, spacing and palette intent.

Regular and compact variants supply light, dark and increased-contrast forms for
identity, processing, success, stopped and attention outcomes. Native screenshots
must verify geometry, contrast, clipping and scale at the actual displayed size.
Compilation, file hashes or palette measurements do not establish that rendering.

The source artwork and images generated from it use the proprietary artwork
terms in the root LICENSE. This geometry document adds no independent reuse or
modification permission. Previously granted permissions remain valid.
