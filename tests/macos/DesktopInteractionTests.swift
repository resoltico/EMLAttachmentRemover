// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import AppKit
import Darwin
import XCTest

final class DesktopInteractionTests: XCTestCase {
  enum InteractionFailure: Error { case expectedPresentation }
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
      !owned.isTerminated, owned.activate(options: [])
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
    let delivery = NSWorkspace.OpenConfiguration()
    delivery.createsNewApplicationInstance = false
    delivery.addsToRecentItems = false
    let recipient = try await NSWorkspace.shared.open(
      [directory], withApplicationAt: appURL, configuration: delivery)
    guard recipient.processIdentifier == pid else { throw InteractionFailure.expectedPresentation }
    app.checkBoxes["Technical details"].click()
    try await requireRun(app, counts: "65 created · 0 up to date", source: sources[0])
    let specialGolden = try Data(
      contentsOf: fixture.appendingPathComponent("pagination-golden.eml"))
    let ordinaryGolden = try Data(
      contentsOf: repository.appendingPathComponent(
        "tests/contract/mime/golden/01-gmail-attachment.eml"))
    for (index, source) in sources.enumerated() {
      XCTAssertEqual(
        try Data(
          contentsOf: source.deletingPathExtension().appendingPathExtension("no-attachments.eml")),
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
    guard app.buttons["Confirm"].waitForExistence(timeout: 5) else {
      throw InteractionFailure.expectedPresentation
    }
    app.buttons["Confirm"].click()
    XCTAssertTrue(app.wait(for: .notRunning, timeout: 20))
    XCTAssertTrue(owned.isTerminated)
  }
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
  private func observedCoreScenario(_ scenario: String) async throws {
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
    guard owned.activate(options: []) else { throw InteractionFailure.expectedPresentation }
    XCTAssertTrue(app.windows["EML Attachment Remover"].waitForExistence(timeout: 20))
    let delivery = NSWorkspace.OpenConfiguration()
    delivery.createsNewApplicationInstance = false
    delivery.addsToRecentItems = false
    let paths = Array(originals.prefix(scenario == "published-error" ? 1 : 2))
    let delivered = try await NSWorkspace.shared.open(
      paths, withApplicationAt: appURL, configuration: delivery)
    guard delivered.processIdentifier == pid else { throw InteractionFailure.expectedPresentation }
    try await requireObservation(
      observation.appendingPathComponent("ready"), prefix: "real publication boundary\n")

    if scenario == "stop" {
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
      try await requireText(app, containing: originals[0].lastPathComponent)
      try await requireAccessibleLabel(app, prefix: "Processing")
      snapshot(app, "active-work-retained-after-close-reopen")
    }

    if scenario == "active-quit" {
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
      XCTAssertTrue(app.buttons["Confirm"].waitForExistence(timeout: 5))
      app.buttons["Confirm"].click()
      XCTAssertTrue(app.wait(for: .notRunning, timeout: 20))
      XCTAssertTrue(owned.isTerminated)
      try await requireObservation(
        observation.appendingPathComponent("second-interrupt"), prefix: "observed ")
      return
    }

    if scenario != "published-error" {
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
        XCTAssertTrue(app.buttons["Confirm"].waitForExistence(timeout: 5))
        app.buttons["Confirm"].click()
        try await requireObservation(
          observation.appendingPathComponent("second-interrupt"), prefix: "observed ")
      }
    }
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
          contentsOf: fixture.appendingPathComponent("published-output/one.no-attachments.eml")),
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
          contentsOf: fixture.appendingPathComponent("publication-output/one.no-attachments.eml")),
        golden)
      XCTAssertFalse(
        FileManager.default.fileExists(
          atPath: fixture.appendingPathComponent("publication-output/two.no-attachments.eml").path))
      snapshot(app, "real-core-\(scenario)-settled")
    }
    app.typeKey("q", modifierFlags: .command)
    XCTAssertTrue(app.buttons["Confirm"].waitForExistence(timeout: 5))
    app.buttons["Confirm"].click()
    XCTAssertTrue(app.wait(for: .notRunning, timeout: 20))
    XCTAssertTrue(owned.isTerminated)
  }

  @MainActor
  private func requireObservation(_ url: URL, prefix: String) async throws {
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

  @MainActor
  func testOwnedDesktopWorkflowAndNativeIngress() async throws {
    let repository = URL(fileURLWithPath: #filePath)
      .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    let inputURL = repository.appendingPathComponent("target/desktop-interaction/input.json")
    let input = try XCTUnwrap(
      JSONSerialization.jsonObject(with: Data(contentsOf: inputURL)) as? [String: String])
    let appURL = URL(fileURLWithPath: try XCTUnwrap(input["app"]))
    let root = URL(fileURLWithPath: try XCTUnwrap(input["fixture"]))
    let sources = ["one.eml", "two.eml"].map { root.appendingPathComponent($0) }
    let folderSource = root.appendingPathComponent("folder-input/folder.eml")
    let serviceSource = root.appendingPathComponent("service-input/日本語-مرحبا-\u{202e}report.eml")
    let serviceDisplayName = #"日本語-مرحبا-\u{202e}report.eml"#
    let allSources = sources + [folderSource, serviceSource]
    let original = try Data(
      contentsOf: repository.appendingPathComponent(
        "tests/contract/mime/inputs/01-gmail-attachment.eml"))
    for source in allSources { observeSource(source, phase: "before GUI interactions") }
    addTeardownBlock { @MainActor in
      for source in allSources {
        self.observeSource(source, phase: "after GUI interactions")
        XCTAssertEqual(try Data(contentsOf: source), original)
      }
    }
    let bundle = try XCTUnwrap(Bundle(url: appURL))
    let identifier = try XCTUnwrap(bundle.bundleIdentifier)
    XCTAssertEqual(input["host_scope"], "fresh-github-hosted-runner")
    // The separate fixture owner retains launch/cleanup authority even if
    // XCTest dies. These fields only admit attachment to that actual instance.
    let ownershipURL = inputURL.deletingLastPathComponent().appendingPathComponent("ownership.json")
    let ownership = try XCTUnwrap(
      JSONSerialization.jsonObject(with: Data(contentsOf: ownershipURL)) as? [String: Any])
    let pid = try XCTUnwrap(ownership["pid"] as? NSNumber).int32Value
    let launchDate = try XCTUnwrap(ownership["launch_date"] as? NSNumber).doubleValue
    let owned = try XCTUnwrap(NSRunningApplication(processIdentifier: pid))
    XCTAssertEqual(owned.bundleIdentifier, identifier)
    XCTAssertEqual(owned.bundleURL?.standardizedFileURL, appURL.standardizedFileURL)
    XCTAssertEqual(ownership["bundle_path"] as? String, appURL.path)
    XCTAssertEqual(owned.launchDate?.timeIntervalSince1970, launchDate)
    guard !owned.isTerminated,
      owned.bundleURL?.standardizedFileURL == appURL.standardizedFileURL,
      owned.launchDate?.timeIntervalSince1970 == launchDate
    else { throw InteractionFailure.expectedPresentation }
    let admitted = NSRunningApplication.runningApplications(withBundleIdentifier: identifier)
    guard admitted.count == 1, admitted[0].processIdentifier == pid else {
      throw InteractionFailure.expectedPresentation
    }
    let defaults = UserDefaults.standard.persistentDomain(forName: identifier) ?? [:]
    let previousDestination = defaults["copyDestinationBytes"] as? String
    let app = XCUIApplication(url: appURL)
    let launched = NSPredicate { _, _ in owned.isFinishedLaunching && !owned.isTerminated }
    let launchObservation = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: launched, object: nil)], timeout: 20)
    guard launchObservation == .completed else {
      XCTFail("Owned application did not finish native launch")
      return
    }
    guard owned.activate(options: []) else { throw InteractionFailure.expectedPresentation }
    let window = app.windows["EML Attachment Remover"]
    XCTAssertTrue(window.waitForExistence(timeout: 20), app.debugDescription)
    let instances = NSRunningApplication.runningApplications(withBundleIdentifier: identifier)
    XCTAssertEqual(instances.count, 1)
    XCTAssertEqual(instances.first?.processIdentifier, owned.processIdentifier)
    snapshot(app, "ready")
    try await requireAccessibleLabel(
      app, prefix: "Drop local EML files or folders to create copies.")
    try await requireAccessibleLabel(app, prefix: "Copy destination: ")
    try await requireAccessibleLabel(app, prefix: "Run status: ")
    app.buttons["About"].click()
    let about = app.windows["About EML Attachment Remover"]
    XCTAssertTrue(about.waitForExistence(timeout: 10), app.debugDescription)
    let version = try XCTUnwrap(
      bundle.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String)
    let identity = app.descendants(matching: .any).allElementsBoundByIndex.first {
      $0.title == "Product identity and licensing"
    }
    XCTAssertTrue(
      (identity?.value as? String)?.contains("EML Attachment Remover \(version)") == true)
    snapshot(app, "about")
    about.buttons["_XCUI:CloseWindow"].click()
    let aboutClosed = NSPredicate { _, _ in !about.exists }
    let aboutCloseResult = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: aboutClosed, object: nil)], timeout: 10)
    guard aboutCloseResult == .completed else {
      XCTFail("About did not close before keyboard navigation: \(app.debugDescription)")
      throw InteractionFailure.expectedPresentation
    }
    app.menuBars.menuBarItems["Help"].click()
    app.menuItems["Software and Artwork Licenses…"].click()
    let licenses = app.windows["Software and Artwork Licenses"]
    XCTAssertTrue(licenses.waitForExistence(timeout: 10), app.debugDescription)
    let licenseText = try XCTUnwrap(
      app.descendants(matching: .any).allElementsBoundByIndex.first {
        $0.title == "Complete software and owner artwork license terms"
      })
    let expectedLicense = try XCTUnwrap(
      String(data: Data(contentsOf: repository.appendingPathComponent("LICENSE")), encoding: .utf8))
    XCTAssertEqual(licenseText.value as? String, expectedLicense)
    licenseText.click()
    app.typeKey("a", modifierFlags: .command)
    let edit = app.menuBars.menuBarItems["Edit"]
    edit.click()
    XCTAssertTrue(edit.menus.menuItems["Copy"].isEnabled)
    app.typeKey(XCUIKeyboardKey.escape, modifierFlags: [])
    snapshot(app, "licenses")
    licenses.buttons["_XCUI:CloseWindow"].click()

    window.buttons["_XCUI:CloseWindow"].click()
    let hidden = NSPredicate { _, _ in !window.exists }
    let hiddenObservation = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: hidden, object: nil)], timeout: 10)
    XCTAssertEqual(hiddenObservation, .completed)
    XCTAssertFalse(owned.isTerminated, "Closing the main window terminated the application")
    guard !owned.isTerminated else { throw InteractionFailure.expectedPresentation }
    let reopen = NSWorkspace.OpenConfiguration()
    reopen.createsNewApplicationInstance = false
    reopen.addsToRecentItems = false
    let restored = try await NSWorkspace.shared.openApplication(at: appURL, configuration: reopen)
    XCTAssertEqual(restored.processIdentifier, owned.processIdentifier)
    guard restored.processIdentifier == owned.processIdentifier else {
      throw InteractionFailure.expectedPresentation
    }
    XCTAssertTrue(window.waitForExistence(timeout: 10), app.debugDescription)
    snapshot(app, "reopened")

    let delivery = NSWorkspace.OpenConfiguration()
    delivery.createsNewApplicationInstance = false
    delivery.addsToRecentItems = false
    guard !owned.isTerminated else { throw InteractionFailure.expectedPresentation }
    let recipient = try await NSWorkspace.shared.open(
      sources, withApplicationAt: appURL, configuration: delivery)
    XCTAssertEqual(
      recipient.processIdentifier, owned.processIdentifier, "File batch reached another instance")
    guard recipient.processIdentifier == owned.processIdentifier else {
      throw InteractionFailure.expectedPresentation
    }
    let finished = NSPredicate { _, _ in
      app.buttons.allElementsBoundByIndex.contains { button in
        let text = button.title + "\n" + button.label + "\n" + (button.value as? String ?? "")
        return text.contains("2 created") && text.contains("Finished")
      }
    }
    let finishedObservation = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: finished, object: nil)], timeout: 30)
    guard finishedObservation == .completed else {
      for button in app.buttons.allElementsBoundByIndex {
        print(
          "Observed button title=\(button.title.debugDescription), label=\(button.label.debugDescription), value=\(String(describing: button.value).prefix(2048))"
        )
      }
      for element in app.descendants(matching: .any).allElementsBoundByIndex {
        if element.title.hasPrefix("Selected message") {
          print("Observed failed-batch detail: \(String(describing: element.value).prefix(16384))")
        }
      }
      XCTFail("Native batch did not reach its two-created presentation: \(app.debugDescription)")
      return
    }
    for text in window.staticTexts.allElementsBoundByIndex {
      print(
        "Observed static text label=\(text.label.debugDescription), value=\(String(describing: text.value).prefix(2048))"
      )
    }
    let technical = app.checkBoxes["Technical details"]
    guard technical.exists else {
      XCTFail("Technical details control is unavailable")
      return
    }
    technical.click()
    print(
      "Observed technical disclosure title=\(technical.title.debugDescription), value=\(String(describing: technical.value))"
    )
    let completed = NSPredicate { _, _ in
      app.descendants(matching: .any).allElementsBoundByIndex.contains {
        $0.title == "Selected message, removed parts and technical observations"
          && ($0.value as? String)?.contains("settled true; reusable true") == true
      }
    }
    let observation = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: completed, object: nil)], timeout: 20)
    XCTAssertEqual(
      observation, .completed, "Settled native report is unavailable: \(app.debugDescription)")
    guard observation == .completed else { return }
    let golden = try Data(
      contentsOf: repository.appendingPathComponent(
        "tests/contract/mime/golden/01-gmail-attachment.eml"))
    for source in sources {
      let copy = source.deletingPathExtension().appendingPathExtension("no-attachments.eml")
      XCTAssertEqual(try Data(contentsOf: copy), golden)
    }
    snapshot(app, "finished")
    try await requireAccessibleLabel(app, prefix: "Completed")
    try await requireAccessibleLabel(app, prefix: "Run summary: ")
    try await checkCompactReport(app, window: window, source: sources[0])

    // The real message importer reruns one original. Two full reports then
    // reserve all report space; the subsequent folder must wait visibly.
    try await choose(app, path: sources[0], folder: false)
    try await requireRun(app, counts: "0 created · 1 up to date", source: sources[0])
    snapshot(app, "message-import-up-to-date")
    try await choose(app, path: folderSource.deletingLastPathComponent(), folder: true)
    try await requireText(app, containing: "Queue paused: dismiss a retained report to make room.")
    XCTAssertFalse(
      FileManager.default.fileExists(
        atPath: folderSource.deletingPathExtension().appendingPathExtension("no-attachments.eml")
          .path))
    snapshot(app, "report-space-paused")
    app.buttons["Dismiss report"].click()
    try await requireRun(app, counts: "1 created · 0 up to date", source: folderSource)
    XCTAssertEqual(
      try Data(
        contentsOf: folderSource.deletingPathExtension().appendingPathExtension(
          "no-attachments.eml")), golden)
    snapshot(app, "folder-import-finished")

    // Keyboard traversal must cause distinct native effects, not merely send keys.
    // Cupertino mouse activation does not establish logical keyboard focus:
    // the first Tab reaches About, and the second reaches Choose messages.
    app.typeKey(XCUIKeyboardKey.tab, modifierFlags: [])
    app.typeKey(XCUIKeyboardKey.tab, modifierFlags: [])
    try DesktopFixtureRequest.picker.send()
    app.typeKey(XCUIKeyboardKey.space, modifierFlags: [])
    let keyboardPicker = app.sheets.firstMatch
    guard keyboardPicker.waitForExistence(timeout: 10) else {
      XCTFail("Tab/Space did not activate the native message importer: \(app.debugDescription)")
      throw InteractionFailure.expectedPresentation
    }
    keyboardPicker.buttons["Cancel"].click()
    app.typeKey(XCUIKeyboardKey.tab, modifierFlags: .shift)
    app.typeKey(XCUIKeyboardKey.space, modifierFlags: [])
    guard about.waitForExistence(timeout: 10) else {
      XCTFail("Shift-Tab/Space did not return to About: \(app.debugDescription)")
      throw InteractionFailure.expectedPresentation
    }
    snapshot(app, "keyboard-forward-and-reverse-native-actions")
    about.buttons["_XCUI:CloseWindow"].click()

    let pickerObservation: [String: Any] = [
      "schema": 1, "owned_pid": pid, "message": sources[0].path,
      "folder": folderSource.deletingLastPathComponent().path,
    ]
    let pickerAttachment = XCTAttachment(
      data: try JSONSerialization.data(withJSONObject: pickerObservation),
      uniformTypeIdentifier: "public.json")
    pickerAttachment.name = "native-picker-observation"
    pickerAttachment.lifetime = .keepAlways
    add(pickerAttachment)

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

    let exportURL = root.appendingPathComponent("service-report.json")
    XCTAssertFalse(FileManager.default.fileExists(atPath: exportURL.path))
    try await saveReport(app, to: exportURL, replacingOwnedSentinel: false)
    try await requireText(app, containing: "Technical report saved and read back successfully.")
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
    try await saveReport(app, to: collisionURL, replacingOwnedSentinel: true)
    try await requireText(app, containing: "Report export: publication NotPublished")
    try await requireText(app, containing: "os error 17")
    XCTAssertEqual(try Data(contentsOf: collisionURL), sentinel)
    snapshot(app, "report-export-collision-refused")
    app.buttons["Dismiss notice"].click()

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
    snapshot(finder, "native-finder-revealed-exact-copy")

    // Free one full-report reservation before a real native file drag.
    guard owned.activate(options: []) else { throw InteractionFailure.expectedPresentation }
    app.buttons["Dismiss report"].click()
    let room = NSPredicate { _, _ in
      window.buttons.allElementsBoundByIndex.filter { $0.title.hasPrefix("Run ") }.count == 1
    }
    let freed = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: room, object: nil)], timeout: 10)
    guard freed == .completed else { throw InteractionFailure.expectedPresentation }
    finder.activate()
    // Move only this newly opened fixture window so the drop target stays visible.
    let titlebar = folder.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0))
      .withOffset(CGVector(dx: 0, dy: 12))
    let position = window.coordinate(withNormalizedOffset: CGVector(dx: 0, dy: 0))
      .withOffset(CGVector(dx: 750, dy: 100))
    titlebar.click(forDuration: 0.3, thenDragTo: position)
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
    guard owned.activate(options: []) else { throw InteractionFailure.expectedPresentation }

    // Both report reservations are occupied. Capture a changed destination in
    // a queued request, then change the current choice before freeing its slot.
    let chosenDestination = root.appendingPathComponent("chosen-destination")
    try await choose(app, path: chosenDestination, folder: true, destination: true)
    try await requireAccessibleLabel(app, prefix: "Copy destination: " + chosenDestination.path)
    let queuedDestination = try await NSWorkspace.shared.open(
      [sources[0]], withApplicationAt: appURL, configuration: delivery)
    guard queuedDestination.processIdentifier == owned.processIdentifier else {
      throw InteractionFailure.expectedPresentation
    }
    try await requireWaiting(app, count: 1)
    let destinationCopy = chosenDestination.appendingPathComponent("one.no-attachments.eml")
    XCTAssertFalse(FileManager.default.fileExists(atPath: destinationCopy.path))
    app.buttons["Beside originals"].click()
    try await requireAccessibleLabel(app, prefix: "Copy destination: Beside each original")
    app.buttons["Dismiss report"].click()
    try await requireRun(app, counts: "1 created · 0 up to date", source: sources[0])
    try await requireText(app, containing: "Copy: " + destinationCopy.path)
    XCTAssertEqual(try Data(contentsOf: destinationCopy), golden)
    XCTAssertEqual(try Data(contentsOf: firstCopy), golden)
    try await requireWaiting(app, count: 0)
    snapshot(app, "queued-run-retains-chosen-destination")
    window.buttons["_XCUI:CloseWindow"].click()
    let hiddenForQuit = NSPredicate { _, _ in !window.exists }
    let hiddenForQuitObservation = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: hiddenForQuit, object: nil)], timeout: 10)
    XCTAssertEqual(hiddenForQuitObservation, .completed)
    app.menuBars.menuBarItems["EML Attachment Remover"].click()
    XCTAssertFalse(window.exists, "Opening the app menu unexpectedly reopened the main window")
    app.menuItems["Quit EML Attachment Remover"].click()
    guard app.buttons["Cancel"].waitForExistence(timeout: 5) else {
      XCTFail("Hidden-window Quit did not expose its Cancel control: \(app.debugDescription)")
      return
    }
    XCTAssertTrue(window.exists, "Quit confirmation did not restore the main window")
    try await requireAccessibleLabel(app, prefix: "Confirmation: ")
    app.buttons["Cancel"].click()
    XCTAssertFalse(owned.isTerminated)
    snapshot(app, "quit-cancelled-from-hidden")
    app.typeKey("q", modifierFlags: .command)
    XCTAssertTrue(app.buttons["Confirm"].waitForExistence(timeout: 5))
    app.buttons["Confirm"].click()
    XCTAssertTrue(app.wait(for: .notRunning, timeout: 20))
    XCTAssertTrue(owned.isTerminated)
    XCTAssertEqual(
      UserDefaults.standard.persistentDomain(forName: identifier)?["copyDestinationBytes"]
        as? String, previousDestination,
      "Invocation override must not persist a destination")
  }

  @MainActor
  private func checkCompactReport(
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
    try await requireText(app, containing: displayedName ?? source.lastPathComponent)
    let detail = window.textViews["Selected message, removed parts and technical observations"]
    if !detail.isHittable || !window.frame.contains(detail.frame) {
      let topControl = app.buttons["About"]
      guard topControl.isHittable else { throw InteractionFailure.expectedPresentation }
      topControl.scroll(byDeltaX: 0, deltaY: -600)
    }
    guard detail.isHittable, detail.frame.height >= 55, window.frame.contains(detail.frame) else {
      XCTFail(
        "Compact selected detail is collapsed or clipped: \(detail.frame); \(app.debugDescription)")
      throw InteractionFailure.expectedPresentation
    }
    let technical = app.checkBoxes["Technical details"]
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
      try await requireText(app, containing: "Items page 1 of 2")
      try await requireText(app, containing: "Removed parts page 1 of 2")
      XCTAssertFalse(app.buttons["Previous items"].isEnabled)
      XCTAssertFalse(app.buttons["Previous parts"].isEnabled)
      guard app.buttons["Next parts"].isEnabled, app.buttons["Next items"].isEnabled else {
        throw InteractionFailure.expectedPresentation
      }
      app.buttons["Next parts"].click()
      try await requireText(app, containing: "Removed parts page 2 of 2")
      let lastPart = detail.value as? String ?? ""
      func observedParts(_ value: String, page: String) throws -> [String] {
        let range = try XCTUnwrap(value.range(of: "Removed parts page " + page + "\n"))
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
      try await requireText(app, containing: "Removed parts page 1 of 2")
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
      try await requireText(app, containing: "Removed parts page 1 of 2")
    }
    snapshot(app, "minimum-compact-accessible-report")
    technical.scroll(byDeltaX: 0, deltaY: 1000)
    guard app.buttons["About"].isHittable,
      window.frame.contains(app.buttons["About"].frame)
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
  private func closeOwnedFinderWindow(_ finder: XCUIApplication, title: String) async throws {
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
  private func finderEntry(_ window: XCUIElement, file: URL, selected: Bool?) throws -> XCUIElement
  {
    // Finder may hide a known extension. Both display names identify this one
    // synthetic file inside the already-bound unique fixture directory.
    let names = [file.lastPathComponent, file.deletingPathExtension().lastPathComponent]
    let elements = window.descendants(matching: .any).allElementsBoundByIndex
    guard elements.count <= 512 else { throw InteractionFailure.expectedPresentation }
    let entryKinds: [XCUIElement.ElementType] = [.icon, .cell, .tableRow, .outlineRow]
    let found = elements.first { element in
      guard entryKinds.contains(element.elementType),
        selected == nil || element.isSelected == selected
      else { return false }
      return [element.title, element.label, element.value as? String ?? ""].contains { value in
        names.contains { value == $0 || value.hasPrefix($0 + ",") }
      }
    }
    guard let found else {
      XCTFail(
        "Owned Finder entry/selection is unavailable for \(file.lastPathComponent): \(window.debugDescription)"
      )
      throw InteractionFailure.expectedPresentation
    }
    return found
  }

  @MainActor
  private func saveReport(_ app: XCUIApplication, to destination: URL, replacingOwnedSentinel: Bool)
    async throws
  {
    app.buttons["Export Report…"].click()
    let panel = app.sheets.firstMatch
    guard panel.waitForExistence(timeout: 10) else {
      XCTFail("Native report save panel did not appear: \(app.debugDescription)")
      throw InteractionFailure.expectedPresentation
    }
    app.typeKey("g", modifierFlags: [.command, .shift])
    app.typeText(destination.deletingLastPathComponent().path)
    app.typeKey(XCUIKeyboardKey.return, modifierFlags: [])
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
    if replacingOwnedSentinel {
      let replace = app.buttons["Replace"]
      guard replace.waitForExistence(timeout: 5) else {
        XCTFail(
          "Expected native replacement confirmation for owned report sentinel: \(app.debugDescription)"
        )
        throw InteractionFailure.expectedPresentation
      }
      replace.click()
    }
    let closed = NSPredicate { _, _ in !panel.exists }
    let result = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: closed, object: nil)], timeout: 10)
    XCTAssertEqual(
      result, .completed, "Native report save panel did not complete: \(app.debugDescription)")
    guard result == .completed else { throw InteractionFailure.expectedPresentation }
  }

  @MainActor
  private func requireWaiting(_ app: XCUIApplication, count: Int) async throws {
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
  private func choose(_ app: XCUIApplication, path: URL, folder: Bool, destination: Bool = false)
    async throws
  {
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
    app.typeKey("g", modifierFlags: [.command, .shift])
    app.typeText(path.path)
    app.typeKey(XCUIKeyboardKey.return, modifierFlags: [])
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
  private func requireAccessibleLabel(_ app: XCUIApplication, prefix: String) async throws {
    let predicate = NSPredicate { _, _ in
      app.descendants(matching: .any).allElementsBoundByIndex.contains {
        $0.title.hasPrefix(prefix) || $0.label.hasPrefix(prefix)
      }
    }
    guard
      await XCTWaiter.fulfillment(
        of: [XCTNSPredicateExpectation(predicate: predicate, object: nil)], timeout: 10)
        == .completed
    else {
      XCTFail("Required native accessibility label is absent: \(prefix); \(app.debugDescription)")
      throw InteractionFailure.expectedPresentation
    }
  }

  @MainActor
  func requireRun(
    _ app: XCUIApplication, counts: String, source: URL, displayedName: String? = nil
  ) async throws {
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
    let detail = NSPredicate { _, _ in
      app.descendants(matching: .any).allElementsBoundByIndex.contains {
        $0.title == "Selected message, removed parts and technical observations"
          && ($0.value as? String)?.contains(displayedName ?? source.lastPathComponent) == true
          && ($0.value as? String)?.contains("settled true; reusable true") == true
      }
    }
    let selected = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: detail, object: nil)], timeout: 20)
    XCTAssertEqual(
      selected, .completed,
      "Expected settled details for \(source.lastPathComponent): \(app.debugDescription)")
    guard selected == .completed else { throw InteractionFailure.expectedPresentation }
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
  private func observeSource(_ source: URL, phase: String) {
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
  func snapshot(_ app: XCUIApplication, _ name: String) {
    let attachment = XCTAttachment(screenshot: app.screenshot())
    attachment.name = name
    attachment.lifetime = .keepAlways
    add(attachment)
  }
}
