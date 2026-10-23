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
final class DesktopInteractionOwner {
  private final class Cancellation { var requested = false }
  private enum Failure: Error {
    case invalidInput, existingApplication, launchIdentity, unexpectedApplication,
      forcedApplication, deadline, cancelled
  }

  private var failure: String?
  private var app: NSRunningApplication?
  private var settings: NSRunningApplication?
  private var ownershipReceipt: [String: Any]?
  private var board: NSPasteboard?
  private var originals: [(URL, Data)] = []
  private var originalsUnchanged = false
  private var childReaped = false
  private var childExit: Int32?
  private var forcedApp = false
  private var timedOut = false
  private var unexpected: [pid_t] = []
  private var unexpectedSettings: [pid_t] = []
  private var stackSamples: [[String: Any]] = []
  private var firstFailureSample: [String: Any]?
  private var sampleTarget: SampleTarget?
  private var normalRunner = false

  struct SampleTarget: Equatable {
    let pid: pid_t
    let launchDate: Date
    let bundlePath: String

    var observation: [String: Any] {
      ["pid": pid, "launch_date": launchDate.timeIntervalSince1970, "bundle_path": bundlePath]
    }
  }

  static func mayRetainFailureSample(
    expected: SampleTarget, before: SampleTarget?, after: SampleTarget?, settled: Bool,
    bytes: UInt64
  ) -> Bool {
    settled && before == expected && after == expected && bytes > 0 && bytes <= 5 * 1024 * 1024
  }

  private var pickerBaselineSettled = false
  private var pickerLogObservation: [String: Any]?
  private var diagnosticProcess: Process?
  private var diagnosticSettled = true
  private var outputPipe: Pipe?
  private let fixtureOutput = DesktopFixtureOutput()
  private var outputDrainComplete: Bool?
  private var heldCopy: (source: URL, held: URL, expected: Data)?
  private var copyWasHeld = false
  private var copyRestoredByRequest = false
  private var copyRestoredDuringCleanup = false
  private var copyRestorationFailure: String?
  private let child = Process()
  private let executionDeadline = ProcessInfo.processInfo.systemUptime + 1800
  private let cancellation = Cancellation()
  private let arguments = CommandLine.arguments
  private var panelClipboard: PanelPathClipboardLease?
  private var nextPanelPathSequence = 1
  private var activePanelPathSequence: Int?
  private var resultURL: URL?
  private var fixtureFilesystem: [String: Any]?

  private struct Configuration {
    let input: [String: String]
    let appPath: String
    let appURL: URL
    let fixture: URL
    let identifier: String
    let control: String
    let receiptURL: URL
  }

  static func main() async {
    let owner = DesktopInteractionOwner()
    await owner.run()
    Darwin.exit(owner.failure == nil ? 0 : 1)
  }

  private func run() async {
    let cancellation = cancellation
    let signals = [SIGINT, SIGTERM].map { number in
      Darwin.signal(number, SIG_IGN)
      let source = DispatchSource.makeSignalSource(signal: number, queue: .main)
      source.setEventHandler { MainActor.assumeIsolated { cancellation.requested = true } }
      source.resume()
      return source
    }
    do {
      let config = try configuration()
      normalRunner = config.control == "normal"
      try captureFixtureFilesystem(config)
      try captureOriginals(config)
      let (launched, date) = try await launchApplications(config)
      try writeOwnership(config, launched: launched, date: date)
      try startRunner(config)
      try await monitorRunner(config, launched: launched, date: date)
      try await drainRunnerOutput()
      if pickerBaselineSettled {
        try await collectPickerLogs(config, launched: launched, date: date)
      }
    } catch { failure = failure ?? String(describing: error) }
    await settleDiagnostic()
    if normalRunner, childReaped, let childExit, childExit != 0, failure != nil {
      await sampleFirstFailure()
    }
    // sampleFirstFailure includes its entire eight-second settlement budget;
    // an unsettled sampler remains recorded for the outer process owner.
    await settleProcesses()
    if panelClipboard?.hasActiveLease == true {
      do { try await drainRunnerOutput() } catch { failure = failure ?? String(describing: error) }
    }
    do {
      try restoreAndWriteObservation()
    } catch { failure = failure ?? String(describing: error) }
    for source in signals { source.cancel() }
    outputPipe?.fileHandleForReading.readabilityHandler = nil
    if let failure {
      try? FileHandle.standardError.write(
        contentsOf: Data("Desktop fixture owner: \(failure)\n".utf8))
    }
  }

