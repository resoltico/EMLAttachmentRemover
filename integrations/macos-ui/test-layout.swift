import AppKit

@main
struct LayoutTests {
  @MainActor static func main() {
    NSApplication.shared.setActivationPolicy(.prohibited)
    FileInputChecks.checkFileDrop()
    FileInputChecks.checkFileService()
    checkProgressStream()
    checkDiagnosticDetails()
    checkLargeResults()
    checkWarningPresentation()
    let window = NSWindow(
      contentRect: NSRect(x: 0, y: 0, width: 560, height: 590),
      styleMask: [.titled, .resizable], backing: .buffered, defer: false)
    let layout = ReportLayout(window: window)
    let initialFrame = window.frame
    layout.makeContent()
    let receipt = sampleReceipt()
    layout.heading(
      "Your copies are ready", subtitle: receipt.subtitle, artwork: .success, tone: .success)
    let items = ReportItems(receipt: receipt, layout: layout, selected: 0)
    let details = ReportDetails(raw: "{}", text: "Public sample", receipt: receipt, layout: layout)
    layout.layoutContent()
    window.contentView?.layoutSubtreeIfNeeded()
    guard let table = items.table, let scroll = table.enclosingScrollView else {
      preconditionFailure("Batch file table was not created")
    }
    FileHandle.standardError.write(
      Data("table width=\(table.frame.width), viewport width=\(scroll.contentSize.width)\n".utf8))
    precondition(
      table.frame.width <= scroll.contentSize.width + 1, "Narrow table requires sideways scrolling")
    precondition(window.frame == initialFrame, "Report presentation resized the window")
    checkDisclosure(details, frame: initialFrame)
    let filenameWidth = table.tableColumns[0].width
    let resultWidth = table.tableColumns[1].width
    guard let resultCell = table.view(atColumn: 1, row: 1, makeIfNecessary: true) as? NSStackView,
      let result = resultCell.arrangedSubviews.compactMap({ $0 as? NSTextField }).first,
      let font = result.font
    else { preconditionFailure("Result text was not rendered") }
    resultCell.layoutSubtreeIfNeeded()
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
    checkConflictLabels(items)
    let resizedFrame = window.frame
    details.detailsButton?.performClick(nil)
    precondition(window.frame == resizedFrame, "Details discarded the user's window size")
    layout.makeContent()
    layout.heading(
      "Remove attachments from email copies", subtitle: "Choose files to get started.",
      artwork: .identity, tone: .accent)
    layout.layoutContent()
    precondition(window.frame == resizedFrame, "Changing state discarded the user's window size")
    withExtendedLifetime((items, details)) {}
    print("Native batch layout checks passed.")
  }

  @MainActor static func checkWarningPresentation() {
    let address = Address(display: "source.eml", text: "source.eml", nativeBase64: nil)
    let warning = Diagnostic(
      code: "RELATED_REFERENCES_MAY_BE_UNRESOLVED", message: "raw technical warning")
    let item = Item(
      index: 0, status: "created", sourceRequest: address, destinationRequest: address,
      publication: Publication(addressVerified: true, finalAddress: address), error: nil,
      warnings: [warning])
    precondition(ResultPresentation.needsReview(item))
    guard let view = ResultPresentation.warnings(item) as? NSStackView,
      let label = view.arrangedSubviews.first as? NSTextField
    else { preconditionFailure("Warning guidance was not rendered") }
    precondition(label.stringValue.contains("embedded images or content may be missing"))
    precondition(!label.stringValue.contains(warning.message))
    precondition(
      !Diagnostic(code: "UNKNOWN", message: "private text").warningGuidance.contains("private text")
    )
  }

  @MainActor static func checkLargeResults() {
    let window = NSWindow(
      contentRect: NSRect(x: 0, y: 0, width: 640, height: 590),
      styleMask: [.titled, .resizable], backing: .buffered, defer: false)
    let layout = ReportLayout(window: window)
    layout.makeContent()
    let receipt = largeReceipt()
    let items = ReportItems(receipt: receipt, layout: layout, selected: 0)
    layout.layoutContent()
    guard let table = items.table, let scroll = table.enclosingScrollView,
      let filter = items.reviewOnlyButton
    else { preconditionFailure("Large report controls were not created") }
    let height = scroll.frame.height
    precondition(height > 140, "Large batch retained the fixed short list")
    window.setContentSize(NSSize(width: 640, height: 950))
    layout.layoutContent()
    precondition(scroll.frame.height > height, "Taller window did not grow the results list")
    let filteredFrame = window.frame
    filter.performClick(nil)
    precondition(window.frame == filteredFrame, "Filtering resized the window")
    precondition(table.numberOfRows == 2, "Review filter did not select incomplete outcomes")
    precondition(items.items.count == 20, "Filtering discarded source receipts")
    table.selectRowIndexes(IndexSet(integer: 1), byExtendingSelection: false)
    let texts = items.card.arrangedSubviews.compactMap { ($0 as? NSTextField)?.stringValue }
    precondition(texts.contains("sample-19.eml"), "Filtered keyboard selection used a wrong item")
    filter.performClick(nil)
    precondition(window.frame == filteredFrame, "Restoring all results resized the window")
    precondition(table.numberOfRows == 20, "All outcomes were not restored")
    precondition(table.selectedRow == 19, "Filter reset lost the selected file")
    withExtendedLifetime(items) {}
  }

