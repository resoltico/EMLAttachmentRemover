// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import AppKit
import Darwin
import XCTest

final class DesktopInteractionTests: XCTestCase {
  var pickerBaselineSettled = false
  private var panelPathSequence = 0
  enum InteractionFailure: Error {
    case expectedPresentation
    case selectedReportCount(Int)
    case selectedReportValueType(String)
    case selectedReportTitle(String)
  }
  static let selectedReportTitle = "Selected message and removed parts"
  static let technicalReportTitle = "Selected message, removed parts and technical observations"
  @MainActor
  func contentFrame(_ window: XCUIElement) throws -> CGRect {
    // A Slint layout Group is observable in the snapshot tree but need not
    // retain an identity that XCTest can resolve as a live XCUIElement.
    let snapshot = try window.snapshot()
    let groups = snapshot.children.filter { $0.elementType == .group }
    guard groups.count == 1 else { throw InteractionFailure.expectedPresentation }
    let frame = groups[0].frame
    guard !frame.isEmpty, snapshot.frame.contains(frame) else {
      throw InteractionFailure.expectedPresentation
    }
    return frame
  }

  @MainActor
  func presentationSnapshots(_ element: XCUIElement) throws -> [XCUIElementSnapshot] {
    var pending = Array((try element.snapshot()).children.reversed())
    var snapshots: [XCUIElementSnapshot] = []
    while let snapshot = pending.popLast() {
      snapshots.append(snapshot)
      pending.append(contentsOf: snapshot.children.reversed())
    }
    return snapshots
  }

  // Native snapshots supplied a 512-unit TextView preview in the actual floor
  // fixture. Read selected-report contents from this one exact live query.
  @MainActor
  func selectedReportText(_ app: XCUIApplication) throws -> (title: String, text: String)? {
    let query = app.textViews.matching(
      NSPredicate(
        format: "title == %@ OR title == %@", Self.selectedReportTitle, Self.technicalReportTitle))
    let count = query.count
    if count == 0 { return nil }
    guard count == 1 else { throw InteractionFailure.selectedReportCount(count) }
    let element = query.element
    let title = element.title
    guard title == Self.selectedReportTitle || title == Self.technicalReportTitle else {
      throw InteractionFailure.selectedReportTitle(title)
    }
    let raw = element.value
    guard let text = raw as? String else {
      throw InteractionFailure.selectedReportValueType(
        raw.map { String(reflecting: type(of: $0)) } ?? "nil")
    }
    return (title, text)
  }

  @MainActor
  func presentationTextValues(_ app: XCUIApplication) throws -> [String] {
    var texts = try presentationSnapshots(app).map {
      $0.title + "\n" + $0.label + "\n"
        + ($0.elementType == .textView ? "" : ($0.value as? String ?? ""))
    }
    if let report = try selectedReportText(app) {
      texts.append(report.title + "\n" + report.text)
    }
    return texts
  }

