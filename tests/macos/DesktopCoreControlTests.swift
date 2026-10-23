// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import AppKit
import Darwin
import XCTest

extension DesktopInteractionTests {
  @MainActor
  func testOwnedCoreStop() async throws { try await observedCoreScenario("stop") }
  @MainActor
  func testOwnedCoreForceStop() async throws { try await observedCoreScenario("force-stop") }
  @MainActor
  func testOwnedCoreActiveQuit() async throws { try await observedCoreScenario("active-quit") }
  @MainActor
  func testOwnedCorePublishedError() async throws {
    try await observedCoreScenario("published-error")
  }

  @MainActor
  private struct CoreControlSession {
    let fixture: URL
    let observation: URL
    let appURL: URL
    let originals: [URL]
    let golden: Data
    let owned: NSRunningApplication
    let app: XCUIApplication
    let delivery: NSWorkspace.OpenConfiguration
  }
  @MainActor
  func observedCoreScenario(_ scenario: String) async throws {
    let session = try await startCoreControlScenario(scenario)
    if scenario == "stop" { try await checkActiveCloseAndReopen(session) }
    if scenario == "active-quit" {
      try await checkActiveQuit(session)
      return
    }
    if scenario != "published-error" { try await interruptCoreRun(session, scenario: scenario) }
    try await checkCoreControlResult(session, scenario: scenario)
  }

