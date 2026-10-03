import AppKit

@main
struct LayoutTests {
  @MainActor static func main() {
    NSApplication.shared.setActivationPolicy(.prohibited)
    let window = NSWindow(
      contentRect: NSRect(x: 0, y: 0, width: 560, height: 590),
      styleMask: [.titled, .resizable], backing: .buffered, defer: false)
    let layout = ReportLayout(window: window)
    layout.makeContent()
    let receipt = sampleReceipt()
    layout.heading(
      "Your copies are ready", subtitle: receipt.subtitle, artwork: .success, color: .systemGreen)
    let items = ReportItems(receipt: receipt, layout: layout, selected: 0)
    let details = ReportDetails(raw: "{}", text: "Public sample", receipt: receipt, layout: layout)
    layout.resizeToContent()
    window.contentView?.layoutSubtreeIfNeeded()
    guard let table = items.table, let scroll = table.enclosingScrollView else {
      preconditionFailure("Batch file table was not created")
    }
    FileHandle.standardError.write(
      Data("table width=\(table.frame.width), viewport width=\(scroll.contentSize.width)\n".utf8))
    precondition(
      table.frame.width <= scroll.contentSize.width + 1, "Narrow table requires sideways scrolling")
    let filenameWidth = table.tableColumns[0].width
    let resultWidth = table.tableColumns[1].width
    guard let result = table.view(atColumn: 1, row: 1, makeIfNecessary: true) as? NSTextField,
      let font = result.font
    else { preconditionFailure("Result text was not rendered") }
    let textWidth = (receipt.report.items[1].label as NSString).size(withAttributes: [.font: font])
      .width
    precondition(result.frame.width >= textWidth, "Full result does not fit")
    precondition(
      result.toolTip == receipt.report.items[1].label, "Result tooltip does not explain the result")
    window.setContentSize(NSSize(width: 1000, height: 590))
    window.contentView?.layoutSubtreeIfNeeded()
    precondition(
      table.tableColumns[0].width > filenameWidth, "Wider window gives filenames no extra space")
    precondition(
      abs(table.tableColumns[1].width - resultWidth) < 1, "Result column absorbs the extra width")
    let source = Address(
      display: "/Public/sample.eml", text: "/Public/sample.eml", nativeBase64: nil)
    for message in [
      "two inputs target one destination", "destination parent changed after binding",
      "destination is not a regular file", "destination already exists",
    ] {
      let conflict = Item(
        index: 0, status: "failed", sourceRequest: source, destinationRequest: source,
        publication: nil, error: Diagnostic(code: "OUTPUT_CONFLICT", message: message), warnings: []
      )
      items.showItem(conflict)
      let texts = items.card.arrangedSubviews.compactMap { ($0 as? NSTextField)?.stringValue }
      precondition(texts.contains("Copy not created"))
      precondition(!texts.contains("An existing copy differs"))
    }
    withExtendedLifetime((items, details)) {}
    print("Native batch layout checks passed.")
  }

  static func sampleReceipt() -> UIReceipt {
    let items = ["created", "existing_verified"].enumerated().map { index, status in
      let address = Address(
        display: "/Public/sample-\(index).eml", text: "/Public/sample-\(index).eml",
        nativeBase64: nil)
      return Item(
        index: index, status: status, sourceRequest: address, destinationRequest: address,
        publication: Publication(addressVerified: true, finalAddress: address), error: nil,
        warnings: [])
    }
    return UIReceipt(
      report: Report(
        version: "4.0.0", ok: true, interrupted: false, batchError: nil,
        summary: ["created": 1, "existing_verified": 1], items: items), processStatus: 0,
      details: [])
  }
}
