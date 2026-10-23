// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import AppKit
import CryptoKit
import Darwin
import XCTest

extension DesktopInteractionTests {
  @MainActor
  func testOwnedBlockedProcessingPreflight() async throws {
    try await blockedChild(operation: "processing", phase: "preflight")
  }
  @MainActor
  func testOwnedBlockedProcessingReadback() async throws {
    try await blockedChild(operation: "processing", phase: "readback")
  }
  @MainActor
  func testOwnedBlockedProcessingStageWrite() async throws {
    try await blockedChild(operation: "processing", phase: "stage-write")
  }
  @MainActor
  func testOwnedBlockedProcessingFileSync() async throws {
    try await blockedChild(operation: "processing", phase: "file-sync")
  }
  @MainActor
  func testOwnedBlockedProcessingPublication() async throws {
    try await blockedChild(operation: "processing", phase: "publication")
  }
  @MainActor
  func testOwnedBlockedProcessingNamespaceSync() async throws {
    try await blockedChild(operation: "processing", phase: "namespace-sync")
  }
  @MainActor
  func testOwnedBlockedExportPreflight() async throws {
    try await dualChildQuit()
  }
  @MainActor
  func testOwnedBlockedExportReadback() async throws {
    try await blockedChild(operation: "export", phase: "readback")
  }
  @MainActor
  func testOwnedBlockedExportStageWrite() async throws {
    try await blockedChild(operation: "export", phase: "stage-write")
  }
  @MainActor
  func testOwnedBlockedExportFileSync() async throws {
    try await blockedChild(operation: "export", phase: "file-sync")
  }
  @MainActor
  func testOwnedBlockedExportPublication() async throws {
    try await blockedChild(operation: "export", phase: "publication")
  }
  @MainActor
  func testOwnedBlockedExportNamespaceSync() async throws {
    try await blockedChild(operation: "export", phase: "namespace-sync")
  }
  @MainActor
  private struct ChildSession {
    let app: XCUIApplication
    let owned: NSRunningApplication
    let fixture: URL
    let observation: URL
    let delivery: NSWorkspace.OpenConfiguration
  }
  @MainActor
  private func childSession(_ scenario: String) throws -> ChildSession {
    let repository = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
      .deletingLastPathComponent().deletingLastPathComponent()
    let inputURL = repository.appendingPathComponent(
      "target/desktop-interaction-\(scenario)/input.json")
    let input = try XCTUnwrap(
      JSONSerialization.jsonObject(with: Data(contentsOf: inputURL)) as? [String: String])
    XCTAssertEqual(input["scenario"], scenario)
    XCTAssertEqual(input["host_scope"], "fresh-github-hosted-runner")
    let appURL = URL(fileURLWithPath: try XCTUnwrap(input["app"]))
    let fixture = URL(fileURLWithPath: try XCTUnwrap(input["fixture"]))
    let observation = URL(fileURLWithPath: try XCTUnwrap(input["observation"]))
    let ownership = try XCTUnwrap(
      JSONSerialization.jsonObject(
        with: Data(
          contentsOf: inputURL.deletingLastPathComponent().appendingPathComponent("ownership.json"))
      ) as? [String: Any])
    let pid = try XCTUnwrap(ownership["pid"] as? NSNumber).int32Value
    let owned = try XCTUnwrap(NSRunningApplication(processIdentifier: pid))
    guard owned.bundleURL?.standardizedFileURL == appURL.standardizedFileURL,
      owned.launchDate?.timeIntervalSince1970
        == (ownership["launch_date"] as? NSNumber)?.doubleValue,
      !owned.isTerminated
    else { throw InteractionFailure.expectedPresentation }
    let app = XCUIApplication(url: appURL)
    _ = owned.activate(options: [])
    guard app.windows["EML Attachment Remover"].waitForExistence(timeout: 20) else {
      throw InteractionFailure.expectedPresentation
    }
    let delivery = NSWorkspace.OpenConfiguration()
    delivery.createsNewApplicationInstance = false
    delivery.addsToRecentItems = false
    return ChildSession(
      app: app, owned: owned, fixture: fixture, observation: observation, delivery: delivery)
  }
  @MainActor
  private func deliver(_ session: ChildSession, _ name: String) async throws {
    let actual = try await NSWorkspace.shared.open(
      [session.fixture.appendingPathComponent(name)],
      withApplicationAt: try XCTUnwrap(session.owned.bundleURL), configuration: session.delivery)
    XCTAssertEqual(actual.processIdentifier, session.owned.processIdentifier)
  }
  @MainActor
  private func selectReport(_ app: XCUIApplication, run: Int) async throws {
    try await showReportDetails(app)
    let button = try XCTUnwrap(
      app.buttons.allElementsBoundByIndex.first { $0.title.hasPrefix("Run \(run)\n") })
    button.click()
    try await showRunControls(app)
  }
  @MainActor
  private func arm(_ session: ChildSession, recipe: String) throws {
    try Data((recipe + "\n").utf8).write(
      to: session.observation.appendingPathComponent("observation"), options: .withoutOverwriting)
  }
  @MainActor
  private func awaitBoundary(_ session: ChildSession, operation: String, phase: String) async throws
    -> UInt64
  {
    let prefix =
      "operation=\(operation) phase=\(phase) index=\(operation == "processing" ? "0" : "none") operation_id="
    let ready = session.observation.appendingPathComponent("ready")
    try await requireObservation(ready, prefix: prefix)
    let text = try String(contentsOf: ready, encoding: .utf8)
    let id = try XCTUnwrap(
      UInt64(text.dropFirst(prefix.count).trimmingCharacters(in: .whitespacesAndNewlines)))
    XCTAssertGreaterThan(id, 0)
    return id
  }
  @MainActor
  private func observedWorkerPID(_ session: ChildSession) throws -> Int32 {
    let bytes = try Data(contentsOf: session.observation.appendingPathComponent("worker-pid"))
    XCTAssertLessThanOrEqual(bytes.count, 128)
    let pid = try XCTUnwrap(
      Int32(String(decoding: bytes, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)))
    XCTAssertGreaterThan(pid, 0)
    return pid
  }
  @MainActor
  private func observedNumber(_ app: XCUIApplication, pattern: String) throws -> UInt64 {
    let expression = try NSRegularExpression(pattern: pattern)
    let texts = try presentationSnapshots(app).map {
      $0.title + "\n" + $0.label + "\n" + (($0.value as? String) ?? "")
    }.joined(separator: "\n")
    let match = try XCTUnwrap(
      expression.firstMatch(in: texts, range: NSRange(texts.startIndex..., in: texts)))
    let range = try XCTUnwrap(Range(match.range(at: 1), in: texts))
    return try XCTUnwrap(UInt64(texts[range]))
  }
  @MainActor
  private func responsive(_ app: XCUIApplication) async throws {
    try await showReportDetails(app)
    let technical = app.checkBoxes["Technical details"]
    let before = String(describing: technical.value)
    technical.click()
    let changed = NSPredicate { _, _ in String(describing: technical.value) != before }
    guard
      await XCTWaiter.fulfillment(
        of: [XCTNSPredicateExpectation(predicate: changed, object: nil)], timeout: 10) == .completed
    else {
      throw InteractionFailure.expectedPresentation
    }
    technical.click()
    try await showRunControls(app)
  }
  @MainActor
  private func stopBlocked(_ session: ChildSession, operation: String) async throws {
    let app = session.app
    try await showRunControls(app)
    let stop = app.buttons[operation == "processing" ? "Stop" : "Cancel Export"]
    XCTAssertTrue(stop.isEnabled)
    stop.click()
    try await requireObservation(
      session.observation.appendingPathComponent("first-interrupt"), prefix: "observed ")
    let force = app.buttons[operation == "processing" ? "Force Stop…" : "Force Stop Export…"]
    let forceReady = NSPredicate { _, _ in force.isEnabled }
    guard
      await XCTWaiter.fulfillment(
        of: [XCTNSPredicateExpectation(predicate: forceReady, object: nil)], timeout: 10)
        == .completed
    else {
      throw InteractionFailure.expectedPresentation
    }
    force.click()
    let accept = app.buttons[operation == "processing" ? "Force Stop" : "Force Stop Export"]
    guard accept.waitForExistence(timeout: 5) else { throw InteractionFailure.expectedPresentation }
    accept.click()
    try await requireObservation(
      session.observation.appendingPathComponent("second-interrupt"), prefix: "observed ")
    let settled = NSPredicate { _, _ in !stop.isEnabled && !force.isEnabled }
    guard
      await XCTWaiter.fulfillment(
        of: [XCTNSPredicateExpectation(predicate: settled, object: nil)], timeout: 20) == .completed
    else {
      throw InteractionFailure.expectedPresentation
    }
  }
  @MainActor
  private func finishChild(_ session: ChildSession) async throws {
    session.app.typeKey("q", modifierFlags: .command)
    guard session.app.buttons["Quit"].waitForExistence(timeout: 5) else {
      throw InteractionFailure.expectedPresentation
    }
    session.app.buttons["Quit"].click()
    XCTAssertTrue(session.app.wait(for: .notRunning, timeout: 20))
    XCTAssertTrue(session.owned.isTerminated)
  }
  @MainActor
  private func assertBlockedEffect(_ session: ChildSession, operation: String, phase: String) throws
  {
    let target =
      operation == "export"
      ? session.fixture.appendingPathComponent("blocked-report.json")
      : expectedCopy(
        for: session.fixture.appendingPathComponent("two.eml"),
        directory: session.fixture.appendingPathComponent("publication-output"))
    if ["readback", "namespace-sync"].contains(phase) {
      let bytes = try Data(contentsOf: target)
      if operation == "processing" {
        let repository = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
          .deletingLastPathComponent().deletingLastPathComponent()
        XCTAssertEqual(
          bytes,
          try Data(
            contentsOf: repository.appendingPathComponent(
              "tests/contract/mime/golden/01-gmail-attachment.eml")))
      } else {
        XCTAssertGreaterThan(bytes.count, 0)
        let document = try XCTUnwrap(JSONSerialization.jsonObject(with: bytes) as? [String: Any])
        XCTAssertEqual(document["schema"] as? String, "eml-technical-report")
        XCTAssertEqual(document["run_id"] as? Int, 1)
      }
    } else {
      XCTAssertFalse(FileManager.default.fileExists(atPath: target.path))
    }
  }
  @MainActor
  private func blockedChild(operation: String, phase: String) async throws {
    let scenario = "blocked-\(operation)-\(phase)"
    let session = try childSession(scenario)
    let app = session.app
    try await deliver(session, "one.eml")
    try await requireRun(
      app, counts: "1 created · 0 up to date",
      source: session.fixture.appendingPathComponent("one.eml"))
    if operation == "export" {
      try await deliver(session, "two.eml")
      try await requireRun(
        app, counts: "1 created · 0 up to date",
        source: session.fixture.appendingPathComponent("two.eml"))
      try await selectReport(app, run: 1)
      try arm(session, recipe: "export:\(phase)")
      try await saveReport(
        app, to: session.fixture.appendingPathComponent("blocked-report.json"),
        expectNameCollision: false)
    } else {
      try arm(session, recipe: "processing:\(phase):0")
      try await deliver(session, "two.eml")
    }
    let operationID = try await awaitBoundary(session, operation: operation, phase: phase)
    try assertBlockedEffect(session, operation: operation, phase: phase)
    let responseBegan = ProcessInfo.processInfo.systemUptime
    try await showReportDetails(app)
    let runID = try observedNumber(app, pattern: "Run summary: Run ([0-9]+)")
    try await showRunControls(app)
    XCTAssertEqual(runID, operation == "export" ? 1 : 2)
    if operation == "export" {
      try await requireText(app, containing: "Export operation \(operationID):")
      XCTAssertEqual(
        try observedNumber(app, pattern: "Export operation ([0-9]+):"), operationID)
    } else {
      try await showReportDetails(app)
      let technical = app.checkBoxes["Technical details"]
      if ((technical.value as? String) ?? (technical.value as? NSNumber)?.stringValue) != "1" {
        technical.click()
      }
      try await requireText(app, containing: "Processing operation: \(operationID)")
      XCTAssertEqual(
        try observedNumber(app, pattern: "Processing operation: ([0-9]+)"), operationID)
    }
    if operation == "export" {
      try await selectReport(app, run: 2)
      try await requireRun(
        app, counts: "1 created · 0 up to date",
        source: session.fixture.appendingPathComponent("two.eml"))
      try await showRunControls(app)
      XCTAssertTrue(app.buttons["Dismiss report"].isEnabled)
      try await deliver(session, "folder-input/folder.eml")
      try await requireWaiting(app, count: 1)
      try await responsive(app)
      app.buttons["Dismiss report"].click()
      try await requireWaiting(app, count: 0)
      try await requireRun(
        app, counts: "1 created · 0 up to date",
        source: session.fixture.appendingPathComponent("folder-input/folder.eml"))
      try await selectReport(app, run: 1)
      XCTAssertFalse(app.buttons["Dismiss report"].isEnabled)
      XCTAssertTrue(app.buttons["Cancel Export"].isEnabled)
    } else {
      try await selectReport(app, run: 1)
      try await requireRun(
        app, counts: "1 created · 0 up to date",
        source: session.fixture.appendingPathComponent("one.eml"))
      XCTAssertTrue(app.buttons["Stop"].isEnabled)
    }
    try await responsive(app)
    snapshot(app, scenario + "-responsive")
    try await stopBlocked(session, operation: operation)
    try assertBlockedEffect(session, operation: operation, phase: phase)
    if operation == "processing" { try await selectReport(app, run: 2) }
    try await showReportDetails(app)
    let technical = app.checkBoxes["Technical details"]
    if ((technical.value as? String) ?? (technical.value as? NSNumber)?.stringValue) != "1" {
      technical.click()
    }
    if operation == "processing" { try await requireText(app, containing: "settled true") }
    if operation == "export" {
      try await requireText(app, containing: "Report export operation \(operationID) for run 1")
      try await requireText(app, containing: "settled: true")
    }
    let settledText =
      try presentationSnapshots(app).first {
        $0.title == "Selected message, removed parts and technical observations"
      }?.value as? String ?? ""
    let facts: [String: Any] = [
      "case": scenario, "operation": operation, "phase": phase,
      "operation_id": operationID, "worker_pid": try observedWorkerPID(session), "run_id": runID,
      "ui_response_seconds": ProcessInfo.processInfo.systemUptime - responseBegan,
      "ui_responsive": true, "other_report_viewed": true,
      "source_reserved": operation == "export", "queued_then_processing": operation == "export",
      "first_stop_observed": true, "force_stop_observed": true,
      "settled_text": settledText,
    ]
    try JSONSerialization.data(withJSONObject: facts, options: [.sortedKeys]).write(
      to: session.fixture.appendingPathComponent("child-ui-observation.json"),
      options: .withoutOverwriting)
    try await finishChild(session)
  }
  @MainActor
  private func changeChildMode(_ app: XCUIApplication, check: Bool) async throws {
    try await showRunControls(app)
    app.popUpButtons["Operation mode"].click()
    let label = check ? "Check only" : "Create copies"
    let choice = app.descendants(matching: .any).matching(
      NSPredicate(format: "label == %@ OR title == %@", label, label)
    ).firstMatch
    guard choice.waitForExistence(timeout: 5) else { throw InteractionFailure.expectedPresentation }
    choice.click()
  }
  @MainActor
  func testOwnedExportWriterBlock() async throws {
    let session = try childSession("export-writer-block")
    let app = session.app
    try await changeChildMode(app, check: true)
    try await deliver(session, "writer-input")
    try await requireText(app, containing: "1024 planned · Check only")
    // Measure this immutable retained document before introducing the owned
    // writer interruption. No later request relies on transport reusability.
    let measured = session.fixture.appendingPathComponent("measured-report.json")
    try await saveReport(app, to: measured, expectNameCollision: false)
    try await requireText(app, containing: "Run 1 report: Report exported and verified.")
    let bytes = try Data(contentsOf: measured)
    XCTAssertGreaterThan(bytes.count, 1024 * 1024)
    let document = try XCTUnwrap(JSONSerialization.jsonObject(with: bytes) as? [String: Any])
    XCTAssertEqual((document["items"] as? [Any])?.count, 1024)
    try await changeChildMode(app, check: false)
    try await deliver(session, "one.eml")
    try await requireRun(
      app, counts: "1 created · 0 up to date",
      source: session.fixture.appendingPathComponent("one.eml"))
    try await selectReport(app, run: 1)
    try arm(session, recipe: "export:request-body")
    try await saveReport(
      app, to: session.fixture.appendingPathComponent("blocked-report.json"),
      expectNameCollision: false)
    let operationID = try await awaitBoundary(session, operation: "export", phase: "request-body")
    XCTAssertFalse(
      FileManager.default.fileExists(
        atPath: session.fixture.appendingPathComponent("blocked-report.json").path))
    // With no body read and a report larger than the pipe, this is the actual
    // parent writer holding its shared report lock, observed by try-inspection.
    try await requireText(app, containing: "Report is busy; waiting for results.")
    try await selectReport(app, run: 2)
    try await requireRun(
      app, counts: "1 created · 0 up to date",
      source: session.fixture.appendingPathComponent("one.eml"))
    try await responsive(app)
    XCTAssertTrue(app.buttons["Cancel Export"].isEnabled)
    app.buttons["Cancel Export"].click()
    try await requireObservation(
      session.observation.appendingPathComponent("first-interrupt"), prefix: "observed ")
    let ended = NSPredicate { _, _ in !app.buttons["Cancel Export"].isEnabled }
    guard
      await XCTWaiter.fulfillment(
        of: [XCTNSPredicateExpectation(predicate: ended, object: nil)], timeout: 20) == .completed
    else { throw InteractionFailure.expectedPresentation }
    try await responsive(app)
    XCTAssertFalse(
      FileManager.default.fileExists(
        atPath: session.fixture.appendingPathComponent("blocked-report.json").path))
    let facts: [String: Any] = [
      "case": "export-writer-block", "operation": "export", "phase": "request-body",
      "operation_id": operationID, "worker_pid": try observedWorkerPID(session),
      "run_id": 1, "writer_busy_observed": true, "other_report_viewed": true, "ui_responsive": true,
      "document_bytes": bytes.count,
      "document_sha256": SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined(),
      "first_stop_observed": true, "source_reserved": true,
    ]
    try JSONSerialization.data(withJSONObject: facts, options: [.sortedKeys]).write(
      to: session.fixture.appendingPathComponent("child-ui-observation.json"),
      options: .withoutOverwriting)
    try await finishChild(session)
  }

