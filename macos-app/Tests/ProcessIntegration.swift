// Native process-boundary fixture. This is test tooling, not a product app mode.
import Darwin
import Foundation
import RunModel
import Synchronization

enum IntegrationFailure: Error {
  case invalidArguments, unexpectedOutcome, changedBytes, residue
  case nativeCleanup(original: Int?, settled: Bool, diagnostic: Int?)
}

@MainActor private final class Delivery {
  var count = 0
  var latest: RunReport?
  var returned = false

  func accept(_ report: RunReport) {
    precondition(!returned, "A completed request received a later callback")
    count += 1
    latest = report
  }
  func finish(_ completion: HelperCompletion) throws {
    returned = true
    guard count > 0, let latest,
      latest.summary?.counts == completion.report.summary?.counts,
      latest.summary?.exitCode == completion.report.summary?.exitCode,
      latest.items.map(\.index) == completion.report.items.map(\.index),
      latest.items.map(\.status) == completion.report.items.map(\.status),
      latest.failure == completion.report.failure
    else { throw IntegrationFailure.unexpectedOutcome }
  }
}

@main struct ProcessIntegration {
  @MainActor static func main() async throws {
    guard (3...4).contains(CommandLine.arguments.count) else {
      throw IntegrationFailure.invalidArguments
    }
    try nativeMachine()
    let mode = CommandLine.arguments.count == 4 ? CommandLine.arguments[3] : "ordinary"
    let repository = URL(filePath: CommandLine.arguments[1], directoryHint: .isDirectory)
    let owned = URL(filePath: CommandLine.arguments[2], directoryHint: .isDirectory)
    guard try FileManager.default.contentsOfDirectory(atPath: owned.path).isEmpty else {
      throw IntegrationFailure.invalidArguments
    }
    let input = try Data(
      contentsOf: repository.appending(path: "tests/contract/mime/inputs/01-gmail-attachment.eml"))
    let golden = try Data(
      contentsOf: repository.appending(path: "tests/contract/mime/golden/01-gmail-attachment.eml"))
    guard input.count == 1028, golden.count == 513 else { throw IntegrationFailure.changedBytes }
    let source = owned.appending(path: "a.eml")
    let copy = owned.appending(path: "a.no-attachments.eml")
    try input.write(to: source, options: [.withoutOverwriting])
    let supervisor = HelperProcess()
    if mode != "ordinary" {
      guard ["version-mismatch", "helper-missing"].contains(mode) else {
        throw IntegrationFailure.invalidArguments
      }
      let request = try QueuedRequest(fileURLs: [source], destination: nil)
      var callbacks = 0
      let completion = await supervisor.run(
        request: request, control: HelperRunControl(id: request.id)
      ) { _ in callbacks += 1 }
      let expected: Bool
      switch (mode, completion.failure) {
      case ("version-mismatch", .invalidVersion), ("helper-missing", .unavailable): expected = true
      default: expected = false
      }
      guard expected, callbacks == 0, completion.report.start == nil,
        completion.report.items.isEmpty, completion.report.summary == nil,
        try Data(contentsOf: source) == input,
        try FileManager.default.contentsOfDirectory(atPath: owned.path) == ["a.eml"]
      else { throw IntegrationFailure.unexpectedOutcome }
      print("actual \(mode): specific failure, no Core stream/copy, source unchanged")
      return
    }
    try await readerAgreement(repository)
    try await nativeKillAndReap()
    for expected in [ItemStatus.created, .upToDate] {
      let request = try QueuedRequest(fileURLs: [source], destination: nil)
      let delivery = Delivery()
      let completion = await supervisor.run(
        request: request, control: HelperRunControl(id: request.id)
      ) { report in
        delivery.accept(report)
      }
      let diagnostic =
        "expected \(expected.rawValue); helper failure: \(String(describing: completion.failure)); callbacks: \(delivery.count); start: \(completion.report.start != nil); summary: \(String(describing: completion.report.summary?.exitCode)); statuses: \(completion.report.items.map(\.status))\nhelper diagnostic: \(String(decoding: completion.stderr.prefix(2048), as: UTF8.self))\n"
      try FileHandle.standardError.write(contentsOf: Data(diagnostic.utf8))
      try delivery.finish(completion)
      guard completion.failure == nil, completion.stderr.isEmpty,
        completion.report.summary?.exitCode == 0,
        completion.report.items.count == 1,
        completion.report.items[0].status == expected,
        completion.report.items[0].removed.count == 1,
        completion.report.items[0].removed[0].filename == "report.pdf",
        completion.report.items[0].removed[0].bytes == 507,
        completion.report.items[0].savedBytes == 515,
        completion.report.items[0].revealPath != nil
      else {
        throw IntegrationFailure.unexpectedOutcome
      }
      guard try Data(contentsOf: source) == input, try Data(contentsOf: copy) == golden else {
        throw IntegrationFailure.changedBytes
      }
      print(
        "actual helper \(expected.rawValue): golden bytes, receipts, stream, EOF and exit passed")
    }
    let missing = try NativePath(
      fileURL: owned.appending(path: "missing-output", directoryHint: .isDirectory))
    let invalidRequest = try QueuedRequest(fileURLs: [source], destination: missing)
    let refusalDelivery = Delivery()
    let refused = await supervisor.run(
      request: invalidRequest, control: HelperRunControl(id: invalidRequest.id)
    ) { refusalDelivery.accept($0) }
    try refusalDelivery.finish(refused)
    guard case .requestRefused = refused.failure,
      refused.report.start == nil, refused.report.items.isEmpty,
      !refused.stderr.isEmpty
    else { throw IntegrationFailure.unexpectedOutcome }
    guard try Data(contentsOf: source) == input, try Data(contentsOf: copy) == golden else {
      throw IntegrationFailure.changedBytes
    }
    let names = try FileManager.default.contentsOfDirectory(atPath: owned.path).sorted()
    guard names == ["a.eml", "a.no-attachments.eml"] else { throw IntegrationFailure.residue }
    print("actual helper pre-stream refusal: no events/new copy, prior bytes unchanged")
    for stops in [1, 2] {
      let name = "cancelled-\(stops).eml"
      let cancelledSource = owned.appending(path: name)
      try input.write(to: cancelledSource, options: [.withoutOverwriting])
      let request = try QueuedRequest(fileURLs: [cancelledSource], destination: nil)
      let control = HelperRunControl(id: request.id)
      for _ in 0..<stops { control.requestStop() }
      // Deliberately notify before admission; the token must retain the intent.
      await supervisor.stop(control: control)
      var callbacks = 0
      let result = await supervisor.run(request: request, control: control) { _ in callbacks += 1 }
      guard case .stoppedBeforeStart = result.failure, result.report.start == nil,
        result.report.items.isEmpty, callbacks == 0,
        try Data(contentsOf: cancelledSource) == input,
        !FileManager.default.fileExists(
          atPath: owned.appending(path: "cancelled-\(stops).no-attachments.eml").path)
      else { throw IntegrationFailure.unexpectedOutcome }
    }
    print("pre-admission stop and force intent: no processing or invented outcomes")
    let name = "native\u{0085}\u{2028}\n\u{1B}.eml"
    let nativeSource = owned.appending(path: name)
    let nativeCopy = owned.appending(path: "native\u{0085}\u{2028}\n\u{1B}.no-attachments.eml")
    try input.write(to: nativeSource, options: [.withoutOverwriting])
    let request = try QueuedRequest(fileURLs: [nativeSource], destination: nil)
    let nativeDelivery = Delivery()
    let result = await supervisor.run(request: request, control: HelperRunControl(id: request.id)) {
      nativeDelivery.accept($0)
    }
    try nativeDelivery.finish(result)
    var expectedNative = try owned.withUnsafeFileSystemRepresentation { pointer -> Data in
      guard let pointer else { throw IntegrationFailure.invalidArguments }
      return Data(bytes: pointer, count: strlen(pointer))
    }
    if expectedNative.last != 47 { expectedNative.append(47) }
    expectedNative.append(contentsOf: name.utf8)
    guard result.failure == nil, result.report.items.count == 1,
      result.report.items[0].status == .created,
      try result.report.items[0].source.native.bytes() == expectedNative,
      try Data(contentsOf: nativeSource) == input, try Data(contentsOf: nativeCopy) == golden
    else {
      throw IntegrationFailure.changedBytes
    }
    print(
      "actual native Unicode separators/LF/ESC path: lossless request and real LF stream, golden copy"
    )
    let finalNames = try FileManager.default.contentsOfDirectory(atPath: owned.path).sorted()
    guard
      finalNames
        == [
          "a.eml", "a.no-attachments.eml", "cancelled-1.eml", "cancelled-2.eml", name,
          nativeCopy.lastPathComponent,
        ].sorted()
    else { throw IntegrationFailure.residue }
  }

