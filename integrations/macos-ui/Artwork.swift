// Original geometric artwork, authored for this project. See ARTWORK.md and LICENSE.
import AppKit

@MainActor
enum Artwork {
  enum Kind: String, CaseIterable {
    case identity, processing, success, attention, stopped
  }

  static let blue = NSColor(srgbRed: 0.12, green: 0.36, blue: 0.68, alpha: 1)
  static let coral = NSColor(srgbRed: 0.94, green: 0.39, blue: 0.27, alpha: 1)
  static let ink = NSColor(srgbRed: 0.08, green: 0.16, blue: 0.25, alpha: 1)

  static func view(_ kind: Kind, color: NSColor) -> ArtworkView {
    ArtworkView(kind: kind, color: color)
  }

  static func draw(_ kind: Kind, in rect: NSRect, color: NSColor) {
    guard rect.width > 0, rect.height > 0 else { return }
    NSGraphicsContext.saveGraphicsState()
    let transform = NSAffineTransform()
    transform.translateX(by: rect.minX, yBy: rect.minY)
    transform.scaleX(by: rect.width / 100, yBy: rect.height / 100)
    transform.concat()
    drawUnit(kind, foreground: color)
    NSGraphicsContext.restoreGraphicsState()
  }

  private static func rectangle(
    _ x: CGFloat, _ y: CGFloat, _ width: CGFloat, _ height: CGFloat, radius: CGFloat = 3,
    color: NSColor
  ) {
    color.setFill()
    NSBezierPath(
      roundedRect: NSRect(x: x, y: y, width: width, height: height), xRadius: radius,
      yRadius: radius
    ).fill()
  }

  private static func outlineRectangle(
    _ rect: NSRect, radius: CGFloat, lineWidth: CGFloat, color: NSColor
  ) {
    let path = NSBezierPath(
      roundedRect: rect, xRadius: radius,
      yRadius: radius)
    path.lineWidth = lineWidth
    path.lineJoinStyle = .round
    color.setStroke()
    path.stroke()
  }

  private static func polygon(_ points: [(CGFloat, CGFloat)], color: NSColor) {
    guard let first = points.first else { return }
    let path = NSBezierPath()
    path.move(to: NSPoint(x: first.0, y: first.1))
    for point in points.dropFirst() { path.line(to: NSPoint(x: point.0, y: point.1)) }
    path.close()
    color.setFill()
    path.fill()
  }

  private static func line(_ points: [(CGFloat, CGFloat)], width: CGFloat, color: NSColor) {
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

  private static func retainedStrips(_ color: NSColor) {
    rectangle(18, 59, 42, 10, color: color)
    rectangle(18, 42, 48, 10, color: color)
    rectangle(18, 25, 34, 10, color: color)
  }

  private static func drawUnit(_ kind: Kind, foreground: NSColor) {
    retainedStrips(foreground)
    switch kind {
    case .identity:
      // A detached asymmetrical attachment tile, kept distinct from a stock file or
      // paperclip glyph.
      polygon([(72, 63), (81, 69), (88, 58), (79, 52)], color: coral)
    case .processing:
      rectangle(74, 59, 9, 9, radius: 2, color: foreground)
      rectangle(74, 42, 9, 9, radius: 2, color: foreground.withAlphaComponent(0.70))
      rectangle(74, 25, 9, 9, radius: 2, color: foreground.withAlphaComponent(0.45))
    case .success:
      line([(69, 45), (75, 39), (86, 57)], width: 5.5, color: foreground)
    case .attention:
      rectangle(75, 43, 6, 18, radius: 2, color: foreground)
      rectangle(75, 31, 6, 6, radius: 2, color: foreground)
    case .stopped:
      outlineRectangle(
        NSRect(x: 70, y: 39, width: 16, height: 16), radius: 2.5, lineWidth: 5,
        color: foreground)
    }
  }
}

@MainActor
final class ArtworkView: NSView {
  private let kind: Artwork.Kind
  private let artworkColor: NSColor

  init(kind: Artwork.Kind, color: NSColor) {
    self.kind = kind
    artworkColor = color
    super.init(frame: .zero)
    translatesAutoresizingMaskIntoConstraints = false
    setAccessibilityElement(false)
  }

  @available(*, unavailable)
  required init?(coder: NSCoder) {
    fatalError("ArtworkView must be created programmatically")
  }

  override var isOpaque: Bool { false }

  override func draw(_ dirtyRect: NSRect) {
    super.draw(dirtyRect)
    Artwork.draw(kind, in: bounds, color: artworkColor)
  }

  override func viewDidChangeEffectiveAppearance() {
    super.viewDidChangeEffectiveAppearance()
    needsDisplay = true
  }
}
