import AppKit

@MainActor
final class ReportDetails: NSObject {
  let rawReport: String
  let detailText: String
  let receipt: UIReceipt?
  let layout: ReportLayout
  var detailsView: NSScrollView?
  var detailsButton: NSButton?
  init(raw: String, text: String, receipt: UIReceipt?, layout: ReportLayout) {
    self.rawReport = raw
    self.detailText = text
    self.receipt = receipt
    self.layout = layout
    super.init()
    makeDetails()
    footer()
  }
  func makeDetails() {
    let errors = receipt?.errorCount ?? 0
    let warnings = receipt?.warningCount ?? 0
    let counts =
      receipt == nil
      ? ""
      : " · \(errors) error\(errors == 1 ? "" : "s") · \(warnings) warning\(warnings == 1 ? "" : "s")"
    let disclosure = UIControls.button(
      "▸ Details" + counts, action: #selector(toggleDetails), target: self)
    detailsButton = disclosure
    disclosure.bezelStyle = .inline
    let disclosureRow = NSStackView(views: [disclosure])
    disclosureRow.orientation = .horizontal
    disclosureRow.alignment = .leading
    layout.add(disclosureRow)
    let scroll = NSScrollView()
    let text = NSTextView()
    text.isEditable = false
    text.isSelectable = true
    text.font = .monospacedSystemFont(ofSize: 11, weight: .regular)
    let limit = 1_000_000
    text.string =
      detailText.count > limit
      ? String(detailText.prefix(limit))
        + "\n[Display truncated. Copy details includes the full report.]" : detailText
    text.textContainer?.widthTracksTextView = true
    text.autoresizingMask = [.width]
    text.setAccessibilityLabel("Processing report details")
    scroll.documentView = text
    scroll.hasVerticalScroller = true
    scroll.borderType = .bezelBorder
    scroll.heightAnchor.constraint(equalToConstant: 140).isActive = true
    scroll.isHidden = true
    detailsView = scroll
    layout.add(scroll)
  }
  func footer() {
    let row = layout.footerHost
    let copy = UIControls.button("Copy details", action: #selector(copyDetails), target: self)
    row.addArrangedSubview(copy)
    let spacer = NSView()
    spacer.setContentHuggingPriority(.defaultLow, for: .horizontal)
    row.addArrangedSubview(spacer)
    let done = UIControls.button("Done", action: #selector(done), target: self)
    done.keyEquivalent = "\r"
    row.addArrangedSubview(done)
    layout.window.defaultButtonCell = done.cell as? NSButtonCell
    layout.resizeToContent()
  }
  @objc private func toggleDetails() {
    guard let detailsView else { return }
    detailsView.isHidden.toggle()
    if let title = detailsButton?.title {
      detailsButton?.title = (detailsView.isHidden ? "▸" : "▾") + String(title.dropFirst())
    }
    layout.resizeToContent()
  }
  @objc private func copyDetails() {
    NSPasteboard.general.clearContents()
    NSPasteboard.general.setString(rawReport, forType: .string)
  }
  @objc private func done() {
    trace("done event=\(NSApp.currentEvent?.type.rawValue ?? 0)")
    layout.window.close()
  }
}
