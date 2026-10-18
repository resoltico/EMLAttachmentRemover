// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import AppKit
import Darwin
import Foundation

// The pipe callback and lifetime owner share only this bounded observation.
// NSLock protects every mutable field; no native app capability crosses threads.
final class PickerDiagnosticOutput: @unchecked Sendable {
  private let lock = NSLock()
  private var tail = Data()
  private var pending = false
  private var ended = false
  private var relayFailure: String?
  func failRelay(_ reason: String) {
    lock.lock()
    relayFailure = relayFailure ?? reason
    lock.unlock()
  }
  func observedRelayFailure() -> String? {
    lock.lock()
    defer { lock.unlock() }
    return relayFailure
  }
  func finish() {
    lock.lock()
    ended = true
    lock.unlock()
  }
  func hasEnded() -> Bool {
    lock.lock()
    defer { lock.unlock() }
    return ended
  }
  func observe(_ bytes: Data) {
    let marker = Data("EML_NATIVE_PICKER_REQUESTED".utf8)
    lock.lock()
    defer { lock.unlock() }
    let combined = tail + bytes
    if combined.range(of: marker) != nil { pending = true }
    tail = combined.suffix(marker.count - 1)
  }
  func take() -> Bool {
    lock.lock()
    defer { lock.unlock() }
    let value = pending
    pending = false
    return value
  }
}

// Test-only lifetime authority. XCTest receives observations for attachment;
// this process retains the NSWorkspace-returned object across runner failure.
#if !PICKER_MARKER_TESTS
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
    var ownershipReceipt: [String: Any]?
    var board: NSPasteboard?
    var originals: [(URL, Data)] = []
    var originalsUnchanged = false
    var childReaped = false
    var childExit: Int32?
    var forcedApp = false
    var timedOut = false
    var unexpected: [pid_t] = []
    var stackSamples: [[String: Any]] = []
    var outputPipe: Pipe?
    let pickerMarker = PickerDiagnosticOutput()
    var outputDrainComplete: Bool?
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
      let configuration = NSWorkspace.OpenConfiguration()
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
      let receipt: [String: Any] = [
        "pid": launched.processIdentifier, "launch_date": date.timeIntervalSince1970,
        "bundle_path": appPath, "pasteboard": pasteboard.name.rawValue,
      ]
      ownershipReceipt = receipt
      try JSONSerialization.data(withJSONObject: receipt).write(
        to: receiptURL, options: .withoutOverwriting)
      guard launched.activate(options: []) else { throw Failure.launchIdentity }
      child.executableURL = URL(fileURLWithPath: arguments[4])
      child.arguments = Array(arguments.dropFirst(5))
      child.standardInput = FileHandle.nullDevice
      if input["picker_stack"] == "true" {
        let pipe = Pipe()
        outputPipe = pipe
        child.standardOutput = pipe
        pipe.fileHandleForReading.readabilityHandler = { handle in
          do {
            let bytes = try handle.read(upToCount: 64 * 1024) ?? Data()
            if bytes.isEmpty {
              handle.readabilityHandler = nil
              pickerMarker.finish()
            } else {
              pickerMarker.observe(bytes)
              do { try FileHandle.standardOutput.write(contentsOf: bytes) } catch {
                pickerMarker.failRelay("Output write failed: \(error)")
              }
            }
          } catch {
            pickerMarker.failRelay("Output read failed: \(error)")
            handle.readabilityHandler = nil
            pickerMarker.finish()
          }

        }
      } else {
        child.standardOutput = FileHandle.standardOutput
      }
      child.standardError = FileHandle.standardError
      try child.run()
      var ready = false
      var shortDeadline: TimeInterval?
      while child.isRunning {
        if pickerMarker.take(), stackSamples.count < 3, !launched.isTerminated {
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
          stackSamples.append([
            "path": sampleURL.path, "status": sample.terminationStatus,
            "reason": sample.terminationReason.rawValue,
            "retained_target_alive_after": !launched.isTerminated,
            "scope": "read-only diagnostic; never a picker or product qualification verdict",
          ])
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
        while !pickerMarker.hasEnded() && ProcessInfo.processInfo.systemUptime < drainDeadline {
          try await Task.sleep(for: .milliseconds(50))
        }
        outputDrainComplete = pickerMarker.hasEnded()
        if !pickerMarker.hasEnded() {
          failure = failure ?? "Interaction output drain did not settle"
        }
        if let relayFailure = pickerMarker.observedRelayFailure() {
          failure = failure ?? relayFailure
        }
      }

    } catch { failure = String(describing: error) }

    // Internal failure/deadline and the controller's opted-in TERM grace.
    // A hard kill or unresolved launch callback can still bypass cleanup;
    // absent evidence must remain unverified.
    if child.isRunning { child.terminate() }
    if let app, !app.isTerminated { _ = app.terminate() }
    let grace = ProcessInfo.processInfo.systemUptime + 2
    while (child.isRunning || app.map { !$0.isTerminated } == true)
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
    while (child.isRunning || app.map { !$0.isTerminated } == true)
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
    if !childReaped || !appTerminated {
      failure = failure ?? "Owned app or interaction runner settlement remains unverified"
    }
    if forcedApp {
      failure = failure ?? "App required forced cleanup; normal app/worker settlement is unverified"
    }
    do {
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
        "worker_settlement": forcedApp
          ? "unverified after forced app exit"
          : "requires successful XCTest terminal report and normal Quit",
        "unexpected_application_pids": unexpected, "originals_captured": originals.count,
        "originals_unchanged": originalsUnchanged,
        "source_check_failure": originalsUnchanged
          ? NSNull() : "Owned original fixture bytes changed or could not be read",
        "owned_pasteboard_released": hadBoard,
        "native_picker_stack_samples": stackSamples,
        "native_picker_output_relay": [
          "enabled": outputPipe != nil,
          "eof_observed": pickerMarker.hasEnded(),
          "drain_within_deadline": outputDrainComplete.map { $0 as Any } ?? NSNull(),
          "failure": pickerMarker.observedRelayFailure().map { $0 as Any } ?? NSNull(),
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
}
