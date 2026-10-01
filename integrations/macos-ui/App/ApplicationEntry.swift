import AppKit

@main
struct ApplicationEntry {
  @MainActor static func main() {
    let app = NSApplication.shared
    app.setActivationPolicy(.regular)
    let delegate = ApplicationDelegate()
    app.delegate = delegate
    withExtendedLifetime(delegate) { app.run() }
  }
}
