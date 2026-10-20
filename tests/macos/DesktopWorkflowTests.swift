// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import AppKit
import Darwin
import XCTest

extension DesktopInteractionTests {
  @MainActor
  func testOwnedDesktopWorkflowAndNativeIngress() async throws {
    let session = try await nativePickerPrelude(
      inputDirectory: "desktop-interaction", scenario: "workflow")
    try await exerciseServiceIngress(session)
    try await exerciseWaitingCapacity(session)
    try await exerciseReportExport(session)
    try await exerciseMissingCopyReveal(session)
    try await exerciseFinderDrop(session)
    try await exerciseCapturedDestination(session)
    try await finishNativePickerSession(session)
  }

  @MainActor
  private func exerciseServiceIngress(_ session: NativePickerSession) async throws {
    let serviceSource = session.serviceSource
    let serviceDisplayName = session.serviceDisplayName
    let golden = session.golden
    let ownership = session.ownership
    let identifier = session.identifier
    let owned = session.owned
    let app = session.app
    // Use the actual system Services route and an owned named pasteboard;
    // never replace the user's general clipboard or call the provider directly.
    let boardName = try XCTUnwrap(ownership["pasteboard"] as? String)
    guard !boardName.isEmpty, boardName != NSPasteboard.general.name.rawValue else {
      throw InteractionFailure.expectedPresentation
    }
    let serviceBoard = NSPasteboard(name: NSPasteboard.Name(boardName))
    // Cocoa requires the writing client to take ownership first, including
    // when a separate fixture process created this unique named board.
    serviceBoard.clearContents()
    guard serviceBoard.writeObjects([serviceSource as NSURL]) else {
      XCTFail("Could not write the owned native file URL to its named pasteboard")
      throw InteractionFailure.expectedPresentation
    }
    NSUpdateDynamicServices()
    guard !owned.isTerminated else { throw InteractionFailure.expectedPresentation }
    guard NSPerformService("Create EML Copies Without Attachments", serviceBoard) else {
      XCTFail("The registered native service did not accept the owned file URL")
      throw InteractionFailure.expectedPresentation
    }
    // Preserve the stock restricted-service protection. Accept its real
    // first-use prompt only inside this fresh hosted, owned-app fixture.
    let runService = app.dialogs.buttons["Run Service"]
    if runService.waitForExistence(timeout: 5) { runService.click() }
    try await requireText(app, containing: "Queue paused: dismiss a retained report to make room.")
    XCTAssertFalse(
      FileManager.default.fileExists(
        atPath: serviceSource.deletingPathExtension().appendingPathExtension("no-attachments.eml")
          .path))
    app.buttons["Dismiss report"].click()
    try await requireRun(
      app, counts: "1 created · 0 up to date", source: serviceSource,
      displayedName: serviceDisplayName)
    try await requireText(app, containing: serviceDisplayName)
    XCTAssertEqual(
      try Data(
        contentsOf: serviceSource.deletingPathExtension().appendingPathExtension(
          "no-attachments.eml")), golden)
    let serviceInstances = NSRunningApplication.runningApplications(
      withBundleIdentifier: identifier
    ).map(\.processIdentifier)
    XCTAssertEqual(serviceInstances, [owned.processIdentifier])
    guard serviceInstances == [owned.processIdentifier] else {
      throw InteractionFailure.expectedPresentation
    }
    snapshot(app, "services-finished-CJK")

  }

