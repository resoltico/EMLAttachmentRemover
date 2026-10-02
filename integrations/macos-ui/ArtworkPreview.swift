import AppKit

@main
struct ArtworkPreview {
  private static func sRGBBitmap(width: Int, height: Int) -> NSBitmapImageRep? {
    // Tag the empty allocation before drawing, so its graphics context uses sRGB.
    NSBitmapImageRep(
      bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height, bitsPerSample: 8,
      samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
      bytesPerRow: 0, bitsPerPixel: 0
    )?.retagging(with: .sRGB)
  }

  @MainActor static func main() throws {
    let destination = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
    try FileManager.default.createDirectory(at: destination, withIntermediateDirectories: true)
    for kind in Artwork.Kind.allCases {
      for size in [16, 28, 32, 128, 256] {
        try save(
          renderedImage(kind, pixels: size),
          at: destination.appendingPathComponent("\(kind.rawValue)-\(size).png"))
      }
    }
    try save(
      iconSourceComposite(pixels: 512),
      at: destination.appendingPathComponent("icon-source.png"))
    try reviewSheet(destination: destination)
  }

  @MainActor private static func renderedImage(
    _ kind: Artwork.Kind, pixels: Int, color: NSColor = .controlAccentColor
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
    Artwork.draw(kind, in: NSRect(x: 0, y: 0, width: pixels, height: pixels), color: color)
    NSGraphicsContext.restoreGraphicsState()
    let image = NSImage(size: NSSize(width: pixels, height: pixels))
    image.addRepresentation(bitmap)
    return image
  }

  @MainActor private static func iconSourceComposite(pixels: Int) -> NSImage {
    guard
      let bitmap = sRGBBitmap(width: pixels, height: pixels),
      let context = NSGraphicsContext(bitmapImageRep: bitmap)
    else {
      preconditionFailure("Cannot allocate icon source preview")
    }
    bitmap.size = NSSize(width: pixels, height: pixels)
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = context
    Artwork.ink.setFill()
    NSRect(x: 0, y: 0, width: pixels, height: pixels).fill()
    Artwork.draw(
      .identity, in: NSRect(x: 0, y: 0, width: pixels, height: pixels), color: .white)
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
    text("EML Attachment Remover", x: 80, y: 1215, size: 36, weight: .semibold)
    text(
      "Original vector graphics · state-family review", x: 80, y: 1170, size: 23,
      color: .darkGray)
    drawSimulatedIcon(in: NSRect(x: 85, y: 775, width: 300, height: 300))
    text("Icon geometry study", x: 105, y: 710, size: 25, weight: .semibold)
    text(
      "Materials and mask are illustrative.", x: 80, y: 670, size: 19,
      color: .darkGray)
    NSColor.white.setFill()
    NSBezierPath(
      roundedRect: NSRect(x: 490, y: 570, width: 1210, height: 545), xRadius: 22, yRadius: 22
    ).fill()
    renderedImage(.identity, pixels: 65).draw(
      in: NSRect(x: 535, y: 1000, width: 65, height: 65))
    text("EML Attachment Remover", x: 620, y: 1029, size: 27, weight: .semibold)
    renderedImage(
      .attention, pixels: 56,
      color: .systemOrange
    ).draw(in: NSRect(x: 530, y: 885, width: 56, height: 56))
    text("Couldn’t create the copy", x: 612, y: 902, size: 34, weight: .semibold)
    text("1 file needs attention", x: 612, y: 863, size: 21, color: .darkGray)
    text("public-good.eml", x: 545, y: 795, size: 25, weight: .semibold)
    text("An existing copy differs", x: 545, y: 750, size: 23, weight: .semibold)
    text(
      "The existing output doesn’t match this run’s result. Nothing was overwritten.", x: 545,
      y: 707, size: 21)
    text("Move or rename the existing output, then run again.", x: 545, y: 668, size: 21)
    text("Details · 1 error · 1 warning", x: 545, y: 607, size: 19, color: .darkGray)
    NSColor.controlAccentColor.setFill()
    NSBezierPath(
      roundedRect: NSRect(x: 1510, y: 595, width: 130, height: 48), xRadius: 12, yRadius: 12
    ).fill()
    text("Done", x: 1550, y: 608, size: 21, color: .white)
    drawIndicators()
    text(
      "Geometry review only. Check compiled icons and live UI appearances separately.", x: 80,
      y: 62, size: 20, color: .darkGray)
    NSGraphicsContext.restoreGraphicsState()
    let image = NSImage(size: NSSize(width: 1800, height: 1300))
    image.addRepresentation(bitmap)
    try save(image, at: destination.appendingPathComponent("original-artwork-review.png"))
  }

  @MainActor private static func drawSimulatedIcon(in rect: NSRect) {
    NSGraphicsContext.saveGraphicsState()
    NSBezierPath(
      roundedRect: rect, xRadius: rect.width * 0.22, yRadius: rect.height * 0.22
    ).addClip()
    Artwork.ink.setFill()
    rect.fill()
    Artwork.draw(.identity, in: rect, color: .white)
    NSGraphicsContext.restoreGraphicsState()
  }

  @MainActor private static func drawIndicators() {
    let labels = ["Identity", "Processing", "Ready", "Attention", "Stopped"]
    for (index, kind) in Artwork.Kind.allCases.enumerated() {
      let x = CGFloat(115 + index * 340)
      let color: NSColor =
        kind == .success
        ? .systemGreen
        : kind == .attention
          ? .systemOrange
          : kind == .stopped ? .secondaryLabelColor : .controlAccentColor
      renderedImage(kind, pixels: 125, color: color).draw(
        in: NSRect(x: x, y: 310, width: 125, height: 125))
      text(labels[index], x: x, y: 260, size: 22, weight: .semibold)
      renderedImage(kind, pixels: 28, color: color).draw(
        in: NSRect(x: x, y: 195, width: 28, height: 28))
      renderedImage(kind, pixels: 16, color: color).draw(
        in: NSRect(x: x + 52, y: 201, width: 16, height: 16))
      text("28 / 16 px", x: x, y: 150, size: 17, color: .darkGray)
    }
  }

  @MainActor private static func text(
    _ value: String, x: CGFloat, y: CGFloat, size: CGFloat, weight: NSFont.Weight = .regular,
    color: NSColor = Artwork.ink
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
