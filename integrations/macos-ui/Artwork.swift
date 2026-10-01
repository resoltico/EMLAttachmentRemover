// Original geometric artwork, authored for this project. See ARTWORK.md and LICENSE.
import AppKit

@MainActor
enum Artwork {
  enum Kind: String, CaseIterable {
    case identity, processing, success, attention, stopped, welcome
  }

  static let blue = NSColor(srgbRed: 0.12, green: 0.36, blue: 0.68, alpha: 1)
  static let coral = NSColor(srgbRed: 0.94, green: 0.39, blue: 0.27, alpha: 1)
  static let ink = NSColor(srgbRed: 0.08, green: 0.16, blue: 0.25, alpha: 1)

  static func image(_ kind: Kind, size: CGFloat = 100, color: NSColor? = nil, appIcon: Bool = false)
    -> NSImage
  {
    // Explicit pixel dimensions make output independent of display density.
    guard
      let bitmap = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: Int(size), pixelsHigh: Int(size), bitsPerSample: 8,
        samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
        bytesPerRow: 0, bitsPerPixel: 0)
    else {
      preconditionFailure("Cannot allocate original artwork bitmap")
    }
    bitmap.size = NSSize(width: size, height: size)
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
    let transform = NSAffineTransform()
    transform.scale(by: size / 100)
    transform.concat()
    if appIcon {
      ink.setFill()
      NSBezierPath(roundedRect: NSRect(x: 4, y: 4, width: 92, height: 92), xRadius: 21, yRadius: 21)
        .fill()
    }
    draw(kind, foreground: color ?? (appIcon ? .white : blue), appIcon: appIcon)
    NSGraphicsContext.restoreGraphicsState()
    let image = NSImage(size: NSSize(width: size, height: size))
    image.addRepresentation(bitmap)
    return image
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

  private static func polygon(_ points: [(CGFloat, CGFloat)], color: NSColor) {
    let path = NSBezierPath()
    path.move(to: NSPoint(x: points[0].0, y: points[0].1))
    for point in points.dropFirst() { path.line(to: NSPoint(x: point.0, y: point.1)) }
    path.close()
    color.setFill()
    path.fill()
  }

  private static func line(_ points: [(CGFloat, CGFloat)], width: CGFloat, color: NSColor) {
    let path = NSBezierPath()
    path.move(to: NSPoint(x: points[0].0, y: points[0].1))
    for point in points.dropFirst() { path.line(to: NSPoint(x: point.0, y: point.1)) }
    path.lineWidth = width
    path.lineCapStyle = .round
    path.lineJoinStyle = .round
    color.setStroke()
    path.stroke()
  }

  private static func retainedStrips(_ color: NSColor) {
    rectangle(21, 59, 43, 10, color: color)
    rectangle(21, 42, 51, 10, color: color)
    rectangle(21, 25, 35, 10, color: color)
  }

  private static func draw(_ kind: Kind, foreground: NSColor, appIcon: Bool) {
    switch kind {
    case .identity, .welcome:
      retainedStrips(foreground)
      // Detached, asymmetrical tile: no document, envelope, badge, or stock glyph.
      polygon([(72, 63), (81, 69), (88, 58), (79, 52)], color: coral)
    case .processing:
      retainedStrips(foreground)
      rectangle(78, 59, 8, 8, radius: 2, color: foreground)
      rectangle(78, 42, 8, 8, radius: 2, color: foreground.withAlphaComponent(0.55))
      rectangle(78, 25, 8, 8, radius: 2, color: foreground.withAlphaComponent(0.25))
    case .success:
      rectangle(18, 57, 35, 9, color: foreground)
      rectangle(18, 39, 26, 9, color: foreground)
      line([(56, 36), (65, 28), (84, 56)], width: 7, color: foreground)
    case .attention:
      // Open corner and slanted edge give this panel its own proportions.
      line(
        [(28, 25), (17, 25), (17, 72), (69, 72), (83, 58), (83, 25), (63, 25)], width: 6,
        color: foreground)
      rectangle(46, 43, 8, 18, radius: 2, color: foreground)
      rectangle(46, 29, 8, 7, radius: 2, color: foreground)
    case .stopped:
      rectangle(22, 38, 12, 34, radius: 3, color: foreground)
      rectangle(43, 28, 12, 44, radius: 3, color: foreground)
      line([(68, 69), (81, 69), (81, 29), (68, 29)], width: 6, color: foreground)
    }
  }
}