  @MainActor private static func readerAgreement(_ repository: URL) async throws {
    let version =
      Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? ""
    for (name, refused) in [("created-attachment", false), ("planned-create-unix", true)] {
      let data = try Data(
        contentsOf: repository.appending(path: "tests/contract/protocol/\(name).jsonl"))
      let bytes = Data(
        String(decoding: data, as: UTF8.self).replacingOccurrences(of: "@VERSION@", with: version)
          .utf8)
      let pipe = Pipe()
      let failures = Mutex(0)
      let delivery = Delivery()
      async let written: Int? = blockingIO {
        var failure: Int?
        do { try pipe.fileHandleForWriting.write(contentsOf: bytes) } catch {
          failure = (error as NSError).code
        }
        do { try pipe.fileHandleForWriting.close() } catch {
          if failure == nil { failure = (error as NSError).code }
        }
        return failure
      }
      let observed = await blockingIO {
        readEvents(
          pipe.fileHandleForReading, version: version,
          update: { report in
            if report.failure != nil { precondition(failures.withLock { $0 } == 1) }
            delivery.accept(report)
          }, fail: { _ in failures.withLock { $0 += 1 } })
      }
      guard await written == nil else { throw IntegrationFailure.unexpectedOutcome }
      try delivery.finish(
        HelperCompletion(report: observed.report, stderr: Data(), failure: observed.error))
      if refused {
        guard case .invalidStream(.incompatibleHelper) = observed.error,
          observed.report.start == nil, observed.report.items.isEmpty,
          observed.report.summary == nil, failures.withLock({ $0 }) == 1
        else { throw IntegrationFailure.unexpectedOutcome }
      } else {
        guard observed.error == nil, observed.report.items.count == 1,
          observed.report.items[0].status == .created,
          observed.report.summary?.exitCode == 0, failures.withLock({ $0 }) == 0
        else { throw IntegrationFailure.unexpectedOutcome }
      }
    }
    print(
      "actual Create reader agreement: matching create accepted, valid plan refused before Start and drained"
    )
  }

