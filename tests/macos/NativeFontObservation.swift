import AppKit
import CoreText
import ImageIO

// Independent CoreText oracle. This tool creates no windows or preference domains.
@main
struct NativeFontObservation {
  static func main() throws {
    let output = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
    try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
    var records: [[String: Any]] = []
    let samples = ["x", String(repeating: "x", count: 100), "AVATAR Toffee office"]
    for family in ["system", "Helvetica"] {
      for weight in family == "system" ? [400, 600] : [400] {
        for size in [13.0, 26.0, 40.0] {
          let font =
            family == "system"
            ? NSFont.systemFont(ofSize: size)
            : NSFont(name: family, size: size)!
          let base = font as CTFont
          // Match CSS/OpenType weight, rather than AppKit's named semibold (590).
          // Preserve the optical instance selected independently by CoreText.
          var variations = CTFontCopyVariation(base) as? [NSNumber: NSNumber] ?? [:]
          variations[NSNumber(value: 0x7767_6874)] = NSNumber(value: weight)  // 'wght'
          let descriptor = CTFontDescriptorCreateWithAttributes(
            [kCTFontVariationAttribute: variations] as CFDictionary)
          let core =
            family == "system"
            ? CTFontCreateCopyWithAttributes(base, size, nil, descriptor) : base
          for sample in samples {
            let attributed = NSAttributedString(
              string: sample, attributes: [.font: core, .foregroundColor: NSColor.black])
            let line = CTLineCreateWithAttributedString(attributed)
            let advance = CTLineGetTypographicBounds(line, nil, nil, nil)
            var images: [[String: Any]] = []
            for scale in [1, 2] {
              let width = Int(ceil(advance + 16)) * scale
              let height = Int(ceil(CTFontGetAscent(core) + CTFontGetDescent(core) + 16)) * scale
              let context = CGContext(
                data: nil, width: width, height: height, bitsPerComponent: 8,
                bytesPerRow: width * 4, space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
              context.setFillColor(CGColor(gray: 1, alpha: 1))
              context.fill(CGRect(x: 0, y: 0, width: width, height: height))
              context.scaleBy(x: CGFloat(scale), y: CGFloat(scale))
              context.textPosition = CGPoint(x: 8, y: 8 + CTFontGetDescent(core))
              CTLineDraw(line, context)
              let bytes = context.data!.assumingMemoryBound(to: UInt8.self)
              var xs: [Int] = []
              var ys: [Int] = []
              for y in 0..<height {
                for x in 0..<width {
                  let i = (y * width + x) * 4
                  if Int(bytes[i]) + Int(bytes[i + 1]) + Int(bytes[i + 2]) < 765 {
                    xs.append(x)
                    ys.append(y)
                  }
                }
              }
              let bounds = [xs.max()! - xs.min()! + 1, ys.max()! - ys.min()! + 1]
              if family == "system" && weight == 400 && sample == "x" {
                let url = output.appendingPathComponent("native-\(Int(size))-\(scale)x.png")
                let destination = CGImageDestinationCreateWithURL(
                  url as CFURL, "public.png" as CFString, 1, nil)!
                CGImageDestinationAddImage(destination, context.makeImage()!, nil)
                guard CGImageDestinationFinalize(destination) else {
                  fatalError("PNG write failed")
                }
              }
              images.append(["scale": scale, "ink": bounds])
            }
            records.append([
              "family": family, "weight": weight, "size": size, "sample": sample,
              "x_height": CTFontGetXHeight(core), "cap_height": CTFontGetCapHeight(core),
              "ascent": CTFontGetAscent(core), "descent": CTFontGetDescent(core),
              "advance": advance, "raster": images,
            ])
          }
        }
      }
    }
    FileHandle.standardOutput.write(
      try JSONSerialization.data(withJSONObject: records, options: [.sortedKeys]))
  }
}
