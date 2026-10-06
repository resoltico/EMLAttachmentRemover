# Original artwork and provenance

The custom interface graphics are original geometric artwork authored for this project. The visual language is a single fixed envelope with a same-size status badge in its lower-right corner. The envelope never changes; only the badge colour and its mark show the state:
- **Identity:** a brand-blue badge with a yellow "snip," echoing the split-paperclip app icon.
- **Processing:** a progress ring.
- **Ready:** a check.
- **Attention:** an exclamation mark.
- **Stopped:** a stop square.

The supplied account states that the drawings were constructed from coordinates rather than adapted from a third-party icon library or system symbol. The supplied editable drawings are retained in the source distribution.

## Runtime interface artwork

`Artwork.swift` is the single geometry source for interface artwork. The application draws that geometry directly in an `NSView`, so the UI stays vector-based at its actual layout size. `ArtworkPreview.swift` uses the same geometry to create explicit sRGB PNG review artifacts. Those bitmaps are QA output, not the application's runtime artwork. The macOS test suite compiles and runs the exporter and checks its image dimensions and sRGB metadata.

`artwork-svg/` holds supplied SVG reference exports of the drawing geometry, for design review and documentation:
- **Folders:** `regular/` covers 24 pt and up, `compact/` covers below 24 pt, and each has `light`, `dark`, `hc-light` and `hc-dark`.
- **Regenerating:** The supplied reference generator is preserved in `ARTWORK-GEOMETRY.md`. It is not a runtime or build input. Update reference drawings deliberately when changing production geometry.
- **Not runtime:** the application does not load these files.

### Geometry

All coordinates are in a 100 × 100 unit square with AppKit's bottom-left origin.

| Element | Geometry |
|---|---|
| Envelope | `#636366` 5.07 / — | `#AEAEB2` 5.80 / — | `#3A3A3C` 9.61 / — | `#E5E5EA` 10.21 / — |
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

**Colours.** Colours resolve from the current drawing appearance: light, dark, and their Increase Contrast variants. The view redraws when its effective appearance changes, and it also honours `accessibilityDisplayShouldIncreaseContrast`. Colours are explicit sRGB values chosen for contrast on the reference surfaces below. System colours vary by appearance and context; the supplied measurements do not establish that every use of a system colour fails contrast. Workspace display-option notifications also invalidate the drawing even when the effective appearance object stays unchanged.

Each cell gives the colour, then its contrast against the window background, then the contrast of the mark against the badge.

| Tone | Light | Dark | Increase Contrast light | Increase Contrast dark |
|---|---|---|---|---|
| Envelope | `#636366` 5.07 / — | `#AEAEB2` 5.80 / — | `#3A3A3C` 9.61 / — | `#E5E5EA` 10.21 / — |
| Accent | `#0066D6` 4.59 / 5.42 | `#409CFF` 4.53 / 6.01 | `#0050A8` 6.55 / 7.74 | `#6BB3FF` 5.81 / 9.52 |
| Success | `#1F7A35` 4.57 / 5.39 | `#30D158` 6.34 / 8.42 | `#146128` 6.42 / 7.58 | `#5BE07A` 7.56 / 12.39 |
| Warning | `#B54700` 4.60 / 5.43 | `#FF9F0A` 6.24 / 8.28 | `#8F3800` 6.50 / 7.68 | `#FFB340` 7.19 / 11.77 |
| Failure | `#C4281C` 4.85 / 5.73 | `#FF6961` 4.55 / 6.03 | `#A11A12` 6.65 / 7.86 | `#FF8A80` 5.62 / 9.20 |
| Neutral | `#636366` 5.07 / 5.99 | `#AEAEB2` 5.80 / 7.70 | `#48484A` 7.72 / 9.12 | `#D1D1D6` 8.43 / 13.80 |
| Identity | `#0B3D91` 8.50 / 6.97 | `#FFD166` 8.89 / 6.97 | `#0B3D91` 8.50 / 6.97 | `#FFD166` 8.89 / 6.97 |

How the ratios were measured:
- **Window backgrounds:** light ratios are the lower of white and `#ECECEC`. Dark ratios are the lower of `#1E1E1E` and `#323232`, plus `#000000` for Increase Contrast dark.
- **Marks:** white on light badges, `#1C1C1E` (or black with Increase Contrast) on dark badges, and the brand blue/yellow pair for Identity.
- **Scope:** solid envelope, badge and mark colours exceed 4.5:1 on these reference backgrounds. The faint progress track is decorative and is excluded; the solid arc carries the visible progress shape. Antialiasing, selected-row backgrounds and other surfaces require separate visual review. This is a palette calculation, not a certification of whole-app WCAG compliance. WCAG 1.4.11 provides a 3:1 benchmark for meaningful graphics; these images are decorative because adjacent text supplies the information.

