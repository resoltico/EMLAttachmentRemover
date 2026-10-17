// Public read-only graphics metadata for owned hosted diagnostics; no renderer control.
import CoreGraphics
import Foundation
import Metal

private func objectValue(_ object: NSObject, _ selector: Selector) -> AnyObject? {
  guard object.responds(to: selector) else { return nil }
  return object.perform(selector)?.takeUnretainedValue()
}
private func architecture(_ device: any MTLDevice) -> [String: Any] {
  // The public API is annotated nonnull. Observe through its public Objective-C
  // getter so a broken implementation cannot become a Swift force-unwrap trap.
  // This is not Metal's private targetDeviceArchitecture property.
  guard let object = device as? NSObject,
    let value = objectValue(object, #selector(getter: MTLDevice.architecture)) as? MTLArchitecture,
    let name = objectValue(value, #selector(getter: MTLArchitecture.name)) as? String,
    name.utf8.count <= 512
  else { return ["state": "unobserved"] }
  return ["state": "observed", "public_architecture_name": name]
}
@main private struct MetalDeviceObservation {
  static func main() throws {
    let devices = MTLCopyAllDevices()
    guard devices.count <= 16 else { throw CocoaError(.coderInvalidValue) }
    let families: [(String, MTLGPUFamily)] = [
      ("apple1", .apple1), ("apple3", .apple3), ("apple5", .apple5),
      ("apple7", .apple7), ("metal3", .metal3),
    ]
    let rows: [[String: Any]] = try devices.map { device in
      guard let object = device as? NSObject,
        let name = objectValue(object, #selector(getter: MTLDevice.name)) as? String,
        name.utf8.count <= 512
      else { throw CocoaError(.coderInvalidValue) }
      return [
        "name": name, "registry_id": device.registryID,
        "has_unified_memory": device.hasUnifiedMemory,
        "families": Dictionary(
          uniqueKeysWithValues: families.map { ($0.0, device.supportsFamily($0.1)) }),
        "architecture": architecture(device),
      ]
    }
    let bytes = try JSONSerialization.data(
      withJSONObject: [
        "scope":
          "public Metal enumeration only; no default-device query, shader/archive load, alternate GPU choice or private architecture inference",
        "device_count": devices.count, "devices": rows,
      ], options: [.sortedKeys])
    try FileHandle.standardOutput.write(contentsOf: bytes)
    try FileHandle.standardOutput.write(contentsOf: Data([10]))
  }
}