  private func configuration() throws -> Configuration {
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
    return Configuration(
      input: input, appPath: appPath, appURL: appURL, fixture: fixture, identifier: identifier,
      control: control, receiptURL: receiptURL)
  }

  // A prelaunch observation, not a continuous pin or namespace-creation authority.
  static func observeFixtureFilesystem(_ fixture: URL) -> [String: Any] {
    var observation: [String: Any] = [
      "phase": "prelaunch", "state": "unavailable", "fixture_path": fixture.path,
      "filesystem_type": NSNull(), "root": NSNull(), "one": NSNull(), "upper": NSNull(),
      "upper_errno": NSNull(), "ascii_alias_relation": NSNull(),
      "descriptor_closed": NSNull(), "errors": [],
    ]
    var errors: [[String: Any]] = []
    func failed(_ stage: String, _ code: Int32?, _ detail: String) {
      errors.append(["stage": stage, "errno": code.map { $0 as Any } ?? NSNull(), "detail": detail])
    }
    func entry(_ facts: stat) -> [String: Any] {
      ["device": Int64(facts.st_dev), "inode": UInt64(facts.st_ino), "mode": UInt32(facts.st_mode)]
    }
    let (descriptor, openError) = fixture.withUnsafeFileSystemRepresentation {
      path -> (Int32, Int32?) in
      guard let path else { return (-1, nil) }
      let descriptor = Darwin.open(path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
      let code = errno
      return (descriptor, descriptor < 0 ? code : nil)
    }
    func inspect() {
      guard descriptor >= 0 else {
        failed("open", openError, "Cannot open the owned fixture directory without following links")
        return
      }
      defer {
        let status = Darwin.close(descriptor)
        let code = errno
        observation["descriptor_closed"] = status == 0
        if status != 0 { failed("close", code, "Fixture observation descriptor did not close") }
      }
      var root = stat()
      guard Darwin.fstat(descriptor, &root) == 0 else {
        failed("fstat", errno, "Cannot observe the held fixture directory")
        return
      }
      observation["root"] = entry(root)
      guard root.st_mode & S_IFMT == S_IFDIR, root.st_ino > 0 else {
        failed("root-type", nil, "Fixture root is not an ordinary directory")
        return
      }
      var filesystem = statfs()
      guard Darwin.fstatfs(descriptor, &filesystem) == 0 else {
        failed("fstatfs", errno, "Cannot observe the held filesystem type")
        return
      }
      let nativeName = withUnsafeBytes(of: filesystem.f_fstypename) { Array($0) }
      guard let nul = nativeName.firstIndex(of: 0), nul > 0,
        let name = String(bytes: nativeName[..<nul], encoding: .utf8)
      else {
        failed("filesystem-name", nil, "Filesystem type is not bounded NUL-terminated UTF8")
        return
      }
      observation["filesystem_type"] = name
      var lower = stat()
      guard Darwin.fstatat(descriptor, "one.eml", &lower, AT_SYMLINK_NOFOLLOW) == 0 else {
        failed("one", errno, "Cannot observe the existing lowercase fixture entry")
        return
      }
      observation["one"] = entry(lower)
      guard lower.st_mode & S_IFMT == S_IFREG, lower.st_ino > 0,
        lower.st_dev == root.st_dev
      else {
        failed("one-type-device", nil, "Lowercase fixture must be regular on the held filesystem")
        return
      }
      var upper = stat()
      let upperStatus = Darwin.fstatat(descriptor, "ONE.EML", &upper, AT_SYMLINK_NOFOLLOW)
      let upperError = errno
      if upperStatus == 0 {
        observation["upper"] = entry(upper)
        guard upper.st_mode & S_IFMT == S_IFREG,
          upper.st_dev == lower.st_dev, upper.st_ino == lower.st_ino
        else {
          failed(
            "upper-identity", nil, "Uppercase lookup does not identify the same regular fixture")
          return
        }
        observation["ascii_alias_relation"] = "same_identity"
      } else {
        observation["upper_errno"] = upperError
        guard upperError == ENOENT else {
          failed("upper", upperError, "Uppercase fixture lookup is unavailable")
          return
        }
        observation["ascii_alias_relation"] = "absent_enoent"
      }
    }
    inspect()
    observation["errors"] = errors
    observation["state"] = errors.isEmpty ? "observed" : "unavailable"
    return observation
  }

  static func writeFixtureFilesystem(_ observation: [String: Any], to destination: URL) throws {
    try JSONSerialization.data(withJSONObject: observation, options: [.prettyPrinted, .sortedKeys])
      .write(to: destination, options: .withoutOverwriting)
  }

  private func captureFixtureFilesystem(_ config: Configuration) throws {
    let observation = Self.observeFixtureFilesystem(config.fixture)
    fixtureFilesystem = observation
    guard let resultURL else { throw Failure.invalidInput }
    let preflight = resultURL.deletingLastPathComponent().appendingPathComponent(
      "fixture-filesystem.json")
    // Keep partial native facts even if prelaunch admission fails or the owner is lost later.
    try Self.writeFixtureFilesystem(observation, to: preflight)
    guard observation["state"] as? String == "observed" else { throw Failure.invalidInput }
  }

  private func captureOriginals(_ config: Configuration) throws {
    let (input, fixture) = (config.input, config.fixture)
    for name in [
      "one.eml", "two.eml", "folder-input/folder.eml",
      "service-input/日本語-مرحبا-\u{202e}report.eml",
    ] {
      let url = fixture.appendingPathComponent(name)
      originals.append((url, try Data(contentsOf: url)))
    }
    if input["writer_session"] == "true" {
      for index in 0..<1024 {
        let url = fixture.appendingPathComponent(String(format: "writer-input/%04d.eml", index))
        originals.append((url, try Data(contentsOf: url)))
      }
    }
    if input["pagination_session"] == "true" {
      for index in 0..<65 {
        let url = fixture.appendingPathComponent(
          String(format: "pagination-input/%03d.eml", index))
        originals.append((url, try Data(contentsOf: url)))
      }
    }
  }

  private func launchApplications(_ config: Configuration) async throws -> (
    NSRunningApplication, Date
  ) {
    let (input, appURL) = (config.input, config.appURL)
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
        input["child_control"] == "true"
      else {
        throw Failure.invalidInput
      }
      configuration.environment = ProcessInfo.processInfo.environment.merging(
        ["EML_OPERATION_OBSERVATION": observation]) { _, replacement in replacement }
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
    return (launched, date)
  }

  private func writeOwnership(_ config: Configuration, launched: NSRunningApplication, date: Date)
    throws
  {
    let (appPath, receiptURL) = (config.appPath, config.receiptURL)
    let pasteboard = NSPasteboard.withUniqueName()
    board = pasteboard
    var receipt: [String: Any] = [
      "pid": launched.processIdentifier, "launch_date": date.timeIntervalSince1970,
      "bundle_path": appPath, "pasteboard": pasteboard.name.rawValue,
    ]
    if let settings, let settingsDate = settings.launchDate, let settingsURL = settings.bundleURL {
      receipt["settings"] = [
        "pid": settings.processIdentifier, "launch_date": settingsDate.timeIntervalSince1970,
        "bundle_path": settingsURL.path,
      ]
    }
    ownershipReceipt = receipt
    sampleTarget = SampleTarget(
      pid: launched.processIdentifier, launchDate: date,
      bundlePath: config.appURL.standardizedFileURL.path)
    try JSONSerialization.data(withJSONObject: receipt).write(
      to: receiptURL, options: .withoutOverwriting)
    guard launched.activate(options: []) else { throw Failure.launchIdentity }
  }

  private func startRunner(_ config: Configuration) throws {
    let input = config.input
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
  }

  private func monitorRunner(_ config: Configuration, launched: NSRunningApplication, date: Date)
    async throws
  {
    let (input, receiptURL, identifier, control) = (
      config.input, config.receiptURL, config.identifier, config.control
    )
    var ready = false
    var shortDeadline: TimeInterval?
    while child.isRunning {
      if let relayFailure = fixtureOutput.observedRelayFailure() {
        failure = failure ?? relayFailure
        throw Failure.invalidInput
      }
      let request = fixtureOutput.take()
      try handleCopyRequest(request, fixture: config.fixture)
      try handlePanelPathRequest(request, fixture: config.fixture)
      try await samplePicker(request, config: config, launched: launched, date: date)
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
  }

  static func readPanelPath(_ url: URL) throws -> String {
    let fd = open(url.path, O_RDONLY | O_NOFOLLOW | O_CLOEXEC)
    guard fd >= 0 else { throw Failure.invalidInput }
    defer { close(fd) }
    var before = stat()
    guard fstat(fd, &before) == 0, before.st_mode & S_IFMT == S_IFREG,
      before.st_uid == geteuid(), before.st_nlink == 1, before.st_size > 0,
      before.st_size <= 4096
    else { throw Failure.invalidInput }
    var bytes = Data()
    var buffer = [UInt8](repeating: 0, count: 4097)
    while true {
      let count = Darwin.read(fd, &buffer, min(buffer.count, 4097 - bytes.count))
      if count < 0 && errno == EINTR { continue }
      guard count >= 0 else { throw Failure.invalidInput }
      if count == 0 { break }
      bytes.append(contentsOf: buffer.prefix(count))
      guard bytes.count <= 4096 else { throw Failure.invalidInput }
    }
    var after = stat()
    guard fstat(fd, &after) == 0, before.st_dev == after.st_dev,
      before.st_ino == after.st_ino, before.st_size == after.st_size,
      before.st_mtimespec.tv_sec == after.st_mtimespec.tv_sec,
      before.st_mtimespec.tv_nsec == after.st_mtimespec.tv_nsec,
      before.st_ctimespec.tv_sec == after.st_ctimespec.tv_sec,
      before.st_ctimespec.tv_nsec == after.st_ctimespec.tv_nsec,
      bytes.count == before.st_size, let path = String(data: bytes, encoding: .utf8),
      (path as NSString).isAbsolutePath, !bytes.contains(0)
    else { throw Failure.invalidInput }
    return path
  }

  private func handlePanelPathRequest(_ request: DesktopFixtureRequest?, fixture: URL) throws {
    guard request == .preparePanelPath || request == .restorePanelClipboard else { return }
    do {
      if request == .preparePanelPath {
        guard activePanelPathSequence == nil, nextPanelPathSequence <= 32 else {
          throw Failure.invalidInput
        }
        let sequence = nextPanelPathSequence
        let path = try Self.readPanelPath(
          fixture.appendingPathComponent("panel-path-\(sequence).request"))
        // configuration() already admitted this disposable hosted runner before
        // any general-pasteboard access. The product and XCTest never write it.
        if panelClipboard == nil { panelClipboard = PanelPathClipboardLease(pasteboard: .general) }
        activePanelPathSequence = sequence
        try panelClipboard!.prepare(path: path, sequence: sequence)
        try Data("owned panel path ready \(sequence)\n".utf8).write(
          to: fixture.appendingPathComponent("panel-path-\(sequence).ready"),
          options: .withoutOverwriting)
        nextPanelPathSequence += 1
      } else {
        guard let sequence = activePanelPathSequence, let clipboard = panelClipboard,
          clipboard.hasActiveLease
        else { throw Failure.invalidInput }
        try clipboard.restore(phase: .requested)
        try Data("owned panel clipboard restored \(sequence)\n".utf8).write(
          to: fixture.appendingPathComponent("panel-path-\(sequence).restored"),
          options: .withoutOverwriting)
        activePanelPathSequence = nil
      }
    } catch {
      panelClipboard?.recordFailure(error)
      throw error
    }
  }

  private func handleCopyRequest(_ request: DesktopFixtureRequest?, fixture: URL) throws {
    if request == .holdCopy {
      guard !copyWasHeld, originals.count == 4 else { throw Failure.invalidInput }
      let source = expectedCopy(for: originals[3].0)
      let held = fixture.appendingPathComponent("held-service-copy.eml")
      let repository = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent()
      let golden = try Data(
        contentsOf: repository.appendingPathComponent(
          "tests/contract/mime/golden/01-gmail-attachment.eml"))
      guard try Data(contentsOf: source) == golden,
        !FileManager.default.fileExists(atPath: held.path)
      else { throw Failure.invalidInput }
      try Self.moveWithoutReplacement(from: source, to: held)
      heldCopy = (source, held, golden)
      copyWasHeld = true
      guard try Data(contentsOf: held) == golden,
        !FileManager.default.fileExists(atPath: source.path)
      else { throw Failure.invalidInput }
      try Self.acknowledge("owned copy held\n", at: fixture.appendingPathComponent("copy-held"))
    } else if request == .restoreCopy {
      guard let held = heldCopy, try Data(contentsOf: held.held) == held.expected,
        !FileManager.default.fileExists(atPath: held.source.path)
      else { throw Failure.invalidInput }
      try Self.moveWithoutReplacement(from: held.held, to: held.source)
      heldCopy = nil
      guard try Data(contentsOf: held.source) == held.expected else {
        throw Failure.invalidInput
      }
      copyRestoredByRequest = true
      try Self.acknowledge(
        "owned copy restored\n", at: fixture.appendingPathComponent("copy-restored"))
    }
  }

  private func samplePicker(
    _ request: DesktopFixtureRequest?, config: Configuration, launched: NSRunningApplication,
    date: Date
  ) async throws {
    let (appURL, fixture) = (config.appURL, config.fixture)
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
        try Self.acknowledge(
          "owned picker baseline settled\n",
          at: fixture.appendingPathComponent("picker-baseline-settled"))
      }
    }
  }

