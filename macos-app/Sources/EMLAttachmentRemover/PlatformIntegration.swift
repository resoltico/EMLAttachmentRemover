import AppKit
import RunModel

/// Only the required native Services, exact reveal, termination and batched file
/// ingress live here. All presentation and window construction remain SwiftUI.
@MainActor
final class PlatformIntegration: NSObject, NSApplicationDelegate {
  var showMainWindow: (() -> Void)?
  private let store = ApplicationStore.shared

  override init() {
    super.init()
    store.onQuitDecision = { allowed in
      NSApplication.shared.reply(toApplicationShouldTerminate: allowed)
    }
  }

  func applicationDidFinishLaunching(_ notification: Notification) {
    NSApplication.shared.servicesProvider = self
    NSUpdateDynamicServices()
  }

  func application(_ application: NSApplication, open urls: [URL]) {
    guard !urls.isEmpty else { return }
    let admitted = store.admit(urls)
    if admitted || store.alert != nil { showMainWindow?() }
  }

  @objc(createEMLCopies:userData:error:)
  func createEMLCopies(
    _ pasteboard: NSPasteboard, userData: String?,
    error: AutoreleasingUnsafeMutablePointer<NSString?>?
  ) {
    error?.pointee = nil
    guard
      let objects = pasteboard.readObjects(
        forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [NSURL],
      !objects.isEmpty
    else {
      error?.pointee = "Select local EML files or folders in Finder first."
      return
    }
    guard store.admit(objects.map { $0 as URL }) else {
      error?.pointee =
        store.acceptsRequests
        ? "The selection could not be queued. Review the app's message and try again."
        : "Answer the app's quit confirmation or reopen it before selecting more files."
      showMainWindow?()
      return
    }
    showMainWindow?()
  }

  func applicationShouldTerminate(_ application: NSApplication) -> NSApplication.TerminateReply {
    switch store.requestQuit() {
    case .now: return .terminateNow
    case .later:
      showMainWindow?()
      return .terminateLater
    case .cancel:
      showMainWindow?()
      return .terminateCancel
    }
  }

  func applicationShouldTerminateAfterLastWindowClosed(_ application: NSApplication) -> Bool {
    // A single SwiftUI Window otherwise requests termination when it closes.
    // Keep the session/owned helper alive; SwiftUI supplies Dock reopening.
    false
  }

  func reveal(_ native: NativePath) {
    Task {
      let url: URL? = await blockingIO {
        do {
          let address = try native.fileURL()
          guard try address.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile == true
          else { return nil }
          return address
        } catch { return nil }
      }
      guard store.acceptsRequests else { return }
      guard let url else {
        store.showMessage(
          "That file address is unavailable. The earlier copy receipt has not been reverified.")
        return
      }
      // Best-effort navigation from an earlier qualified receipt, not a fresh
      // identity/content check. Never fall back to merely opening its folder.
      NSWorkspace.shared.activateFileViewerSelecting([url])
    }
  }
}
