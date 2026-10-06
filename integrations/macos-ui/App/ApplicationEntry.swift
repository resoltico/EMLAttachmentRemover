import AppKit
import Darwin

@main
struct ApplicationEntry {
  @MainActor static func main() {
    // A closed processor input is a reportable transport failure, not an app-killing signal.
    signal(SIGPIPE, SIG_IGN)
    let app = NSApplication.shared
    app.setActivationPolicy(.regular)
    let delegate = ApplicationDelegate()
    app.delegate = delegate
    withExtendedLifetime(delegate) { app.run() }
  }
}