  private func drainRunnerOutput() async throws {
    if outputPipe != nil {
      let drainDeadline = ProcessInfo.processInfo.systemUptime + 2
      while !fixtureOutput.hasEnded() && ProcessInfo.processInfo.systemUptime < drainDeadline {
        try await Task.sleep(for: .milliseconds(50))
      }
      outputDrainComplete = fixtureOutput.observedEOF()
      if !fixtureOutput.hasEnded() {
        failure = failure ?? "Interaction output drain did not settle"
      }
      if let relayFailure = fixtureOutput.observedRelayFailure() {
        failure = failure ?? relayFailure
      }
    }
  }

  private func collectPickerLogs(
    _ config: Configuration, launched: NSRunningApplication, date: Date
  ) async throws {
    let appURL = config.appURL
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
    // log combines separate --process/--predicate filters as alternatives.
    // Keep ownership and context in one conjunction; an owned real-log
    // negative control confirmed that unrelated message text is excluded.
    let predicate = NSPredicate(
      format:
        "processIdentifier == %d AND processImagePath == %@ AND (logType == %@ OR logType == %@ OR composedMessage CONTAINS[c] %@ OR composedMessage CONTAINS[c] %@ OR composedMessage CONTAINS[c] %@ OR composedMessage CONTAINS[c] %@ OR composedMessage CONTAINS[c] %@ OR subsystem CONTAINS[c] %@ OR composedMessage BEGINSWITH %@ OR composedMessage BEGINSWITH %@)",
      launched.processIdentifier, image.path, "error", "fault", "exception", "ViewBridge",
      "NSOpenPanel", "NSSavePanel",
      "openAndSavePanel", "ViewBridge", "EML lifecycle: ", "EML panic source: "
    ).predicateFormat
    log.arguments = [
      "show", "--start",
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
      "filter": "fault/error, panel/ViewBridge or exact lifecycle/panic prefixes; partial logs",
      "predicate": predicate,
      "bytes":
        (try FileManager.default.attributesOfItem(atPath: logURL.path)[.size] as? NSNumber)?
        .uint64Value ?? 0,
      "scope":
        "owned product PID and unique executable path only; sampled output-size stop is not a hard byte cap; diagnostic, not product qualification",
    ]
  }

