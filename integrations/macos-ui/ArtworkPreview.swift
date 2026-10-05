import AppKit

@main
struct ArtworkPreview {
  @MainActor private static let ink = NSColor(srgbRed: 0.11, green: 0.11, blue: 0.12, alpha: 1)

  private static func sRGBBitmap(width: Int, height: Int) -> NSBitmapImageRep? {
    // Tag the empty allocation before drawing, so its graphics context uses sRGB.
    NSBitmapImageRep(
      bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height, bitsPerSample: 8,
      samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
      bytesPerRow: 0, bitsPerPixel: 0
    )?.retagging(with: .sRGB)
  }

  /// The tone each state uses in the application's most common presentation.
  @MainActor static func defaultTone(_ kind: Artwork.Kind) -> Artwork.Tone {
    switch kind {
    case .identity, .processing: return .accent
    case .success: return .success
    case .attention: return .warning
    case .stopped: return .neutral
    }
  }

  @MainActor static func main() throws {
    let destination = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
    try FileManager.default.createDirectory(at: destination, withIntermediateDirectories: true)
    for kind in Artwork.Kind.allCases {
      for size in [16, 28, 32, 128, 256] {
        try save(
          renderedImage(kind, pixels: size, style: .light),
          at: destination.appendingPathComponent("\(kind.rawValue)-\(size).png"))
      }
    }
    try save(
      identitySourceComposite(pixels: 512),
      at: destination.appendingPathComponent("identity-source.png"))
    try reviewSheet(destination: destination)
  }

  @MainActor private static func renderedImage(
    _ kind: Artwork.Kind, pixels: Int, style: Artwork.Style, tone: Artwork.Tone? = nil
  ) -> NSImage {
    precondition(pixels > 0)
    guard
      let bitmap = sRGBBitmap(width: pixels, height: pixels),
      let context = NSGraphicsContext(bitmapImageRep: bitmap)
    else {
      preconditionFailure("Cannot allocate artwork preview")
    }
    bitmap.size = NSSize(width: pixels, height: pixels)
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = context
    Artwork.draw(
      kind, in: NSRect(x: 0, y: 0, width: pixels, height: pixels), tone: tone ?? defaultTone(kind),
      style: style)
    NSGraphicsContext.restoreGraphicsState()
    let image = NSImage(size: NSSize(width: pixels, height: pixels))
    image.addRepresentation(bitmap)
    return image
  }

  /// Identity artwork on a plain light card, for documentation. Not the application icon.
  @MainActor private static func identitySourceComposite(pixels: Int) -> NSImage {
    guard
      let bitmap = sRGBBitmap(width: pixels, height: pixels),
      let context = NSGraphicsContext(bitmapImageRep: bitmap)
    else {
      preconditionFailure("Cannot allocate identity preview")
    }
    bitmap.size = NSSize(width: pixels, height: pixels)
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = context
    NSColor.white.setFill()
    NSRect(x: 0, y: 0, width: pixels, height: pixels).fill()
    Artwork.draw(
      .identity, in: NSRect(x: 0, y: 0, width: pixels, height: pixels), tone: .accent,
      style: .light)
    NSGraphicsContext.restoreGraphicsState()
    let image = NSImage(size: NSSize(width: pixels, height: pixels))
    image.addRepresentation(bitmap)
    return image
  }

  @MainActor private static func reviewSheet(destination: URL) throws {
    guard
      let bitmap = sRGBBitmap(width: 1800, height: 1300),
      let context = NSGraphicsContext(bitmapImageRep: bitmap)
    else {
      throw NSError(domain: "EMLArtwork", code: 1)
    }
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = context
    NSColor(srgbRed: 0.96, green: 0.97, blue: 0.98, alpha: 1).setFill()
    NSRect(x: 0, y: 0, width: 1800, height: 1300).fill()
    text("EML Attachment Remover", x: 80, y: 1215, size: 36, weight: .semibold, color: ink)
    text(
      "Status artwork · light, dark and Increase Contrast · 56 / 28 / 18 px", x: 80, y: 1170,
      size: 23, color: .darkGray)
    let names = ["Light", "Dark", "Increase Contrast · light", "Increase Contrast · dark"]
    for (index, style) in Artwork.Style.all.enumerated() {
      drawPanel(style, title: names[index], top: CGFloat(1110 - index * 250))
    }
    text(
      "Geometry review only. Check live UI appearances and the compiled icon separately.",
      x: 80, y: 62, size: 20, color: .darkGray)
    NSGraphicsContext.restoreGraphicsState()
    let image = NSImage(size: NSSize(width: 1800, height: 1300))
    image.addRepresentation(bitmap)
    try save(image, at: destination.appendingPathComponent("original-artwork-review.png"))
  }

  @MainActor private static func drawPanel(_ style: Artwork.Style, title: String, top: CGFloat) {
    let background =
      style.dark
      ? NSColor(srgbRed: 0.12, green: 0.12, blue: 0.12, alpha: 1)
      : NSColor(srgbRed: 0.925, green: 0.925, blue: 0.925, alpha: 1)
    background.setFill()
    NSBezierPath(
      roundedRect: NSRect(x: 80, y: top - 230, width: 1640, height: 230), xRadius: 18, yRadius: 18
    ).fill()
    let label = style.dark ? NSColor(white: 0.92, alpha: 1) : ink
    text(title, x: 110, y: top - 50, size: 22, weight: .semibold, color: label)
    let columns: [(String, Artwork.Kind, Artwork.Tone)] = [
      ("Identity", .identity, .accent), ("Processing", .processing, .accent),
      ("Ready", .success, .success), ("Attention", .attention, .warning),
      ("Failed", .attention, .failure), ("Stopped", .stopped, .neutral),
    ]
    for (index, column) in columns.enumerated() {
      let (name, kind, tone) = column
      let x = CGFloat(390 + index * 220)
      renderedImage(kind, pixels: 56, style: style, tone: tone).draw(
        in: NSRect(x: x, y: top - 110, width: 56, height: 56))
      renderedImage(kind, pixels: 28, style: style, tone: tone).draw(
        in: NSRect(x: x + 76, y: top - 96, width: 28, height: 28))
      renderedImage(kind, pixels: 18, style: style, tone: tone).draw(
        in: NSRect(x: x + 124, y: top - 91, width: 18, height: 18))
      text(name, x: x, y: top - 160, size: 19, color: label)
    }
  }

  @MainActor private static func text(
    _ value: String, x: CGFloat, y: CGFloat, size: CGFloat, weight: NSFont.Weight = .regular,
    color: NSColor
  ) {
    (value as NSString).draw(
      at: NSPoint(x: x, y: y),
      withAttributes: [
        .font: NSFont.systemFont(ofSize: size, weight: weight), .foregroundColor: color,
      ])
  }

  private static func save(_ image: NSImage, at url: URL) throws {
    guard let bitmap = image.representations.first as? NSBitmapImageRep,
      let png = bitmap.representation(using: .png, properties: [:])
    else { throw NSError(domain: "EMLArtwork", code: 2) }
    try png.write(to: url)
  }
}
