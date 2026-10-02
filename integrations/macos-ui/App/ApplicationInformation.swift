import AppKit

@MainActor
final class ApplicationInformation {
  private var licenseWindow: NSWindow?

  func showAbout() {
    NSApp.orderFrontStandardAboutPanel(options: [
      .credits: NSAttributedString(
        string: "MIT License\nRead the full terms in the app’s License menu.")
    ])
  }

  func showLicense() {
    if let licenseWindow {
      licenseWindow.makeKeyAndOrderFront(nil)
      return
    }
    guard let url = Bundle.main.url(forResource: "LICENSE", withExtension: nil),
      let license = try? String(contentsOf: url, encoding: .utf8)
    else {
      let alert = NSAlert()
      alert.messageText = "The license document is unavailable"
      alert.informativeText =
        "The application installation is incomplete. Download a complete replacement."
      alert.runModal()
      return
    }
    let window = NSWindow(
      contentRect: NSRect(x: 0, y: 0, width: 580, height: 480),
      styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false
    )
    window.title = "MIT License — EML Attachment Remover"
    window.minSize = NSSize(width: 420, height: 300)
    window.isReleasedWhenClosed = false
    let scroll = NSScrollView(frame: window.contentView?.bounds ?? .zero)
    scroll.autoresizingMask = [.width, .height]
    scroll.hasVerticalScroller = true
    let text = NSTextView(frame: scroll.bounds)
    text.isEditable = false
    text.isSelectable = true
    text.isVerticallyResizable = true
    text.isHorizontallyResizable = false
    text.autoresizingMask = [.width]
    text.textContainer?.widthTracksTextView = true
    text.textContainerInset = NSSize(width: 20, height: 20)
    text.font = .systemFont(ofSize: 13)
    text.string = license.components(separatedBy: "\n\n")
      .map { $0.components(separatedBy: "\n").joined(separator: " ") }
      .joined(separator: "\n\n")
    text.setAccessibilityLabel("Full MIT license")
    scroll.documentView = text
    window.contentView?.addSubview(scroll)
    window.center()
    licenseWindow = window
    window.makeKeyAndOrderFront(nil)
  }
}
