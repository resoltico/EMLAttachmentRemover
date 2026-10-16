// Native GUI tooling only. This is not a product automation mode.
import AppKit
import ApplicationServices
import Foundation

private enum GUIFailure: Error {
  case unavailable(String)
  case budget, launchUnknown, unexpectedApplication
  case accessibility(Int)
  case missingControl(String)
  case bytesChanged, cleanupUnknown
}

@MainActor private final class LaunchReceipt {
  var application: NSRunningApplication?
  var error: Error?
  var complete = false
}

@MainActor private struct Budget {
  let end: ContinuousClock.Instant
  func check() throws { if ContinuousClock.now >= end { throw GUIFailure.budget } }
  func pause() async throws {
    try check()
    try await Task.sleep(for: .milliseconds(50))
  }
}

@MainActor private struct Accessibility {
  let application: NSRunningApplication
  let root: AXUIElement

  init(_ application: NSRunningApplication) throws {
    self.application = application
    root = AXUIElementCreateApplication(application.processIdentifier)
    guard AXUIElementSetMessagingTimeout(root, 0.5) == .success else {
      throw GUIFailure.accessibility(-1)
    }
  }
  func attribute(_ element: AXUIElement, _ key: String, budget: Budget) throws -> CFTypeRef? {
    try budget.check()
    guard !application.isTerminated else { throw GUIFailure.unexpectedApplication }
    guard AXUIElementSetMessagingTimeout(element, 0.5) == .success else {
      throw GUIFailure.accessibility(-1)
    }
    var value: CFTypeRef?
    let status = AXUIElementCopyAttributeValue(element, key as CFString, &value)
    switch status {
    case .success: return value
    case .noValue, .attributeUnsupported: return nil
    default:
      print(
        "AX read failed: operation=copyAttribute attribute=\(key) status=\(status.rawValue) remaining=\(ContinuousClock.now.duration(to: budget.end))"
      )
      throw GUIFailure.accessibility(Int(status.rawValue))
    }
  }
  func elements(from start: AXUIElement? = nil, budget: Budget) throws -> [AXUIElement] {
    var result: [AXUIElement] = []
    var pending: [(AXUIElement, Int)] = [(start ?? root, 0)]
    while let (element, depth) = pending.popLast() {
      try budget.check()
      guard depth <= 40, result.count < 20_000 else { throw GUIFailure.accessibility(-2) }
      result.append(element)
      if let children = try attribute(element, kAXChildrenAttribute, budget: budget)
        as? [AXUIElement]
      {
        pending.append(contentsOf: children.map { ($0, depth + 1) })
      }
    }
    return result
  }
  func text(in start: AXUIElement? = nil, budget: Budget) throws -> [String] {
    var strings: [String] = []
    for element in try elements(from: start, budget: budget) {
      for key in [kAXTitleAttribute, kAXValueAttribute, kAXDescriptionAttribute] {
        if let value = try attribute(element, key, budget: budget) as? String {
          strings.append(value)
        }
      }
    }
    return strings
  }
  func identified(_ identifier: String, budget: Budget) throws -> [AXUIElement] {
    try elements(budget: budget).filter {
      try attribute($0, kAXIdentifierAttribute, budget: budget) as? String == identifier
    }
  }
  func press(_ title: String, budget: Budget) throws {
    for element in try elements(budget: budget) {
      let role = try attribute(element, kAXRoleAttribute, budget: budget) as? String
      guard [kAXButtonRole, kAXDisclosureTriangleRole].contains(role ?? "") else { continue }
      let values = try [kAXTitleAttribute, kAXDescriptionAttribute].compactMap {
        try attribute(element, $0, budget: budget) as? String
      }
      guard values.contains(where: { $0 == title }) else { continue }
      try budget.check()
      let status = AXUIElementPerformAction(element, kAXPressAction as CFString)
      guard status == .success else { throw GUIFailure.accessibility(Int(status.rawValue)) }
      return
    }
    throw GUIFailure.missingControl(title)
  }
}