  @MainActor
  private func exerciseWaitingCapacity(_ session: NativePickerSession) async throws {
    let sources = session.sources
    let allSources = session.allSources
    let appURL = session.appURL
    let delivery = session.delivery
    let owned = session.owned
    let app = session.app
    let window = session.window
    let serviceSource = session.serviceSource
    let serviceDisplayName = session.serviceDisplayName
    let golden = session.golden
    let pid = owned.processIdentifier
    // Two retained full reports keep these real file-open requests waiting.
    // Observe each admission before sending the next, then exercise the
    // ninth-request refusal and remove each retained request through the UI.
    for expected in 1...8 {
      guard !owned.isTerminated else { throw InteractionFailure.expectedPresentation }
      let queued = try await NSWorkspace.shared.open(
        [sources[0]], withApplicationAt: appURL, configuration: delivery)
      guard queued.processIdentifier == pid else { throw InteractionFailure.expectedPresentation }
      try await requireWaiting(app, count: expected)
    }
    try await requireText(app, containing: "Queue paused: dismiss a retained report to make room.")
    snapshot(app, "eight-waiting-requests")
    let refused = try await NSWorkspace.shared.open(
      [sources[0]], withApplicationAt: appURL, configuration: delivery)
    guard refused.processIdentifier == pid else { throw InteractionFailure.expectedPresentation }
    try await requireText(
      app, containing: "8 requests are already waiting. Remove a waiting run before adding another."
    )
    try await requireWaiting(app, count: 8)
    snapshot(app, "ninth-request-refused")
    try await checkCompactReport(
      app, window: window, source: serviceSource, displayedName: serviceDisplayName)
    app.buttons["Dismiss notice"].click()
    for remaining in stride(from: 7, through: 0, by: -1) {
      // Read the actual native title: XCTest's filtered identifier/title query
      // did not resolve these buttons despite their retained AX snapshots.
      let available = NSPredicate { _, _ in
        window.buttons.allElementsBoundByIndex.contains { $0.title == "Remove waiting run" }
      }
      let visible = await XCTWaiter.fulfillment(
        of: [XCTNSPredicateExpectation(predicate: available, object: nil)], timeout: 10)
      guard visible == .completed,
        let remove = window.buttons.allElementsBoundByIndex.first(where: {
          $0.title == "Remove waiting run"
        })
      else {
        XCTFail("Visible waiting-run removal control is unavailable: \(app.debugDescription)")
        throw InteractionFailure.expectedPresentation
      }
      print(
        "Native removal control: exists=\(remove.exists), enabled=\(remove.isEnabled), hittable=\(remove.isHittable), frame=\(remove.frame)"
      )
      remove.click()
      try await requireWaiting(app, count: remaining)
    }
    for source in allSources {
      XCTAssertEqual(
        try Data(
          contentsOf: source.deletingPathExtension().appendingPathExtension("no-attachments.eml")),
        golden)
    }
    snapshot(app, "waiting-requests-removed")

  }

  @MainActor
  private func exerciseReportExport(_ session: NativePickerSession) async throws {
    let root = session.root
    let app = session.app
    let serviceSource = session.serviceSource
    let exportURL = root.appendingPathComponent("service-report.json")
    XCTAssertFalse(FileManager.default.fileExists(atPath: exportURL.path))
    try await saveReport(app, to: exportURL, expectNameCollision: false)
    try await requireText(app, containing: "Run 4 report: Saved and read back successfully.")
    let exported = try XCTUnwrap(
      JSONSerialization.jsonObject(with: Data(contentsOf: exportURL)) as? [String: Any])
    let items = try XCTUnwrap(exported["items"] as? [[String: Any]])
    XCTAssertEqual(items.count, 1)
    let item = try XCTUnwrap(items.first)
    XCTAssertEqual(item["status"] as? String, "created")
    XCTAssertEqual((item["removed"] as? [[String: Any]])?.count, 1)
    let source = try XCTUnwrap(item["source"] as? [String: Any])
    let nativeSource = try XCTUnwrap(source["native"] as? [String: Any])
    XCTAssertEqual(nativeSource["encoding"] as? String, "unix-bytes")
    let sourceBytes = try XCTUnwrap(
      serviceSource.withUnsafeFileSystemRepresentation { pointer in
        pointer.map { Data(bytes: $0, count: strlen($0)) }
      })
    XCTAssertEqual(nativeSource["base64"] as? String, sourceBytes.base64EncodedString())
    let summary = try XCTUnwrap(exported["summary"] as? [String: Any])
    let counts = try XCTUnwrap(summary["counts"] as? [String: Any])
    XCTAssertEqual(counts["created"] as? Int, 1)
    XCTAssertEqual(counts["failed"] as? Int, 0)
    XCTAssertEqual(counts["not_processed"] as? Int, 0)
    XCTAssertEqual(summary["exit_code"] as? Int, 0)
    XCTAssertEqual(summary["interrupted"] as? Bool, false)
    XCTAssertTrue(summary["run_error"] is NSNull)
    XCTAssertTrue(exported["protocol_failure"] is NSNull)
    let worker = try XCTUnwrap(exported["worker"] as? [String: Any])
    XCTAssertEqual(worker["settled"] as? Bool, true)
    XCTAssertEqual(worker["reusable"] as? Bool, true)
    XCTAssertTrue(worker["failure"] is NSNull)
    snapshot(app, "technical-report-exported")

    let collisionURL = root.appendingPathComponent("report-collision.json")
    let sentinel = Data("Owned report collision sentinel\n".utf8)
    XCTAssertEqual(try Data(contentsOf: collisionURL), sentinel)
    try await saveReport(app, to: collisionURL, expectNameCollision: true)
    XCTAssertFalse(app.buttons["Cancel export"].isEnabled)
    XCTAssertTrue(app.buttons["Export Report…"].isEnabled)
    XCTAssertEqual(try Data(contentsOf: collisionURL), sentinel)
    snapshot(app, "report-export-collision-cancelled")
    let retryURL = root.appendingPathComponent("service-report-after-cancel.json")
    XCTAssertFalse(FileManager.default.fileExists(atPath: retryURL.path))
    try await saveReport(app, to: retryURL, expectNameCollision: false)
    try await requireText(app, containing: "Run 4 report: Saved and read back successfully.")
    let retry = try XCTUnwrap(
      JSONSerialization.jsonObject(with: Data(contentsOf: retryURL)) as? [String: Any])
    let retryItems = try XCTUnwrap(retry["items"] as? [[String: Any]])
    XCTAssertEqual(retryItems.count, 1)
    let retrySource = try XCTUnwrap(retryItems.first?["source"] as? [String: Any])
    let retryNativeSource = try XCTUnwrap(retrySource["native"] as? [String: Any])
    XCTAssertEqual(retryNativeSource["base64"] as? String, sourceBytes.base64EncodedString())
    XCTAssertEqual(try Data(contentsOf: collisionURL), sentinel)
    snapshot(app, "report-export-retry-after-collision-cancel")

  }