  private func currentSampleTarget() -> SampleTarget? {
    guard let app, !app.isTerminated, let date = app.launchDate, let bundle = app.bundleURL,
      let current = NSRunningApplication(processIdentifier: app.processIdentifier),
      !current.isTerminated, current.launchDate == date,
      current.bundleURL?.standardizedFileURL == bundle.standardizedFileURL
    else { return nil }
    return SampleTarget(
      pid: app.processIdentifier, launchDate: date, bundlePath: bundle.standardizedFileURL.path)
  }

  private func sampleFirstFailure() async {
    // This is a single diagnostic on the failed execution, before any app cleanup.
    // It never retries XCTest or changes the original failure/qualification verdict.
    guard firstFailureSample == nil else { return }
    var record: [String: Any] = [
      "scope": "first failed XCTest runner; diagnostic only, not a product verdict",
      "request_seconds": 2, "maximum_seconds": 8, "retained_byte_limit": 5 * 1024 * 1024,
      "raw_retained": false,
    ]
    defer { firstFailureSample = record }
    guard let expected = sampleTarget, let resultURL else {
      record["failure"] = "Owned launch identity unavailable"
      return
    }
    record["expected_target"] = expected.observation
    let before = currentSampleTarget()
    record["before_target"] = before?.observation ?? NSNull()
    guard before == expected, diagnosticSettled, diagnosticProcess?.isRunning != true else {
      record["failure"] = "Owned target changed, exited, or previous diagnostic remains active"
      return
    }
    // Private temporary output is never uploaded. Publish only a bounded, fully
    // settled sample whose exact native launch identity survived the observation.
    let temporary = FileManager.default.temporaryDirectory.appendingPathComponent(
      "eml-owned-failure-sample-\(UUID().uuidString)")
    defer { try? FileManager.default.removeItem(at: temporary) }
    let raw = temporary.appendingPathComponent("sample.txt")
    let sample = Process()
    do {
      try FileManager.default.createDirectory(
        at: temporary, withIntermediateDirectories: false,
        attributes: [.posixPermissions: 0o700])
      sample.executableURL = URL(fileURLWithPath: "/usr/bin/sample")
      sample.arguments = [String(expected.pid), "2", "-file", raw.path]
      sample.standardInput = FileHandle.nullDevice
      sample.standardOutput = FileHandle.nullDevice
      sample.standardError = FileHandle.nullDevice
      let started = ProcessInfo.processInfo.systemUptime
      try sample.run()
      diagnosticProcess = sample
      diagnosticSettled = false
      record["sampler_pid"] = sample.processIdentifier
      let deadline = started + 8
      var oversized = false
      while sample.isRunning && ProcessInfo.processInfo.systemUptime < deadline - 0.5 {
        let size = (try? FileManager.default.attributesOfItem(atPath: raw.path)[.size]) as? NSNumber
        if (size?.uint64Value ?? 0) > 5 * 1024 * 1024 {
          oversized = true
          break
        }
        try? await Task.sleep(for: .milliseconds(25))
      }
      let deadlineReached = sample.isRunning && !oversized
      let terminatedSampler = sample.isRunning
      if terminatedSampler { sample.terminate() }
      while sample.isRunning && ProcessInfo.processInfo.systemUptime < deadline {
        try? await Task.sleep(for: .milliseconds(25))
      }
      diagnosticSettled = !sample.isRunning
      record["elapsed_seconds"] = ProcessInfo.processInfo.systemUptime - started
      record["deadline_reached"] = deadlineReached
      record["output_limit_reached"] = oversized
      record["sampler_termination_requested"] = terminatedSampler
      record["sampler_settled"] = diagnosticSettled
      if diagnosticSettled {
        sample.waitUntilExit()
        record["status"] = sample.terminationStatus
        record["reason"] = sample.terminationReason.rawValue
        diagnosticProcess = nil
      }
      let after = currentSampleTarget()
      record["after_target"] = after?.observation ?? NSNull()
      let bytes =
        ((try? FileManager.default.attributesOfItem(atPath: raw.path)[.size]) as? NSNumber)?
        .uint64Value ?? 0
      record["bytes"] = bytes
      if Self.mayRetainFailureSample(
        expected: expected, before: before, after: after, settled: diagnosticSettled, bytes: bytes)
      {
        let published = resultURL.deletingLastPathComponent().appendingPathComponent(
          "first-failure-owned-app-sample.txt")
        let descriptor = Darwin.open(raw.path, O_RDONLY | O_NOFOLLOW | O_CLOEXEC)
        guard descriptor >= 0 else { throw POSIXError(.EIO) }
        let input = FileHandle(fileDescriptor: descriptor, closeOnDealloc: true)
        defer { try? input.close() }
        var metadata = stat()
        guard fstat(descriptor, &metadata) == 0,
          metadata.st_mode & S_IFMT == S_IFREG, metadata.st_size == Int64(bytes),
          let contents = try input.read(upToCount: 5 * 1024 * 1024 + 1),
          contents.count == Int(bytes)
        else { throw Failure.invalidInput }
        try contents.write(to: published, options: .withoutOverwriting)
        record["path"] = published.path
        record["raw_retained"] = true
      } else {
        record["failure"] = "Sample unsettled, empty, oversized, or target launch identity changed"
      }
    } catch {
      record["failure"] = String(describing: error)
    }
  }

