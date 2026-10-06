import Foundation

@main
struct LaunchChecks {
  @MainActor
  static func main() throws {
    let manyPaths = Array(repeating: "example.eml", count: 4096)
    let framed = try RequestTransport.frame(manyPaths)
    let length = framed.prefix(4).reduce(UInt32(0)) { ($0 << 8) | UInt32($1) }
    precondition(Int(length) == framed.count - 4)
    let object = try JSONSerialization.jsonObject(with: framed.dropFirst(4)) as? [String: Any]
    precondition((object?["paths"] as? [String])?.count == 4096)
    let invalid = ProcessingRun()
    do {
      try invalid.start(
        paths: ["bad\0path"], version: "4.0.0",
        updated: { _ in preconditionFailure("rejected launch produced an update") },
        completed: { _, _, _, _ in preconditionFailure("rejected launch ran a processor") })
      preconditionFailure("Invalid request path was not rejected")
    } catch {
      precondition(!invalid.isRunning)
    }
    let process = Process()
    process.executableURL = URL(fileURLWithPath: "/usr/bin/true")
    process.arguments = Array(repeating: String(repeating: "x", count: 253), count: 4096)
    process.environment = [:]
    do {
      try process.run()
      preconditionFailure("oversized selection unexpectedly launched")
    } catch {
      let message = launchExplanation(error)
      precondition(message.contains("Select fewer files"))
      precondition(!message.contains("CPython"))
    }
    let other = NSError(domain: "EMLRuntime", code: 6)
    precondition(launchExplanation(other).contains("runtime configuration"))
    precondition(
      launchExplanation(NSError(domain: "EMLBundledRuntime", code: 1)).contains("fresh copy"))
    print("Native launch failure checks passed.")
  }
}
