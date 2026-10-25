# Original artwork and provenance

The retained interface drawings are original geometric artwork authored for this project. The visual language is a single fixed envelope with a same-size status badge in its lower-right corner. The envelope never changes; only the badge colour and its mark show the state:
- **Identity:** a brand-blue badge with a yellow "snip," echoing the split-paperclip app icon.
- **Processing:** a progress ring.
- **Ready:** a check.
- **Attention:** an exclamation mark.
- **Stopped:** a stop square.

The supplied account states that the drawings were constructed from coordinates rather than adapted from a third-party icon library or system symbol. The supplied editable drawings are retained in the source distribution.

## Interface design references

The editable SVG drawings in `artwork-svg/` are retained design references. The current desktop interface uses standard Slint widgets and structured text; it does not embed this envelope-and-badge set.

`artwork-svg/` holds supplied SVG reference exports of the drawing geometry, for design review and documentation:
- **Folders:** `regular/` covers 24 pt and up, `compact/` covers below 24 pt, and each has `light`, `dark`, `hc-light` and `hc-dark`.
- **Geometry:** `ARTWORK-GEOMETRY.md` describes the source role and required native visual checks. Update drawings deliberately when changing production geometry.
- **Use:** retain the supplied geometry and palettes when working with these reference drawings.

### Geometry

The [SVG inputs](artwork-svg/) are authoritative for coordinates, transforms,
stroke widths and fills. Their coordinate system has y increasing downward.
Preserve their view boxes and aspect ratio. Regular, compact and Increase
Contrast variants carry their own drawing weights; do not duplicate those
values in frontend code or derive one variant by scaling another's strokes.

### Accessibility

**Colours.** The reference drawings contain explicit sRGB light, dark and Increase Contrast palettes. The following measurements describe those drawings on the stated reference surfaces. The current interface follows Slint's palette.


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

**Not colour alone.** The five reference artwork kinds have distinct marks. The current desktop interface presents outcome and status text from the shared core vocabulary.

**Motion and accessibility.** The reference progress ring is static. Slint owns interactive widget animation and accessibility behavior; meaningful interface elements have explicit roles and labels.

### Interface artwork provenance

The envelope-and-badge set is custom artwork with AI assistance (Claude, Anthropic), under the project owner's art direction:
- **How it was drawn:** every shape was defined from explicit coordinates written for this project, as listed above.
- **Sources:** the supplied originality statement says no icon library, system symbol or third-party artwork was used, adapted or consulted as a drawing source. This is the supplying agent’s account, not an independently certified ownership record.
- **Generic marks:** the envelope, check, exclamation mark, stop square and ring are conventional generic forms, drawn independently. Like any such forms, they resemble many others. AI assistance and generic geometry limit any claim of exclusive copyright.


## Application icon

`EML.icon` is the editable Icon Composer source package for the macOS application icon. Its SVG assets contain only the authored foreground geometry on a 1024-pixel design canvas; the package defines the brand background and foreground layers. The source does not contain a rounded-rectangle mask. Apple tooling and macOS apply the final enclosure and appearance rendering.

The build copies the committed `EML.icns`; it runs no `actool`, `iconutil` or Xcode command. The file contains all ten 16, 32, 128, 256 and 512 pixel icon slots at 1x and 2x. It is a flat default-appearance export. The editable source also retains dark and tinted appearance definitions; macOS may frame the flat icon on newer systems.

### Re-export after an approved design change

Export once with full Xcode, then commit the resulting `.icns`. The compatibility deployment setting below produces a complete legacy icon set; it does not set the application's deployment floor.

```sh
mkdir -p target/icon-export
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun actool artwork/EML.icon \
  --compile target/icon-export --output-format human-readable-text \
  --app-icon EML --include-all-app-icons --target-device mac \
  --minimum-deployment-target 10.12 --platform macosx \
  --output-partial-info-plist target/icon-export/icon-partial.plist
iconutil -c iconset target/icon-export/EML.icns -o target/icon-export/EML.iconset
```

Inspect the ten PNGs, including the 1024×1024 `icon_512x512@2x.png`, then copy `target/icon-export/EML.icns` to `artwork/EML.icns`. The current export used Apple's actool 27.1 from the unchanged source. `EML.icon` and its SVG layers remain the editable design authority.

## License

The icon document, SVG layers, reference SVGs in `artwork-svg/`, retained construction material in `ICON-GEOMETRY.md` and `ARTWORK-GEOMETRY.md`, and generated artwork use the proprietary artwork terms in the root `LICENSE`. Other application, preview and build code remains MPL-covered. Keep the artwork unchanged when redistributing it with the application; independent reuse or modification requires permission. Previously granted permissions remain valid. This notice records licensing intent, not a certification of copyright ownership or trademark clearance.

The application uses Slint widgets, native system dialogs, windowing and fonts at runtime. Those operating-system components are not copied into the artwork or redistributed under this project's MPL 2.0 license. Compiler/SDK and operating-system use remain subject to their respective vendor terms.

## Split-paperclip icon provenance

The owner-supplied icon uses AI-assisted coordinate construction defining four parallel wires and three semicircular turns. `ICON-GEOMETRY.md` documents the geometry; the SVG files are the editable drawing. The supplied account states that this geometry is independently drawn. File hashes establish identity, not originality or ownership; AI assistance and generic geometry limit any claim of exclusive copyright.

The source palette is Royal blue with white and yellow halves, a darker navy background for dark appearance, and reduced yellow-layer opacity in tinted appearance. Background colours use `extended-srgb`; SVG assets use their explicit fills without an untagged colour-space setting. Actual compilation, not an unofficial schema, is the integration gate.

Apple permits system-provided symbols inside applications for the corresponding Apple platforms, subject to symbol-specific restrictions, and prohibits their use or confusingly similar images in app icons, logos and other trademark uses. See sections 2.4 and 2.10 of the [Xcode and Apple SDKs Agreement](https://www.apple.com/legal/sla/docs/xcode.pdf) and [SF Symbols guidance](https://developer.apple.com/design/human-interface-guidelines/sf-symbols). This application retains custom interface artwork and does not copy or reference SF Symbols.