  private func settleDiagnostic() async {
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
  }

  private func settleProcesses() async {
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
  }

  private func restoreAndWriteObservation() throws {
    if let clipboard = panelClipboard, clipboard.hasActiveLease {
      if childReaped && outputDrainComplete == true && fixtureOutput.observedEOF() {
        do { try clipboard.restore(phase: .runnerSettled) } catch {
          failure = failure ?? "Panel clipboard restoration failed: \(error)"
        }
      } else {
        clipboard.recordFailure(Failure.invalidInput)
        failure = failure ?? "Panel clipboard restoration unverified: runner/output not settled"
      }
    }
    let appTerminated = app?.isTerminated == true
    let settingsTerminated = settings?.isTerminated ?? true
    if let held = heldCopy {
      do {
        guard try Data(contentsOf: held.held) == held.expected,
          !FileManager.default.fileExists(atPath: held.source.path)
        else { throw Failure.invalidInput }
        try Self.moveWithoutReplacement(from: held.held, to: held.source)
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
      "fixture_filesystem": fixtureFilesystem ?? NSNull(),
      "source_check_failure": originalsUnchanged
        ? NSNull() : "Owned original fixture bytes changed or could not be read",
      "owned_pasteboard_released": hadBoard,
      "panel_path_clipboard": panelClipboard?.observation
        ?? PanelPathClipboardLease.unusedObservation,
      "first_failure_owned_app_sample": firstFailureSample ?? NSNull(),
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
        "eof_observed": fixtureOutput.observedEOF(),
        "drain_within_deadline": outputDrainComplete.map { $0 as Any } ?? NSNull(),
        "failure": fixtureOutput.observedRelayFailure().map { $0 as Any } ?? NSNull(),
      ],
    ]
    var observation = result
    observation["ownership_receipt"] = ownershipReceipt.map { $0 as Any } ?? NSNull()
    try JSONSerialization.data(
      withJSONObject: observation, options: [.prettyPrinted, .sortedKeys]
    ).write(to: resultURL, options: .withoutOverwriting)
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
