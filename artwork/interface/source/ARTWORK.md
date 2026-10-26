# Original artwork and provenance

The custom interface graphics are original geometric artwork authored for this project. The visual language is a single fixed envelope with a same-size status badge in its lower-right corner. The envelope never changes; only the badge colour and its mark show the state:
- **Identity:** a brand-blue badge with a yellow "snip," echoing the split-paperclip app icon.
- **Processing:** a progress ring.
- **Ready:** a check.
- **Attention:** an exclamation mark.
- **Stopped:** a stop square.

No stock symbol, traced image, third-party icon library, downloaded artwork or Apple symbol image is used as a source.

## Runtime interface artwork

`Artwork.swift` is the single geometry source for interface artwork. The application draws that geometry directly in an `NSView`, so the UI stays vector-based at its actual layout size. `ArtworkPreview.swift` uses the same geometry to create explicit sRGB PNG review artifacts. Those bitmaps are QA output, not the application's runtime artwork. The macOS test suite compiles and runs the exporter and checks its image dimensions and sRGB metadata.

`artwork-svg/` holds SVG reference exports of the same geometry, for design review and documentation:
- **Folders:** `regular/` covers 24 pt and up, `compact/` covers below 24 pt, and each has `light`, `dark`, `hc-light` and `hc-dark`.
- **Regenerating:** `generate_svgs.py` mirrors the Swift geometry. Run it again whenever `Artwork.swift` changes.
- **Not runtime:** the application does not load these files.

### Geometry

All coordinates are in a 100 × 100 unit square with AppKit's bottom-left origin.

| Element | Geometry |
|---|---|
| Envelope | Rounded rectangle at x 8, y 34, 62 × 46, corner radius 9; flap polyline (15, 73) → (39, 54) → (63, 73) |
| Envelope cut-out | Circle of radius 25 at (72, 30), leaving a constant 7-unit gap around the badge |
| Badge | Solid circle of radius 18 at (72, 30), the same footprint in every state. The processing ring's outer edge matches it. |
| Identity mark | Two collinear diagonal segments: (63.5, 21.5) → (68.5, 26.5) and (75.5, 33.5) → (80.5, 38.5) |
| Check | (63, 30) → (69, 24) → (81, 37) |
| Exclamation | Bar centred on x 72 from y 28 to 42; dot of radius 3 at (72, 22) |
| Stop | Rounded square at (65, 23), 14 × 14, radius 3 |
| Progress ring | Faint full track, plus an arc from the top running clockwise through 229° |

### Stroke weights

| Condition | Envelope | Marks | Ring | Exclamation (width / dot) |
|---|---|---|---|---|
| Regular, 24 pt and up | 8 | 5.5 | 7 | 5 / 3 |
| Compact, below 24 pt | 9 | 6.5 | 7 | 6.5 / 3.6 |
| Increase Contrast | +1.5 | +1 | +0.5 | — |

The compact weights keep strokes about one device pixel wide or more at 18 pt on 1x displays.

### Accessibility

**Colours.** Colours resolve from the drawing appearance: light, dark, and their Increase Contrast variants. The view redraws when its effective appearance changes, and it also honours `accessibilityDisplayShouldIncreaseContrast`. Colours are explicit sRGB values chosen for contrast, not the stock system colours: system green and orange measure only about 2.2:1 on a white window.

Each cell gives the colour, then its contrast against the window background, then the contrast of the mark against the badge.

| Tone | Light | Dark | Increase Contrast light | Increase Contrast dark |
|---|---|---|---|---|
| Envelope | `#636366` 5.1 / — | `#AEAEB2` 5.8 / — | `#3A3A3C` 9.6 / — | `#E5E5EA` 10.2 / — |
| Accent | `#0066D6` 4.6 / 5.4 | `#409CFF` 4.5 / 6.0 | `#0050A8` 6.6 / 7.7 | `#6BB3FF` 5.8 / 9.5 |
| Success | `#1F7A35` 4.6 / 5.4 | `#30D158` 6.3 / 8.4 | `#146128` 6.4 / 7.6 | `#5BE07A` 7.6 / 12.4 |
| Warning | `#B54700` 4.6 / 5.4 | `#FF9F0A` 6.2 / 8.3 | `#8F3800` 6.5 / 7.7 | `#FFB340` 7.2 / 11.8 |
| Failure | `#C4281C` 4.9 / 5.7 | `#FF6961` 4.6 / 6.0 | `#A11A12` 6.7 / 7.9 | `#FF8A80` 5.6 / 9.2 |
| Neutral | `#636366` 5.1 / 6.0 | `#AEAEB2` 5.8 / 7.7 | `#48484A` 7.7 / 9.1 | `#D1D1D6` 8.4 / 13.8 |
| Identity | `#0B3D91` 8.5 / 7.0 | `#FFD166` 8.9 / 7.0 | `#0B3D91` 8.5 / 7.0 | `#FFD166` 8.9 / 7.0 |

