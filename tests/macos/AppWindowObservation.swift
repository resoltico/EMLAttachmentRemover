import CoreGraphics
// Read-only observation of the exact owned app PID supplied by the Rust harness.
// No launch, termination, preferences, accessibility grants or other app output.
import Foundation

@main
struct AppWindowObservation {
  static func main() throws {
    guard (2...3).contains(CommandLine.arguments.count),
      let pid = Int32(CommandLine.arguments[1]), pid > 0
    else {
      throw NSError(domain: "OwnedAppObservation", code: 1)
    }
    let diagnosticURL =
      CommandLine.arguments.count == 3 ? URL(fileURLWithPath: CommandLine.arguments[2]) : nil
    let started = ProcessInfo.processInfo.systemUptime
    var polls = 0
    var lastOwnedOnScreen: [[String: Any]] = []
    let deadline = Date().addingTimeInterval(20)
    while Date() < deadline {
      polls += 1
      let windows =
        CGWindowListCopyWindowInfo(.optionOnScreenOnly, kCGNullWindowID) as? [[String: Any]] ?? []
      lastOwnedOnScreen = observations(windows, pid: pid)
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
    if let diagnosticURL {
      let all = CGWindowListCopyWindowInfo(.optionAll, kCGNullWindowID) as? [[String: Any]] ?? []
      let diagnostic: [String: Any] = [
        "owner_pid": pid,
        "poll_count": polls,
        "elapsed_seconds": ProcessInfo.processInfo.systemUptime - started,
        "last_on_screen_owned_windows": lastOwnedOnScreen,
        "owned_windows_at_failure": observations(all, pid: pid),
        "window_observation_limit": 32,
        "scope": "Failure-only owned-PID window diagnostics; original acceptance is unchanged.",
      ]
      do {
        try JSONSerialization.data(withJSONObject: diagnostic, options: [.sortedKeys])
          .write(to: diagnosticURL, options: [.atomic])
      } catch {
        FileHandle.standardError.write(
          Data("Could not retain owned-window diagnostics: \(error)\n".utf8))
      }
    }
    throw NSError(
      domain: "OwnedAppObservation", code: 2,
      userInfo: [NSLocalizedDescriptionKey: "No matching visible main window before deadline"])
  }

  static func observations(_ windows: [[String: Any]], pid: Int32) -> [[String: Any]] {
    windows.filter {
      ($0[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value == pid
    }.prefix(32).map { window in
      let bounds = window[kCGWindowBounds as String] as? [String: Any] ?? [:]
      var coordinates: [String: Any] = [:]
      for key in ["X", "Y", "Width", "Height"] {
        if let number = bounds[key] as? NSNumber, number.doubleValue.isFinite {
          coordinates[key] = number
        } else {
          coordinates[key] = NSNull()
        }
      }
      return [
        "owner_pid": pid,
        "window_id": window[kCGWindowNumber as String] ?? NSNull(),
        "layer": window[kCGWindowLayer as String] ?? NSNull(),
        "is_on_screen": window[kCGWindowIsOnscreen as String] ?? NSNull(),
        "bounds": coordinates,
      ]
    }
  }
}
