import AppKit

@main
struct CreateIcon {
    @MainActor static func main() throws {
        let root = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        for size in [16, 32, 128, 256, 512] {
            for scale in [1, 2] {
                let image = Artwork.image(.identity, size: CGFloat(size * scale), appIcon: true)
                guard let bitmap = image.representations.first as? NSBitmapImageRep,
                      let png = bitmap.representation(using: .png, properties: [:]) else {
                    throw NSError(domain: "EMLArtwork", code: 1)
                }
                try png.write(to: root.appendingPathComponent("icon_\(size)x\(size)\(scale == 2 ? "@2x" : "").png"))
            }
        }
    }
}