  static func largeReceipt() -> UIReceipt {
    let statuses = Array(repeating: "created", count: 18) + ["failed", "not_run"]
    let items = statuses.enumerated().map { index, status in
      let address = Address(
        display: "/Public/sample-\(index).eml", text: "/Public/sample-\(index).eml",
        nativeBase64: nil)
      return Item(
        index: index, status: status, sourceRequest: address, destinationRequest: address,
        publication: status == "created"
          ? Publication(addressVerified: true, finalAddress: address) : nil,
        error: status == "created"
          ? nil : Diagnostic(code: "INPUT_ERROR", message: "public sample"),
        warnings: [])
    }
    return UIReceipt(
      report: Report(
        version: "4.0.0", ok: false, interrupted: false, batchError: nil,
        summary: ["created": 18, "failed": 1, "not_run": 1], items: items),
      processStatus: 9, details: [])
  }

  static func checkDiagnosticDetails() {
    let address = Address(display: "source.eml", text: "source.eml", nativeBase64: nil)
    let diagnostic = Diagnostic(code: "PARSE_ERROR", message: "raw internal diagnostic")
    let item = Item(
      index: 0, status: "failed", sourceRequest: address, destinationRequest: address,
      publication: nil, error: diagnostic, warnings: [])
    let receipt = UIReceipt(
      report: Report(
        version: "4.0.0", ok: false, interrupted: false, batchError: diagnostic,
        summary: ["failed": 1], items: [item]), processStatus: 5, details: [])
    let text = ReportText.details(receipt, version: "4.0.0", build: "17")
    precondition(text.contains("Problem: " + diagnostic.guidance))
    precondition(text.contains("Technical problem (PARSE_ERROR): raw internal diagnostic"))
    precondition(text.contains("Technical run problem (PARSE_ERROR): raw internal diagnostic"))
    precondition(!item.explanation.contains(diagnostic.message))
  }

  static func checkProgressStream() {
    var stream = ProgressStream()
    let first = Data(
      "EML_PROGRESS {\"schema\":1,\"stage\":\"processing\",\"completed\":0,\"total\":3}\n".utf8)
    precondition(stream.append(first.prefix(12)).isEmpty)
    let start = stream.append(first.dropFirst(12))
    precondition(start.count == 1 && start[0].completed == 0 && start[0].total == 3)
    let next = Data(
      "EML_PROGRESS {\"schema\":1,\"stage\":\"processing\",\"completed\":2,\"total\":3}\n".utf8)
    precondition(stream.append(next).count == 1)
    precondition(stream.append(first).isEmpty, "Progress moved backwards")
    let wrongTotal = Data(
      "EML_PROGRESS {\"schema\":1,\"stage\":\"processing\",\"completed\":3,\"total\":4}\n".utf8)
    precondition(stream.append(wrongTotal).isEmpty, "Progress changed the admitted total")
    let final = Data(
      "EML_PROGRESS {\"schema\":1,\"stage\":\"reporting\",\"completed\":3,\"total\":3}\n".utf8)
    precondition(stream.append(final).count == 1)
    precondition(stream.append(next).isEmpty, "Reporting returned to processing")
    precondition(
      stream.append(Data("EML_PROGRESS ".utf8) + Data([0x22, 0xc2, 0x22, 0x0a])).isEmpty)
    precondition(stream.append(Data(repeating: 65, count: 5000)).isEmpty)
    precondition(stream.append(Data("\nordinary diagnostic\n".utf8)).isEmpty)
    precondition(String(decoding: stream.finish(), as: UTF8.self).contains("ordinary diagnostic"))
    var unfinished = ProgressStream()
    precondition(unfinished.append(first.dropLast()).isEmpty)
    precondition(unfinished.finish().isEmpty, "Unfinished progress became a diagnostic")
    var longDiagnostic = ProgressStream()
    precondition(longDiagnostic.append(Data(repeating: 65, count: 5000)).isEmpty)
    precondition(
      longDiagnostic.finish() == Data(repeating: 65, count: 1024) + Data([10]),
      "A long final diagnostic disappeared at EOF")
  }

  @MainActor static func checkConflictLabels(_ items: ReportItems) {
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
  }

