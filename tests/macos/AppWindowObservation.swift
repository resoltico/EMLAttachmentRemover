import CoreGraphics
// Read-only observation of the exact owned app PID supplied by the Rust harness.
// No launch, termination, preferences, accessibility grants or other app output.
import Foundation

@main
struct AppWindowObservation {
  static func main() throws {
    guard CommandLine.arguments.count == 2,
      let pid = Int32(CommandLine.arguments[1]), pid > 0
    else {
      throw NSError(domain: "OwnedAppObservation", code: 1)
    }
    let deadline = Date().addingTimeInterval(20)
    while Date() < deadline {
      let windows =
        CGWindowListCopyWindowInfo(.optionOnScreenOnly, kCGNullWindowID) as? [[String: Any]] ?? []
      let matching = windows.compactMap { window -> [String: Any]? in
        guard let owner = window[kCGWindowOwnerPID as String] as? NSNumber,
          owner.int32Value == pid,
          let layer = window[kCGWindowLayer as String] as? NSNumber, layer.intValue == 0,
          let bounds = window[kCGWindowBounds as String] as? [String: Any],
          let width = bounds["Width"] as? NSNumber,
          let height = bounds["Height"] as? NSNumber,
          width.doubleValue >= 760, height.doubleValue >= 520
        else { return nil }
        return ["owner_pid": pid, "width": width, "height": height]
      }
      if !matching.isEmpty {
        let output: [String: Any] = [
          "windows": matching,
          "scope":
            "actual owned app startup/window presence; no paint, GUI interaction or accessibility qualification",
        ]
        FileHandle.standardOutput.write(
          try JSONSerialization.data(withJSONObject: output, options: [.sortedKeys]))
        FileHandle.standardOutput.write(Data([10]))
        return
      }
      Thread.sleep(forTimeInterval: 0.02)
    }
    throw NSError(
      domain: "OwnedAppObservation", code: 2,
      userInfo: [NSLocalizedDescriptionKey: "No matching visible main window before deadline"])
  }
}