How the ratios were measured:
- **Window backgrounds:** light ratios are the lower of white and `#ECECEC`. Dark ratios are the lower of `#1E1E1E` and `#323232`, plus `#000000` for Increase Contrast dark.
- **Marks:** white on light badges, `#1C1C1E` (or black with Increase Contrast) on dark badges, and the brand blue/yellow pair for Identity.
- **Requirement:** everything meets WCAG 2.2 non-text contrast (1.4.11, 3:1), and nearly all of it reaches 4.5:1.

**Not colour alone.** Every state has a distinct mark shape, so colour is never the only difference.

**Motion.** Nothing animates; the progress ring is static. Reduce Motion therefore needs no special handling. Any future animation must stop when `accessibilityDisplayShouldReduceMotion` is set.

**Decorative role.** The state graphics are deliberately decorative. The adjacent text owns status meaning and accessibility, and `ArtworkView` is hidden from accessibility.

### Interface artwork provenance

The envelope-and-badge set was designed on 5 October 2026 with AI assistance (Claude, Anthropic), under the project owner's art direction:
- **Selection:** the owner chose it from eight original proposals.
- **How it was drawn:** every shape was defined from explicit coordinates written for this project, as listed above.
- **Sources:** no icon library, system symbol or third-party artwork was used, adapted or consulted as a drawing source.
- **Generic marks:** the envelope, check, exclamation mark, stop square and ring are conventional generic forms, drawn independently. Like any such forms, they resemble many others. AI assistance and generic geometry limit any claim of exclusive copyright.

It replaces the earlier retained-message-strip artwork with its coral attachment tile.

## Application icon

`EML.icon` is the editable Icon Composer source package for the macOS application icon. Its SVG assets contain only the authored foreground geometry on a 1024-pixel design canvas; the package defines the brand background and foreground layers. The source does not contain a rounded-rectangle mask. Apple tooling and macOS apply the final enclosure and appearance rendering.

`build.sh` compiles `EML.icon` with Xcode's `actool`. The resulting `Assets.car` carries the layered icon for current macOS releases, while the generated `EML.icns` is the downlevel representation used by older supported macOS releases. Both are produced from the same icon source before the application is signed. A native build fails rather than silently falling back to a separately rendered legacy icon if a compatible `actool` is unavailable.

The icon shows a paperclip cut into separated white and yellow halves on Royal blue. Dark appearance has a navy background; tinted appearance reduces the yellow layer opacity. Background values use extended-sRGB tags accepted by Apple tooling, and SVG fills specify their RGB colours. The system supplies the enclosure, materials and remaining appearance rendering.

## License

The icon document, SVG layers, interface drawing code in `Artwork.swift`, construction script in `ICON-GEOMETRY.md`, and generated artwork use the proprietary artwork terms in the root `LICENSE`. Other application, preview and build code remains MPL-covered. Keep the artwork unchanged when redistributing it with the application; independent reuse or modification requires permission. Previously granted permissions remain valid. This notice records licensing intent, not a certification of copyright ownership or trademark clearance.

The application still uses native macOS controls, menus, file selection, drawing APIs, and system fonts at runtime. Those operating-system components are not copied into the artwork or redistributed under this project's MPL 2.0 license. Compiler/SDK and operating-system use remain subject to their respective vendor terms.

## Split-paperclip icon provenance

The replacement icon was supplied by the project owner on 5 October 2026 with an AI-assisted construction script defining four parallel wires and three semicircular turns from explicit measurements. `ICON-GEOMETRY.md` preserves that script; the SVG files preserve the editable drawing. The supplied account says this geometry was drawn independently, replacing an earlier Feather-derived exploration. That exploration is excluded from this repository and its deliverables. File hashes establish identity, not originality or ownership; AI assistance and generic geometry limit any claim of exclusive copyright.

The production palette is Royal blue with white and yellow halves, a darker navy background for dark appearance, and reduced yellow-layer opacity in tinted appearance. The supplied JSON needed repair for the installed Apple tools: background colours use `extended-srgb`, and the unsupported untagged-SVG colour-space setting is omitted. Actual compilation, not an unofficial schema, is the integration gate.

Apple permits system-provided symbols inside applications for the corresponding Apple platforms, subject to symbol-specific restrictions, and prohibits their use or confusingly similar images in app icons, logos and other trademark uses. See sections 2.4 and 2.10 of the [Xcode and Apple SDKs Agreement](https://www.apple.com/legal/sla/docs/xcode.pdf) and [SF Symbols guidance](https://developer.apple.com/design/human-interface-guidelines/sf-symbols). This application retains custom interface artwork and does not copy or reference SF Symbols.