  @MainActor static func checkDisclosure(_ details: ReportDetails, frame: NSRect) {
    guard let button = details.detailsButton, let row = button.superview else {
      preconditionFailure("Details disclosure control is absent")
    }
    precondition(button.bezelStyle == .disclosure && button.title.isEmpty)
    row.layoutSubtreeIfNeeded()
    precondition(
      row.bounds.insetBy(dx: -0.5, dy: -0.5).contains(button.frame),
      "Disclosure control is clipped by its row")
    details.detailsButton?.performClick(nil)
    precondition(details.detailsView?.isHidden == false, "Details did not expand")
    if let view = details.detailsView {
      precondition(
        view.visibleRect.height >= view.bounds.height - 1,
        "Expanded details remain outside the report viewport")
    }
    precondition(details.layout.window.frame == frame, "Opening details resized the window")
    precondition(details.detailsButton?.state == .on)
    precondition(details.detailsButton?.isAccessibilityExpanded() == true)
    details.detailsButton?.performClick(nil)
    precondition(details.detailsView?.isHidden == true, "Details did not collapse")
    precondition(details.layout.window.frame == frame, "Closing details resized the window")
    precondition(details.detailsButton?.state == .off)
    precondition(details.detailsButton?.isAccessibilityExpanded() == false)
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

enum FileInputChecks {
  @MainActor static func checkFileDrop() {
    let pasteboard = NSPasteboard.withUniqueName()
    defer { pasteboard.releaseGlobally() }
    let drop = FileDropView(frame: .zero)
    let urls = [
      URL(fileURLWithPath: "/Public/a b.eml"), URL(fileURLWithPath: "/Public/line\n.eml"),
      URL(fileURLWithPath: "/Public/a?#%00.eml"),
    ]
    precondition(pasteboard.writeObjects(urls as [NSURL]))
    precondition(!drop.accept(pasteboard), "Drop without a consumer was accepted")
    var received: [String] = []
    drop.onFiles = { received = $0 }
    precondition(drop.accept(pasteboard))
    precondition(received == urls.map(\.path), "Drop changed file paths")
    received = []
    pasteboard.clearContents()
    let local = NSURL(string: "file://LOCALHOST/Public/caf%C3%A9%2520.eml")!
    precondition(pasteboard.writeObjects([local]))
    precondition(drop.accept(pasteboard))
    precondition(received == ["/Public/café%20.eml"], "Local authority or literal percent changed")
    received = []
    pasteboard.clearContents()
    let remote = NSURL(string: "file://example.com/Public/a.eml")!
    precondition(pasteboard.writeObjects([remote]))
    precondition(!drop.accept(pasteboard), "Remote file authority became a local pathname")
    precondition(received.isEmpty)
    pasteboard.clearContents()
    precondition(pasteboard.writeObjects([urls[0] as NSURL, remote]))
    precondition(!drop.accept(pasteboard), "Remote authority selection was partly accepted")
    precondition(received.isEmpty)
    pasteboard.clearContents()
    precondition(pasteboard.writeObjects([NSURL(string: "https://example.com/mail.eml")!]))
    precondition(!drop.accept(pasteboard), "Web URL was accepted as a local file")
    precondition(received.isEmpty)
    pasteboard.clearContents()
    precondition(
      pasteboard.writeObjects([urls[0] as NSURL, NSURL(string: "https://example.com/mail.eml")!]))
    precondition(!drop.accept(pasteboard), "Mixed local and remote drop was partly accepted")
    precondition(received.isEmpty)
    pasteboard.clearContents()
    pasteboard.setString("/Public/a.eml", forType: .string)
    precondition(!drop.accept(pasteboard), "Plain text was accepted as a local file")
    precondition(received.isEmpty)
    for value in [
      "file:///Public/a%00b.eml", "file:///Public/%FF.eml", "file:Public/a.eml",
      "file:///Public/a.eml?mode=x", "file:///Public/a.eml#fragment",
      "file://public@localhost/Public/a.eml", "file://localhost:123/Public/a.eml",
    ] {
      pasteboard.clearContents()
      guard let unsupportedURL = NSURL(string: value) else {
        preconditionFailure("File URI probe could not be constructed")
      }
      precondition(pasteboard.writeObjects([urls[0] as NSURL, unsupportedURL]))
      precondition(!drop.accept(pasteboard), "Ambiguous file URI was partly accepted: " + value)
      precondition(received.isEmpty)
    }
  }

  @MainActor static func checkFileService() {
    let service = EmailCopyService()
    precondition(service.responds(to: NSSelectorFromString("createEMLCopies:userData:error:")))
    let pasteboard = NSPasteboard.withUniqueName()
    defer { pasteboard.releaseGlobally() }
    let urls = [URL(fileURLWithPath: "/Public/a b.eml"), URL(fileURLWithPath: "/Public/folder")]
    precondition(pasteboard.writeObjects(urls as [NSURL]))
    var error: NSString?
    service.createEMLCopies(pasteboard, userData: nil, error: &error)
    precondition(error != nil, "Unavailable service silently accepted a request")
    var received: [String] = []
    service.onFiles = {
      received = $0
      return true
    }
    let originalCount = pasteboard.changeCount
    service.createEMLCopies(pasteboard, userData: nil, error: &error)
    precondition(error == nil && received == urls.map(\.path))
    precondition(pasteboard.changeCount == originalCount, "Service changed the caller's pasteboard")
    pasteboard.clearContents()
    pasteboard.setString("/Public/a.eml", forType: .string)
    received = []
    service.createEMLCopies(pasteboard, userData: nil, error: &error)
    precondition(error != nil && received.isEmpty, "Service accepted a plain-text path")
  }
}