  @MainActor
  private func exerciseMissingCopyReveal(_ session: NativePickerSession) async throws {
    let root = session.root
    let app = session.app
    let serviceSource = session.serviceSource
    let golden = session.golden
    let original = session.original
    // The external owner moves only this synthetic output. The receipt remains valid
    // historical evidence, while actual navigation must report its missing path.
    let serviceCopy = serviceSource.deletingPathExtension().appendingPathExtension(
      "no-attachments.eml")
    let heldCopy = root.appendingPathComponent("held-service-copy.eml")
    XCTAssertFalse(FileManager.default.fileExists(atPath: heldCopy.path))
    try DesktopFixtureRequest.holdCopy.send()
    let heldAcknowledgment = root.appendingPathComponent("copy-held")
    try await requireObservation(heldAcknowledgment, prefix: "owned copy held\n")
    XCTAssertEqual(try Data(contentsOf: heldAcknowledgment), Data("owned copy held\n".utf8))
    do {
      let reveal = app.buttons["Show in Finder"]
      XCTAssertTrue(reveal.isEnabled, "Earlier usable-copy receipt should permit navigation")
      reveal.click()
      try await requireText(
        app, containing: "The earlier reported copy is unavailable at that address.")
      snapshot(app, "finder-missing-copy-reported")
      XCTAssertEqual(try Data(contentsOf: heldCopy), golden)
      XCTAssertEqual(try Data(contentsOf: serviceSource), original)
      try DesktopFixtureRequest.restoreCopy.send()
      let restored = root.appendingPathComponent("copy-restored")
      try await requireObservation(restored, prefix: "owned copy restored\n")
      XCTAssertEqual(try Data(contentsOf: restored), Data("owned copy restored\n".utf8))
    } catch {
      // Restore the owned output even when the UI assertion fails.
      let primaryError = error
      do {
        try DesktopFixtureRequest.restoreCopy.send()
        let restored = root.appendingPathComponent("copy-restored")
        try await requireObservation(restored, prefix: "owned copy restored\n")
        XCTAssertEqual(try Data(contentsOf: restored), Data("owned copy restored\n".utf8))
      } catch {
        XCTFail("Owned copy restoration failed: \(error); primary UI failure: \(primaryError)")
      }
      throw primaryError
    }
    XCTAssertEqual(try Data(contentsOf: serviceCopy), golden)
    app.buttons["Dismiss notice"].click()

  }

