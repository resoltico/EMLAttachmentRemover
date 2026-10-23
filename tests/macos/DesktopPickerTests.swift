// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import AppKit
import Darwin
import XCTest

extension DesktopInteractionTests {
  @MainActor
  struct NativePickerSession {
    let root: URL
    let appURL: URL
    let sources: [URL]
    let allSources: [URL]
    let serviceSource: URL
    let serviceDisplayName: String
    let original: Data
    let golden: Data
    let identifier: String
    let ownership: [String: Any]
    let owned: NSRunningApplication
    let app: XCUIApplication
    let window: XCUIElement
    let delivery: NSWorkspace.OpenConfiguration
    let previousDestination: String?
  }

  @MainActor
  func testOwnedNativePickerCompatibility() async throws {
    let session = try await nativePickerPrelude(
      inputDirectory: "desktop-interaction-picker", scenario: "picker")
    let folderSource = session.root.appendingPathComponent("folder-input/folder.eml")
    try await requireRun(session.app, counts: "1 created · 0 up to date", source: folderSource)
    try await requireWaiting(session.app, count: 0)
    for source in session.sources + [folderSource] {
      XCTAssertEqual(
        try Data(
          contentsOf: expectedCopy(for: source)),
        session.golden)
    }
    snapshot(session.app, "native-picker-compatibility-complete")
    try await finishNativePickerSession(session)
  }

  @MainActor
  func nativePickerPrelude(inputDirectory: String, scenario: String) async throws
    -> NativePickerSession
  {
    let repository = URL(fileURLWithPath: #filePath)
      .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    let inputURL = repository.appendingPathComponent("target/\(inputDirectory)/input.json")
    let input = try XCTUnwrap(
      JSONSerialization.jsonObject(with: Data(contentsOf: inputURL)) as? [String: String])
    XCTAssertEqual(input["scenario"], scenario)
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
      throw InteractionFailure.expectedPresentation
    }
    _ = owned.activate(options: [])
    let window = app.windows["EML Attachment Remover"]
    XCTAssertTrue(window.waitForExistence(timeout: 20), app.debugDescription)
    try await requireOwnedForeground(owned)
    let instances = NSRunningApplication.runningApplications(withBundleIdentifier: identifier)
    XCTAssertEqual(instances.count, 1)
    XCTAssertEqual(instances.first?.processIdentifier, owned.processIdentifier)
    snapshot(app, "ready")
    let golden = try Data(
      contentsOf: repository.appendingPathComponent(
        "tests/contract/mime/golden/01-gmail-attachment.eml"))
    let delivery = NSWorkspace.OpenConfiguration()
    delivery.createsNewApplicationInstance = false
    delivery.addsToRecentItems = false
    let session = NativePickerSession(
      root: root,
      appURL: appURL,
      sources: sources,
      allSources: allSources,
      serviceSource: serviceSource,
      serviceDisplayName: serviceDisplayName,
      original: original,
      golden: golden,
      identifier: identifier,
      ownership: ownership,
      owned: owned,
      app: app,
      window: window,
      delivery: delivery,
      previousDestination: previousDestination)
    try await checkAboutAndLicenses(session)
    try await checkMainWindowReopen(session)
    try await checkOpenedBatch(session)
    try await checkPickerQueueAdmission(session)
    try await checkPickerKeyboardTraversal(session, scenario: scenario)
    return session
  }

