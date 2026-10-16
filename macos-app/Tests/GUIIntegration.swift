// Native GUI tooling only. This is not a product automation mode.
import AppKit
import ApplicationServices
import CrashEvidence
import Darwin
import Foundation

private enum GUIFailure: Error {
  case unavailable(String)
  case budget, launchUnknown
  case unexpectedApplication(String)
  case accessibility(Int)
  case missingControl(String)
  case bytesChanged, cleanupUnknown
}

@MainActor private final class Evidence {
  private(set) var failed = false
  // Reporting never interrupts owned cleanup. main refuses success if any
  // checked direct stderr write failed.
  func record(_ message: String) {
    do { try FileHandle.standardError.write(contentsOf: Data((message + "\n").utf8)) } catch {
      failed = true
    }
  }
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
  let evidence: Evidence

  init(_ application: NSRunningApplication, evidence: Evidence) throws {
    self.application = application
    self.evidence = evidence
    root = AXUIElementCreateApplication(application.processIdentifier)
    guard AXUIElementSetMessagingTimeout(root, 0.5) == .success else {
      throw GUIFailure.accessibility(-1)
    }
  }
  func attribute(_ element: AXUIElement, _ key: String, budget: Budget) throws -> CFTypeRef? {
    try budget.check()
    let terminated = application.isTerminated
    guard !terminated else {
      evidence.record("GUI guard: ax_liveness terminated=\(terminated)")
      throw GUIFailure.unexpectedApplication("ax_liveness")
    }
    guard AXUIElementSetMessagingTimeout(element, 0.5) == .success else {
      throw GUIFailure.accessibility(-1)
    }
    var value: CFTypeRef?
    let status = AXUIElementCopyAttributeValue(element, key as CFString, &value)
    switch status {
    case .success: return value
    case .noValue, .attributeUnsupported: return nil
    default:
      evidence.record(
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
  private struct MainImage: Decodable {
    let uuid: UUID
    let architecture: String
    let executable: String
  }
  @MainActor static func main() async {
    let evidence = Evidence()
    do {
      try await run(evidence: evidence)
      if evidence.failed {
        evidence.record("GUI evidence channel previously failed; case is unqualified")
        exit(2)
      }
    } catch {
      evidence.record("GUI fixture failed: \(error)")
      exit(evidence.failed ? 2 : 1)
    }
  }

  @MainActor private static func run(evidence: Evidence) async throws {
    let began = ContinuousClock.now
    guard CommandLine.arguments.count == 6,
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
    let evidenceDirectory = URL(filePath: CommandLine.arguments[5], directoryHint: .isDirectory)
    let expectationURL = evidenceDirectory.appending(path: "expected-main-image.json")
    guard (try expectationURL.resourceValues(forKeys: [.fileSizeKey]).fileSize ?? Int.max) <= 16_384
    else { throw GUIFailure.unavailable("Expected main-image identity is not bounded") }
    let expectedImage = try JSONDecoder().decode(
      MainImage.self, from: Data(contentsOf: expectationURL))
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
    var launchIdentity: OwnedLaunch?
    var failure: Error?
    do {
      let configuration = NSWorkspace.OpenConfiguration()
      configuration.createsNewApplicationInstance = true
      configuration.arguments = ["-copyDestinationBytes", ""]
      try action.check()
      let submittedAt = Date()
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
      let identifierMatch = launched.bundleIdentifier == "io.github.resoltico.emlattachmentremover"
      let urlMatch = launched.bundleURL?.standardizedFileURL == appURL.standardizedFileURL
      evidence.record(
        "GUI guard: initial_receipt identifierMatch=\(identifierMatch) urlMatch=\(urlMatch) terminated=\(launched.isTerminated)"
      )
      guard identifierMatch, urlMatch else {
        throw GUIFailure.unexpectedApplication("initial_receipt_identity")
      }
      application = launched
      let receivedAt = Date()
      if !launched.isTerminated, let executable = launched.executableURL,
        executable.standardizedFileURL.path == expectedImage.executable,
        let launchDate = launched.launchDate,
        launchDate >= submittedAt, launchDate <= receivedAt
      {
        launchIdentity = OwnedLaunch(
          pid: Int(launched.processIdentifier), executablePath: expectedImage.executable,
          imageUUID: expectedImage.uuid, architecture: expectedImage.architecture,
          bundleIdentifier: "io.github.resoltico.emlattachmentremover", launchDate: launchDate,
          submittedAt: submittedAt, receivedAt: receivedAt)
      }
      evidence.record(
        "GUI launch-instance crash-attribution snapshot available=\(launchIdentity != nil)")
      evidence.record("GUI phase: initial receipt identity matched")
      let ax = try Accessibility(launched, evidence: evidence)
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
          let terminated = launched.isTerminated
          guard !terminated else {
            evidence.record("GUI guard: startup_liveness terminated=\(terminated)")
            throw GUIFailure.unexpectedApplication("startup_liveness")
          }
          // Only startup reads are retried; each traversal starts afresh.
        }
        try await action.pause()
      }
      evidence.record("GUI phase: empty window ready")
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
      evidence.record("GUI phase: file-open submitted")
      while !opened.complete { try await action.pause() }
      let errorPresent = opened.error != nil
      let appPresent = opened.application != nil
      let sameInstance = opened.application?.isEqual(launched) == true
      evidence.record(
        "GUI guard: file_open_receipt errorPresent=\(errorPresent) appPresent=\(appPresent) sameInstance=\(sameInstance)"
      )
      guard !errorPresent, sameInstance else {
        throw GUIFailure.unexpectedApplication("file_open_receipt")
      }
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
      let runCount = try ax.identified("run-list-entry", budget: action).count
      guard runCount == 1 else {
        evidence.record("GUI guard: admitted_run_count runCount=\(runCount)")
        throw GUIFailure.unexpectedApplication("admitted_run_count")
      }
      helperSettlementProven = true  // Actual selected processing result, not version probe.
      evidence.record("GUI phase: selected processing settlement proven")
      guard try Data(contentsOf: source) == input, try Data(contentsOf: copy) == golden else {
        throw GUIFailure.bytesChanged
      }
      evidence.record(
        "actual GUI Open With: one selected Created1/upToDate0 run, processing exit0/summary0 and independent golden/source bytes"
      )
    } catch {
      evidence.record("GUI phase: failure; file-open submitted=\(fileOpenSubmitted)")
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
      evidence.record("GUI guard: before_cleanup terminated=true")
      failure = GUIFailure.unexpectedApplication("terminated_before_cleanup")
    }
    let unexpectedStartupEnd =
      !fileOpenSubmitted && application?.isTerminated == true ? Date() : nil
    let normalCleanupEnd = min(cleanup.end, ContinuousClock.now.advanced(by: .seconds(15)))
    evidence.record(
      "GUI phase: cleanup start receiptClosureKnown=\(receiptClosureKnown) ownedCount=\(ownedApplications.count) helperSettlementProven=\(helperSettlementProven)"
    )
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
          try Accessibility(target, evidence: evidence).press(
            "Stop and Quit", budget: Budget(end: normalCleanupEnd))
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
    let allTerminated = ownedApplications.allSatisfy(\.isTerminated)
    evidence.record(
      "GUI phase: cleanup observation receiptClosureKnown=\(receiptClosureKnown) ownedCount=\(ownedApplications.count) allTerminated=\(allTerminated) helperSettlementProven=\(helperSettlementProven)"
    )
    if let launchIdentity, let observedEnd = unexpectedStartupEnd {
      do {
        let observation = try await collectStartupReport(
          launch: launchIdentity, observedEnd: observedEnd,
          evidenceDirectory: evidenceDirectory,
          deadline: min(cleanup.end, ContinuousClock.now.advanced(by: .seconds(2))))
        evidence.record("GUI owned startup report: " + observation)
      } catch {
        evidence.record(
          "GUI owned startup report unobserved: collection failed; original failure retained")
      }
    }
    guard receiptClosureKnown, !ownedApplications.isEmpty,
      allTerminated, helperSettlementProven
    else {
      evidence.record("GUI cleanup is unverified; retain this owned fixture and abort later cases")
      throw failure ?? GUIFailure.cleanupUnknown
    }
    if let failure { throw failure }
    evidence.record(
      "actual GUI fixture: exact application terminated after proven selected processing settlement"
    )
  }

  @MainActor private static func isOwned(_ application: NSRunningApplication, appURL: URL) -> Bool {
    application.bundleIdentifier == "io.github.resoltico.emlattachmentremover"
      && application.bundleURL?.standardizedFileURL == appURL.standardizedFileURL
  }
}
