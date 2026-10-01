import AppKit

@MainActor
final class ReportLayout {
  let window: NSWindow
  let version: String
  var content = NSStackView()
  var footerHost = NSStackView()
  var headline: NSTextField?
  init(window: NSWindow, version: String) {
    self.window = window
    self.version = version
  }
  func makeContent() {
    trace("make-content")
    let outer = NSView(
      frame: window.contentView?.bounds ?? NSRect(x: 0, y: 0, width: 640, height: 590))
    window.contentView = outer
    content = NSStackView()
    content.orientation = .vertical
    content.alignment = .leading
    content.spacing = 16
    content.translatesAutoresizingMaskIntoConstraints = false
    let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 640, height: 450))
    scroll.hasVerticalScroller = true
    scroll.autohidesScrollers = true
    scroll.drawsBackground = false
    scroll.translatesAutoresizingMaskIntoConstraints = false
    let document = FlippedView(frame: NSRect(x: 0, y: 0, width: 640, height: 450))
    document.translatesAutoresizingMaskIntoConstraints = false
    document.addSubview(content)
    scroll.documentView = document
    outer.addSubview(scroll)
    footerHost = NSStackView()
    footerHost.orientation = .horizontal
    footerHost.translatesAutoresizingMaskIntoConstraints = false
    outer.addSubview(footerHost)
    NSLayoutConstraint.activate([
      footerHost.leadingAnchor.constraint(equalTo: outer.leadingAnchor, constant: 28),
      footerHost.trailingAnchor.constraint(equalTo: outer.trailingAnchor, constant: -28),
      footerHost.bottomAnchor.constraint(equalTo: outer.bottomAnchor, constant: -20),
      scroll.leadingAnchor.constraint(equalTo: outer.leadingAnchor),
      scroll.trailingAnchor.constraint(equalTo: outer.trailingAnchor),
      scroll.topAnchor.constraint(equalTo: outer.topAnchor),
      scroll.bottomAnchor.constraint(equalTo: footerHost.topAnchor, constant: -16),
      document.widthAnchor.constraint(equalTo: scroll.contentView.widthAnchor),
      content.leadingAnchor.constraint(equalTo: document.leadingAnchor, constant: 28),
      content.trailingAnchor.constraint(equalTo: document.trailingAnchor, constant: -28),
      content.topAnchor.constraint(equalTo: document.topAnchor, constant: 24),
      content.bottomAnchor.constraint(equalTo: document.bottomAnchor, constant: -12),
    ])
    let identity = NSStackView()
    identity.orientation = .horizontal
    identity.spacing = 14
    let icon = NSImageView(image: Artwork.image(.identity))
    icon.setAccessibilityElement(false)
    icon.widthAnchor.constraint(equalToConstant: 40).isActive = true
    icon.heightAnchor.constraint(equalToConstant: 40).isActive = true
    let names = NSStackView(views: [
      UIControls.label("EML Attachment Remover", size: 18, weight: .semibold),
      UIControls.label("Version \(version)", color: .secondaryLabelColor),
    ])
    names.orientation = .vertical
    names.alignment = .leading
    names.spacing = 4
    identity.addArrangedSubview(icon)
    identity.addArrangedSubview(names)
    add(identity)
    let separator = NSBox(frame: NSRect(x: 0, y: 0, width: 584, height: 64))
    separator.boxType = .separator
    add(separator)
  }
  func add(_ view: NSView) {
    content.addArrangedSubview(view)
    view.widthAnchor.constraint(equalTo: content.widthAnchor).isActive = true
  }
  func heading(_ title: String, subtitle: String, artwork: Artwork.Kind, color: NSColor) {
    let row = NSStackView()
    row.orientation = .horizontal
    row.alignment = .top
    row.spacing = 12
    let image = NSImageView(image: Artwork.image(artwork, color: color))
    image.setAccessibilityElement(false)
    image.widthAnchor.constraint(equalToConstant: 28).isActive = true
    image.heightAnchor.constraint(equalToConstant: 28).isActive = true
    let titleField = UIControls.label(title, size: 24, weight: .semibold)
    headline = titleField
    let words = NSStackView(views: [
      titleField, UIControls.label(subtitle, color: .secondaryLabelColor),
    ])
    words.orientation = .vertical
    words.alignment = .leading
    words.spacing = 6
    row.addArrangedSubview(image)
    row.addArrangedSubview(words)
    add(row)
  }
  func resizeToContent() {
    window.contentView?.layoutSubtreeIfNeeded()
    let maximum = (window.screen ?? NSScreen.main)?.visibleFrame.height ?? 900
    let height = min(maximum - 100, max(460, content.fittingSize.height + 85))
    guard let view = window.contentView else { return }
    window.setContentSize(NSSize(width: view.bounds.width, height: height))
  }
}
