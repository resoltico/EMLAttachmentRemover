import Darwin
import Foundation
import RunModel

enum HelperFailure: Error, Sendable {
  case unavailable
  case invalidVersion
  case launch(Int)
  case transport(Int)
  case ownership(Int)
  case invalidStream(ProtocolFailure)
  case signalled(Int32)
  case requestRefused
  case stoppedBeforeStart
}
struct HelperCompletion: Sendable {
  let report: RunReport
  let stderr: Data
  let failure: HelperFailure?
  let termination: HelperExit?
  init(report: RunReport, stderr: Data, failure: HelperFailure?, termination: HelperExit? = nil) {
    self.report = report
    self.stderr = stderr
    self.failure = failure
    self.termination = termination
  }
}
private struct PipeCapture: Sendable {
  let bytes: Data
  let exceeded: Bool
  let error: Int?
}
private struct WriteCapture: Sendable {
  let error: Int?
  let brokenPipe: Bool
}

private enum LaunchResult {
  case started, stopped
  case failed(Int)
}

/// One helper, one actor, one reaper. Only launch/signal/reap use the narrow
/// native owner; Foundation pipes/Data and protocol observations remain shared.
actor HelperProcess {
  private var current: HelperChild?
  private var generation: UUID?
  private var activeControl: HelperRunControl?
  private var phaseFailure: HelperFailure?
  private var phaseStopAttempted = false

  func stop(control: HelperRunControl) {
    guard activeControl === control else { return }
    let intent = control.snapshot()
    guard intent.stops > 0 else { return }
    if intent.stops == 2 || !phaseStopAttempted {
      phaseStopAttempted = signalCurrent(
        intent.stops == 2 && !intent.inputMayHaveBeenSent ? SIGKILL : SIGINT)
    }
  }

  func forceKill(control: HelperRunControl) {
    guard activeControl === control else { return }
    signalCurrent(SIGKILL)
  }

  @discardableResult
  private func signalCurrent(_ value: Int32) -> Bool {
    guard let current, current.hasSignalAuthority else { return false }
    if let code = current.signal(value), code != ESRCH, phaseFailure == nil {
      phaseFailure = .transport(Int(code))
    }
    return true
  }

  private func transportFailed(_ id: UUID, failure: HelperFailure, requestBrokenPipe: Bool) {
    guard generation == id else { return }
    // Reconcile only an exact broken-pipe consequence of an observed user stop.
    // An earlier independently observed error remains latched across cancellation.
    if requestBrokenPipe && (activeControl?.snapshot().stops ?? 0) > 0 { return }
    if phaseFailure == nil { phaseFailure = failure }
    if !phaseStopAttempted {
      phaseStopAttempted = signalCurrent(SIGINT)
    }
  }

  private func notification(_ id: UUID) -> @Sendable (HelperFailure, Bool) -> Void {
    let supervisor = self
    return { failure, brokenPipe in
      let acknowledged = DispatchSemaphore(value: 0)
      Task {
        await supervisor.transportFailed(id, failure: failure, requestBrokenPipe: brokenPipe)
        acknowledged.signal()
      }
      // Dedicated I/O threads only. No notification can outlive its phase.
      acknowledged.wait()
    }
  }

  private func awaitExit(probeDeadline: ContinuousClock.Instant? = nil) async -> HelperExit? {
    var escalated = false
    while let owned = current {
      // poll revokes shared authority synchronously on terminal reap. No actor
      // suspension or background exit notification can leave a stale PID live.
      switch owned.poll() {
      case .exited(let status): return status
      case .failed(let code):
        if phaseFailure == nil { phaseFailure = .ownership(Int(code)) }
        if code == ECHILD { return nil }
        if !escalated {
          escalated = true
          signalCurrent(SIGKILL)
        }
      case .running: break
      }
      if let deadline = probeDeadline, ContinuousClock.now >= deadline, !escalated {
        escalated = true
        if phaseFailure == nil { phaseFailure = .transport(Int(ETIMEDOUT)) }
        signalCurrent(SIGKILL)
      }
      do { try await Task.sleep(for: .milliseconds(20)) } catch { await Task.yield() }
    }
    return nil
  }

  private func launch(
    helper: URL, arguments: [String], input: FileHandle, output: FileHandle,
    errors: FileHandle, parentChildEnds: [FileHandle], control: HelperRunControl
  ) async -> LaunchResult {
    guard control.snapshot().stops == 0 else {
      var closeError: Int?
      for handle in parentChildEnds {
        do { try handle.close() } catch {
          if closeError == nil { closeError = (error as NSError).code }
        }
      }
      if let closeError { return .failed(closeError) }
      return .stopped
    }
    generation = UUID()
    phaseFailure = nil
    phaseStopAttempted = false
    let spawned = await blockingIO {
      HelperChild.spawn(
        executable: helper, arguments: arguments, input: input,
        output: output, errors: errors, parentChildEnds: parentChildEnds)
    }
    current = spawned.child
    if let error = spawned.error {
      if current != nil {
        // No request has been fed. Keep the spawned capability through failed
        // parent cleanup, and settle this child before returning startup error.
        signalCurrent(SIGKILL)
        _ = await awaitExit()
      }
      current = nil
      generation = nil
      return .failed(error)
    }
    guard current != nil else { return .failed(Int(EPROTO)) }
    if control.snapshot().stops > 0 { stop(control: control) }
    return .started
  }

  private func settled() {
    precondition(current?.hasSignalAuthority != true, "An owned child must settle before release")
    current = nil
    generation = nil
    phaseFailure = nil
  }

  func run(
    request: QueuedRequest, control: HelperRunControl,
    update: @escaping @MainActor @Sendable (RunReport) -> Void
  ) async -> HelperCompletion {
    let version =
      Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? ""
    let empty = RunReport(expectedVersion: version)
    guard activeControl == nil, request.id == control.id, !version.isEmpty,
      Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String == version,
      let helper = Bundle.main.url(forAuxiliaryExecutable: "remove-eml-attachments")
    else {
      return HelperCompletion(report: empty, stderr: Data(), failure: .unavailable)
    }
    activeControl = control
    defer {
      activeControl = nil
      settled()
    }
    if control.snapshot().stops > 0 {
      return HelperCompletion(report: empty, stderr: Data(), failure: .stoppedBeforeStart)
    }

    // Probe the actual bundled executable, fully draining/reaping it before a
    // real request. No shell, PATH fallback or helper-name version inference.
    let probeInput = Pipe()
    do { try probeInput.fileHandleForWriting.close() } catch {
      return HelperCompletion(
        report: empty, stderr: Data(), failure: .transport((error as NSError).code))
    }
    let probeOut = Pipe()
    let probeErr = Pipe()
    switch await launch(
      helper: helper, arguments: ["--version"], input: probeInput.fileHandleForReading,
      output: probeOut.fileHandleForWriting, errors: probeErr.fileHandleForWriting,
      parentChildEnds: [
        probeInput.fileHandleForReading, probeOut.fileHandleForWriting,
        probeErr.fileHandleForWriting,
      ],
      control: control)
    {
    case .failed(let code):
      return HelperCompletion(report: empty, stderr: Data(), failure: .launch(code))
    case .stopped:
      return HelperCompletion(report: empty, stderr: Data(), failure: .stoppedBeforeStart)
    case .started: break
    }
    let probeNotice = notification(generation!)
    async let versionOut = blockingIO {
      capture(
        probeOut.fileHandleForReading, retain: 256,
        onError: { probeNotice(.transport($0), false) },
        onOverflow: { probeNotice(.invalidVersion, false) })
    }
    async let versionErr = blockingIO {
      capture(
        probeErr.fileHandleForReading, retain: 65_536,
        onError: { probeNotice(.transport($0), false) },
        onOverflow: { probeNotice(.invalidVersion, false) })
    }
    let probeExit = await awaitExit(probeDeadline: .now.advanced(by: .seconds(10)))
    let (versionBytes, versionErrors) = await (versionOut, versionErr)
    let probeFailure = phaseFailure
    settled()
    if let probeFailure {
      return HelperCompletion(report: empty, stderr: versionErrors.bytes, failure: probeFailure)
    }
    if control.snapshot().stops > 0 {
      return HelperCompletion(
        report: empty, stderr: versionErrors.bytes, failure: .stoppedBeforeStart)
    }
    guard probeExit?.normal == true, probeExit?.code == 0, versionBytes.error == nil,
      versionErrors.error == nil, !versionBytes.exceeded, !versionErrors.exceeded,
      versionErrors.bytes.isEmpty,
      versionBytes.bytes == Data("remove-eml-attachments \(version)\n".utf8)
    else {
      return HelperCompletion(report: empty, stderr: versionErrors.bytes, failure: .invalidVersion)
    }

    let document: Data
    do { document = try request.encoded() } catch {
      return HelperCompletion(report: empty, stderr: Data(), failure: .unavailable)
    }
    if control.snapshot().stops > 0 {
      return HelperCompletion(report: empty, stderr: Data(), failure: .stoppedBeforeStart)
    }
    let input = Pipe()
    let output = Pipe()
    let errors = Pipe()
    // Suppress SIGPIPE for this owned pipe descriptor only. A closed helper
    // input becomes a checked transport error, never termination of the app.
    guard fcntl(input.fileHandleForWriting.fileDescriptor, F_SETNOSIGPIPE, 1) == 0 else {
      return HelperCompletion(report: empty, stderr: Data(), failure: .transport(Int(errno)))
    }
    switch await launch(
      helper: helper, arguments: ["--format", "json", "--request", "-"],
      input: input.fileHandleForReading, output: output.fileHandleForWriting,
      errors: errors.fileHandleForWriting,
      parentChildEnds: [
        input.fileHandleForReading, output.fileHandleForWriting, errors.fileHandleForWriting,
      ], control: control)
    {
    case .failed(let code):
      return HelperCompletion(report: empty, stderr: Data(), failure: .launch(code))
    case .stopped:
      return HelperCompletion(report: empty, stderr: Data(), failure: .stoppedBeforeStart)
    case .started: break
    }
    let notice = notification(generation!)
    async let written = blockingIO {
      var failure: Int?
      var brokenPipe = false
      if control.beginFeeding() {
        do { try input.fileHandleForWriting.write(contentsOf: document) } catch {
          failure = (error as NSError).code
          brokenPipe = isBrokenPipe(error)
          notice(.transport(failure!), brokenPipe)
        }
      }
      do { try input.fileHandleForWriting.close() } catch {
        if failure == nil { failure = (error as NSError).code }
        notice(.transport((error as NSError).code), false)
      }
      return WriteCapture(error: failure, brokenPipe: brokenPipe)
    }
    async let stderr = blockingIO {
      capture(
        errors.fileHandleForReading, retain: 65_536,
        onError: { notice(.transport($0), false) })
    }
    async let stdout = blockingIO {
      readEvents(
        output.fileHandleForReading, version: version, update: update,
        fail: { notice($0, false) })
    }
    let actualExit = await awaitExit()
    let (writeResult, diagnostic, observed) = await (written, stderr, stdout)
    var report = observed.report
    let intent = control.snapshot()
    let failure: HelperFailure?
    if phaseFailure == nil, observed.error == nil, diagnostic.error == nil,
      intent.stops > 0,
      !intent.inputMayHaveBeenSent || (actualExit?.normal == true && actualExit?.code == 130),
      report.start == nil, report.failure == nil,
      writeResult.error == nil || writeResult.brokenPipe
    {
      failure = .stoppedBeforeStart
    } else if let phaseFailure {
      failure = phaseFailure
    } else if let observedFailure = observed.error {
      failure = observedFailure
    } else if let actualExit, !actualExit.normal {
      failure = .signalled(actualExit.code)
    } else if let writeError = writeResult.error {
      failure = .transport(writeError)
    } else if let error = diagnostic.error {
      failure = .transport(error)
    } else if actualExit == nil {
      failure = .ownership(Int(ECHILD))
    } else if report.start == nil && actualExit?.code == 2 {
      failure = .requestRefused
    } else {
      do {
        try report.finish(exitCode: actualExit!.code)
        failure = nil
      } catch let reason as ProtocolFailure { failure = .invalidStream(reason) } catch {
        failure = .invalidStream(.malformedEvent)
      }
    }
    // The last snapshot is acknowledged before returning and launching another
    // queued request; completion is not merely a termination notification.
    let completedReport = report
    await blockingIO { publish(completedReport, update: update) }
    return HelperCompletion(
      report: report, stderr: diagnostic.bytes, failure: failure, termination: actualExit)
  }

}