  @MainActor
  private func waitExport(_ app: XCUIApplication, text: String) async throws {
    try await requireText(app, containing: text)
    let settled = NSPredicate { _, _ in !app.buttons["Cancel Export"].isEnabled }
    guard
      await XCTWaiter.fulfillment(
        of: [XCTNSPredicateExpectation(predicate: settled, object: nil)], timeout: 20) == .completed
    else { throw InteractionFailure.expectedPresentation }
  }
  @MainActor
  private func releaseChild(_ session: ChildSession) throws {
    let pending = session.observation.appendingPathComponent("release-" + UUID().uuidString)
    try Data("release\n".utf8).write(to: pending, options: .withoutOverwriting)
    try FileManager.default.linkItem(
      at: pending, to: session.observation.appendingPathComponent("release"))
    try FileManager.default.removeItem(at: pending)
  }
  @MainActor
  private func archiveControl(_ session: ChildSession, label: String) throws {
    let history = session.fixture.appendingPathComponent("control-history/" + label)
    try FileManager.default.createDirectory(
      at: history.deletingLastPathComponent(), withIntermediateDirectories: true)
    // Callers first observe the export lane settled (actual process and pipe
    // threads). Preserve this complete admitted directory, then create the next
    // operation's independent private directory at the environment's path.
    try FileManager.default.moveItem(at: session.observation, to: history)
    try FileManager.default.createDirectory(
      at: session.observation, withIntermediateDirectories: false,
      attributes: [.posixPermissions: 0o700])
  }
  @MainActor
  private func independentCheckReport(_ session: ChildSession) throws -> Data {
    let repository = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
      .deletingLastPathComponent().deletingLastPathComponent()
    let bundle = try XCTUnwrap(Bundle(url: try XCTUnwrap(session.owned.bundleURL)))
    let version = try XCTUnwrap(
      bundle.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String)
    let build = try XCTUnwrap(bundle.object(forInfoDictionaryKey: "CFBundleVersion") as? String)
    let rows = try String(
      contentsOf: repository.appendingPathComponent(
        "tests/contract/protocol/created-attachment.jsonl"), encoding: .utf8
    ).split(separator: "\n").map(String.init)
    let planned = try String(
      contentsOf: repository.appendingPathComponent(
        "tests/contract/protocol/planned-create-unix.jsonl"), encoding: .utf8
    ).split(separator: "\n").map(String.init)
    func literal(_ text: String) throws -> String {
      String(
        decoding: try JSONSerialization.data(
          withJSONObject: text, options: [.fragmentsAllowed, .withoutEscapingSlashes]),
        as: UTF8.self)
    }
    func path(_ url: URL) throws -> String {
      let native = try XCTUnwrap(
        url.withUnsafeFileSystemRepresentation { $0.map { Data(bytes: $0, count: strlen($0)) } })
      return "{\"display\":" + (try literal(url.path))
        + ",\"native\":{\"encoding\":\"unix-bytes\",\"base64\":"
        + (try literal(native.base64EncodedString())) + "}}"
    }
    let source = session.fixture.appendingPathComponent("one.eml")
    let destination = expectedCopy(
      for: source, directory: session.fixture.appendingPathComponent("publication-output"))
    var item = rows[1]
    let oldSource = try XCTUnwrap(item.range(of: "\"source\":"))
    let oldStatus = try XCTUnwrap(item.range(of: ",\"status\":"))
    item.replaceSubrange(
      oldSource.lowerBound..<oldStatus.lowerBound, with: "\"source\":" + (try path(source)))
    let startDestination = try XCTUnwrap(item.range(of: "\"destination\":"))
    let endDestination = try XCTUnwrap(item.range(of: ",\"source_bytes\":"))
    item.replaceSubrange(
      startDestination.lowerBound..<endDestination.lowerBound,
      with: "\"destination\":" + (try path(destination)))
    item = item.replacingOccurrences(of: "\"status\":\"created\"", with: "\"status\":\"planned\"")
      .replacingOccurrences(of: "\"plan\":null", with: "\"plan\":\"create\"")
    let pStart = try XCTUnwrap(planned[1].range(of: "\"publication\":"))
    let pEnd = try XCTUnwrap(planned[1].range(of: ",\"existing_observation\":"))
    let startPublication = try XCTUnwrap(item.range(of: "\"publication\":"))
    let endPublication = try XCTUnwrap(item.range(of: ",\"existing_observation\":"))
    item.replaceSubrange(
      startPublication.lowerBound..<endPublication.lowerBound,
      with: String(planned[1][pStart.lowerBound..<pEnd.lowerBound]))
    func bareRecord(_ line: String, type: String) throws -> String {
      let prefix = "{\"type\":\"" + type + "\","
      guard line.hasPrefix(prefix) else { throw InteractionFailure.expectedPresentation }
      return "{" + line.dropFirst(prefix.count)
    }
    item = try bareRecord(item, type: "item")
    let start = try bareRecord(
      planned[0].replacingOccurrences(of: "@VERSION@", with: version), type: "start")
    let summary = try bareRecord(planned[2], type: "summary")
    let worker =
      "{\"operation_id\":1,\"exit_code\":0,\"signal\":null,\"input_may_have_been_sent\":true,\"settled\":true,\"reusable\":true,\"failure\":null,\"faults\":[],\"stderr\":[],\"stderr_truncated\":false,\"interrupt_calls_succeeded\":0,\"first_interrupt_observed\":false}"
    return Data(
      ("{\"schema\":\"eml-technical-report\",\"schema_version\":1,\"version\":"
        + (try literal(version)) + ",\"build\":" + build
        + ",\"run_id\":1,\"terminal\":\"completed\",\"start\":" + start + ",\"items\":[" + item
        + "],\"summary\":" + summary + ",\"protocol_failure\":null,\"worker\":" + worker + "}").utf8
    )
  }
  @MainActor
  func testOwnedExportPublication() async throws {
    let session = try childSession("export-publication")
    let app = session.app
    try await changeChildMode(app, check: true)
    try await deliver(session, "one.eml")
    try await requireText(app, containing: "1 planned · Check only")
    let expected = try independentCheckReport(session)
    try expected.write(
      to: session.fixture.appendingPathComponent("independent-report.json"),
      options: .withoutOverwriting)
    let ordinary = session.fixture.appendingPathComponent("ordinary-report.json")
    try await saveReport(app, to: ordinary, expectNameCollision: false)
    try await waitExport(app, text: "Report exported and verified.")
    XCTAssertEqual(try Data(contentsOf: ordinary), expected)
    let sentinel = Data("Independent report occupancy sentinel\n".utf8)
    let occupied = session.fixture.appendingPathComponent("occupied-report.json")
    try sentinel.write(to: occupied, options: .withoutOverwriting)
    try await saveReport(app, to: occupied, expectNameCollision: true)
    XCTAssertEqual(try Data(contentsOf: occupied), sentinel)
    let directory = session.fixture.appendingPathComponent("directory-report.json")
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
    try await saveReport(app, to: directory, expectNameCollision: true)
    XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: directory.path), [])
    let link = session.fixture.appendingPathComponent("symlink-report.json")
    try FileManager.default.createSymbolicLink(at: link, withDestinationURL: occupied)
    try await saveReport(app, to: link, expectNameCollision: true)
    XCTAssertEqual(
      try FileManager.default.destinationOfSymbolicLink(atPath: link.path), occupied.path)
    XCTAssertEqual(try Data(contentsOf: occupied), sentinel)
    let race = session.fixture.appendingPathComponent("racing-report.json")
    try arm(session, recipe: "export:publication")
    try await saveReport(app, to: race, expectNameCollision: false)
    _ = try await awaitBoundary(session, operation: "export", phase: "publication")
    try sentinel.write(to: race, options: .withoutOverwriting)
    try releaseChild(session)
    try await waitExport(app, text: "Report was not published.")
    XCTAssertEqual(try Data(contentsOf: race), sentinel)
    try archiveControl(session, label: "racing-winner")
    let pre = session.fixture.appendingPathComponent("pre-edge-report.json")
    try arm(session, recipe: "export:publication")
    try await saveReport(app, to: pre, expectNameCollision: false)
    _ = try await awaitBoundary(session, operation: "export", phase: "publication")
    app.buttons["Cancel Export"].click()
    try await requireObservation(
      session.observation.appendingPathComponent("first-interrupt"), prefix: "observed ")
    try releaseChild(session)
    try await waitExport(app, text: "Report was not published.")
    XCTAssertFalse(FileManager.default.fileExists(atPath: pre.path))
    try archiveControl(session, label: "pre-edge-stop")
    let post = session.fixture.appendingPathComponent("post-edge-report.json")
    try arm(session, recipe: "export:namespace-sync")
    try await saveReport(app, to: post, expectNameCollision: false)
    _ = try await awaitBoundary(session, operation: "export", phase: "namespace-sync")
    app.buttons["Cancel Export"].click()
    try await requireObservation(
      session.observation.appendingPathComponent("first-interrupt"), prefix: "observed ")
    try releaseChild(session)
    try await waitExport(app, text: "Report exported and verified.")
    XCTAssertEqual(try Data(contentsOf: post), expected)
    try archiveControl(session, label: "post-edge-stop")
    let lost = session.fixture.appendingPathComponent("blocked-report.json")
    try arm(session, recipe: "export:after-publication")
    try await saveReport(app, to: lost, expectNameCollision: false)
    let operationID = try await awaitBoundary(
      session, operation: "export", phase: "after-publication")
    XCTAssertEqual(try Data(contentsOf: lost), expected)
    try await stopBlocked(session, operation: "export")
    try await requireText(app, containing: "Export outcome unknown")
    XCTAssertEqual(try Data(contentsOf: lost), expected)
    let facts: [String: Any] = [
      "case": "export-publication", "operation": "export", "phase": "after-publication",
      "operation_id": operationID, "worker_pid": try observedWorkerPID(session), "run_id": 1,
      "cases": [
        "ordinary", "occupied", "directory", "symlink", "racing-winner", "pre-edge-stop",
        "post-edge-stop", "post-edge-loss",
      ],
      "expected_bytes": expected.count,
      "expected_sha256": SHA256.hash(data: expected).map { String(format: "%02x", $0) }.joined(),
      "winner_sha256": SHA256.hash(data: sentinel).map { String(format: "%02x", $0) }.joined(),
      "no_replace_observed": true, "post_edge_bytes_preserved": true,
    ]
    try JSONSerialization.data(withJSONObject: facts, options: [.sortedKeys]).write(
      to: session.fixture.appendingPathComponent("child-ui-observation.json"),
      options: .withoutOverwriting)
    try await finishChild(session)
  }

  @MainActor
  private func dualChildQuit() async throws {
    let session = try childSession("blocked-export-preflight")
    let app = session.app
    try await deliver(session, "one.eml")
    try await requireRun(
      app, counts: "1 created · 0 up to date",
      source: session.fixture.appendingPathComponent("one.eml"))
    try await deliver(session, "two.eml")
    try await requireRun(
      app, counts: "1 created · 0 up to date",
      source: session.fixture.appendingPathComponent("two.eml"))
    try await selectReport(app, run: 1)
    try arm(session, recipe: "dual-quit")
    try await saveReport(
      app, to: session.fixture.appendingPathComponent("blocked-report.json"),
      expectNameCollision: false)
    let export = ChildSession(
      app: app, owned: session.owned, fixture: session.fixture,
      observation: session.observation.appendingPathComponent("export"), delivery: session.delivery)
    let processing = ChildSession(
      app: app, owned: session.owned, fixture: session.fixture,
      observation: session.observation.appendingPathComponent("processing"),
      delivery: session.delivery)
    let exportID = try await awaitBoundary(export, operation: "export", phase: "preflight")
    let responseBegan = ProcessInfo.processInfo.systemUptime
    try await selectReport(app, run: 2)
    try await requireRun(
      app, counts: "1 created · 0 up to date",
      source: session.fixture.appendingPathComponent("two.eml"))
    try await deliver(session, "folder-input/folder.eml")
    try await requireWaiting(app, count: 1)
    try await showRunControls(app)
    app.buttons["Dismiss report"].click()
    let processingID = try await awaitBoundary(
      processing, operation: "processing", phase: "preflight")
    try await showReportDetails(app)
    let processingRunID = try observedNumber(app, pattern: "Run summary: Run ([0-9]+)")
    XCTAssertEqual(processingRunID, 3)
    let technical = app.checkBoxes["Technical details"]
    if ((technical.value as? String) ?? (technical.value as? NSNumber)?.stringValue) != "1" {
      technical.click()
    }
    try await requireText(app, containing: "Processing operation: \(processingID)")
    XCTAssertEqual(try observedNumber(app, pattern: "Processing operation: ([0-9]+)"), processingID)
    XCTAssertFalse(
      FileManager.default.fileExists(
        atPath: session.fixture.appendingPathComponent("blocked-report.json").path))
    let processingCopy = expectedCopy(
      for: session.fixture.appendingPathComponent("folder-input/folder.eml"),
      directory: session.fixture.appendingPathComponent("publication-output"))
    XCTAssertFalse(FileManager.default.fileExists(atPath: processingCopy.path))
    XCTAssertNotEqual(exportID, processingID)
    try await selectReport(app, run: 1)
    XCTAssertFalse(app.buttons["Dismiss report"].isEnabled)
    XCTAssertTrue(app.buttons["Stop"].isEnabled)
    XCTAssertTrue(app.buttons["Cancel Export"].isEnabled)
    try await responsive(app)
    app.typeKey("o", modifierFlags: .command)
    let panel = app.sheets.firstMatch
    guard panel.waitForExistence(timeout: 10) else { throw InteractionFailure.expectedPresentation }
    // Native ingress and control draining remain live inside AppKit's panel.
    for count in 1...8 {
      try await deliver(session, "one.eml")
      try await requireWaiting(app, count: count)
    }
    try await deliver(session, "one.eml")
    let notice = "8 requests are already waiting. Remove a waiting run before adding another."
    try await requireText(app, containing: notice)
    app.typeKey("q", modifierFlags: .command)
    guard app.buttons["Cancel"].waitForExistence(timeout: 10) else {
      throw InteractionFailure.expectedPresentation
    }
    XCTAssertFalse(panel.exists)
    app.buttons["Cancel"].click()
    XCTAssertFalse(session.owned.isTerminated)
    try await requireWaiting(app, count: 8)
    for lane in [processing, export] {
      XCTAssertFalse(
        FileManager.default.fileExists(
          atPath: lane.observation.appendingPathComponent("first-interrupt").path))
    }
    app.typeKey("q", modifierFlags: .command)
    guard app.buttons["Quit"].waitForExistence(timeout: 5) else {
      throw InteractionFailure.expectedPresentation
    }
    snapshot(app, "dual-child-quit-with-panel-pending-batch-and-notice")
    app.buttons["Quit"].click()
    XCTAssertTrue(app.wait(for: .notRunning, timeout: 25))
    XCTAssertTrue(session.owned.isTerminated)
    for lane in [processing, export] {
      try await requireObservation(
        lane.observation.appendingPathComponent("first-interrupt"), prefix: "observed ")
      try await requireObservation(
        lane.observation.appendingPathComponent("second-interrupt"), prefix: "observed ")
    }
    let facts: [String: Any] = [
      "case": "blocked-export-preflight", "operation": "export", "phase": "preflight",
      "operation_id": exportID, "worker_pid": try observedWorkerPID(export),
      "processing_operation_id": processingID, "processing_run_id": processingRunID,
      "processing_worker_pid": try observedWorkerPID(processing), "run_id": 1,
      "ui_response_seconds": ProcessInfo.processInfo.systemUptime - responseBegan,
      "ui_responsive": true, "other_report_viewed": true, "source_reserved": true,
      "queued_then_processing": true,
      "first_stop_observed": true, "force_stop_observed": true, "dual_quit": true,
      "native_panel_seen": true, "native_panel_cancelled": true,
      "pending_requests": 8, "cosmetic_notice": notice, "quit_cancel_preserved_children": true,
      "app_quit_completed": session.owned.isTerminated,
    ]
    try JSONSerialization.data(withJSONObject: facts, options: [.sortedKeys]).write(
      to: session.fixture.appendingPathComponent("child-ui-observation.json"),
      options: .withoutOverwriting)
  }

}
