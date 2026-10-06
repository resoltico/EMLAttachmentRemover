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
    let disclosure = NSButton(title: "", target: self, action: #selector(toggleDetails))
    disclosure.bezelStyle = .disclosure
    disclosure.setButtonType(.onOff)
    disclosure.state = .off
    detailsButton = disclosure
    disclosure.setAccessibilityLabel("Processing details" + counts)
    disclosure.setAccessibilityExpanded(false)
    let label = UIControls.label("Details" + counts)
    label.isSelectable = false
    label.setAccessibilityElement(false)
    label.addGestureRecognizer(
      NSClickGestureRecognizer(target: self, action: #selector(toggleDetails)))
    let disclosureRow = NSStackView(views: [disclosure, label])
    disclosureRow.orientation = .horizontal
    disclosureRow.alignment = .centerY
    disclosureRow.spacing = 4
    disclosureRow.edgeInsets = NSEdgeInsets(top: 3, left: 0, bottom: 3, right: 0)
    layout.add(disclosureRow)
    let scroll = NSScrollView()
    let text = NSTextView()
    text.isEditable = false
    text.isSelectable = true
    text.textContainerInset = NSSize(width: 8, height: 8)
    text.font = .systemFont(ofSize: 13)
    let limit = 1_000_000
    text.string =
      detailText.count > limit
      ? String(detailText.prefix(limit))
        + "\n[More information is available through Copy technical report.]" : detailText
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
    let copy = UIControls.button(
      receipt == nil ? "Copy technical details" : "Copy technical report",
      action: #selector(copyContents), target: self)
    row.addArrangedSubview(copy)
    let spacer = NSView()
    spacer.setContentHuggingPriority(.defaultLow, for: .horizontal)
    row.addArrangedSubview(spacer)
    let done = UIControls.button("Done", action: #selector(done), target: self)
    done.keyEquivalent = "\r"
    row.addArrangedSubview(done)
    layout.window.defaultButtonCell = done.cell as? NSButtonCell
    layout.layoutContent()
  }
  @objc private func toggleDetails() {
    guard let detailsView else { return }
    detailsView.isHidden.toggle()
    detailsButton?.state = detailsView.isHidden ? .off : .on
    detailsButton?.setAccessibilityExpanded(!detailsView.isHidden)
    layout.layoutContent()
    if !detailsView.isHidden { _ = detailsView.scrollToVisible(detailsView.bounds) }
  }
  @objc private func copyContents() {
    NSPasteboard.general.clearContents()
    NSPasteboard.general.setString(rawReport, forType: .string)
  }
  @objc private func done() {
    trace("done event=\(NSApp.currentEvent?.type.rawValue ?? 0)")
    layout.window.close()
  }
}
