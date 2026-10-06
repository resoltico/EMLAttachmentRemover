// Copyright (c) 2026 Ervins Strauhmanis.
// SPDX-License-Identifier: LicenseRef-Proprietary-Artwork
// Original geometric artwork. See ARTWORK.md and the proprietary terms in LICENSE.
import AppKit

/// Status artwork: one unchanging envelope with a same-size status badge in its corner.
///
/// Geometry uses a 100 × 100 unit square with AppKit's bottom-left origin. The envelope
/// never changes between states; only the badge colour and its mark do. Colours resolve
/// from the drawing appearance (light, dark and their Increase Contrast variants), and
/// stroke weights grow at compact sizes and with Increase Contrast. Nothing animates, so
/// Reduce Motion needs no special handling.
@MainActor
enum Artwork {
  enum Kind: String, CaseIterable {
    case identity, processing, success, attention, stopped
  }

  /// Semantic badge colour. Identity ignores it and always uses the brand pair.
  enum Tone: String, CaseIterable {
    case accent, success, warning, failure, neutral
  }

  /// Appearance-dependent drawing parameters.
  struct Style: Equatable {
    var dark: Bool
    var highContrast: Bool

    static let light = Style(dark: false, highContrast: false)
    static let dark = Style(dark: true, highContrast: false)
    static let highContrastLight = Style(dark: false, highContrast: true)
    static let highContrastDark = Style(dark: true, highContrast: true)
    static let all = [light, dark, highContrastLight, highContrastDark]

    /// Index into the four-entry colour tables: light, dark, high-contrast light/dark.
    var paletteIndex: Int { (dark ? 1 : 0) + (highContrast ? 2 : 0) }

    /// Resolves the style from the current drawing appearance and accessibility settings.
    @MainActor static func current() -> Style {
      let match = NSAppearance.currentDrawing().bestMatch(from: [
        .aqua, .darkAqua, .accessibilityHighContrastAqua, .accessibilityHighContrastDarkAqua,
      ])
      let dark = match == .darkAqua || match == .accessibilityHighContrastDarkAqua
      let highContrast =
        match == .accessibilityHighContrastAqua || match == .accessibilityHighContrastDarkAqua
        || NSWorkspace.shared.accessibilityDisplayShouldIncreaseContrast
      return Style(dark: dark, highContrast: highContrast)
    }
  }

  /// Measured sRGB colours. ARTWORK.md lists every contrast ratio.
  enum Palette {
    // Order in every table: light, dark, high-contrast light, high-contrast dark.
    private static let envelopeHex: [UInt32] = [0x63_6366, 0xAE_AEB2, 0x3A_3A3C, 0xE5_E5EA]
    private static let markHex: [UInt32] = [0xFF_FFFF, 0x1C_1C1E, 0xFF_FFFF, 0x00_0000]
    private static let identityBadgeHex: [UInt32] = [0x0B_3D91, 0xFF_D166, 0x0B_3D91, 0xFF_D166]
    private static let identityMarkHex: [UInt32] = [0xFF_D166, 0x0B_3D91, 0xFF_D166, 0x0B_3D91]

    static func envelope(_ style: Style) -> NSColor { color(envelopeHex, style) }
    static func mark(_ style: Style) -> NSColor { color(markHex, style) }
    static func identityBadge(_ style: Style) -> NSColor { color(identityBadgeHex, style) }
    static func identityMark(_ style: Style) -> NSColor { color(identityMarkHex, style) }
    static func badge(_ tone: Tone, _ style: Style) -> NSColor {
      let table: [UInt32]
      switch tone {
      case .accent: table = [0x00_66D6, 0x40_9CFF, 0x00_50A8, 0x6B_B3FF]
      case .success: table = [0x1F_7A35, 0x30_D158, 0x14_6128, 0x5B_E07A]
      case .warning: table = [0xB5_4700, 0xFF_9F0A, 0x8F_3800, 0xFF_B340]
      case .failure: table = [0xC4_281C, 0xFF_6961, 0xA1_1A12, 0xFF_8A80]
      case .neutral: table = [0x63_6366, 0xAE_AEB2, 0x48_484A, 0xD1_D1D6]
      }
      return color(table, style)
    }

