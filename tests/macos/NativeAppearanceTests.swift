// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import AppKit
import XCTest

extension DesktopInteractionTests {
  private enum NativeSettingsPane { case appearance, display }

  private struct AppearanceState {
    let selection: String
    let contrast: Bool
    let transparency: Bool
  }
  @MainActor
  private final class Restoration { var completed = false }

  @MainActor
  func testOwnedNativeAppearance() async throws {
    let repository = URL(fileURLWithPath: #filePath)
      .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    let inputURL = repository.appendingPathComponent(
      "target/desktop-interaction-appearance/input.json")
    let input = try XCTUnwrap(
      JSONSerialization.jsonObject(with: Data(contentsOf: inputURL)) as? [String: String])
    guard input["host_scope"] == "fresh-github-hosted-runner",
      input["appearance_session"] == "true"
    else { throw InteractionFailure.expectedPresentation }
    let appURL = URL(fileURLWithPath: try XCTUnwrap(input["app"]))
    let fixture = URL(fileURLWithPath: try XCTUnwrap(input["fixture"]))
    let receipt = try XCTUnwrap(
      JSONSerialization.jsonObject(
        with: Data(
          contentsOf: inputURL.deletingLastPathComponent().appendingPathComponent("ownership.json"))
      )
        as? [String: Any])
    let pid = try XCTUnwrap(receipt["pid"] as? NSNumber).int32Value
    let productOwner = try boundApplication(receipt, url: appURL)
    let settingsReceipt = try XCTUnwrap(receipt["settings"] as? [String: Any])
    let settingsURL = URL(fileURLWithPath: "/System/Applications/System Settings.app")
    let settingsOwner = try boundApplication(settingsReceipt, url: settingsURL)
    let app = XCUIApplication(url: appURL)
    let settings = XCUIApplication(url: settingsURL)
    try await requireOwnedForeground(productOwner)
    guard app.windows["EML Attachment Remover"].waitForExistence(timeout: 20)
    else { throw InteractionFailure.expectedPresentation }
    let source = fixture.appendingPathComponent("one.eml")
    let original = try Data(contentsOf: source)
    let golden = try Data(
      contentsOf: repository.appendingPathComponent(
        "tests/contract/mime/golden/01-gmail-attachment.eml"))
    let configuration = NSWorkspace.OpenConfiguration()
    configuration.createsNewApplicationInstance = false
    configuration.addsToRecentItems = false
    let recipient = try await NSWorkspace.shared.open(
      [source], withApplicationAt: appURL, configuration: configuration)
    XCTAssertEqual(recipient.processIdentifier, pid)
    let technical = app.checkBoxes["Technical details"]
    guard technical.waitForExistence(timeout: 10) else {
      throw InteractionFailure.expectedPresentation
    }
    technical.click()
    try await requireRun(app, counts: "1 created · 0 up to date", source: source)

    try await settingsPane(settings, owner: settingsOwner, pane: .appearance)
    var selected = [String]()
    for name in ["Light", "Dark", "Auto"] {
      if try settingSelected(try await setting(settings, name: name), name: name) {
        selected.append(name)
      }
    }
    guard selected.count == 1 else {
      XCTFail("Original native appearance selector is unavailable: \(settings.debugDescription)")
      throw InteractionFailure.expectedPresentation
    }
    try await settingsPane(settings, owner: settingsOwner, pane: .display)
    let before = AppearanceState(
      selection: selected[0],
      contrast: try settingSelected(
        try await setting(settings, name: "Increase contrast"), name: "Increase contrast"),
      transparency: try settingSelected(
        try await setting(settings, name: "Reduce transparency"), name: "Reduce transparency"))
    let beforeSettings = try await nativeDisplaySettings(
      settings, owner: settingsOwner, contrast: before.contrast, transparency: before.transparency)
    let restoration = Restoration()
    addTeardownBlock { @MainActor in
      if !restoration.completed {
        _ = try await self.restoreAppearance(
          before, settings: settings, owner: settingsOwner, product: app, productOwner: productOwner
        )
      }
    }
    try await setSetting(settings, name: "Increase contrast", selected: false)
    try await settingsPane(settings, owner: settingsOwner, pane: .appearance)
    var observations = [String]()
    for (name, dark) in [("Light", false), ("Dark", true)] {
      try await setSetting(settings, name: name, selected: true)
      try await switchAppearanceApplication(productOwner, from: settingsOwner)
      let expected =
        "Interface native appearance: \(dark ? "dark" : "light"); increased contrast: false"
      try await requireText(app, containing: expected)
      observations.append(expected)
      snapshot(app, "native-appearance-\(name.lowercased())")
      try await switchAppearanceApplication(settingsOwner, from: productOwner)
    }
    try await settingsPane(settings, owner: settingsOwner, pane: .display)
    try await setSetting(settings, name: "Increase contrast", selected: true)
    let contrastSettings = try await nativeDisplaySettings(
      settings, owner: settingsOwner, contrast: true)
    try await switchAppearanceApplication(productOwner, from: settingsOwner)
    let contrast = "Interface native appearance: dark; increased contrast: true"
    try await requireText(app, containing: contrast)
    observations.append(contrast)
    snapshot(app, "native-appearance-increased-contrast")
    let restoredSettings = try await restoreAppearance(
      before, settings: settings, owner: settingsOwner, product: app, productOwner: productOwner)
    restoration.completed = true
    XCTAssertEqual(try Data(contentsOf: source), original)
    XCTAssertEqual(
      try Data(
        contentsOf: source.deletingPathExtension().appendingPathExtension("no-attachments.eml")),
      golden)
    let observation: [String: Any] = [
      "schema": 1, "owned_pid": pid, "settings_pid": settingsOwner.processIdentifier,
      "conditions": ["light", "dark", "increased-contrast"], "restored": true,
      "original_selector": before.selection, "original_contrast": before.contrast,
      "original_reduce_transparency": before.transparency,
      "native_product_observations": observations,
      "native_settings_observations": [
        "before": beforeSettings, "increased_contrast": contrastSettings,
        "restored": restoredSettings,
      ],
    ]
    let attachment = XCTAttachment(
      data: try JSONSerialization.data(withJSONObject: observation),
      uniformTypeIdentifier: "public.json")
    attachment.name = "native-appearance-observation"
    attachment.lifetime = .keepAlways
    add(attachment)
    app.typeKey("q", modifierFlags: .command)
    guard app.buttons["Confirm"].waitForExistence(timeout: 5) else {
      throw InteractionFailure.expectedPresentation
    }
    app.buttons["Confirm"].click()
    XCTAssertTrue(app.wait(for: .notRunning, timeout: 20))
    XCTAssertTrue(productOwner.isTerminated)
  }

  @MainActor
  private func boundApplication(_ receipt: [String: Any], url: URL) throws -> NSRunningApplication {
    let pid = try XCTUnwrap(receipt["pid"] as? NSNumber).int32Value
    let date = try XCTUnwrap(receipt["launch_date"] as? NSNumber).doubleValue
    let owner = try XCTUnwrap(NSRunningApplication(processIdentifier: pid))
    guard !owner.isTerminated, receipt["bundle_path"] as? String == url.path,
      owner.bundleURL?.standardizedFileURL == url.standardizedFileURL,
      owner.launchDate?.timeIntervalSince1970 == date,
      let identifier = owner.bundleIdentifier,
      NSRunningApplication.runningApplications(withBundleIdentifier: identifier).count == 1
    else { throw InteractionFailure.expectedPresentation }
    return owner
  }

  @MainActor
  private func settingsPane(
    _ settings: XCUIApplication, owner: NSRunningApplication, pane: NativeSettingsPane
  ) async throws {
    guard !owner.isTerminated else { throw InteractionFailure.expectedPresentation }
    let requested = owner.activate(options: [])
    print("Retained Settings activation request: PID \(owner.processIdentifier), sent=\(requested)")
    try await requireOwnedForeground(owner)
    let paneTitle = pane == .appearance ? "Appearance" : "Display"
    let readinessControl = pane == .appearance ? "Light" : "Increase contrast"
    if settings.windows[paneTitle].exists {
      _ = try await setting(settings, name: readinessControl)
      print("Native Settings pane already observed: \(paneTitle), PID \(owner.processIdentifier)")
      return
    }
    // A LaunchServices URL acknowledgment does not establish pane navigation.
    // Use the owned application's native View command, then observe its controls.
    let section = pane == .appearance ? paneTitle : "Accessibility"
    let view = settings.menuBars.menuBarItems["View"]
    guard view.waitForExistence(timeout: 15), view.isEnabled, view.isHittable else {
      throw InteractionFailure.expectedPresentation
    }
    view.click()
    let commands = view.menus.menuItems.matching(
      NSPredicate(format: "title == %@ OR label == %@", section, section))
    let command = commands.firstMatch
    guard command.waitForExistence(timeout: 5), commands.count == 1,
      command.isEnabled, command.isHittable
    else {
      throw InteractionFailure.expectedPresentation
    }
    command.click()
    if pane == .display {
      let main = settings.windows["main"]
      let named = NSPredicate(format: "title == %@ OR label == %@", "Display", "Display")
      var link: XCUIElement?
      let available = NSPredicate { _, _ in
        link = nil
        if settings.windows[paneTitle].exists { return true }
        let buttons = main.buttons.matching(named)
        let labels = main.staticTexts.matching(named)
        let element: XCUIElement
        let buttonCount = buttons.count
        if buttonCount == 1 {
          element = buttons.firstMatch
        } else if buttonCount == 0, labels.count == 1 {
          element = labels.firstMatch
        } else {
          return false
        }
        guard element.exists, element.isEnabled, element.isHittable else { return false }
        link = element
        return true
      }
      guard
        await XCTWaiter.fulfillment(
          of: [XCTNSPredicateExpectation(predicate: available, object: nil)], timeout: 15)
          == .completed
      else {
        XCTFail("Native Accessibility Display link is unavailable: \(settings.debugDescription)")
        throw InteractionFailure.expectedPresentation
      }
      if let link { link.click() }
    }
    _ = try await setting(settings, name: readinessControl)
    print("Native Settings pane observed through controls: \(pane), PID \(owner.processIdentifier)")
  }

  @MainActor
  private func switchAppearanceApplication(
    _ target: NSRunningApplication, from source: NSRunningApplication
  ) async throws {
    if NSWorkspace.shared.frontmostApplication?.processIdentifier == target.processIdentifier {
      try await requireOwnedForeground(target)
      return
    }
    guard !target.isTerminated, !source.isTerminated, source.isActive,
      NSWorkspace.shared.frontmostApplication?.processIdentifier == source.processIdentifier
    else { throw InteractionFailure.expectedPresentation }
    // Request this retained process directly; the system app switcher's MRU
    // order is not an identity binding. The acknowledgment is not a focus oracle.
    let requested = target.activate(options: [])
    print(
      "Retained native activation request: source=\(source.processIdentifier), target=\(target.processIdentifier), sent=\(requested)"
    )
    try await requireOwnedForeground(target)
  }

  @MainActor
  private func setting(_ settings: XCUIApplication, name: String) async throws -> XCUIElement {
    var found: XCUIElement?
    let roles: [XCUIElementQuery]
    let named: NSPredicate
    switch name {
    case "Increase contrast":
      roles = [settings.switches]
      named = NSPredicate(format: "identifier == %@", "AX_INCREASE_CONTRAST")
    case "Reduce transparency":
      roles = [settings.switches]
      named = NSPredicate(format: "identifier == %@", "AX_REDUCE_TRANSPARENCY")
    default:
      roles = [settings.radioButtons, settings.checkBoxes, settings.buttons]
      named = NSPredicate(format: "title == %@ OR label == %@", name, name)
    }
    let predicate = NSPredicate { _, _ in
      found = nil
      var count = 0
      for role in roles {
        let current = role.count
        guard current <= 2048 - count else { return false }
        count += current
      }
      let matching = roles.map { $0.matching(named) }
      let matches = matching.map { $0.count }
      guard matches.reduce(0, +) == 1, let index = matches.firstIndex(of: 1) else { return false }
      let element = matching[index].firstMatch
      guard element.exists else { return false }
      found = element
      return true
    }
    let result = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: predicate, object: nil)], timeout: 15)
    guard result == .completed, let found else {
      XCTFail("Native Settings control \(name) is unavailable: \(settings.debugDescription)")
      throw InteractionFailure.expectedPresentation
    }
    return found
  }

  @MainActor
  private func settingSelected(_ element: XCUIElement, name: String) throws -> Bool {
    if ["Light", "Dark", "Auto"].contains(name), element.elementType == .button {
      let selected = element.isSelected
      print(
        "Native appearance selector \(name): selected=\(selected), value=\(String(describing: element.value))"
      )
      return selected
    }
    if let value = element.value as? NSNumber { return value.boolValue }
    if let value = element.value as? String {
      if ["1", "true"].contains(value) { return true }
      if ["0", "false"].contains(value) { return false }
    }
    if element.elementType == .radioButton { return element.isSelected }
    XCTFail("Native Settings control has no observable selected value: \(element.debugDescription)")
    throw InteractionFailure.expectedPresentation
  }

  @MainActor
  private func setSetting(_ settings: XCUIApplication, name: String, selected: Bool) async throws {
    let element = try await setting(settings, name: name)
    if try settingSelected(element, name: name) != selected {
      guard element.isEnabled && element.isHittable else {
        throw InteractionFailure.expectedPresentation
      }
      element.click()
    }
    let predicate = NSPredicate { _, _ in
      (try? self.settingSelected(element, name: name)) == selected
    }
    let result = await XCTWaiter.fulfillment(
      of: [XCTNSPredicateExpectation(predicate: predicate, object: nil)], timeout: 10)
    guard result == .completed else { throw InteractionFailure.expectedPresentation }
  }

  @MainActor
  private func nativeDisplaySettings(
    _ settings: XCUIApplication, owner: NSRunningApplication,
    contrast: Bool, transparency: Bool? = nil
  ) async throws -> [String: Bool] {
    try await settingsPane(settings, owner: owner, pane: .display)
    let actualContrast = try settingSelected(
      try await setting(settings, name: "Increase contrast"), name: "Increase contrast")
    let actualTransparency = try settingSelected(
      try await setting(settings, name: "Reduce transparency"), name: "Reduce transparency")
    print(
      "Native Settings display readback: contrast=\(actualContrast), transparency=\(actualTransparency)"
    )
    guard actualContrast == contrast, transparency.map({ actualTransparency == $0 }) != false else {
      XCTFail("Native Settings display values differ from the requested/restored selectors")
      throw InteractionFailure.expectedPresentation
    }
    return ["increase_contrast": actualContrast, "reduce_transparency": actualTransparency]
  }

  @MainActor
  private func restoreAppearance(
    _ state: AppearanceState, settings: XCUIApplication, owner: NSRunningApplication,
    product: XCUIApplication, productOwner: NSRunningApplication
  ) async throws -> [String: Bool] {
    try await settingsPane(settings, owner: owner, pane: .display)
    try await setSetting(settings, name: "Increase contrast", selected: state.contrast)
    try await setSetting(settings, name: "Reduce transparency", selected: state.transparency)
    try await settingsPane(settings, owner: owner, pane: .appearance)
    try await setSetting(settings, name: state.selection, selected: true)
    let display = try await nativeDisplaySettings(
      settings, owner: owner, contrast: state.contrast, transparency: state.transparency)
    try await switchAppearanceApplication(productOwner, from: owner)
    if state.selection != "Auto" {
      try await requireText(
        product,
        containing:
          "Interface native appearance: \(state.selection.lowercased()); increased contrast: \(state.contrast)"
      )
    } else {
      try await requireText(product, containing: "increased contrast: \(state.contrast)")
    }
    snapshot(product, "native-appearance-restored")
    return display
  }
}