  // Always make one bounded gesture to the requested end. A visible endpoint
  // can coexist with a clipped header/footer; the outer window also includes
  // titlebar space that is not part of the Slint content viewport.
  @MainActor
  func showReportDetails(_ app: XCUIApplication) async throws {
    let window = app.windows["EML Attachment Remover"]
    let before = try contentFrame(window)
    let technical = app.checkBoxes["Technical details"]
    let top = app.buttons["Choose messages…"]
    let anchor: XCUIElement
    if top.exists && top.isHittable && before.contains(top.frame) {
      anchor = top
    } else if technical.exists && technical.isHittable && before.contains(technical.frame) {
      anchor = technical
    } else {
      XCTFail("No physically visible content anchor for the report-end scroll")
      throw InteractionFailure.expectedPresentation
    }
    anchor.scroll(byDeltaX: 0, deltaY: -1000)
    var snapshotFailure: String?
    let settled = NSPredicate { _, _ in
      do {
        let after = try self.contentFrame(window)
        return technical.exists && technical.isHittable && after.contains(technical.frame)
      } catch {
        snapshotFailure = String(describing: error)
        return false
      }
    }
    let result = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: settled, object: nil)], timeout: 5)
    guard result == .completed else {
      XCTFail(
        "Report controls remain clipped after the one owned scroll; snapshot error: \(snapshotFailure ?? "none")"
      )
      throw InteractionFailure.expectedPresentation
    }
  }

  @MainActor
  func showRunControls(_ app: XCUIApplication) async throws {
    let window = app.windows["EML Attachment Remover"]
    let before = try contentFrame(window)
    let top = app.buttons["Choose messages…"]
    let technical = app.checkBoxes["Technical details"]
    let anchor: XCUIElement
    if technical.exists && technical.isHittable && before.contains(technical.frame) {
      anchor = technical
    } else if top.exists && top.isHittable && before.contains(top.frame) {
      anchor = top
    } else {
      XCTFail("No physically visible content anchor for the run-control-end scroll")
      throw InteractionFailure.expectedPresentation
    }
    anchor.scroll(byDeltaX: 0, deltaY: 1000)
    var snapshotFailure: String?
    let settled = NSPredicate { _, _ in
      do {
        let after = try self.contentFrame(window)
        return top.exists && top.isHittable && after.contains(top.frame)
      } catch {
        snapshotFailure = String(describing: error)
        return false
      }
    }
    let result = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: settled, object: nil)], timeout: 5)
    guard result == .completed else {
      XCTFail(
        "Run controls remain clipped after the one owned scroll; snapshot error: \(snapshotFailure ?? "none")"
      )
      throw InteractionFailure.expectedPresentation
    }
  }

  @MainActor
  func closeOwnedFinderWindow(_ finder: XCUIApplication, title: String) async throws {
    let matches = finder.windows.allElementsBoundByIndex.filter { $0.title == title }
    guard !matches.isEmpty else { return }
    guard matches.count == 1, let ownedWindow = matches.first else {
      XCTFail("Owned Finder window is ambiguous; refusing to close another window")
      throw InteractionFailure.expectedPresentation
    }
    let exactTitle = finder.windows.matching(NSPredicate(format: "title == %@", title))
    guard exactTitle.count == 1 else {
      XCTFail("Exact-title Finder query did not identify the already-owned window before closing")
      throw InteractionFailure.expectedPresentation
    }
    ownedWindow.buttons["_XCUI:CloseWindow"].click()
    let closed = NSPredicate { _, _ in exactTitle.count == 0 }
    let result = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: closed, object: nil)], timeout: 10)
    guard result == .completed else {
      XCTFail("Owned Finder window did not close: \(finder.debugDescription)")
      throw InteractionFailure.expectedPresentation
    }
  }

  @MainActor
  func finderEntry(_ window: XCUIElement, file: URL, selected: Bool?) throws -> XCUIElement {
    // Finder may hide a known extension. Both display names identify this one
    // synthetic file inside the already-bound unique fixture directory.
    let names = [file.lastPathComponent, file.deletingPathExtension().lastPathComponent]
    let entryKinds: [XCUIElement.ElementType] = [.image, .icon, .cell, .tableRow, .outlineRow]
    var examined = 0
    func matchesName(_ value: String) -> Bool {
      names.contains { value == $0 || value.hasPrefix($0 + ",") }
    }
    for kind in entryKinds {
      let elements = window.descendants(matching: kind).allElementsBoundByIndex
      examined += elements.count
      guard examined <= 512 else { throw InteractionFailure.expectedPresentation }
      for element in elements {
        let matches =
          matchesName(element.title) || matchesName(element.label)
          || matchesName(element.value as? String ?? "")
        if matches && (selected == nil || element.isSelected == selected) { return element }
      }
    }
    XCTFail(
      "Owned Finder entry/selection is unavailable for \(file.lastPathComponent): \(window.debugDescription)"
    )
    throw InteractionFailure.expectedPresentation
  }

  @MainActor
  func saveReport(
    _ app: XCUIApplication, fixture: URL, to destination: URL, expectNameCollision: Bool
  )
    async throws
  {
    try await showRunControls(app)
    let exportButton = app.buttons["Export Report…"]
    let available = NSPredicate { _, _ in exportButton.isEnabled }
    guard
      await XCTWaiter.fulfillment(
        of: [XCTNSPredicateExpectation(predicate: available, object: nil)], timeout: 20)
        == .completed
    else { throw InteractionFailure.expectedPresentation }
    exportButton.click()
    let panel = app.sheets.firstMatch
    guard panel.waitForExistence(timeout: 10) else {
      XCTFail("Native report folder panel did not appear: \(app.debugDescription)")
      throw InteractionFailure.expectedPresentation
    }
    try await navigateNativePanel(
      app, panel: panel, fixture: fixture, to: destination.deletingLastPathComponent())
    panel.buttons["Open"].click()
    let closed = NSPredicate { _, _ in !panel.exists }
    let result = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: closed, object: nil)], timeout: 10)
    guard result == .completed else {
      XCTFail("Native report folder panel did not complete: \(app.debugDescription)")
      throw InteractionFailure.expectedPresentation
    }
    // The draft is between run controls and report details. Scroll from the
    // still-visible export action once before resolving its native AX fields.
    guard exportButton.isHittable,
      app.windows["EML Attachment Remover"].frame.contains(exportButton.frame)
    else { throw InteractionFailure.expectedPresentation }
    exportButton.scroll(byDeltaX: 0, deltaY: -300)
    let filename = app.textFields["Report filename"]
    guard filename.waitForExistence(timeout: 5) else {
      XCTFail("Report filename form is unavailable: \(app.debugDescription)")
      throw InteractionFailure.expectedPresentation
    }
    guard filename.isHittable,
      app.windows["EML Attachment Remover"].frame.contains(filename.frame)
    else { throw InteractionFailure.expectedPresentation }
    filename.click()
    app.typeKey("a", modifierFlags: .command)
    app.typeText(destination.lastPathComponent)
    XCTAssertEqual(filename.value as? String, destination.lastPathComponent)
    try await requireAccessibleText(app, prefix: "Exact report destination: " + destination.path)
    try await requireText(app, containing: "Existing files are never replaced.")
    XCTAssertFalse(app.buttons["Replace"].exists)
    let start = app.buttons["Export Report"]
    guard start.isHittable, app.windows["EML Attachment Remover"].frame.contains(start.frame)
    else { throw InteractionFailure.expectedPresentation }
    start.click()
    if expectNameCollision {
      // Occupancy is a worker refusal, not a UI filesystem preflight. Keep the
      // exact filename draft available after the owned child settles.
      try await requireText(app, containing: "Report was not published.")
      XCTAssertEqual(filename.value as? String, destination.lastPathComponent)
      XCTAssertTrue(filename.isEnabled)
      XCTAssertFalse(app.sheets.firstMatch.exists)
      XCTAssertFalse(app.buttons["Replace"].exists)
    }
    guard start.exists, start.isHittable,
      app.windows["EML Attachment Remover"].frame.contains(start.frame)
    else { throw InteractionFailure.expectedPresentation }
    start.scroll(byDeltaX: 0, deltaY: 1000)
    try await showRunControls(app)
  }

  @MainActor
  func navigateNativePanel(_ app: XCUIApplication, panel: XCUIElement, fixture: URL, to path: URL)
    async throws
  {
    app.typeKey("g", modifierFlags: [.command, .shift])
    let goTo = panel.sheets["GoToWindow"]
    let field = goTo.textFields["PathTextField"]
    guard goTo.waitForExistence(timeout: 10),
      panel.sheets.matching(identifier: "GoToWindow").count == 1,
      field.waitForExistence(timeout: 5), field.isEnabled && field.isHittable
    else {
      XCTFail("Owned native Go To path control is unavailable: \(panel.debugDescription)")
      throw InteractionFailure.expectedPresentation
    }
    field.click()
    field.typeKey("a", modifierFlags: .command)
    panelPathSequence += 1
    guard panelPathSequence <= 32 else { throw InteractionFailure.expectedPresentation }
    let sequence = panelPathSequence
    try DesktopFixtureRequest.preparePanelPath(sequence: sequence, path: path.path).send()
    let ready = fixture.appendingPathComponent("panel-path-\(sequence).ready")
    let readyBytes = Data("owned panel path ready \(sequence)\n".utf8)
    try await requireObservation(ready, prefix: "owned panel path ready \(sequence)\n")
    guard try Data(contentsOf: ready) == readyBytes else {
      throw InteractionFailure.expectedPresentation
    }
    field.typeKey("v", modifierFlags: .command)
    let entered = field.value as? String
    try DesktopFixtureRequest.restorePanelClipboard.send()
    let restored = fixture.appendingPathComponent("panel-path-\(sequence).restored")
    let restoredBytes = Data("owned panel clipboard restored \(sequence)\n".utf8)
    try await requireObservation(restored, prefix: "owned panel clipboard restored \(sequence)\n")
    guard try Data(contentsOf: restored) == restoredBytes else {
      throw InteractionFailure.expectedPresentation
    }
    print(
      "Owned native Go To path equality: \(entered == path.path); actual UTF8 bytes: \(String(describing: entered?.utf8.count))"
    )
    guard entered == path.path else {
      XCTFail(
        "Native Go To path differs: expected owned path \(path.path); actual String UTF8 bytes: \(String(describing: entered?.utf8.count))"
      )
      throw InteractionFailure.expectedPresentation
    }
    field.typeKey(XCUIKeyboardKey.return, modifierFlags: [])
    let closed = NSPredicate { _, _ in !goTo.exists }
    let result = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: closed, object: nil)], timeout: 10)
    guard result == .completed else {
      XCTFail("Owned native Go To sheet did not finish navigation: \(panel.debugDescription)")
      throw InteractionFailure.expectedPresentation
    }
  }

  @MainActor
  func requireWaiting(_ app: XCUIApplication, count: Int) async throws {
    if !app.sheets.firstMatch.exists { try await showRunControls(app) }
    let label = "Waiting requests: \(count)"
    let predicate = NSPredicate { _, _ in
      app.staticTexts.matching(NSPredicate(format: "value == %@", label)).count == 1
    }
    let result = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: predicate, object: nil)], timeout: 10)
    XCTAssertEqual(
      result, .completed, "Expected exactly \(count) waiting requests: \(app.debugDescription)")
    guard result == .completed else { throw InteractionFailure.expectedPresentation }
  }

  @MainActor
  func choose(
    _ app: XCUIApplication, fixture: URL, path: URL, folder: Bool, destination: Bool = false
  )
    async throws
  {
    try await showRunControls(app)
    if !pickerBaselineSettled {
      let acknowledgment = fixture.appendingPathComponent("picker-baseline-settled")
      try DesktopFixtureRequest.pickerBaseline.send()
      try await requireObservation(acknowledgment, prefix: "owned picker baseline settled\n")
      XCTAssertEqual(
        try Data(contentsOf: acknowledgment), Data("owned picker baseline settled\n".utf8))
      pickerBaselineSettled = true
    }
    try DesktopFixtureRequest.picker.send()
    if destination {
      app.buttons["Copy destination…"].click()
    } else if folder {
      app.buttons["Choose folder…"].click()
    } else {
      app.typeKey("o", modifierFlags: .command)
    }
    let panel = app.sheets.firstMatch
    guard panel.waitForExistence(timeout: 10) else {
      XCTFail("Native input panel did not appear: \(app.debugDescription)")
      throw InteractionFailure.expectedPresentation
    }
    try await navigateNativePanel(app, panel: panel, fixture: fixture, to: path)
    if panel.exists {
      let open = panel.buttons["Open"]
      guard open.waitForExistence(timeout: 5), open.isEnabled else {
        XCTFail("Native input selection did not become available: \(app.debugDescription)")
        throw InteractionFailure.expectedPresentation
      }
      open.click()
    }
    let closed = NSPredicate { _, _ in !panel.exists }
    let result = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: closed, object: nil)], timeout: 10)
    XCTAssertEqual(
      result, .completed, "The real native input panel did not complete: \(app.debugDescription)")
    guard result == .completed else { throw InteractionFailure.expectedPresentation }
  }

  @MainActor
  func requireNativePanelMessage(_ panel: XCUIElement, expected: String) async throws {
    let messages = panel.staticTexts.matching(
      NSPredicate(format: "identifier == %@ OR value == %@", "_messageTextField", expected))
    let predicate = NSPredicate { _, _ in
      panel.exists && messages.count == 1 && messages.firstMatch.exists
        && (messages.firstMatch.value as? String) == expected
    }
    let result = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: predicate, object: nil)], timeout: 10)
    guard result == .completed, messages.count == 1, messages.firstMatch.exists,
      (messages.firstMatch.value as? String) == expected
    else {
      let identified = panel.staticTexts.matching(identifier: "_messageTextField")
      if identified.count == 1, identified.firstMatch.exists {
        let message = identified.firstMatch
        print(
          "Native panel message: value=\(String(describing: message.value)), label=\(message.label.debugDescription), title=\(message.title.debugDescription)"
        )
      }
      XCTFail("Required complete native panel message is absent or ambiguous: \(expected)")
      throw InteractionFailure.expectedPresentation
    }
    print("Native panel message readback: \(String(describing: messages.firstMatch.value))")
  }

  @MainActor
  func requireAccessibleText(_ app: XCUIApplication, prefix: String) async throws {
    enum Attribute { case title, label, value }
    var match: (element: XCUIElementSnapshot, attribute: Attribute)?
    var snapshotFailure: String?
    let predicate = NSPredicate { _, _ in
      let snapshots: [XCUIElementSnapshot]
      do { snapshots = try self.presentationSnapshots(app) } catch {
        snapshotFailure = String(describing: error)
        return false
      }
      for image in snapshots where image.elementType == .image {
        if image.title.hasPrefix(prefix) {
          match = (image, .title)
          return true
        }
        if image.label.hasPrefix(prefix) {
          match = (image, .label)
          return true
        }
      }
      for text in snapshots where text.elementType == .staticText {
        if (text.value as? String)?.hasPrefix(prefix) == true {
          match = (text, .value)
          return true
        }
      }
      return false
    }
    guard
      await XCTWaiter.fulfillment(
        of: [XCTNSPredicateExpectation(predicate: predicate, object: nil)], timeout: 10)
        == .completed
    else {
      XCTFail(
        "Required native accessible text is absent: \(prefix); snapshot error: \(snapshotFailure ?? "none"); \(app.debugDescription)"
      )
      let diagnostics = (try? presentationSnapshots(app)) ?? []
      for element in diagnostics.filter({ $0.elementType == .staticText }).prefix(16) {
        let value = element.value
        let kind = value.map { String(reflecting: Swift.type(of: $0)) } ?? "nil"
        print(
          "Native StaticText value type=\(kind), value=\(String(describing: value).prefix(1024).debugDescription)"
        )
      }
      throw InteractionFailure.expectedPresentation
    }
    let observed = try XCTUnwrap(match)
    let value: String?
    switch observed.attribute {
    case .title: value = observed.element.title
    case .label: value = observed.element.label
    case .value: value = observed.element.value as? String
    }
    guard value?.hasPrefix(prefix) == true else {
      XCTFail("Native accessible attribute changed before acceptance: \(prefix)")
      throw InteractionFailure.expectedPresentation
    }
    print(
      "Native accessibility observation: kind=\(observed.element.elementType), attribute=\(observed.attribute), value=\(String(describing: value).prefix(4096))"
    )
  }

  @MainActor
  func requireRun(
    _ app: XCUIApplication, counts: String, source: URL, displayedName: String? = nil
  ) async throws {
    try await requireRunCounts(app, counts: counts)
    try await showReportDetails(app)
    let technical = app.checkBoxes["Technical details"]
    let value =
      (technical.value as? NSNumber)?.intValue
      ?? (technical.value as? String).flatMap(Int.init)
    guard value == 0 || value == 1 else { throw InteractionFailure.expectedPresentation }
    if value == 0 { technical.click() }
    var reportFailure: String?
    let predicate = NSPredicate { _, _ in
      do {
        guard let report = try self.selectedReportText(app),
          report.title == Self.technicalReportTitle
        else { return false }
        return report.text.contains(displayedName ?? source.lastPathComponent)
          && report.text.contains("settled true; reusable true")
      } catch {
        reportFailure = String(describing: error)
        return false
      }
    }
    let selected = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: predicate, object: nil)], timeout: 20)
    XCTAssertEqual(
      selected, .completed,
      "Expected settled details for \(source.lastPathComponent); report error: \(reportFailure ?? "none"): \(app.debugDescription)"
    )
    guard selected == .completed else { throw InteractionFailure.expectedPresentation }
    try await showRunControls(app)
  }

  @MainActor
  func requireRunCounts(_ app: XCUIApplication, counts: String) async throws {
    try await showReportDetails(app)
    var snapshotFailure: String?
    let predicate = NSPredicate { _, _ in
      let snapshots: [XCUIElementSnapshot]
      do { snapshots = try self.presentationSnapshots(app) } catch {
        snapshotFailure = String(describing: error)
        return false
      }
      let summaries = snapshots.filter { $0.elementType == .staticText }.compactMap { element in
        [element.title, element.label, element.value as? String ?? ""].first {
          $0.hasPrefix("Run summary: ")
        }
      }
      guard summaries.count == 1 else { return false }
      let text = summaries[0]
      let lines = text.components(separatedBy: .newlines)
      return lines.contains("\(counts) · 0 failed · 0 not processed")
        && lines.contains("Finished.") && !text.contains("Finished with errors")
    }
    let result = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: predicate, object: nil)], timeout: 30)
    XCTAssertEqual(
      result, .completed,
      "Expected terminal run counts \(counts); snapshot error: \(snapshotFailure ?? "none"): \(app.debugDescription)"
    )
    guard result == .completed else { throw InteractionFailure.expectedPresentation }
  }

  @MainActor
  func requireText(_ app: XCUIApplication, containing text: String) async throws {
    var presentationFailure: String?
    let predicate = NSPredicate { _, _ in
      do {
        return try self.presentationTextValues(app).contains { $0.contains(text) }
      } catch {
        presentationFailure = String(describing: error)
        return false
      }
    }
    let result = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: predicate, object: nil)], timeout: 20)
    XCTAssertEqual(
      result, .completed,
      "Expected presentation \(text); presentation error: \(presentationFailure ?? "none"): \(app.debugDescription)"
    )
    guard result == .completed else { throw InteractionFailure.expectedPresentation }
  }

  @MainActor
  func observeSource(_ source: URL, phase: String) {
    var facts = stat()
    let result = source.withUnsafeFileSystemRepresentation { path -> Int32 in
      guard let path else { return -1 }
      return lstat(path, &facts)
    }
    print(
      "Owned source \(phase) \(source.lastPathComponent.debugDescription): stat=\(result); dev=\(facts.st_dev); inode=\(facts.st_ino); uid=\(facts.st_uid); gid=\(facts.st_gid); mode=\(facts.st_mode); size=\(facts.st_size); flags=\(facts.st_flags); mtime=\(facts.st_mtimespec.tv_sec).\(facts.st_mtimespec.tv_nsec); ctime=\(facts.st_ctimespec.tv_sec).\(facts.st_ctimespec.tv_nsec)"
    )
    source.withUnsafeFileSystemRepresentation { path in
      guard let path else { return }
      let count = listxattr(path, nil, 0, XATTR_NOFOLLOW)
      guard count >= 0 && count <= 65536 else {
        print("Owned source xattr name extent unverified: \(count)")
        return
      }
      var names = [CChar](repeating: 0, count: max(1, count))
      let observed = names.withUnsafeMutableBufferPointer { buffer in
        listxattr(path, buffer.baseAddress, buffer.count, XATTR_NOFOLLOW)
      }
      guard observed >= 0 && observed <= names.count else {
        print("Owned source xattr names changed or unavailable: \(observed)")
        return
      }
      let labels = names.prefix(observed).split(separator: 0).map {
        String(decoding: $0.map { UInt8(bitPattern: $0) }, as: UTF8.self)
      }
      print("Owned source xattr names \(phase): \(labels.debugDescription)")
    }
  }

  @MainActor
  func focusOwnedWindow(
    _ owned: NSRunningApplication, window: XCUIElement, avoiding other: XCUIElement? = nil
  ) async throws {
    guard !owned.isTerminated, window.exists, !window.frame.isEmpty,
      let front = NSWorkspace.shared.frontmostApplication,
      front.processIdentifier == owned.processIdentifier
        || front.bundleIdentifier == "com.apple.finder"
    else { throw InteractionFailure.expectedPresentation }
    let titlebar = window.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0))
      .withOffset(CGVector(dx: 0, dy: 12))
    let covered = other.map { $0.exists && $0.frame.contains(titlebar.screenPoint) } ?? false
    guard window.frame.contains(titlebar.screenPoint),
      !covered
    else {
      XCTFail("Owned product titlebar is covered; no focus click sent")
      throw InteractionFailure.expectedPresentation
    }
    titlebar.click()
    try await requireOwnedForeground(owned)
  }

  @MainActor
  func requireOwnedForeground(_ owned: NSRunningApplication) async throws {
    guard !owned.isTerminated else { throw InteractionFailure.expectedPresentation }
    let active = NSPredicate { _, _ in
      !owned.isTerminated && owned.isActive
        && NSWorkspace.shared.frontmostApplication?.processIdentifier == owned.processIdentifier
    }
    guard
      await XCTWaiter.fulfillment(
        of: [XCTNSPredicateExpectation(predicate: active, object: nil)], timeout: 10) == .completed
    else {
      let front = NSWorkspace.shared.frontmostApplication
      XCTFail(
        "Retained native application did not become frontmost: PID \(owned.processIdentifier), alive=\(!owned.isTerminated), active=\(owned.isActive), actual front PID=\(String(describing: front?.processIdentifier)), bundle=\(String(describing: front?.bundleIdentifier))"
      )
      throw InteractionFailure.expectedPresentation
    }
  }

  @MainActor
  func snapshot(_ element: XCUIElement, _ name: String) {
    let attachment = XCTAttachment(screenshot: element.screenshot())
    attachment.name = name
    attachment.lifetime = .keepAlways
    add(attachment)
  }
}
