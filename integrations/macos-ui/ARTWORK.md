# Original artwork and provenance

The application icon and custom interface graphics are original geometric artwork authored for this project. The visual language is a retained-message stack plus a detached state area: identity uses an asymmetrical coral attachment tile; processing uses a progressive detached sequence; success, attention, and stopped reuse the same retained strips with a compact check, exclamation mark, or stop-square. No stock symbol, traced image, third-party icon library, downloaded artwork, or Apple symbol image is used as a source.

## Runtime interface artwork

`Artwork.swift` is the single geometry source for interface artwork. The application draws that geometry directly in an `NSView`, so normal UI rendering remains vector-based at the actual layout size and resolves dynamic AppKit colors in the current appearance. `ArtworkPreview.swift` uses the same geometry to create explicit sRGB PNG review artifacts; those bitmaps are QA output and are not the application's runtime artwork.

The review exporter uses the same semantic accent and status colors as the application. Its simulated icon is a geometry study, not a rendering of the compiled Icon Composer document. Review compiled app icons in Icon Composer and the supported operating systems; the poster cannot establish their materials, mask, appearance variants, or older-system compatibility. The macOS test suite compiles and runs the exporter and checks its image dimensions and sRGB metadata.

The state graphics are deliberately decorative: the adjacent text owns status meaning and accessibility. The graphics therefore reinforce rather than replace the textual processing state.

## Application icon

`EML.icon` is the editable Icon Composer source package for the macOS application icon. Its SVG assets contain only the authored foreground geometry on a 1024-pixel design canvas; the package defines the brand background and foreground layers. The source does not contain a rounded-rectangle mask. Apple tooling and macOS apply the final enclosure and appearance rendering.

`build.sh` compiles `EML.icon` with Xcode's `actool`. The resulting `Assets.car` carries the layered icon for current macOS releases, while the generated `EML.icns` is the downlevel representation used by older supported macOS releases. Both are produced from the same icon source before the application is signed. A native build fails rather than silently falling back to a separately rendered legacy icon if a compatible `actool` is unavailable.

The icon's retained strips and detached attachment remain deliberately simple so the mark survives small Dock, Finder, and window representations. Color is defined in sRGB. Appearance-specific rendering that does not need a bespoke brand override is left to Icon Composer/macOS rather than duplicated as hand-maintained raster variants.

## License

The drawing code, SVG geometry, Icon Composer document, and generated artwork are distributed under the project's MIT license; retain the copyright and permission notice in `LICENSE`. No additional third-party graphics attribution is needed for this authored asset set. This records provenance and licensing of these project assets; it is not a worldwide copyright, patent, or trademark-clearance certification.

The application still uses native macOS controls, menus, file selection, drawing APIs, and system fonts at runtime. Those operating-system components are not copied into the artwork or redistributed under this project's MIT license. Compiler/SDK and operating-system use remain subject to their respective vendor terms.
