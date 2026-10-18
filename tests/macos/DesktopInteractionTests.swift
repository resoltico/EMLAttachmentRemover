// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import AppKit
import Darwin
import XCTest

final class DesktopInteractionTests: XCTestCase {
  private enum InteractionFailure: Error { case expectedPresentation }
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
    let serviceSource = root.appendingPathComponent("service-input/日本語.eml")
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
    try await requireRun(app, counts: "1 created · 0 up to date", source: serviceSource)
    try await requireText(app, containing: "日本語.eml")
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
    app.buttons["Dismiss notice"].click()
    for remaining in stride(from: 7, through: 0, by: -1) {
      app.buttons.matching(identifier: "Remove waiting run").firstMatch.click()
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
    let filename = panel.textFields["Save As:"]
    guard filename.waitForExistence(timeout: 5) else {
      XCTFail("Native Save As control is unavailable: \(app.debugDescription)")
      throw InteractionFailure.expectedPresentation
    }
    filename.click()
    app.typeKey("a", modifierFlags: .command)
    app.typeText(destination.lastPathComponent)
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
  private func choose(_ app: XCUIApplication, path: URL, folder: Bool) async throws {
    if folder {
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
  private func requireRun(_ app: XCUIApplication, counts: String, source: URL) async throws {
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
          && ($0.value as? String)?.contains(source.lastPathComponent) == true
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
  private func requireText(_ app: XCUIApplication, containing text: String) async throws {
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
  private func snapshot(_ app: XCUIApplication, _ name: String) {
    let attachment = XCTAttachment(screenshot: app.screenshot())
    attachment.name = name
    attachment.lifetime = .keepAlways
    add(attachment)
  }
}
