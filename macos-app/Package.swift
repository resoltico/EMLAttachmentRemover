// swift-tools-version: 6.2
import PackageDescription

let package = Package(
  name: "EMLAttachmentRemover",
  platforms: [.macOS(.v15)],
  products: [.library(name: "RunModel", targets: ["RunModel"])],
  targets: [
    .target(name: "RunModel"),
    .testTarget(name: "RunModelTests", dependencies: ["RunModel"]),
  ],
  swiftLanguageModes: [.v6]
)
