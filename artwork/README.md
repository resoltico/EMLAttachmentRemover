# Artwork sources and exports

The macOS app bundles [EML.icns](app-icon/EML.icns). The Slint interface uses
standard widgets and structured text; the envelope-and-badge drawings are design
references and are not loaded by the application.

## Application icon

[EML.icon](app-icon/EML.icon/) is the editable Icon Composer package, with SVG
foreground layers on a 1024 × 1024 canvas. It defines a split white/yellow
paperclip on Royal blue, a navy dark-appearance background and reduced yellow
opacity for tinted appearance. Background colours use `extended-srgb`; SVGs
specify their own fills. The [construction reference](app-icon/GEOMETRY.md)
describes the geometry.

Distribution copies the committed `EML.icns`; Cargo builds require no Icon
Composer export tools. The ICNS contains ten icon slots: 16, 32, 128, 256 and
512 pixels at 1x and 2x. It is a flat default-appearance export.

### Exporting the icon

Export requires full Xcode. The deployment setting below selects a complete
ICNS representation; the app's deployment floor is set by distribution tooling.

```sh
mkdir -p target/icon-export
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun actool artwork/app-icon/EML.icon \
  --compile target/icon-export --output-format human-readable-text \
  --app-icon EML --include-all-app-icons --target-device mac \
  --minimum-deployment-target 10.12 --platform macosx \
  --output-partial-info-plist target/icon-export/icon-partial.plist
iconutil -c iconset target/icon-export/EML.icns -o target/icon-export/EML.iconset
```

Inspect the ten PNGs, including the 1024×1024 `icon_512x512@2x.png`, then copy
`target/icon-export/EML.icns` to `artwork/app-icon/EML.icns`.
The Icon Composer package and its SVG layers define the editable design.

## Interface reference drawings

[Interface geometry](interface/GEOMETRY.md) describes the SVG variants,
construction helpers and review requirements. [preview.png](interface/preview.png)
is a reference image, not a screenshot of the Slint app.

The drawings use a fixed envelope and a constant-size badge. Identity uses a
split diagonal mark; processing uses a static ring; success uses a check;
attention uses an exclamation mark; stopped uses a square. Attention has warning
and failure colours. Each kind has regular/compact and light/dark/increased-contrast
variants. The SVGs define the coordinates, strokes and palettes.

## License

The root [LICENSE](../LICENSE) identifies the artwork covered by proprietary
terms and the software and documentation covered by MPL-2.0. Preserve those
terms when redistributing. This guide does not grant additional artwork rights.
