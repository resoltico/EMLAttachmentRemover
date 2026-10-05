import AppKit

@main
struct ArtworkTests {
  @MainActor static func main() {
    NSApplication.shared.setActivationPolicy(.prohibited)
    checkPalette()
    checkAppearance()
    checkInvalidation()
    print("Native artwork palette, appearance and invalidation checks passed.")
  }

  @MainActor private static func checkPalette() {
    let backgrounds: [[UInt32]] = [
      [0xFFFFFF, 0xECECEC], [0x1E1E1E, 0x323232],
      [0xFFFFFF, 0xECECEC], [0x000000, 0x1E1E1E, 0x323232],
    ]
    for (index, style) in Artwork.Style.all.enumerated() {
      for background in backgrounds[index] {
        let luminance = luminance(background)
        precondition(contrast(Artwork.Palette.envelope(style), luminance) >= 4.5)
        precondition(contrast(Artwork.Palette.identityBadge(style), luminance) >= 4.5)
        for tone in Artwork.Tone.allCases {
          let badge = Artwork.Palette.badge(tone, style)
          precondition(contrast(badge, luminance) >= 4.5)
          precondition(contrast(Artwork.Palette.mark(style), luminanceOf(badge)) >= 4.5)
        }
      }
      precondition(
        contrast(
          Artwork.Palette.identityMark(style),
          luminanceOf(Artwork.Palette.identityBadge(style))) >= 4.5)
    }
  }

  private static func luminance(_ hex: UInt32) -> Double {
    let values = [hex >> 16 & 255, hex >> 8 & 255, hex & 255].map { Double($0) / 255 }
    return luminance(values)
  }

  private static func luminance(_ values: [Double]) -> Double {
    let linear = values.map { $0 <= 0.04045 ? $0 / 12.92 : pow(($0 + 0.055) / 1.055, 2.4) }
    return linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722
  }

  @MainActor private static func luminanceOf(_ color: NSColor) -> Double {
    guard let rgb = color.usingColorSpace(.sRGB) else {
      preconditionFailure("Artwork colour cannot be converted to sRGB")
    }
    return luminance([
      Double(rgb.redComponent), Double(rgb.greenComponent), Double(rgb.blueComponent),
    ])
  }

  @MainActor private static func contrast(_ color: NSColor, _ background: Double) -> Double {
    let foreground = luminanceOf(color)
    return (max(foreground, background) + 0.05) / (min(foreground, background) + 0.05)
  }

  @MainActor private static func checkAppearance() {
    let cases: [(NSAppearance.Name, Bool)] = [(.aqua, false), (.darkAqua, true)]
    for (name, dark) in cases {
      guard let appearance = NSAppearance(named: name) else {
        preconditionFailure("Supported artwork appearance unavailable")
      }
      appearance.performAsCurrentDrawingAppearance {
        let style = Artwork.Style.current()
        precondition(style.dark == dark)
        precondition(
          style.highContrast
            == NSWorkspace.shared.accessibilityDisplayShouldIncreaseContrast)
      }
    }
  }

  @MainActor private static func checkInvalidation() {
    let window = NSWindow(
      contentRect: NSRect(x: 0, y: 0, width: 28, height: 28),
      styleMask: [.borderless], backing: .buffered, defer: false)
    let view = Artwork.view(.processing, tone: .accent)
    window.contentView = view
    precondition(!view.isAccessibilityElement())
    view.needsDisplay = false
    NSWorkspace.shared.notificationCenter.post(
      name: NSWorkspace.accessibilityDisplayOptionsDidChangeNotification,
      object: NSWorkspace.shared)
    precondition(view.needsDisplay, "Display options did not invalidate the artwork")
    view.needsDisplay = false
    view.appearance = NSAppearance(named: .darkAqua)
    precondition(view.needsDisplay, "Appearance change did not invalidate the artwork")
    withExtendedLifetime(window) {}
  }
}
