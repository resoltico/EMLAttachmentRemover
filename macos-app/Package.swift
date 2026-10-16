// swift-tools-version: 6.2
import PackageDescription

let package = Package(
  name: "EMLAttachmentRemover",
  platforms: [.macOS(.v15)],
  products: [
    .library(name: "RunModel", targets: ["RunModel"]),
    .executable(name: "EMLAttachmentRemover", targets: ["EMLAttachmentRemover"]),
  ],
  targets: [
    .target(name: "RunModel"),
    .executableTarget(name: "EMLAttachmentRemover", dependencies: ["RunModel"]),
    .testTarget(name: "RunModelTests", dependencies: ["RunModel", "EMLAttachmentRemover"]),
  ],
  swiftLanguageModes: [.v6]
)