@main private struct GUIIntegration {
  @MainActor static func main() async throws {
    let began = ContinuousClock.now
    guard CommandLine.arguments.count == 5,
      let seconds = UInt64(CommandLine.arguments[4]), (30...120).contains(seconds)
    else {
      throw GUIFailure.unavailable(
        "Expected app, repository, owned fixture directory and remaining budget")
    }
    let overallEnd = began.advanced(by: .seconds(seconds))
    let action = Budget(end: overallEnd.advanced(by: .seconds(-25)))
    let cleanup = Budget(end: overallEnd)
    let environment = ProcessInfo.processInfo.environment
    guard environment["CI"] == "true", environment["GITHUB_ACTIONS"] == "true",
      environment["RUNNER_ENVIRONMENT"] == "github-hosted",
      let temporary = environment["RUNNER_TEMP"]
    else { throw GUIFailure.unavailable("This fixture requires the owned hosted-runner workflow") }
    // Never prompt or change TCC, accessibility settings or owner preferences.
    guard AXIsProcessTrusted() else {
      throw GUIFailure.unavailable("Accessibility is unavailable to this owned GUI launcher")
    }
    let appURL = URL(filePath: CommandLine.arguments[1], directoryHint: .isDirectory)
    let repository = URL(filePath: CommandLine.arguments[2], directoryHint: .isDirectory)
    let owned = URL(filePath: CommandLine.arguments[3], directoryHint: .isDirectory)
    let temporaryRoot = URL(filePath: temporary, directoryHint: .isDirectory)
      .resolvingSymlinksInPath().standardizedFileURL
    let actualOwned = owned.resolvingSymlinksInPath().standardizedFileURL
    guard owned.standardizedFileURL == actualOwned,
      actualOwned.path.hasPrefix(temporaryRoot.path + "/")
    else {
      throw GUIFailure.unavailable("Fixture must be a non-symlink strict descendant of RUNNER_TEMP")
    }
    guard try FileManager.default.contentsOfDirectory(atPath: owned.path).isEmpty,
      !NSWorkspace.shared.runningApplications.contains(where: {
        $0.bundleIdentifier == "io.github.resoltico.emlattachmentremover"
      })
    else {
      throw GUIFailure.unavailable("Fixture must be empty with no existing product app instance")
    }
    let receipt = LaunchReceipt()
    var receipts = [receipt]
    var application: NSRunningApplication?
    var helperSettlementProven = false
    var fileOpenSubmitted = false
    var failure: Error?
    do {
      let configuration = NSWorkspace.OpenConfiguration()
      configuration.createsNewApplicationInstance = true
      configuration.arguments = ["-copyDestinationBytes", ""]
      try action.check()
      NSWorkspace.shared.openApplication(at: appURL, configuration: configuration) {
        result, error in
        Task { @MainActor in
          receipt.application = result
          receipt.error = error
          receipt.complete = true
        }
      }
      while !receipt.complete { try await action.pause() }
      guard receipt.error == nil, let launched = receipt.application else {
        throw GUIFailure.launchUnknown
      }
      guard isOwned(launched, appURL: appURL) else { throw GUIFailure.unexpectedApplication }
      application = launched
      print("GUI phase: exact launch receipt validated")
      let ax = try Accessibility(launched)
      while true {
        do {
          if try ax.text(budget: action).contains(where: {
            $0.contains("Drop EML files or folders")
          }) {
            break
          }
        } catch GUIFailure.accessibility(let code)
          where code == Int(AXError.cannotComplete.rawValue)
        {
          guard !launched.isTerminated else { throw GUIFailure.unexpectedApplication }
          // Only startup reads are retried; each traversal starts afresh.
        }
        try await action.pause()
      }
      print("GUI phase: empty window ready")
      let source = owned.appending(path: "a.eml")
      let copy = owned.appending(path: "a.no-attachments.eml")
      let input = try Data(
        contentsOf: repository.appending(path: "tests/contract/mime/inputs/01-gmail-attachment.eml")
      )
      let golden = try Data(
        contentsOf: repository.appending(path: "tests/contract/mime/golden/01-gmail-attachment.eml")
      )
      try input.write(to: source, options: [.withoutOverwriting])
      let opened = LaunchReceipt()
      let open = NSWorkspace.OpenConfiguration()
      open.createsNewApplicationInstance = false
      try action.check()
      receipts.append(opened)
      NSWorkspace.shared.open([source], withApplicationAt: appURL, configuration: open) {
        result, error in
        Task { @MainActor in
          opened.application = result
          opened.error = error
          opened.complete = true
        }
      }
      fileOpenSubmitted = true
      print("GUI phase: file-open submitted")
      while !opened.complete { try await action.pause() }
      guard opened.error == nil, opened.application?.isEqual(launched) == true
      else { throw GUIFailure.unexpectedApplication }
      while true {
        let details = try ax.identified("run-details", budget: action)
        if details.count == 1, try ax.text(in: details[0], budget: action).contains("Finished") {
          break
        }
        try await action.pause()
      }
      try ax.press("Technical report", budget: action)
      while true {
        let details = try ax.identified("run-details", budget: action)
        if details.count == 1 {
          let text = try ax.text(in: details[0], budget: action)
          if text.contains("a.eml"),
            text.contains(where: { $0.contains("Actual helper exit: 0") }),
            text.contains(where: { $0.contains("Summary exit: 0") }),
            text.contains(where: {
              $0.contains("Created 1") && $0.contains("Up to date 0") && $0.contains("Failed 0")
            })
          {
            break
          }
        }
        try await action.pause()
      }
      guard try ax.identified("run-list-entry", budget: action).count == 1 else {
        throw GUIFailure.unexpectedApplication
      }
      helperSettlementProven = true  // Actual selected processing result, not version probe.
      print("GUI phase: selected processing settlement proven")
      guard try Data(contentsOf: source) == input, try Data(contentsOf: copy) == golden else {
        throw GUIFailure.bytesChanged
      }
      print(
        "actual GUI Open With: one selected Created1/upToDate0 run, processing exit0/summary0 and independent golden/source bytes"
      )
    } catch {
      print("GUI phase: failure; file-open submitted=\(fileOpenSubmitted)")
      failure = error
    }

    // Launch receipt acquisition/validation is inside this same cleanup boundary.
    // An overdue callback remains Unknown, never proof that no app was launched.
    while receipts.contains(where: { !$0.complete }), ContinuousClock.now < cleanup.end {
      do { try await cleanup.pause() } catch { break }
    }
    let receiptClosureKnown = receipts.allSatisfy(\.complete)
    var ownedApplications: [NSRunningApplication] = []
    for returned in receipts.compactMap(\.application) where isOwned(returned, appURL: appURL) {
      if !ownedApplications.contains(where: { $0.isEqual(returned) }) {
        ownedApplications.append(returned)
      }
    }
    if application?.isTerminated == true, failure == nil {
      failure = GUIFailure.unexpectedApplication
    }
    let normalCleanupEnd = min(cleanup.end, ContinuousClock.now.advanced(by: .seconds(15)))
    for target in ownedApplications where !target.isTerminated {
      if !target.terminate() { failure = failure ?? GUIFailure.cleanupUnknown }
    }
    while ownedApplications.contains(where: { !$0.isTerminated }),
      ContinuousClock.now < normalCleanupEnd
    {
      for target in ownedApplications where !target.isTerminated {
        // A failed active case may still have the real quit confirmation. Only
        // press its exact action; no global app/menu/name targeting is used.
        do {
          try Accessibility(target).press("Stop and Quit", budget: Budget(end: normalCleanupEnd))
        } catch {
          // Keep waiting within the same cleanup allowance.
        }
      }
      do { try await cleanup.pause() } catch { break }
    }
    for target in ownedApplications where !target.isTerminated {
      _ = target.forceTerminate()
      failure = failure ?? GUIFailure.cleanupUnknown
    }
    while ownedApplications.contains(where: { !$0.isTerminated }), ContinuousClock.now < cleanup.end
    {
      do { try await cleanup.pause() } catch { break }
    }
    // No Controller/session/global process scan is an absence proof here.
    guard receiptClosureKnown, !ownedApplications.isEmpty,
      ownedApplications.allSatisfy(\.isTerminated), helperSettlementProven
    else {
      try FileHandle.standardError.write(
        contentsOf: Data(
          "GUI cleanup is unverified; retain this owned fixture and abort later cases\n".utf8))
      throw failure ?? GUIFailure.cleanupUnknown
    }
    if let failure { throw failure }
    print(
      "actual GUI fixture: exact application terminated after proven selected processing settlement"
    )
  }

  @MainActor private static func isOwned(_ application: NSRunningApplication, appURL: URL) -> Bool {
    application.bundleIdentifier == "io.github.resoltico.emlattachmentremover"
      && application.bundleURL?.standardizedFileURL == appURL.standardizedFileURL
  }
}