  @MainActor
  private func startCoreControlScenario(_ scenario: String) async throws -> CoreControlSession {
    let repository = URL(fileURLWithPath: #filePath)
      .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    let inputURL = repository.appendingPathComponent(
      "target/desktop-interaction-\(scenario)/input.json")
    let input = try XCTUnwrap(
      JSONSerialization.jsonObject(with: Data(contentsOf: inputURL)) as? [String: String])
    XCTAssertEqual(input["scenario"], scenario)
    XCTAssertEqual(input["host_scope"], "fresh-github-hosted-runner")
    let appURL = URL(fileURLWithPath: try XCTUnwrap(input["app"]))
    let fixture = URL(fileURLWithPath: try XCTUnwrap(input["fixture"]))
    let observation = URL(fileURLWithPath: try XCTUnwrap(input["observation"]))
    let receipt = try XCTUnwrap(
      JSONSerialization.jsonObject(
        with: Data(
          contentsOf: inputURL.deletingLastPathComponent().appendingPathComponent("ownership.json"))
      ) as? [String: Any])
    let pid = try XCTUnwrap(receipt["pid"] as? NSNumber).int32Value
    let owned = try XCTUnwrap(NSRunningApplication(processIdentifier: pid))
    let bundle = try XCTUnwrap(Bundle(url: appURL))
    guard owned.bundleURL?.standardizedFileURL == appURL.standardizedFileURL,
      owned.bundleIdentifier == bundle.bundleIdentifier,
      owned.launchDate?.timeIntervalSince1970 == (receipt["launch_date"] as? NSNumber)?.doubleValue,
      !owned.isTerminated,
      NSRunningApplication.runningApplications(
        withBundleIdentifier: try XCTUnwrap(bundle.bundleIdentifier)
      ).count == 1
    else { throw InteractionFailure.expectedPresentation }
    let original = try Data(
      contentsOf: repository.appendingPathComponent(
        "tests/contract/mime/inputs/01-gmail-attachment.eml"))
    let golden = try Data(
      contentsOf: repository.appendingPathComponent(
        "tests/contract/mime/golden/01-gmail-attachment.eml"))
    let originals = [
      "one.eml", "two.eml", "folder-input/folder.eml", "service-input/日本語-مرحبا-\u{202e}report.eml",
    ].map {
      fixture.appendingPathComponent($0)
    }
    addTeardownBlock { @MainActor in
      for source in originals { XCTAssertEqual(try Data(contentsOf: source), original) }
    }
    let app = XCUIApplication(url: appURL)
    _ = owned.activate(options: [])
    XCTAssertTrue(app.windows["EML Attachment Remover"].waitForExistence(timeout: 20))
    try await requireOwnedForeground(owned)
    let delivery = NSWorkspace.OpenConfiguration()
    delivery.createsNewApplicationInstance = false
    delivery.addsToRecentItems = false
    let paths = Array(originals.prefix(scenario == "published-error" ? 1 : 2))
    let delivered = try await NSWorkspace.shared.open(
      paths, withApplicationAt: appURL, configuration: delivery)
    guard delivered.processIdentifier == pid else { throw InteractionFailure.expectedPresentation }
    try await requireObservation(
      observation.appendingPathComponent("ready"),
      prefix:
        "operation=processing phase=\(scenario == "published-error" ? "after-publication" : "publication") index=\(scenario == "published-error" ? 0 : 1) operation_id="
    )

    return CoreControlSession(
      fixture: fixture, observation: observation, appURL: appURL, originals: originals,
      golden: golden, owned: owned, app: app, delivery: delivery)
  }

  @MainActor
  private func checkActiveCloseAndReopen(_ session: CoreControlSession) async throws {
    let app = session.app
    let owned = session.owned
    let appURL = session.appURL
    let delivery = session.delivery
    let observation = session.observation
    let originals = session.originals
    let pid = session.owned.processIdentifier

    let window = app.windows["EML Attachment Remover"]
    window.buttons["_XCUI:CloseWindow"].click()
    let hidden = NSPredicate { _, _ in !window.exists }
    guard
      await XCTWaiter.fulfillment(
        of: [XCTNSPredicateExpectation(predicate: hidden, object: nil)], timeout: 10)
        == .completed,
      !owned.isTerminated
    else { throw InteractionFailure.expectedPresentation }
    let reopened = try await NSWorkspace.shared.openApplication(
      at: appURL, configuration: delivery)
    guard reopened.processIdentifier == pid, window.waitForExistence(timeout: 10) else {
      throw InteractionFailure.expectedPresentation
    }
    XCTAssertTrue(app.buttons["Stop"].isEnabled)
    XCTAssertFalse(
      FileManager.default.fileExists(
        atPath: observation.appendingPathComponent("first-interrupt").path))
    try await showReportDetails(app)
    try await requireText(app, containing: originals[0].lastPathComponent)
    try await showRunControls(app)
    try await requireAccessibleText(app, prefix: "Processing")
    snapshot(app, "active-work-retained-after-close-reopen")
  }

  @MainActor
  private func checkActiveQuit(_ session: CoreControlSession) async throws {
    let app = session.app
    let owned = session.owned
    let appURL = session.appURL
    let delivery = session.delivery
    let observation = session.observation
    let originals = session.originals
    let pid = session.owned.processIdentifier

    snapshot(app, "real-core-running-before-active-quit")
    let queued = try await NSWorkspace.shared.open(
      [originals[0]], withApplicationAt: appURL, configuration: delivery)
    XCTAssertEqual(queued.processIdentifier, pid)
    try await requireWaiting(app, count: 1)
    app.typeKey("q", modifierFlags: .command)
    XCTAssertTrue(app.buttons["Cancel"].waitForExistence(timeout: 5))
    app.buttons["Cancel"].click()
    XCTAssertFalse(owned.isTerminated)
    XCTAssertFalse(
      FileManager.default.fileExists(
        atPath: observation.appendingPathComponent("first-interrupt").path))
    snapshot(app, "active-quit-cancelled")
    app.typeKey("q", modifierFlags: .command)
    XCTAssertTrue(app.buttons["Quit"].waitForExistence(timeout: 5))
    app.buttons["Quit"].click()
    XCTAssertTrue(app.wait(for: .notRunning, timeout: 20))
    XCTAssertTrue(owned.isTerminated)
    try await requireObservation(
      observation.appendingPathComponent("second-interrupt"), prefix: "observed ")
    return
  }

  @MainActor
  private func interruptCoreRun(_ session: CoreControlSession, scenario: String) async throws {
    let app = session.app
    let observation = session.observation

    try await showRunControls(app)
    snapshot(app, "real-core-running-before-\(scenario)")
    XCTAssertTrue(app.buttons["Stop"].isEnabled)
    app.buttons["Stop"].click()
    try await requireObservation(
      observation.appendingPathComponent("first-interrupt"), prefix: "observed ")
    if scenario == "force-stop" {
      XCTAssertTrue(app.buttons["Force Stop…"].isEnabled)
      app.buttons["Force Stop…"].click()
      XCTAssertTrue(app.buttons["Cancel"].waitForExistence(timeout: 5))
      app.buttons["Cancel"].click()
      XCTAssertFalse(
        FileManager.default.fileExists(
          atPath: observation.appendingPathComponent("second-interrupt").path))
      app.buttons["Force Stop…"].click()
      XCTAssertTrue(app.buttons["Force Stop"].waitForExistence(timeout: 5))
      app.buttons["Force Stop"].click()
      try await requireObservation(
        observation.appendingPathComponent("second-interrupt"), prefix: "observed ")
    }
  }

  @MainActor
  private func checkCoreControlResult(_ session: CoreControlSession, scenario: String) async throws
  {
    let app = session.app
    let owned = session.owned
    let fixture = session.fixture
    let originals = session.originals
    let golden = session.golden
    try await showReportDetails(app)
    let technical = app.checkBoxes["Technical details"]
    technical.click()
    try await requireText(app, containing: "settled true")
    if scenario == "published-error" {
      try await requireText(app, containing: "Protocol failure: None")
      try await requireText(
        app,
        containing: "Worker exit Some(1); signal None; failure None; settled true; reusable true")
      try await requireText(
        app, containing: "0 created · 0 up to date · 1 failed · 0 not processed")
      try await requireText(app, containing: "ADDRESS_CHANGED")
      try await requireText(app, containing: "Copy published with an error")
      try await requireText(app, containing: "Interrupt calls 0; first interrupt observed false")
      XCTAssertFalse(app.buttons["Show in Finder"].isEnabled)
      XCTAssertEqual(
        try Data(
          contentsOf: expectedCopy(
            for: originals[0], directory: fixture.appendingPathComponent("published-output"))),
        golden)
      snapshot(app, "real-core-published-with-error")
    } else {
      try await requireText(
        app,
        containing: "Interrupt calls \(scenario == "stop" ? 1 : 2); first interrupt observed true")
      if scenario == "stop" {
        try await requireText(app, containing: "Protocol failure: None")
        try await requireText(
          app,
          containing:
            "Worker exit Some(130); signal None; failure None; settled true; reusable true")
        try await requireText(
          app, containing: "1 created · 0 up to date · 1 failed · 0 not processed")
      } else {
        try await requireText(
          app,
          containing:
            "1 item has no reported result. Complete copies may exist; inspect the destination.")
        try await checkCompactReport(
          app, window: app.windows["EML Attachment Remover"], source: originals[0])
      }
      XCTAssertEqual(
        try Data(
          contentsOf: expectedCopy(
            for: originals[0], directory: fixture.appendingPathComponent("publication-output"))),
        golden)
      XCTAssertFalse(
        FileManager.default.fileExists(
          atPath: expectedCopy(
            for: originals[1], directory: fixture.appendingPathComponent("publication-output")
          ).path))
      snapshot(app, "real-core-\(scenario)-settled")
    }
    app.typeKey("q", modifierFlags: .command)
    XCTAssertTrue(app.buttons["Quit"].waitForExistence(timeout: 5))
    app.buttons["Quit"].click()
    XCTAssertTrue(app.wait(for: .notRunning, timeout: 20))
    XCTAssertTrue(owned.isTerminated)
  }

  @MainActor
  func requireObservation(_ url: URL, prefix: String) async throws {
    let predicate = NSPredicate { _, _ in
      guard let metadata = try? FileManager.default.attributesOfItem(atPath: url.path),
        let size = metadata[.size] as? NSNumber, size.intValue <= 128,
        let bytes = try? Data(contentsOf: url), bytes.count <= 128,
        let text = String(data: bytes, encoding: .utf8)
      else { return false }
      return text.hasPrefix(prefix)
    }
    let result = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: predicate, object: nil)], timeout: 20)
    guard result == .completed else {
      XCTFail("Owned fixture observation unavailable: \(url.lastPathComponent)")
      throw InteractionFailure.expectedPresentation
    }
  }

}
