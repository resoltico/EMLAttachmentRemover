import AppKit

// Fresh hosted-runner diagnostic only. No product, preference or input-file APIs.
@MainActor
final class NativePanelDiagnostic: NSObject, NSApplicationDelegate {
  private var window: NSWindow?
  private var panel: NSOpenPanel?
  private var completed = false
  private var observedVisible = false
  private var cancellationRequested = false
  private(set) var exitCode: Int32 = 2

  private func record(_ phase: String) {
    FileHandle.standardOutput.write(Data("Native AppKit panel diagnostic: \(phase)\n".utf8))
  }

  func applicationDidFinishLaunching(_ notification: Notification) {
    let parent = NSWindow(
      contentRect: NSRect(x: 100, y: 100, width: 400, height: 240),
      styleMask: [.titled, .closable], backing: .buffered, defer: false)
    parent.isReleasedWhenClosed = false
    parent.title = "Owned native panel diagnostic"
    window = parent
    parent.makeKeyAndOrderFront(nil)
    NSApp.activate(ignoringOtherApps: true)
    DispatchQueue.main.async { [self] in
      record("constructor entry")
      let ownedPanel = NSOpenPanel()
      panel = ownedPanel
      record("constructor returned")
      ownedPanel.beginSheetModal(for: parent) { [self] response in
        completed = true
        record("callback response \(response.rawValue)")
        exitCode = observedVisible && cancellationRequested && response == .cancel ? 0 : 3
        ownedPanel.orderOut(nil)
        panel = nil
        parent.close()
        window = nil
        NSApp.terminate(nil)
      }
      record("presentation returned")
      DispatchQueue.main.asyncAfter(deadline: .now() + 2) { [self] in
        observedVisible = ownedPanel.isVisible
        record("cancel retained panel; visible=\(observedVisible)")
        cancellationRequested = true
        ownedPanel.cancel(nil)
        DispatchQueue.main.asyncAfter(deadline: .now() + 5) { [self] in
          if !completed {
            record("callback deadline exceeded")
            exit(2)
          }
        }
      }
    }
  }
}

@main
struct DiagnosticMain {
  @MainActor
  static func main() {
    let application = NSApplication.shared
    let delegate = NativePanelDiagnostic()
    application.setActivationPolicy(.regular)
    application.delegate = delegate
    withExtendedLifetime(delegate) { application.run() }
    exit(delegate.exitCode)
  }
}
