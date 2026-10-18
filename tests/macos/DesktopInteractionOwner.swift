// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import AppKit
import Darwin
import Foundation

// Test-only lifetime authority. XCTest receives observations for attachment;
// this process retains the NSWorkspace-returned object across runner failure.
@main
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
      child.standardOutput = FileHandle.standardOutput
      child.standardError = FileHandle.standardError
      try child.run()
      var ready = false
      var shortDeadline: TimeInterval?
      while child.isRunning {
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
      ]
      var observation = result
      observation["ownership_receipt"] = ownershipReceipt.map { $0 as Any } ?? NSNull()
      try JSONSerialization.data(
        withJSONObject: observation, options: [.prettyPrinted, .sortedKeys]
      ).write(to: resultURL, options: .withoutOverwriting)
    } catch { failure = failure ?? String(describing: error) }
    for source in signals { source.cancel() }
    if let failure {
      try? FileHandle.standardError.write(
        contentsOf: Data("Desktop fixture owner: \(failure)\n".utf8))
    }
    Darwin.exit(failure == nil ? 0 : 1)
  }
}
