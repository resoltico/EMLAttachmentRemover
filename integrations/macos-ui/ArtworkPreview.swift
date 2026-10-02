import AppKit

@main
struct ArtworkPreview {
  @MainActor static func main() throws {
    let destination = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
    try FileManager.default.createDirectory(at: destination, withIntermediateDirectories: true)
    for kind in Artwork.Kind.allCases {
      for size in [16, 32, 128, 256] {
        try save(
          Artwork.image(kind, size: CGFloat(size)),
          at: destination.appendingPathComponent("\(kind.rawValue)-\(size).png"))
      }
    }
    try save(
      Artwork.image(.identity, size: 512, appIcon: true),
      at: destination.appendingPathComponent("app-icon.png"))
    try reviewSheet(destination: destination)
  }

  @MainActor private static func reviewSheet(destination: URL) throws {
    guard
      let bitmap = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: 1800, pixelsHigh: 1300, bitsPerSample: 8,
        samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
        bytesPerRow: 0, bitsPerPixel: 0)
    else {
      throw NSError(domain: "EMLArtwork", code: 1)
    }
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
    NSColor(srgbRed: 0.96, green: 0.97, blue: 0.98, alpha: 1).setFill()
    NSRect(x: 0, y: 0, width: 1800, height: 1300).fill()
    text("EML Attachment Remover", x: 80, y: 1215, size: 36, weight: .semibold)
    text("Original graphics · v4.0.0 review", x: 80, y: 1170, size: 23, color: .darkGray)
    Artwork.image(.identity, size: 300, appIcon: true).draw(
      in: NSRect(x: 85, y: 775, width: 300, height: 300))
    text("Application icon", x: 105, y: 710, size: 25, weight: .semibold)
    text("Retained strips + detached tile", x: 80, y: 670, size: 19, color: .darkGray)
    NSColor.white.setFill()
    NSBezierPath(
      roundedRect: NSRect(x: 490, y: 570, width: 1210, height: 545), xRadius: 22, yRadius: 22
    ).fill()
    Artwork.image(.identity, size: 65).draw(in: NSRect(x: 535, y: 1000, width: 65, height: 65))
    text("EML Attachment Remover", x: 620, y: 1029, size: 27, weight: .semibold)
    Artwork.image(
      .attention, size: 56, color: NSColor(srgbRed: 0.73, green: 0.36, blue: 0.09, alpha: 1)
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
    Artwork.blue.setFill()
    NSBezierPath(
      roundedRect: NSRect(x: 1510, y: 595, width: 130, height: 48), xRadius: 12, yRadius: 12
    ).fill()
    text("Done", x: 1550, y: 608, size: 21, color: .white)
    drawIndicators()
    text(
      "Drawn from original geometry. The preview and application use the same renderer.", x: 80,
      y: 62, size: 20, color: .darkGray)
    NSGraphicsContext.restoreGraphicsState()
    let image = NSImage(size: NSSize(width: 1800, height: 1300))
    image.addRepresentation(bitmap)
    try save(image, at: destination.appendingPathComponent("original-artwork-review.png"))
  }

  @MainActor private static func drawIndicators() {
    let labels = ["Identity", "Processing", "Ready", "Attention", "Stopped", "Welcome"]
    for (index, kind) in Artwork.Kind.allCases.enumerated() {
      let x = CGFloat(95 + index * 285)
      let color: NSColor =
        kind == .success
        ? NSColor(srgbRed: 0.13, green: 0.49, blue: 0.34, alpha: 1)
        : kind == .attention
          ? NSColor(srgbRed: 0.73, green: 0.36, blue: 0.09, alpha: 1) : Artwork.blue
      Artwork.image(kind, size: 125, color: color).draw(
        in: NSRect(x: x, y: 310, width: 125, height: 125))
      text(labels[index], x: x, y: 260, size: 22, weight: .semibold)
      Artwork.image(kind, size: 32, color: color).draw(
        in: NSRect(x: x, y: 195, width: 32, height: 32))
      Artwork.image(kind, size: 16, color: color).draw(
        in: NSRect(x: x + 54, y: 201, width: 16, height: 16))
      text("32 / 16 px", x: x, y: 150, size: 17, color: .darkGray)
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
