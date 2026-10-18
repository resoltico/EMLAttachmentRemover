// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import AppKit
import Darwin
import Foundation

// Test-only lifetime authority. XCTest receives observations for attachment;
// this process retains the NSWorkspace-returned object across runner failure.
#if !DESKTOP_FIXTURE_TESTS
  @main
#endif
@MainActor
struct DesktopInteractionOwner {
  private final class Cancellation { var requested = false }
  private enum Failure: Error {
    case invalidInput, existingApplication, launchIdentity, unexpectedApplication,
      forcedApplication, deadline, cancelled
  }

  static func main() async {
    var failure: String?
    var app: NSRunningApplication?
    var settings: NSRunningApplication?
    var ownershipReceipt: [String: Any]?
    var board: NSPasteboard?
    var originals: [(URL, Data)] = []
    var originalsUnchanged = false
    var childReaped = false
    var childExit: Int32?
    var forcedApp = false
    var timedOut = false
    var unexpected: [pid_t] = []
    var unexpectedSettings: [pid_t] = []
    var stackSamples: [[String: Any]] = []
    var pickerBaselineSettled = false
    var pickerLogObservation: [String: Any]?
    var diagnosticProcess: Process?
    var diagnosticSettled = true
    var outputPipe: Pipe?
    let fixtureOutput = DesktopFixtureOutput()
    var outputDrainComplete: Bool?
    var heldCopy: (source: URL, held: URL, expected: Data)?
    var copyWasHeld = false
    var copyRestoredByRequest = false
    var copyRestoredDuringCleanup = false
    var copyRestorationFailure: String?
    let child = Process()
    let executionDeadline = ProcessInfo.processInfo.systemUptime + 1800
    let cancellation = Cancellation()
    let signals = [SIGINT, SIGTERM].map { number in
      Darwin.signal(number, SIG_IGN)
      let source = DispatchSource.makeSignalSource(signal: number, queue: .main)
      source.setEventHandler { MainActor.assumeIsolated { cancellation.requested = true } }
      source.resume()
      return source
    }
    let arguments = CommandLine.arguments
    var resultURL: URL?
    do {
      let environment = ProcessInfo.processInfo.environment
      guard environment["GITHUB_ACTIONS"] == "true",
        environment["RUNNER_ENVIRONMENT"] == "github-hosted", arguments.count >= 5
      else { throw Failure.invalidInput }
      let inputURL = URL(fileURLWithPath: arguments[1])
      let receiptURL = URL(fileURLWithPath: arguments[2])
      resultURL = URL(fileURLWithPath: arguments[3])
      let input =
        try JSONSerialization.jsonObject(with: Data(contentsOf: inputURL)) as? [String: String]
      guard let input, input["host_scope"] == "fresh-github-hosted-runner",
        let appPath = input["app"], let fixturePath = input["fixture"]
      else { throw Failure.invalidInput }
      let control = input["owner_control"] ?? "normal"
      guard ["normal", "deadline", "force-app"].contains(control) else {
        throw Failure.invalidInput
      }
      let appURL = URL(fileURLWithPath: appPath)
      guard let identifier = Bundle(url: appURL)?.bundleIdentifier else {
        throw Failure.invalidInput
      }
      guard NSRunningApplication.runningApplications(withBundleIdentifier: identifier).isEmpty
      else { throw Failure.existingApplication }
      let fixture = URL(fileURLWithPath: fixturePath)
      for name in [
        "one.eml", "two.eml", "folder-input/folder.eml",
        "service-input/日本語-مرحبا-\u{202e}report.eml",
      ] {
        let url = fixture.appendingPathComponent(name)
        originals.append((url, try Data(contentsOf: url)))
      }
      if input["pagination_session"] == "true" {
        for index in 0..<65 {
          let url = fixture.appendingPathComponent(
            String(format: "pagination-input/%03d.eml", index))
          originals.append((url, try Data(contentsOf: url)))
        }
      }
      let configuration = NSWorkspace.OpenConfiguration()
      if input["appearance_session"] == "true" {
        let settingsURL = URL(fileURLWithPath: "/System/Applications/System Settings.app")
        guard
          NSRunningApplication.runningApplications(
            withBundleIdentifier: "com.apple.systempreferences"
          )
          .isEmpty
        else { throw Failure.existingApplication }
        let settingsConfiguration = NSWorkspace.OpenConfiguration()
        settingsConfiguration.createsNewApplicationInstance = true
        settingsConfiguration.addsToRecentItems = false
        let ownedSettings = try await NSWorkspace.shared.openApplication(
          at: settingsURL, configuration: settingsConfiguration)
        settings = ownedSettings
        guard ownedSettings.bundleIdentifier == "com.apple.systempreferences",
          ownedSettings.bundleURL?.standardizedFileURL == settingsURL.standardizedFileURL,
          ownedSettings.launchDate != nil
        else { throw Failure.launchIdentity }
      }
      configuration.createsNewApplicationInstance = true
      configuration.addsToRecentItems = false
      configuration.arguments = ["-copyDestinationBytes", input["destination"] ?? ""]
      if let observation = input["observation"] {
        guard
          ["stop", "force-stop", "active-quit", "published-error"].contains(input["scenario"] ?? "")
        else {
          throw Failure.invalidInput
        }
        configuration.environment = ProcessInfo.processInfo.environment.merging(
          ["EML_PUBLICATION_OBSERVATION": observation]) { _, replacement in replacement }
      }
      if cancellation.requested { throw Failure.cancelled }
      let launched = try await NSWorkspace.shared.openApplication(
        at: appURL, configuration: configuration)
      app = launched
      if cancellation.requested { throw Failure.cancelled }
      if ProcessInfo.processInfo.systemUptime >= executionDeadline {
        timedOut = true
        throw Failure.deadline
      }
      guard let date = launched.launchDate,
        launched.bundleURL?.standardizedFileURL == appURL.standardizedFileURL
      else { throw Failure.launchIdentity }
      let launchDeadline = min(executionDeadline, ProcessInfo.processInfo.systemUptime + 20)
      while !launched.isFinishedLaunching {
        if cancellation.requested { throw Failure.cancelled }
        if launched.isTerminated || ProcessInfo.processInfo.systemUptime >= launchDeadline {
          throw Failure.launchIdentity
        }
        try await Task.sleep(for: .milliseconds(50))
      }
      let pasteboard = NSPasteboard.withUniqueName()
      board = pasteboard
      var receipt: [String: Any] = [
        "pid": launched.processIdentifier, "launch_date": date.timeIntervalSince1970,
        "bundle_path": appPath, "pasteboard": pasteboard.name.rawValue,
      ]
      if let settings, let settingsDate = settings.launchDate, let settingsURL = settings.bundleURL
      {
        receipt["settings"] = [
          "pid": settings.processIdentifier, "launch_date": settingsDate.timeIntervalSince1970,
          "bundle_path": settingsURL.path,
        ]
      }
      ownershipReceipt = receipt
      try JSONSerialization.data(withJSONObject: receipt).write(
        to: receiptURL, options: .withoutOverwriting)
      guard launched.activate(options: []) else { throw Failure.launchIdentity }
      child.executableURL = URL(fileURLWithPath: arguments[4])
      child.arguments = Array(arguments.dropFirst(5))
      child.standardInput = FileHandle.nullDevice
      if input["fixture_output"] == "true" {
        let pipe = Pipe()
        outputPipe = pipe
        child.standardOutput = pipe
        try fixtureOutput.relay(from: pipe.fileHandleForReading, to: FileHandle.standardOutput)
      } else {
        child.standardOutput = FileHandle.standardOutput
      }
      child.standardError = FileHandle.standardError
      try child.run()
      var ready = false
      var shortDeadline: TimeInterval?
      while child.isRunning {
        if let relayFailure = fixtureOutput.observedRelayFailure() {
          failure = failure ?? relayFailure
          throw Failure.invalidInput
        }
        let request = fixtureOutput.take()
        if request == .holdCopy {
          guard !copyWasHeld, originals.count == 4 else { throw Failure.invalidInput }
          let source = originals[3].0.deletingPathExtension().appendingPathExtension(
            "no-attachments.eml")
          let held = fixture.appendingPathComponent("held-service-copy.eml")
          let repository = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent()
          let golden = try Data(
            contentsOf: repository.appendingPathComponent(
              "tests/contract/mime/golden/01-gmail-attachment.eml"))
          guard try Data(contentsOf: source) == golden,
            !FileManager.default.fileExists(atPath: held.path)
          else { throw Failure.invalidInput }
          try moveWithoutReplacement(from: source, to: held)
          heldCopy = (source, held, golden)
          copyWasHeld = true
          guard try Data(contentsOf: held) == golden,
            !FileManager.default.fileExists(atPath: source.path)
          else { throw Failure.invalidInput }
          try acknowledge("owned copy held\n", at: fixture.appendingPathComponent("copy-held"))
        } else if request == .restoreCopy {
          guard let held = heldCopy, try Data(contentsOf: held.held) == held.expected,
            !FileManager.default.fileExists(atPath: held.source.path)
          else { throw Failure.invalidInput }
          try moveWithoutReplacement(from: held.held, to: held.source)
          heldCopy = nil
          guard try Data(contentsOf: held.source) == held.expected else {
            throw Failure.invalidInput
          }
          copyRestoredByRequest = true
          try acknowledge(
            "owned copy restored\n", at: fixture.appendingPathComponent("copy-restored"))
        }
        if request == .picker || request == .pickerBaseline,
          stackSamples.count < 4, !launched.isTerminated
        {
          if request == .pickerBaseline && pickerBaselineSettled { throw Failure.invalidInput }
          let sample = Process()
          let sampleURL = URL(fileURLWithPath: arguments[3]).deletingLastPathComponent()
            .appendingPathComponent("native-picker-stack-\(stackSamples.count).txt")
          guard !FileManager.default.fileExists(atPath: sampleURL.path),
            launched.launchDate?.timeIntervalSince1970 == date.timeIntervalSince1970,
            launched.bundleURL?.standardizedFileURL == appURL.standardizedFileURL
          else { throw Failure.launchIdentity }
          sample.executableURL = URL(fileURLWithPath: "/usr/bin/sample")
          sample.arguments = [String(launched.processIdentifier), "3", "-file", sampleURL.path]
          sample.standardInput = FileHandle.nullDevice
          sample.standardOutput = FileHandle.standardError
          sample.standardError = FileHandle.standardError
          try sample.run()
          diagnosticProcess = sample
          let sampleDeadline = min(executionDeadline, ProcessInfo.processInfo.systemUptime + 10)
          while sample.isRunning && !cancellation.requested
            && ProcessInfo.processInfo.systemUptime < sampleDeadline
          {
            try await Task.sleep(for: .milliseconds(50))
          }
          if sample.isRunning { sample.terminate() }
          let settle = ProcessInfo.processInfo.systemUptime + 0.25
          while sample.isRunning && ProcessInfo.processInfo.systemUptime < settle {
            try await Task.sleep(for: .milliseconds(50))
          }
          guard !sample.isRunning else { throw Failure.deadline }
          sample.waitUntilExit()
          diagnosticProcess = nil
          stackSamples.append([
            "phase": request == .pickerBaseline ? "before-picker" : "after-picker-request",
            "path": sampleURL.path, "status": sample.terminationStatus,
            "reason": sample.terminationReason.rawValue,
            "retained_target_alive_after": !launched.isTerminated,
            "scope": "read-only diagnostic; never a picker or product qualification verdict",
          ])
          if request == .pickerBaseline {
            pickerBaselineSettled = true
            try acknowledge(
              "owned picker baseline settled\n",
              at: fixture.appendingPathComponent("picker-baseline-settled"))
          }
        }
        let runnerReady =
          input["runner_ready"].map { FileManager.default.fileExists(atPath: $0) } ?? true
        if !ready && runnerReady {
          let marker = receiptURL.deletingPathExtension().appendingPathExtension("ready.json")
          try JSONSerialization.data(withJSONObject: [
            "app_pid": launched.processIdentifier, "runner_pid": child.processIdentifier,
          ]).write(to: marker, options: .withoutOverwriting)
          ready = true
          if control == "deadline" { shortDeadline = ProcessInfo.processInfo.systemUptime + 0.25 }
          if control == "force-app" {
            forcedApp = true
            _ = launched.forceTerminate()
            throw Failure.forcedApplication
          }
        }
        unexpected = NSRunningApplication.runningApplications(withBundleIdentifier: identifier)
          .filter { !$0.isTerminated && $0.processIdentifier != launched.processIdentifier }.map(
            \.processIdentifier)
        if !unexpected.isEmpty { throw Failure.unexpectedApplication }
        if let settings {
          unexpectedSettings = NSRunningApplication.runningApplications(
            withBundleIdentifier: "com.apple.systempreferences"
          ).filter { !$0.isTerminated && $0.processIdentifier != settings.processIdentifier }
            .map(\.processIdentifier)
          if settings.isTerminated || !unexpectedSettings.isEmpty {
            throw Failure.unexpectedApplication
          }
        }
        if cancellation.requested { throw Failure.cancelled }
        if ProcessInfo.processInfo.systemUptime
          >= min(executionDeadline, shortDeadline ?? executionDeadline)
        {
          timedOut = true
          throw Failure.deadline
        }
        try await Task.sleep(for: .milliseconds(50))
      }
      // Observed exit precedes this reap; never wait synchronously on a live child.
      child.waitUntilExit()
      childReaped = true
      childExit = child.terminationStatus
      if child.terminationReason != .exit || child.terminationStatus != 0 {
        failure =
          "Interaction runner failed: reason=\(child.terminationReason.rawValue), status=\(child.terminationStatus)"
      }
      if outputPipe != nil {
        let drainDeadline = ProcessInfo.processInfo.systemUptime + 2
        while !fixtureOutput.hasEnded() && ProcessInfo.processInfo.systemUptime < drainDeadline {
          try await Task.sleep(for: .milliseconds(50))
        }
        outputDrainComplete = fixtureOutput.hasEnded()
        if !fixtureOutput.hasEnded() {
          failure = failure ?? "Interaction output drain did not settle"
        }
        if let relayFailure = fixtureOutput.observedRelayFailure() {
          failure = failure ?? relayFailure
        }
      }
      // Collect after runner settlement so the interval includes late initialization errors.
      if pickerBaselineSettled {
        // Query only the retained product PID and its owned launch interval.
        // System-wide logs and unrelated processes are outside this fixture.
        let logURL = URL(fileURLWithPath: arguments[3]).deletingLastPathComponent()
          .appendingPathComponent(
            "native-picker-owned-process.log")
        try Data().write(to: logURL, options: .withoutOverwriting)
        let output = try FileHandle(forWritingTo: logURL)
        let log = Process()
        log.executableURL = URL(fileURLWithPath: "/usr/bin/log")
        let end = Date().timeIntervalSince1970
        guard let image = Bundle(url: appURL)?.executableURL else { throw Failure.invalidInput }
        let predicate = NSPredicate(format: "processImagePath == %@", image.path).predicateFormat
        log.arguments = [
          "show", "--process", String(launched.processIdentifier), "--start",
          "@\(Int(date.timeIntervalSince1970))", "--end", "@\(Int(end) + 1)", "--style",
          "compact", "--info", "--debug", "--color", "none", "--predicate", predicate,
        ]
        log.standardInput = FileHandle.nullDevice
        log.standardOutput = output
        log.standardError = output
        try log.run()
        diagnosticProcess = log
        let deadline = min(executionDeadline, ProcessInfo.processInfo.systemUptime + 8)
        var outputLimitReached = false
        while log.isRunning && !cancellation.requested
          && ProcessInfo.processInfo.systemUptime < deadline
        {
          let size =
            (try FileManager.default.attributesOfItem(atPath: logURL.path)[.size] as? NSNumber)?
            .uint64Value ?? 0
          if size >= 1024 * 1024 {
            outputLimitReached = true
            break
          }
          try await Task.sleep(for: .milliseconds(50))
        }
        let timedOut = log.isRunning && ProcessInfo.processInfo.systemUptime >= deadline
        if log.isRunning { log.terminate() }
        let settle = ProcessInfo.processInfo.systemUptime + 0.25
        while log.isRunning && ProcessInfo.processInfo.systemUptime < settle {
          try await Task.sleep(for: .milliseconds(50))
        }
        guard !log.isRunning else { throw Failure.deadline }
        log.waitUntilExit()
        diagnosticProcess = nil
        try output.close()
        pickerLogObservation = [
          "path": logURL.path, "pid": launched.processIdentifier,
          "start": date.timeIntervalSince1970, "end": end,
          "status": log.terminationStatus, "reason": log.terminationReason.rawValue,
          "timed_out": timedOut, "output_limit_reached": outputLimitReached,
          "bytes":
            (try FileManager.default.attributesOfItem(atPath: logURL.path)[.size] as? NSNumber)?
            .uint64Value ?? 0,
          "scope":
            "owned product PID and unique executable path only; sampled output-size stop is not a hard byte cap; diagnostic, not product qualification",
        ]
      }

    } catch { failure = failure ?? String(describing: error) }

    // Retain the exact diagnostic child across throwing reads or cancellation.
    // A failure to settle it remains a failed owner result.
    if let diagnostic = diagnosticProcess {
      if diagnostic.isRunning { diagnostic.terminate() }
      let deadline = ProcessInfo.processInfo.systemUptime + 0.25
      while diagnostic.isRunning && ProcessInfo.processInfo.systemUptime < deadline {
        try? await Task.sleep(for: .milliseconds(25))
      }
      diagnosticSettled = !diagnostic.isRunning
      if diagnosticSettled {
        diagnostic.waitUntilExit()
      } else {
        failure = failure ?? "Owned read-only diagnostic did not settle"
      }
    }

    // Internal failure/deadline and the controller's opted-in TERM grace.
    // A hard kill or unresolved launch callback can still bypass cleanup;
    // absent evidence must remain unverified.
    if child.isRunning { child.terminate() }
    if let app, !app.isTerminated { _ = app.terminate() }
    if let settings, !settings.isTerminated { _ = settings.terminate() }
    let grace = ProcessInfo.processInfo.systemUptime + 2
    while (child.isRunning || app.map { !$0.isTerminated } == true
      || settings.map { !$0.isTerminated } == true)
      && ProcessInfo.processInfo.systemUptime < grace
    {
      try? await Task.sleep(for: .milliseconds(50))
    }
    if child.isRunning {
      // Foundation Process does not expose a retained kernel PID pin.
      // Leave any hard escalation to the outer Rust session/group owner.
      failure =
        failure
        ?? "Interaction runner did not terminate cooperatively; outer owned-group cleanup required"
    }
    if let app, !app.isTerminated {
      forcedApp = true
      _ = app.forceTerminate()
    }
    let cleanupDeadline = ProcessInfo.processInfo.systemUptime + 5
    while (child.isRunning || app.map { !$0.isTerminated } == true
      || settings.map { !$0.isTerminated } == true)
      && ProcessInfo.processInfo.systemUptime < cleanupDeadline
    {
      try? await Task.sleep(for: .milliseconds(50))
    }
    if child.processIdentifier > 0 && !child.isRunning {
      child.waitUntilExit()
      childReaped = true
      childExit = child.terminationStatus
    }
    let appTerminated = app?.isTerminated == true
    let settingsTerminated = settings?.isTerminated ?? true
    if let settings {
      unexpectedSettings = NSRunningApplication.runningApplications(
        withBundleIdentifier: "com.apple.systempreferences"
      ).filter { !$0.isTerminated && $0.processIdentifier != settings.processIdentifier }
        .map(\.processIdentifier)
      if !unexpectedSettings.isEmpty {
        failure = failure ?? "Unexpected Settings instance; no authority to terminate it"
      }
    }
    if !settingsTerminated {
      failure =
        failure ?? "Owned Settings did not terminate normally; disposable-host cleanup required"
    }
    if !childReaped || !appTerminated {
      failure = failure ?? "Owned app or interaction runner settlement remains unverified"
    }
    if forcedApp {
      failure = failure ?? "App required forced cleanup; normal app/worker settlement is unverified"
    }
    do {
      if let held = heldCopy {
        do {
          guard try Data(contentsOf: held.held) == held.expected,
            !FileManager.default.fileExists(atPath: held.source.path)
          else { throw Failure.invalidInput }
          try moveWithoutReplacement(from: held.held, to: held.source)
          guard try Data(contentsOf: held.source) == held.expected else {
            throw Failure.invalidInput
          }
          heldCopy = nil
          copyRestoredDuringCleanup = true
        } catch {
          copyRestorationFailure = String(describing: error)
          failure = failure ?? "Owned copy restoration failed: \(error)"
        }
      }
      originalsUnchanged =
        !originals.isEmpty
        && originals.allSatisfy { url, bytes in (try? Data(contentsOf: url)) == bytes }
      if !originalsUnchanged {
        failure = failure ?? "Owned original fixture bytes changed or could not be read"
      }
      let hadBoard = board != nil
      board?.releaseGlobally()
      board = nil
      guard let resultURL else { throw Failure.invalidInput }
      let result: [String: Any] = [
        "failure": failure.map { $0 as Any } ?? NSNull(), "timed_out": timedOut,
        "cancelled": cancellation.requested, "runner_reaped": childReaped,
        "runner_status": childExit.map { $0 as Any } ?? NSNull(),
        "owned_app_terminated": appTerminated, "forced_app_cleanup": forcedApp,
        "owned_settings_terminated": settingsTerminated,
        "unexpected_settings_application_pids": unexpectedSettings,
        "system_appearance_restoration": settings == nil
          ? "not_applicable"
          : (failure == nil
            ? "requires successful XCTest restoration receipt"
            : "unverified after failure; disposable hosted session contains remaining state"),
        "worker_settlement": forcedApp
          ? "unverified after forced app exit"
          : "requires successful XCTest terminal report and normal Quit",
        "unexpected_application_pids": unexpected, "originals_captured": originals.count,
        "originals_unchanged": originalsUnchanged,
        "source_check_failure": originalsUnchanged
          ? NSNull() : "Owned original fixture bytes changed or could not be read",
        "owned_pasteboard_released": hadBoard,
        "native_picker_stack_samples": stackSamples,
        "native_picker_owned_process_log": pickerLogObservation ?? NSNull(),
        "native_picker_diagnostic_settled": diagnosticSettled,
        "missing_copy_observation": [
          "held": copyWasHeld, "restored_by_request": copyRestoredByRequest,
          "restored_during_cleanup": copyRestoredDuringCleanup,
          "copy_left_held": heldCopy != nil,
          "restoration_failure": copyRestorationFailure.map { $0 as Any } ?? NSNull(),
          "scope": "only the owned synthetic output; hard-kill restoration remains unverified",
        ],
        "fixture_output_relay": [
          "enabled": outputPipe != nil,
          "eof_observed": fixtureOutput.hasEnded(),
          "drain_within_deadline": outputDrainComplete.map { $0 as Any } ?? NSNull(),
          "failure": fixtureOutput.observedRelayFailure().map { $0 as Any } ?? NSNull(),
        ],
      ]
      var observation = result
      observation["ownership_receipt"] = ownershipReceipt.map { $0 as Any } ?? NSNull()
      try JSONSerialization.data(
        withJSONObject: observation, options: [.prettyPrinted, .sortedKeys]
      ).write(to: resultURL, options: .withoutOverwriting)
    } catch { failure = failure ?? String(describing: error) }
    for source in signals { source.cancel() }
    outputPipe?.fileHandleForReading.readabilityHandler = nil
    if let failure {
      try? FileHandle.standardError.write(
        contentsOf: Data("Desktop fixture owner: \(failure)\n".utf8))
    }
    Darwin.exit(failure == nil ? 0 : 1)
  }

  static func moveWithoutReplacement(from source: URL, to destination: URL) throws {
    let code = source.withUnsafeFileSystemRepresentation { old -> Int32 in
      destination.withUnsafeFileSystemRepresentation { new -> Int32 in
        guard let old, let new else { return EINVAL }
        return renamex_np(old, new, UInt32(RENAME_EXCL)) == 0 ? 0 : errno
      }
    }
    guard code == 0 else { throw POSIXError(POSIXErrorCode(rawValue: code) ?? .EIO) }
  }

  static func acknowledge(_ text: String, at destination: URL) throws {
    let pending = destination.deletingLastPathComponent().appendingPathComponent(
      "ack-\(UUID().uuidString)")
    try Data(text.utf8).write(to: pending, options: .withoutOverwriting)
    try FileManager.default.linkItem(at: pending, to: destination)
    try FileManager.default.removeItem(at: pending)
  }
}
