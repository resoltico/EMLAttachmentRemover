// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import AppKit
import Darwin
import XCTest

extension DesktopInteractionTests {
  @MainActor
  func testOwnedPagination() async throws {
    let repository = URL(fileURLWithPath: #filePath)
      .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    let inputURL = repository.appendingPathComponent(
      "target/desktop-interaction-pagination/input.json")
    let input = try XCTUnwrap(
      JSONSerialization.jsonObject(with: Data(contentsOf: inputURL)) as? [String: String])
    XCTAssertEqual(input["host_scope"], "fresh-github-hosted-runner")
    XCTAssertEqual(input["pagination_session"], "true")
    let appURL = URL(fileURLWithPath: try XCTUnwrap(input["app"]))
    let fixture = URL(fileURLWithPath: try XCTUnwrap(input["fixture"]))
    let receipt = try XCTUnwrap(
      JSONSerialization.jsonObject(
        with: Data(
          contentsOf: inputURL.deletingLastPathComponent().appendingPathComponent("ownership.json"))
      ) as? [String: Any])
    let pid = try XCTUnwrap(receipt["pid"] as? NSNumber).int32Value
    let owned = try XCTUnwrap(NSRunningApplication(processIdentifier: pid))
    guard owned.bundleURL?.standardizedFileURL == appURL.standardizedFileURL,
      owned.bundleIdentifier == Bundle(url: appURL)?.bundleIdentifier,
      owned.launchDate?.timeIntervalSince1970 == (receipt["launch_date"] as? NSNumber)?.doubleValue,
      !owned.isTerminated
    else { throw InteractionFailure.expectedPresentation }
    let directory = fixture.appendingPathComponent("pagination-input")
    let sources = (0..<65).map { directory.appendingPathComponent(String(format: "%03d.eml", $0)) }
    let originals = try sources.map { try Data(contentsOf: $0) }
    addTeardownBlock { @MainActor in
      for (source, original) in zip(sources, originals) {
        XCTAssertEqual(try Data(contentsOf: source), original)
      }
    }
    let app = XCUIApplication(url: appURL)
    let window = app.windows["EML Attachment Remover"]
    guard window.waitForExistence(timeout: 20) else {
      throw InteractionFailure.expectedPresentation
    }
    _ = owned.activate(options: [])
    try await requireOwnedForeground(owned)
    let delivery = NSWorkspace.OpenConfiguration()
    delivery.createsNewApplicationInstance = false
    delivery.addsToRecentItems = false
    let recipient = try await NSWorkspace.shared.open(
      [directory], withApplicationAt: appURL, configuration: delivery)
    guard recipient.processIdentifier == pid else { throw InteractionFailure.expectedPresentation }
    try await requireRunCounts(app, counts: "65 created · 0 up to date")
    let previousItems = app.buttons["Previous items"]
    if previousItems.isEnabled { previousItems.click() }
    var observedFirstItem: XCUIElement?
    let firstPage = NSPredicate { _, _ in
      observedFirstItem = window.buttons.allElementsBoundByIndex.first {
        $0.title.hasPrefix("000.eml · ")
      }
      return observedFirstItem != nil
    }
    let selectedPage = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: firstPage, object: nil)], timeout: 10)
    guard selectedPage == .completed, let firstItem = observedFirstItem else {
      XCTFail("The first pagination item did not become available after bounded navigation")
      throw InteractionFailure.expectedPresentation
    }
    firstItem.click()
    let technical = app.checkBoxes["Technical details"]
    let technicalValue = try XCTUnwrap(technical.value as? NSNumber)
    XCTAssertTrue(technicalValue == 0 || technicalValue == 1)
    if technicalValue == 0 { technical.click() }
    let disclosed = NSPredicate { _, _ in (technical.value as? NSNumber) == 1 }
    let disclosure = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: disclosed, object: nil)], timeout: 10)
    guard disclosure == .completed else {
      XCTFail("Pagination technical disclosure did not become enabled: \(technical.value as Any)")
      throw InteractionFailure.expectedPresentation
    }
    try await requireRun(app, counts: "65 created · 0 up to date", source: sources[0])
    let specialGolden = try Data(
      contentsOf: fixture.appendingPathComponent("pagination-golden.eml"))
    let ordinaryGolden = try Data(
      contentsOf: repository.appendingPathComponent(
        "tests/contract/mime/golden/01-gmail-attachment.eml"))
    for (index, source) in sources.enumerated() {
      XCTAssertEqual(
        try Data(
          contentsOf: expectedCopy(for: source)),
        index == 0 ? specialGolden : ordinaryGolden)
    }
    try await checkCompactReport(app, window: window, source: sources[0], exercisePagination: true)
    let observation: [String: Any] = [
      "owned_pid": pid, "items": 65, "removed_parts_in_first_item": 65,
      "page_transitions": ["parts-next", "parts-previous", "items-next", "items-previous"],
      "outputs_match_independent_goldens": true,
    ]
    let attachment = XCTAttachment(
      data: try JSONSerialization.data(withJSONObject: observation),
      uniformTypeIdentifier: "public.json")
    attachment.name = "native-pagination-observation"
    attachment.lifetime = .keepAlways
    add(attachment)
    app.typeKey("q", modifierFlags: .command)
    guard app.buttons["Quit"].waitForExistence(timeout: 5) else {
      throw InteractionFailure.expectedPresentation
    }
    app.buttons["Quit"].click()
    XCTAssertTrue(app.wait(for: .notRunning, timeout: 20))
    XCTAssertTrue(owned.isTerminated)
  }
  @MainActor
  func checkCompactReport(
    _ app: XCUIApplication, window: XCUIElement, source: URL, displayedName: String? = nil,
    exercisePagination: Bool = false
  )
    async throws
  {
    let previous = window.frame
    let corner = window.coordinate(withNormalizedOffset: CGVector(dx: 1, dy: 1))
      .withOffset(CGVector(dx: -2, dy: -2))
    let compactCorner = window.coordinate(withNormalizedOffset: CGVector(dx: 0, dy: 0))
      .withOffset(CGVector(dx: 758, dy: 518))
    corner.click(forDuration: 0.3, thenDragTo: compactCorner)
    let compact = NSPredicate { _, _ in
      let frame = window.frame
      return frame.width >= 760 && frame.width <= 780 && frame.height >= 520
        && frame.height <= 560
    }
    let resized = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: compact, object: nil)], timeout: 10)
    guard resized == .completed else {
      XCTFail("Owned window did not reach its minimum compact size: \(window.frame)")
      throw InteractionFailure.expectedPresentation
    }
    let selector = try XCTUnwrap(
      window.descendants(matching: .any).allElementsBoundByIndex.first {
        $0.title == "Select run" || $0.label == "Select run"
      })
    XCTAssertTrue(selector.isEnabled && selector.isHittable, app.debugDescription)
    XCTAssertFalse(
      window.buttons.allElementsBoundByIndex.contains { $0.title.hasPrefix("Run 1\n") })
    // Scroll before querying the report: AppKit can exclude the clipped
    // report subtree from accessibility in the compact viewport.
    let topControl = app.buttons["Choose messages…"]
    guard topControl.isHittable else { throw InteractionFailure.expectedPresentation }
    topControl.scroll(byDeltaX: 0, deltaY: -600)
    try await requireText(app, containing: displayedName ?? source.lastPathComponent)
    let detail = window.textViews["Selected message, removed parts and technical observations"]
    // The retained frames must show the detail and bottom actions physically,
    // even when accessibility reports clipped controls as hittable.
    guard detail.isHittable, detail.frame.height >= 55, window.frame.contains(detail.frame) else {
      XCTFail(
        "Compact selected detail is collapsed or clipped: \(detail.frame); \(app.debugDescription)")
      throw InteractionFailure.expectedPresentation
    }
    let technical = app.checkBoxes["Technical details"]
    print(
      "Compact actual frames: window=\(window.frame), detail=\(detail.frame), technical=\(technical.frame), finder=\(app.buttons["Show in Finder"].frame)"
    )
    guard technical.isEnabled && technical.isHittable && window.frame.contains(technical.frame),
      window.frame.contains(app.buttons["Show in Finder"].frame)
    else { throw InteractionFailure.expectedPresentation }
    technical.click()
    let unchecked = String(describing: technical.value)
    technical.click()
    XCTAssertNotEqual(String(describing: technical.value), unchecked)
    try await requireText(app, containing: "settled true")
    for label in ["Previous items", "Next items", "Previous parts", "Next parts"] {
      let control = app.buttons[label]
      if control.exists && control.isEnabled {
        guard control.isHittable && window.frame.contains(control.frame) else {
          throw InteractionFailure.expectedPresentation
        }
      }
    }
    if exercisePagination {
      try await exerciseCompactPagination(app, window: window, source: source, detail: detail)
    }
    snapshot(app, "minimum-compact-accessible-report")
    technical.scroll(byDeltaX: 0, deltaY: 1000)
    guard app.buttons["Choose messages…"].isHittable,
      window.frame.contains(app.buttons["Choose messages…"].frame)
    else { throw InteractionFailure.expectedPresentation }
    let restore = window.coordinate(withNormalizedOffset: CGVector(dx: 0, dy: 0))
      .withOffset(CGVector(dx: previous.width - 2, dy: previous.height - 2))
    window.coordinate(withNormalizedOffset: CGVector(dx: 1, dy: 1))
      .withOffset(CGVector(dx: -2, dy: -2)).click(forDuration: 0.3, thenDragTo: restore)
    let restored = NSPredicate { _, _ in
      abs(window.frame.width - previous.width) < 3 && abs(window.frame.height - previous.height) < 3
    }
    let result = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: restored, object: nil)], timeout: 10)
    guard result == .completed else { throw InteractionFailure.expectedPresentation }
  }

  @MainActor
  private func exerciseCompactPagination(
    _ app: XCUIApplication, window: XCUIElement, source: URL, detail: XCUIElement
  ) async throws {
    try await requireText(app, containing: "Items page 1 of 2")
    try await requireText(app, containing: "Removed parts · page 1 of 2")
    XCTAssertFalse(app.buttons["Previous items"].isEnabled)
    XCTAssertFalse(app.buttons["Previous parts"].isEnabled)
    guard app.buttons["Next parts"].isEnabled, app.buttons["Next items"].isEnabled else {
      throw InteractionFailure.expectedPresentation
    }
    app.buttons["Next parts"].click()
    try await requireText(app, containing: "Removed parts · page 2 of 2")
    let lastPart = detail.value as? String ?? ""
    func observedParts(_ value: String, page: String) throws -> [String] {
      let range = try XCTUnwrap(value.range(of: "Removed parts · page " + page + "\n"))
      return value[range.upperBound...].components(separatedBy: "\n")
        .prefix { $0 != "Technical observations" }.filter { !$0.isEmpty }
        .map { $0.components(separatedBy: " · ")[0] }
    }
    XCTAssertEqual(try observedParts(lastPart, page: "2 of 2"), ["part-064.bin"])
    XCTAssertFalse(app.buttons["Next parts"].isEnabled)
    guard app.buttons["Previous parts"].isEnabled, app.buttons["Previous parts"].isHittable else {
      throw InteractionFailure.expectedPresentation
    }
    snapshot(app, "compact-removed-parts-second-page")
    app.buttons["Previous parts"].click()
    try await requireText(app, containing: "Removed parts · page 1 of 2")
    let firstParts = detail.value as? String ?? ""
    XCTAssertEqual(
      try observedParts(firstParts, page: "1 of 2"),
      (0..<64).map { String(format: "part-%03d.bin", $0) })
    app.buttons["Next items"].click()
    try await requireText(app, containing: "Items page 2 of 2")
    try await requireText(
      app,
      containing: "Source: "
        + source.deletingLastPathComponent().appendingPathComponent("064.eml").path)
    XCTAssertFalse(app.buttons["Next items"].isEnabled)
    guard app.buttons["Previous items"].isEnabled, app.buttons["Previous items"].isHittable,
      window.frame.contains(app.buttons["Previous items"].frame)
    else { throw InteractionFailure.expectedPresentation }
    snapshot(app, "compact-items-second-page")
    app.buttons["Previous items"].click()
    try await requireText(app, containing: "Items page 1 of 2")
    try await requireText(app, containing: "Source: " + source.path)
    try await requireText(app, containing: "Removed parts · page 1 of 2")
  }

}