**Not colour alone.** The five artwork kinds have distinct marks. Outcomes that share a kind, including created and existing copies or warning and failure tones, remain distinguished by adjacent text. Colour is never the only source of outcome information.

**Motion.** Nothing animates; the progress ring is static. Reduce Motion therefore needs no special handling. Any future animation must stop when `accessibilityDisplayShouldReduceMotion` is set.

**Decorative role.** The state graphics are deliberately decorative. The adjacent text owns status meaning and accessibility, and `ArtworkView` is hidden from accessibility.

### Interface artwork provenance

The envelope-and-badge set was designed on 5 October 2026 with AI assistance (Claude, Anthropic), under the project owner's art direction:
- **How it was drawn:** every shape was defined from explicit coordinates written for this project, as listed above.
- **Sources:** the supplied originality statement says no icon library, system symbol or third-party artwork was used, adapted or consulted as a drawing source. This is the supplying agent’s account, not an independently certified ownership record.
- **Generic marks:** the envelope, check, exclamation mark, stop square and ring are conventional generic forms, drawn independently. Like any such forms, they resemble many others. AI assistance and generic geometry limit any claim of exclusive copyright.


## Application icon

`EML.icon` is the editable Icon Composer source package for the macOS application icon. Its SVG assets contain only the authored foreground geometry on a 1024-pixel design canvas; the package defines the brand background and foreground layers. The source does not contain a rounded-rectangle mask. Apple tooling and macOS apply the final enclosure and appearance rendering.

`build.sh` compiles `EML.icon` with Xcode's `actool`. The resulting `Assets.car` carries the layered icon for current macOS releases, while the generated `EML.icns` is the downlevel representation used by older supported macOS releases. Both are produced from the same icon source before the application is signed. A native build fails rather than silently falling back to a separately rendered legacy icon if a compatible `actool` is unavailable.

The icon shows a paperclip cut into separated white and yellow halves on Royal blue. Dark appearance has a navy background; tinted appearance reduces the yellow layer opacity. Background values use extended-sRGB tags accepted by Apple tooling, and SVG fills specify their RGB colours. The system supplies the enclosure, materials and remaining appearance rendering.

## License

The icon document, SVG layers, interface drawing code in `Artwork.swift`, reference SVGs in `artwork-svg/`, construction scripts in `ICON-GEOMETRY.md` and `ARTWORK-GEOMETRY.md`, and generated artwork use the proprietary artwork terms in the root `LICENSE`. Other application, preview and build code remains MPL-covered. Keep the artwork unchanged when redistributing it with the application; independent reuse or modification requires permission. Previously granted permissions remain valid. This notice records licensing intent, not a certification of copyright ownership or trademark clearance.

The application still uses native macOS controls, menus, file selection, drawing APIs, and system fonts at runtime. Those operating-system components are not copied into the artwork or redistributed under this project's MPL 2.0 license. Compiler/SDK and operating-system use remain subject to their respective vendor terms.

## Split-paperclip icon provenance

The replacement icon was supplied by the project owner on 5 October 2026 with an AI-assisted construction script defining four parallel wires and three semicircular turns from explicit measurements. `ICON-GEOMETRY.md` preserves that script; the SVG files preserve the editable drawing. The supplied account says this geometry was drawn independently, replacing an earlier Feather-derived exploration. That exploration is excluded from this repository and its deliverables. File hashes establish identity, not originality or ownership; AI assistance and generic geometry limit any claim of exclusive copyright.

The production palette is Royal blue with white and yellow halves, a darker navy background for dark appearance, and reduced yellow-layer opacity in tinted appearance. The supplied JSON needed repair for the installed Apple tools: background colours use `extended-srgb`, and the unsupported untagged-SVG colour-space setting is omitted. Actual compilation, not an unofficial schema, is the integration gate.

Apple permits system-provided symbols inside applications for the corresponding Apple platforms, subject to symbol-specific restrictions, and prohibits their use or confusingly similar images in app icons, logos and other trademark uses. See sections 2.4 and 2.10 of the [Xcode and Apple SDKs Agreement](https://www.apple.com/legal/sla/docs/xcode.pdf) and [SF Symbols guidance](https://developer.apple.com/design/human-interface-guidelines/sf-symbols). This application retains custom interface artwork and does not copy or reference SF Symbols.