    private static func color(_ table: [UInt32], _ style: Style) -> NSColor {
      let value = table[style.paletteIndex]
      return NSColor(
        srgbRed: CGFloat((value >> 16) & 0xFF) / 255, green: CGFloat((value >> 8) & 0xFF) / 255,
        blue: CGFloat(value & 0xFF) / 255, alpha: 1)
    }
  }

  /// Stroke weights in artwork units for one rendering.
  struct Metrics: Equatable {
    let envelope: CGFloat
    let mark: CGFloat
    let ring: CGFloat
    /// Width of the exclamation bar and radius of its dot; bolder when compact.
    let exclamationWidth: CGFloat
    let exclamationDot: CGFloat

    /// Compact sizes (below 24 points) and Increase Contrast both thicken strokes so the
    /// lines stay at least about one pixel wide at 18 points on non-Retina displays.
    init(size: CGFloat, style: Style) {
      let compact: CGFloat = size < 24 ? 1 : 0
      let contrast: CGFloat = style.highContrast ? 1 : 0
      envelope = 8 + compact + contrast * 1.5
      mark = 5.5 + compact + contrast
      ring = 7 + contrast * 0.5
      exclamationWidth = 5 + compact * 1.5
      exclamationDot = 3 + compact * 0.6
    }
  }

  static let badgeCenter = NSPoint(x: 72, y: 30)
  static let badgeRadius: CGFloat = 18
  /// Radius of the envelope cut-out around the badge; leaves a constant 7-unit gap.
  static let badgeClearance: CGFloat = 25

  static func view(_ kind: Kind, tone: Tone) -> ArtworkView {
    ArtworkView(kind: kind, tone: tone)
  }

  /// Draws the artwork into `rect`. Pass `style` to render a specific appearance (previews);
  /// leave it `nil` to follow the current drawing appearance.
  static func draw(_ kind: Kind, in rect: NSRect, tone: Tone, style: Style? = nil) {
    guard rect.width > 0, rect.height > 0 else { return }
    let resolved = style ?? Style.current()
    let metrics = Metrics(size: min(rect.width, rect.height), style: resolved)
    NSGraphicsContext.saveGraphicsState()
    let transform = NSAffineTransform()
    transform.translateX(by: rect.minX, yBy: rect.minY)
    transform.scaleX(by: rect.width / 100, yBy: rect.height / 100)
    transform.concat()
    drawEnvelope(Palette.envelope(resolved), metrics: metrics)
    drawBadge(kind, tone: tone, style: resolved, metrics: metrics)
    NSGraphicsContext.restoreGraphicsState()
  }

  // MARK: - Envelope (identical in every state)

  private static func drawEnvelope(_ color: NSColor, metrics: Metrics) {
    NSGraphicsContext.saveGraphicsState()
    let clip = NSBezierPath(rect: NSRect(x: 0, y: 0, width: 100, height: 100))
    clip.append(circle(badgeCenter, radius: badgeClearance))
    clip.windingRule = .evenOdd
    clip.addClip()
    color.setStroke()
    let body = NSBezierPath(
      roundedRect: NSRect(x: 8, y: 34, width: 62, height: 46), xRadius: 9, yRadius: 9)
    body.lineWidth = metrics.envelope
    body.stroke()
    stroke([(15, 73), (39, 54), (63, 73)], width: metrics.envelope, color: color)
    NSGraphicsContext.restoreGraphicsState()
  }

  // MARK: - Badge (same footprint in every state)