  @MainActor
  private func checkAboutAndLicenses(_ session: NativePickerSession) async throws {
    let app = session.app
    let repository = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
      .deletingLastPathComponent().deletingLastPathComponent()
    let bundle = try XCTUnwrap(Bundle(url: session.appURL))
    try await requireAccessibleText(
      app, prefix: "Drop local EML files or folders to create copies.")
    try await requireAccessibleText(app, prefix: "Copy destination: ")
    try await requireAccessibleText(app, prefix: "Run status: ")
    XCTAssertFalse(app.buttons["About"].exists)
    app.menuBars.menuBarItems["EML Attachment Remover"].click()
    app.menuItems["About EML Attachment Remover"].click()
    // AppKit exposes its standard About panel as an untitled native Dialog.
    // The owned app and the content below establish its identity.
    let about = app.dialogs.firstMatch
    guard about.waitForExistence(timeout: 10), app.dialogs.count == 1 else {
      XCTFail("Expected one native About dialog: \(app.debugDescription)")
      throw InteractionFailure.expectedPresentation
    }
    let version = try XCTUnwrap(
      bundle.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String)
    let build = try XCTUnwrap(
      bundle.object(forInfoDictionaryKey: "CFBundleVersion") as? String)
    XCTAssertNotEqual(version, build)
    XCTAssertTrue(about.descendants(matching: .any)["EML Attachment Remover"].exists)
    // AppKit formats the standard About panel's version label. Inspect the
    // actual text without imposing the removed custom Slint window's format.
    let aboutText = about.descendants(matching: .any).allElementsBoundByIndex.map {
      $0.label + " " + (($0.value as? String) ?? "")
    }.joined(separator: "\n")
    XCTAssertTrue(aboutText.contains(version), aboutText)
    XCTAssertTrue(aboutText.contains(build), aboutText)
    let copyright = try XCTUnwrap(
      bundle.object(forInfoDictionaryKey: "NSHumanReadableCopyright") as? String)
    XCTAssertFalse(copyright.isEmpty)
    XCTAssertTrue(aboutText.contains(copyright), aboutText)
    app.menuBars.menuBarItems["EML Attachment Remover"].click()
    app.menuItems["About EML Attachment Remover"].click()
    XCTAssertEqual(app.dialogs.count, 1)
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
    app.menuItems["About Slint and Licenses…"].click()
    let licenses = app.windows["About Slint and Licenses"]
    XCTAssertTrue(licenses.waitForExistence(timeout: 10), app.debugDescription)
    XCTAssertTrue(licenses.images["#MadeWithSlint"].exists, licenses.debugDescription)
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

  }

  @MainActor
  private func checkMainWindowReopen(_ session: NativePickerSession) async throws {
    let window = session.window
    let app = session.app
    let owned = session.owned
    let appURL = session.appURL
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

  }

  @MainActor
  private func checkOpenedBatch(_ session: NativePickerSession) async throws {
    let app = session.app
    let window = session.window
    let owned = session.owned
    let appURL = session.appURL
    let sources = session.sources
    let delivery = session.delivery
    let golden = session.golden
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
      throw InteractionFailure.expectedPresentation
    }
    for text in window.staticTexts.allElementsBoundByIndex {
      print(
        "Observed static text label=\(text.label.debugDescription), value=\(String(describing: text.value).prefix(2048))"
      )
    }
    try showReportDetails(app)
    let technical = app.checkBoxes["Technical details"]
    guard technical.exists else {
      XCTFail("Technical details control is unavailable")
      throw InteractionFailure.expectedPresentation
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
    guard observation == .completed else { throw InteractionFailure.expectedPresentation }
    for source in sources {
      let copy = expectedCopy(for: source)
      XCTAssertEqual(try Data(contentsOf: copy), golden)
    }
    snapshot(app, "finished")
    try await requireAccessibleText(app, prefix: "Run summary: ")
    try showRunControls(app)
    try await requireAccessibleText(app, prefix: "Completed")
    try await checkCompactReport(app, window: window, source: sources[0])

  }

  @MainActor
  private func checkPickerQueueAdmission(_ session: NativePickerSession) async throws {
    let app = session.app
    let root = session.root
    let sources = session.sources
    let golden = session.golden
    let folderSource = session.root.appendingPathComponent("folder-input/folder.eml")
    // The real message importer reruns one original. Two full reports then
    // reserve all report space; the subsequent folder must wait visibly.
    try await choose(app, fixture: root, path: sources[0], folder: false)
    try await requireRun(app, counts: "0 created · 1 up to date", source: sources[0])
    snapshot(app, "message-import-up-to-date")
    try await choose(
      app, fixture: root, path: folderSource.deletingLastPathComponent(), folder: true)
    try await requireText(app, containing: "Queue paused: dismiss a retained report to make room.")
    XCTAssertFalse(
      FileManager.default.fileExists(
        atPath: expectedCopy(for: folderSource)
          .path))
    snapshot(app, "report-space-paused")
    app.buttons["Dismiss report"].click()
    try await requireRun(app, counts: "1 created · 0 up to date", source: folderSource)
    XCTAssertEqual(
      try Data(
        contentsOf: expectedCopy(for: folderSource)), golden)
    snapshot(app, "folder-import-finished")

  }

