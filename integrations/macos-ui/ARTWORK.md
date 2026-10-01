# Original artwork and provenance

The application icon and custom interface graphics are defined by original geometric drawing code in `Artwork.swift`. The design uses three retained strips and a detached asymmetrical tile, with related processing, success, attention, and stopped indicators. Coordinates, shapes, spacing, and palette were authored from scratch for this project during the graphics remediation; no stock symbol, traced image, third-party icon library, or downloaded artwork is used as a source.

`CreateIcon.swift` renders the same geometry into the application icon. The application renders its custom interface images from that geometry at runtime. `ArtworkPreview.swift` produces the review sheet from the same renderer. No Apple symbol images, exported vectors, or symbol-derived app icons are included in these outputs.

The drawing code and its generated artwork are distributed under the project's MIT license; retain the copyright and permission notice in `LICENSE`. No additional third-party graphics attribution is needed for this authored asset set. This records provenance and licensing of these project assets; it is not a worldwide copyright, patent, or trademark-clearance certification.

The application still uses native macOS controls, menus, file selection, drawing APIs, and system fonts at runtime. Those operating-system components are not copied into the artwork or redistributed under this project's MIT license. Compiler/SDK and operating-system use remain subject to their respective vendor terms.

The former symbol-derived application icon and custom symbol-loading code were removed before release. Historical test methodology and result receipts are retained separately; superseded artwork and executable candidates are not release assets.