  private static func drawBadge(_ kind: Kind, tone: Tone, style: Style, metrics: Metrics) {
    let ink = Palette.mark(style)
    switch kind {
    case .identity:
      fillBadge(Palette.identityBadge(style))
      let mark = Palette.identityMark(style)
      stroke([(63.5, 21.5), (68.5, 26.5)], width: metrics.mark, color: mark)
      stroke([(75.5, 33.5), (80.5, 38.5)], width: metrics.mark, color: mark)
    case .processing:
      drawProgressRing(Palette.badge(tone, style), style: style, metrics: metrics)
    case .success:
      fillBadge(Palette.badge(tone, style))
      stroke([(63, 30), (69, 24), (81, 37)], width: metrics.mark, color: ink)
    case .attention:
      fillBadge(Palette.badge(tone, style))
      let width = metrics.exclamationWidth
      fillRoundedRect(
        NSRect(x: 72 - width / 2, y: 28, width: width, height: 14), radius: 2, color: ink)
      ink.setFill()
      circle(NSPoint(x: 72, y: 22), radius: metrics.exclamationDot).fill()
    case .stopped:
      fillBadge(Palette.badge(tone, style))
      fillRoundedRect(NSRect(x: 65, y: 23, width: 14, height: 14), radius: 3, color: ink)
    }
  }

  /// A static progress ring: a faint full track plus a solid arc that starts at the top
  /// and runs clockwise. Its outer edge matches the solid badge radius exactly.
  private static func drawProgressRing(_ color: NSColor, style: Style, metrics: Metrics) {
    let radius = badgeRadius - metrics.ring / 2
    let track = circle(badgeCenter, radius: radius)
    track.lineWidth = metrics.ring
    color.withAlphaComponent(style.highContrast ? 0.45 : 0.3).setStroke()
    track.stroke()
    let arc = NSBezierPath()
    arc.appendArc(
      withCenter: badgeCenter, radius: radius, startAngle: 90, endAngle: -139, clockwise: true)
    arc.lineWidth = metrics.ring
    arc.lineCapStyle = .round
    color.setStroke()
    arc.stroke()
  }

  // MARK: - Primitives

  private static func fillBadge(_ color: NSColor) {
    color.setFill()
    circle(badgeCenter, radius: badgeRadius).fill()
  }

  private static func circle(_ center: NSPoint, radius: CGFloat) -> NSBezierPath {
    NSBezierPath(
      ovalIn: NSRect(
        x: center.x - radius, y: center.y - radius, width: radius * 2, height: radius * 2))
  }

  private static func fillRoundedRect(_ rect: NSRect, radius: CGFloat, color: NSColor) {
    color.setFill()
    NSBezierPath(roundedRect: rect, xRadius: radius, yRadius: radius).fill()
  }

  private static func stroke(_ points: [(CGFloat, CGFloat)], width: CGFloat, color: NSColor) {
    guard let first = points.first else { return }
    let path = NSBezierPath()
    path.move(to: NSPoint(x: first.0, y: first.1))
    for point in points.dropFirst() { path.line(to: NSPoint(x: point.0, y: point.1)) }
    path.lineWidth = width
    path.lineCapStyle = .round
    path.lineJoinStyle = .round
    color.setStroke()
    path.stroke()
  }
}

@MainActor
final class ArtworkView: NSView {
  private let kind: Artwork.Kind
  private let tone: Artwork.Tone
  private let displayNotifications = NSWorkspace.shared.notificationCenter

  init(kind: Artwork.Kind, tone: Artwork.Tone) {
    self.kind = kind
    self.tone = tone
    super.init(frame: .zero)
    translatesAutoresizingMaskIntoConstraints = false
    setAccessibilityElement(false)
    displayNotifications.addObserver(
      self, selector: #selector(displayOptionsChanged),
      name: NSWorkspace.accessibilityDisplayOptionsDidChangeNotification, object: nil)
  }

  @available(*, unavailable)
  required init?(coder: NSCoder) {
    fatalError("ArtworkView must be created programmatically")
  }

  deinit { displayNotifications.removeObserver(self) }

  @objc private func displayOptionsChanged(_ notification: Notification) {
    needsDisplay = true
  }

  override var isOpaque: Bool { false }

  override func draw(_ dirtyRect: NSRect) {
    super.draw(dirtyRect)
    Artwork.draw(kind, in: bounds, tone: tone)
  }

  /// Follow effective-appearance changes as well as workspace display-option changes.
  override func viewDidChangeEffectiveAppearance() {
    super.viewDidChangeEffectiveAppearance()
    needsDisplay = true
  }
}