  @MainActor
  private func checkPickerKeyboardTraversal(_ session: NativePickerSession, scenario: String)
    async throws
  {
    let app = session.app
    let sources = session.sources
    let pid = session.owned.processIdentifier
    let folderSource = session.root.appendingPathComponent("folder-input/folder.eml")
    // The first focusable main-window control is the message importer.
    // Forward and reverse traversal must open the actual distinct native pickers.
    app.typeKey(XCUIKeyboardKey.tab, modifierFlags: [])
    try DesktopFixtureRequest.picker.send()
    app.typeKey(XCUIKeyboardKey.space, modifierFlags: [])
    let keyboardPicker = app.sheets.firstMatch
    guard keyboardPicker.waitForExistence(timeout: 10) else {
      XCTFail("Tab/Space did not activate the native message importer: \(app.debugDescription)")
      throw InteractionFailure.expectedPresentation
    }
    keyboardPicker.buttons["Cancel"].click()
    app.typeKey(XCUIKeyboardKey.tab, modifierFlags: [])
    try DesktopFixtureRequest.picker.send()
    app.typeKey(XCUIKeyboardKey.space, modifierFlags: [])
    guard keyboardPicker.waitForExistence(timeout: 10) else {
      XCTFail("Tab/Space did not reach the native folder importer: \(app.debugDescription)")
      throw InteractionFailure.expectedPresentation
    }
    try await requireNativePanelMessage(
      keyboardPicker, expected: "Choose folders containing EML messages")
    keyboardPicker.buttons["Cancel"].click()
    app.typeKey(XCUIKeyboardKey.tab, modifierFlags: .shift)
    try DesktopFixtureRequest.picker.send()
    app.typeKey(XCUIKeyboardKey.space, modifierFlags: [])
    guard keyboardPicker.waitForExistence(timeout: 10) else {
      XCTFail("Shift-Tab/Space did not return to the message importer: \(app.debugDescription)")
      throw InteractionFailure.expectedPresentation
    }
    try await requireNativePanelMessage(keyboardPicker, expected: "Choose EML messages")
    snapshot(app, "keyboard-forward-and-reverse-native-actions")
    keyboardPicker.buttons["Cancel"].click()

    let pickerObservation: [String: Any] = [
      "schema": 1, "scenario": scenario, "owned_pid": pid, "message": sources[0].path,
      "folder": folderSource.deletingLastPathComponent().path,
    ]
    let pickerAttachment = XCTAttachment(
      data: try JSONSerialization.data(withJSONObject: pickerObservation),
      uniformTypeIdentifier: "public.json")
    pickerAttachment.name = "native-picker-observation"
    pickerAttachment.lifetime = .keepAlways
    add(pickerAttachment)

  }

  @MainActor
  func finishNativePickerSession(_ session: NativePickerSession) async throws {
    let window = session.window
    let app = session.app
    let owned = session.owned
    let identifier = session.identifier
    let previousDestination = session.previousDestination
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
      throw InteractionFailure.expectedPresentation
    }
    XCTAssertTrue(window.exists, "Quit confirmation did not restore the main window")
    try await requireAccessibleText(app, prefix: "Confirmation: ")
    XCTAssertLessThan(app.buttons["Quit"].frame.height, 50)
    XCTAssertLessThan(app.buttons["Cancel"].frame.height, 50)
    app.buttons["Cancel"].click()
    XCTAssertFalse(owned.isTerminated)
    snapshot(app, "quit-cancelled-from-hidden")
    app.typeKey("q", modifierFlags: .command)
    XCTAssertTrue(app.buttons["Quit"].waitForExistence(timeout: 5))
    app.buttons["Quit"].click()
    XCTAssertTrue(app.wait(for: .notRunning, timeout: 20))
    XCTAssertTrue(owned.isTerminated)
    XCTAssertEqual(
      UserDefaults.standard.persistentDomain(forName: identifier)?["copyDestinationBytes"]
        as? String, previousDestination,
      "Invocation override must not persist a destination")
  }

}