  @MainActor
  private func exerciseFinderDrop(_ session: NativePickerSession) async throws {
    let root = session.root
    let window = session.window
    let app = session.app
    let sources = session.sources
    let owned = session.owned
    let golden = session.golden
    let original = session.original
    // Use the first report's copy in the uniquely named owned fixture root.
    // The Finder window can then be identified without touching another folder.
    let firstRun = try XCTUnwrap(
      window.buttons.allElementsBoundByIndex.first {
        $0.title.hasPrefix("Run 1\n")
      })
    firstRun.click()
    try await requireRun(app, counts: "2 created · 0 up to date", source: sources[0])
    let finder = XCUIApplication(bundleIdentifier: "com.apple.finder")
    let folderTitle = root.lastPathComponent
    let finderWindowCount = finder.windows.count
    guard finder.windows.allElementsBoundByIndex.allSatisfy({ $0.title != folderTitle }) else {
      XCTFail("Owned fixture Finder window already exists; no ambiguous window will be closed")
      throw InteractionFailure.expectedPresentation
    }
    app.buttons["Show in Finder"].click()
    let found = NSPredicate { _, _ in
      finder.windows.count == finderWindowCount + 1
        && finder.windows.allElementsBoundByIndex.filter { $0.title == folderTitle }.count == 1
    }
    let revealed = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: found, object: nil)], timeout: 15)
    guard revealed == .completed else {
      XCTFail("Finder did not reveal the owned copy's unique folder: \(finder.debugDescription)")
      throw InteractionFailure.expectedPresentation
    }
    let folder = try XCTUnwrap(
      finder.windows.allElementsBoundByIndex.first { $0.title == folderTitle })
    addTeardownBlock { @MainActor in
      try await self.closeOwnedFinderWindow(finder, title: folderTitle)
    }
    let firstCopy = sources[0].deletingPathExtension().appendingPathExtension("no-attachments.eml")
    _ = try finderEntry(folder, file: firstCopy, selected: true)
    snapshot(folder, "native-finder-revealed-exact-copy")

    // Move only this newly opened fixture window so the titlebar and drop target stay visible.
    let titlebar = folder.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0))
      .withOffset(CGVector(dx: 0, dy: 12))
    let position = window.coordinate(withNormalizedOffset: CGVector(dx: 0, dy: 0))
      .withOffset(CGVector(dx: 750, dy: 100))
    titlebar.click(forDuration: 0.3, thenDragTo: position)
    // Free one full-report reservation before a real native file drag.
    try await focusOwnedWindow(owned, window: window, avoiding: folder)
    app.buttons["Dismiss report"].click()
    let room = NSPredicate { _, _ in
      window.buttons.allElementsBoundByIndex.filter { $0.title.hasPrefix("Run ") }.count == 1
    }
    let freed = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: room, object: nil)], timeout: 10)
    guard freed == .completed else { throw InteractionFailure.expectedPresentation }
    finder.activate()
    let originalEntry = try finderEntry(folder, file: sources[0], selected: nil)
    originalEntry.click()
    let drop = window.coordinate(withNormalizedOffset: CGVector(dx: 0.1, dy: 0.7))
    guard originalEntry.isHittable, !folder.frame.contains(drop.screenPoint) else {
      XCTFail(
        "Owned source and product drop point are not both exposed: \(finder.debugDescription)")
      throw InteractionFailure.expectedPresentation
    }
    originalEntry.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
      .click(forDuration: 1, thenDragTo: drop)
    try await requireRun(app, counts: "0 created · 1 up to date", source: sources[0])
    XCTAssertEqual(try Data(contentsOf: firstCopy), golden)
    XCTAssertEqual(try Data(contentsOf: sources[0]), original)
    snapshot(app, "native-finder-file-drop-up-to-date")
    try await closeOwnedFinderWindow(finder, title: folderTitle)
    try await focusOwnedWindow(owned, window: window)

  }

  @MainActor
  private func exerciseCapturedDestination(_ session: NativePickerSession) async throws {
    let root = session.root
    let app = session.app
    let sources = session.sources
    let appURL = session.appURL
    let delivery = session.delivery
    let owned = session.owned
    let golden = session.golden
    let firstCopy = sources[0].deletingPathExtension().appendingPathExtension("no-attachments.eml")
    // Both report reservations are occupied. Capture a changed destination in
    // a queued request, then change the current choice before freeing its slot.
    let chosenDestination = root.appendingPathComponent("chosen-destination")
    try await choose(app, fixture: root, path: chosenDestination, folder: true, destination: true)
    try await requireAccessibleText(app, prefix: "Copy destination: " + chosenDestination.path)
    let queuedDestination = try await NSWorkspace.shared.open(
      [sources[0]], withApplicationAt: appURL, configuration: delivery)
    guard queuedDestination.processIdentifier == owned.processIdentifier else {
      throw InteractionFailure.expectedPresentation
    }
    try await requireWaiting(app, count: 1)
    let destinationCopy = chosenDestination.appendingPathComponent("one.no-attachments.eml")
    XCTAssertFalse(FileManager.default.fileExists(atPath: destinationCopy.path))
    app.buttons["Beside originals"].click()
    try await requireAccessibleText(app, prefix: "Copy destination: Beside each original")
    app.buttons["Dismiss report"].click()
    try await requireRun(app, counts: "1 created · 0 up to date", source: sources[0])
    try await requireText(app, containing: "Copy: " + destinationCopy.path)
    XCTAssertEqual(try Data(contentsOf: destinationCopy), golden)
    XCTAssertEqual(try Data(contentsOf: firstCopy), golden)
    try await requireWaiting(app, count: 0)
    snapshot(app, "queued-run-retains-chosen-destination")
  }

}
