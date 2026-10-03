import Foundation

@main
struct LaunchChecks {
  @MainActor
  static func main() throws {
    let countLimited = ProcessingRun()
    do {
      try countLimited.start(
        paths: Array(repeating: "example.eml", count: 4096), version: "4.0.0",
        preparing: { preconditionFailure("rejected launch prepared a report") },
        completed: { _, _, _, _ in preconditionFailure("rejected launch ran a processor") })
      preconditionFailure("Foundation argument-count overflow was not rejected")
    } catch {
      precondition(!countLimited.isRunning)
      precondition(launchExplanation(error).contains("Select fewer files"))
    }
    try validateProcessArguments(Array(repeating: "x", count: 4096))
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
    print("Native launch failure checks passed.")
  }
}