func blockingIO<Value: Sendable>(
  _ operation: @escaping @Sendable () -> Value
) async -> Value {
  await withCheckedContinuation { continuation in
    Thread.detachNewThread { continuation.resume(returning: operation()) }
  }
}
private func isBrokenPipe(_ error: any Error) -> Bool {
  let native = error as NSError
  if native.domain == NSPOSIXErrorDomain && native.code == Int(EPIPE) { return true }
  guard let underlying = native.userInfo[NSUnderlyingErrorKey] as? NSError else { return false }
  return underlying.domain == NSPOSIXErrorDomain && underlying.code == Int(EPIPE)
}
private func capture(
  _ handle: FileHandle, retain: Int,
  onError: @escaping @Sendable (Int) -> Void,
  onOverflow: @escaping @Sendable () -> Void = {}
) -> PipeCapture {
  var retained = Data()
  var exceeded = false
  var failure: Int?
  do {
    while let chunk = try handle.read(upToCount: 65_536), !chunk.isEmpty {
      let admitted = min(chunk.count, retain - retained.count)
      retained.append(chunk.prefix(admitted))
      if !exceeded && admitted != chunk.count {
        exceeded = true
        onOverflow()
      }
    }
  } catch {
    failure = (error as NSError).code
    onError(failure!)
  }
  do { try handle.close() } catch {
    if failure == nil { failure = (error as NSError).code }
    onError((error as NSError).code)
  }
  return PipeCapture(bytes: retained, exceeded: exceeded, error: failure)
}
struct EventCapture: Sendable {
  let report: RunReport
  let error: HelperFailure?
}
func readEvents(
  _ handle: FileHandle, version: String,
  update: @escaping @MainActor @Sendable (RunReport) -> Void,
  fail: @escaping @Sendable (HelperFailure) -> Void
) -> EventCapture {
  var report = RunReport(expectedVersion: version)
  var splitter = LFFrameSplitter()
  var failure: HelperFailure?
  do {
    while let chunk = try handle.read(upToCount: 65_536), !chunk.isEmpty {
      guard failure == nil else { continue }  // Drain after failure; never decode again.
      do {
        var batch = 0
        try splitter.append(chunk) { frame in
          let event = try ProtocolEvent.decode(frame)
          if case .start(let start) = event, start.dryRun {
            throw ProtocolFailure.incompatibleHelper
          }
          try report.accept(event)
          batch += 1
          if report.items.isEmpty || batch == 64 {
            publish(report, update: update)
            batch = 0
          }
        }
        if batch > 0 { publish(report, update: update) }
      } catch {
        let reason = (error as? ProtocolFailure) ?? .malformedEvent
        report.invalidate(reason)
        failure = .invalidStream(reason)
        fail(.invalidStream(reason))
        publish(report, update: update)
      }
    }
  } catch {
    if failure == nil {
      let observed = HelperFailure.transport((error as NSError).code)
      failure = observed
      fail(observed)
    }
  }
  splitter.finish()
  do { try handle.close() } catch {
    if failure == nil {
      let observed = HelperFailure.transport((error as NSError).code)
      failure = observed
      fail(observed)
    }
  }
  return EventCapture(report: report, error: failure)
}
private func publish(
  _ report: RunReport, update: @escaping @MainActor @Sendable (RunReport) -> Void
) {
  let acknowledged = DispatchSemaphore(value: 0)
  Task { @MainActor in
    defer { acknowledged.signal() }
    update(report)
  }
  acknowledged.wait()
}