  private static func nativeMachine() throws {
    var translated: Int32 = 0
    var size = MemoryLayout<Int32>.size
    let status = sysctlbyname("sysctl.proc_translated", &translated, &size, nil, 0)
    if status == 0 {
      guard size == MemoryLayout<Int32>.size, translated == 0 else {
        throw IntegrationFailure.invalidArguments
      }
    } else {
      guard errno == ENOENT else { throw IntegrationFailure.invalidArguments }
    }
    var system = utsname()
    guard uname(&system) == 0 else { throw IntegrationFailure.invalidArguments }
    let machine = withUnsafeBytes(of: &system.machine) {
      String(decoding: $0.prefix { $0 != 0 }, as: UTF8.self)
    }
    #if arch(arm64)
      guard machine == "arm64" else { throw IntegrationFailure.invalidArguments }
    #elseif arch(x86_64)
      guard machine == "x86_64" else { throw IntegrationFailure.invalidArguments }
    #else
      throw IntegrationFailure.invalidArguments
    #endif
    print(
      "actual native fixture machine: \(machine), untranslated; \(ProcessInfo.processInfo.operatingSystemVersionString)"
    )
  }

  private static func nativeKillAndReap() async throws {
    guard let helper = Bundle.main.url(forAuxiliaryExecutable: "remove-eml-attachments") else {
      throw IntegrationFailure.invalidArguments
    }
    let input = Pipe()
    let output = Pipe()
    let errors = Pipe()
    let spawned = HelperChild.spawn(
      executable: helper, arguments: ["--format", "json", "--request", "-"],
      input: input.fileHandleForReading, output: output.fileHandleForWriting,
      errors: errors.fileHandleForWriting,
      parentChildEnds: [
        input.fileHandleForReading, output.fileHandleForWriting, errors.fileHandleForWriting,
      ])
    var original = spawned.error
    var diagnosticError: Int?
    var exit: HelperExit?
    var settled = spawned.child == nil
    if let child = spawned.child {
      let alias = child
      if let code = child.signal(SIGKILL), original == nil { original = Int(code) }
      let observed = await waitForChild(child, seconds: 5)
      exit = observed.0
      if let code = observed.1, original == nil { original = Int(code) }
      if child.hasSignalAuthority {
        // Preserve the first failure, but finish actual owned cleanup before
        // throwing. The outer Controller remains the bounded fallback.
        let cleanupSignal = child.signal(SIGKILL)
        let cleanup = await Task.detached { await waitForChild(child, seconds: 2) }.value
        let message =
          "native fixture cleanup: signal=\(String(describing: cleanupSignal)), wait=\(String(describing: cleanup.1)), authority=\(child.hasSignalAuthority)\n"
        do { try FileHandle.standardError.write(contentsOf: Data(message.utf8)) } catch {
          diagnosticError = (error as NSError).code
        }
      }
      settled = !child.hasSignalAuthority && !alias.hasSignalAuthority
    } else if original == nil {
      original = Int(EPROTO)
    }
    var empty = false
    if original == nil, settled {
      do {
        let stdout = try output.fileHandleForReading.readToEnd() ?? Data()
        let stderr = try errors.fileHandleForReading.readToEnd() ?? Data()
        empty = stdout.isEmpty && stderr.isEmpty
      } catch { original = (error as NSError).code }
    }
    for handle in [
      input.fileHandleForWriting, output.fileHandleForReading, errors.fileHandleForReading,
    ] {
      do { try handle.close() } catch { if original == nil { original = (error as NSError).code } }
    }
    guard original == nil, settled, empty, exit == HelperExit(code: SIGKILL, normal: false) else {
      let message =
        "native fixture failure: original=\(String(describing: original)), settled=\(settled), exit=\(String(describing: exit))\n"
      do { try FileHandle.standardError.write(contentsOf: Data(message.utf8)) } catch {
        if diagnosticError == nil { diagnosticError = (error as NSError).code }
      }
      throw IntegrationFailure.nativeCleanup(
        original: original, settled: settled, diagnostic: diagnosticError)
    }
    print("actual owned SIGKILL: terminal wait status9, shared authority revoked, EOF settled")
  }

  private static func waitForChild(_ child: HelperChild, seconds: Int64) async -> (
    HelperExit?, Int32?
  ) {
    let deadline = ContinuousClock.now.advanced(by: .seconds(seconds))
    while ContinuousClock.now < deadline {
      switch child.poll() {
      case .exited(let exit): return (exit, nil)
      case .failed(let code): return (nil, code)
      case .running:
        do { try await Task.sleep(for: .milliseconds(10)) } catch { return (nil, ECANCELED) }
      }
    }
    return (nil, ETIMEDOUT)
  }
}
