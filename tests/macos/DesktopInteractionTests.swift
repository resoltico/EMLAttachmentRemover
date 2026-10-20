// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import AppKit
import Darwin
import XCTest

final class DesktopInteractionTests: XCTestCase {
  var pickerBaselineSettled = false
  enum InteractionFailure: Error { case expectedPresentation }
  @MainActor
  func closeOwnedFinderWindow(_ finder: XCUIApplication, title: String) async throws {
    let matches = finder.windows.allElementsBoundByIndex.filter { $0.title == title }
    guard !matches.isEmpty else { return }
    guard matches.count == 1, let ownedWindow = matches.first else {
      XCTFail("Owned Finder window is ambiguous; refusing to close another window")
      throw InteractionFailure.expectedPresentation
    }
    ownedWindow.buttons["_XCUI:CloseWindow"].click()
    let closed = NSPredicate { _, _ in
      finder.windows.allElementsBoundByIndex.allSatisfy { $0.title != title }
    }
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
  func saveReport(_ app: XCUIApplication, to destination: URL, expectNameCollision: Bool)
    async throws
  {
    app.buttons["Export Report…"].click()
    let panel = app.sheets.firstMatch
    guard panel.waitForExistence(timeout: 10) else {
      XCTFail("Native report save panel did not appear: \(app.debugDescription)")
      throw InteractionFailure.expectedPresentation
    }
    try await navigateNativePanel(app, panel: panel, to: destination.deletingLastPathComponent())
    let filename = panel.textFields["saveAsNameTextField"]
    guard filename.waitForExistence(timeout: 5) else {
      XCTFail("Native Save As control is unavailable: \(app.debugDescription)")
      throw InteractionFailure.expectedPresentation
    }
    filename.click()
    app.typeKey("a", modifierFlags: .command)
    app.typeText(destination.lastPathComponent)
    guard filename.value as? String == destination.lastPathComponent else {
      XCTFail(
        "Native Save filename was not replaced: \(String(describing: filename.value)); \(app.debugDescription)"
      )
      throw InteractionFailure.expectedPresentation
    }
    panel.buttons["Save"].click()
    if expectNameCollision {
      try await requireText(app, containing: "An item already exists with that name.")
      XCTAssertTrue(panel.exists, "Refusing an occupied name must keep the Save panel usable")
      XCTAssertEqual(filename.value as? String, destination.lastPathComponent)
      XCTAssertFalse(
        panel.sheets.firstMatch.exists, "An occupied name must not open a Replace sheet")
      XCTAssertFalse(panel.buttons["Replace"].exists)
      XCTAssertTrue(filename.isEnabled && panel.buttons["Cancel"].isEnabled)
      panel.buttons["Cancel"].click()
    }
    let closed = NSPredicate { _, _ in !panel.exists }
    let result = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: closed, object: nil)], timeout: 10)
    XCTAssertEqual(
      result, .completed, "Native report save panel did not complete: \(app.debugDescription)")
    guard result == .completed else { throw InteractionFailure.expectedPresentation }
  }

  @MainActor
  func navigateNativePanel(_ app: XCUIApplication, panel: XCUIElement, to path: URL)
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
    field.typeText(path.path)
    let entered = field.value as? String
    print("Owned native Go To path readback: \(String(describing: entered))")
    guard entered == path.path else {
      XCTFail(
        "Native Go To path differs: expected \(path.path), actual \(String(describing: entered))")
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
    try await navigateNativePanel(app, panel: panel, to: path)
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
    var match: (element: XCUIElement, attribute: Attribute)?
    let predicate = NSPredicate { _, _ in
      for image in app.images.allElementsBoundByIndex {
        if image.title.hasPrefix(prefix) {
          match = (image, .title)
          return true
        }
        if image.label.hasPrefix(prefix) {
          match = (image, .label)
          return true
        }
      }
      for text in app.staticTexts.allElementsBoundByIndex {
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
      XCTFail("Required native accessible text is absent: \(prefix); \(app.debugDescription)")
      for element in app.staticTexts.allElementsBoundByIndex.prefix(16) {
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
    let detail = app.textViews["Selected message, removed parts and technical observations"]
    let predicate = NSPredicate { _, _ in
      guard detail.exists, let value = detail.value as? String else { return false }
      return value.contains(displayedName ?? source.lastPathComponent)
        && value.contains("settled true; reusable true")
    }
    let selected = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: predicate, object: nil)], timeout: 20)
    XCTAssertEqual(
      selected, .completed,
      "Expected settled details for \(source.lastPathComponent): \(app.debugDescription)")
    guard selected == .completed else { throw InteractionFailure.expectedPresentation }
  }

  @MainActor
  func requireRunCounts(_ app: XCUIApplication, counts: String) async throws {
    let predicate = NSPredicate { _, _ in
      app.buttons.allElementsBoundByIndex.contains {
        let text = $0.title + "\n" + $0.label + "\n" + ($0.value as? String ?? "")
        return text.contains(counts) && text.contains("Finished")
          && !text.contains("Finished with errors") && text.contains("0 failed · 0 not processed")
      }
    }
    let result = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: predicate, object: nil)], timeout: 30)
    XCTAssertEqual(
      result, .completed, "Expected terminal run counts \(counts): \(app.debugDescription)")
    guard result == .completed else { throw InteractionFailure.expectedPresentation }
  }

  @MainActor
  func requireText(_ app: XCUIApplication, containing text: String) async throws {
    let predicate = NSPredicate { _, _ in
      app.descendants(matching: .any).allElementsBoundByIndex.contains {
        ($0.title + "\n" + $0.label + "\n" + ($0.value as? String ?? "")).contains(text)
      }
    }
    let result = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: predicate, object: nil)], timeout: 20)
    XCTAssertEqual(result, .completed, "Expected presentation \(text): \(app.debugDescription)")
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
