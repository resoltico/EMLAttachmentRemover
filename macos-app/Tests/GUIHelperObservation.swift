// Read-only positive lifecycle observation for one owned GUI processing helper.
import AppKit
import Darwin
import Foundation
import Synchronization

enum HelperObservationFailure: Error { case unknown, budget }

struct ProcessingIdentity: Equatable, Sendable {
  let pid: pid_t
  let parent: UInt32
  let startSeconds: UInt64
  let startMicroseconds: UInt64
  let executable: Data
  // These public fields are stability observations, not a guaranteed unique cookie.
}

@MainActor
func processingIdentity(_ pid: pid_t, parent: NSRunningApplication, expected: Data) throws
  -> ProcessingIdentity
{
  guard !parent.isTerminated, pid > 0 else { throw HelperObservationFailure.unknown }
  var info = proc_bsdinfo()
  let size = MemoryLayout<proc_bsdinfo>.size
  guard proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, Int32(size)) == size,
    info.pbi_pid == UInt32(pid), info.pbi_ppid == UInt32(parent.processIdentifier),
    info.pbi_status != UInt32(SZOMB)
  else { throw HelperObservationFailure.unknown }
  // sys/proc_info.h defines PROC_PIDPATHINFO_MAXSIZE as 4*MAXPATHLEN;
  // the expression macro is unavailable to Swift's importer.
  var path = [UInt8](repeating: 0, count: 4 * Int(MAXPATHLEN))
  let count = proc_pidpath(pid, &path, UInt32(path.count))
  guard count > 0, count < path.count, let terminator = path.firstIndex(of: 0) else {
    throw HelperObservationFailure.unknown
  }
  let executable = Data(path[..<terminator])
  guard executable == expected, !parent.isTerminated else { throw HelperObservationFailure.unknown }
  return ProcessingIdentity(
    pid: pid, parent: info.pbi_ppid, startSeconds: info.pbi_start_tvsec,
    startMicroseconds: info.pbi_start_tvusec, executable: executable)
}

/// Enumeration can establish a positive candidate only. Empty/failed/truncated
/// results cannot establish helper absence or cleanup completion.
@MainActor
func findProcessingIdentity(parent: NSRunningApplication) throws -> ProcessingIdentity {
  guard !parent.isTerminated, let executable = parent.executableURL else {
    throw HelperObservationFailure.unknown
  }
  let helper = executable.deletingLastPathComponent().appending(path: "remove-eml-attachments")
  let expected = try helper.withUnsafeFileSystemRepresentation { pointer -> Data in
    guard let pointer else { throw HelperObservationFailure.unknown }
    return Data(bytes: pointer, count: strlen(pointer))
  }
  var pids = [pid_t](repeating: 0, count: 256)
  let capacity = pids.count * MemoryLayout<pid_t>.stride
  // libproc takes a byte capacity but returns a PID count.
  let count = proc_listchildpids(parent.processIdentifier, &pids, Int32(capacity))
  guard count > 0, count < pids.count else {
    throw HelperObservationFailure.unknown
  }
  var matches: [ProcessingIdentity] = []
  for pid in pids.prefix(Int(count)) where pid > 0 {
    if let identity = try? processingIdentity(pid, parent: parent, expected: expected) {
      matches.append(identity)
    }
  }
  guard matches.count == 1 else { throw HelperObservationFailure.unknown }
  return matches[0]
}

final class ProcessingExitObservation: Sendable {
  private struct State: Sendable {
    var registered = false
    var exited = false
    var qualified = false
  }
  private final class SharedState: Sendable { let value = Mutex(State()) }
  private let shared = SharedState()
  private let source: any DispatchSourceProcess
  private let identity: ProcessingIdentity

  init(_ identity: ProcessingIdentity) {
    self.identity = identity
    source = DispatchSource.makeProcessSource(
      identifier: identity.pid, eventMask: .exit,
      queue: DispatchQueue(label: "owned-helper-exit-observation"))
    let shared = self.shared
    source.setRegistrationHandler { shared.value.withLock { $0.registered = true } }
    source.setEventHandler { shared.value.withLock { $0.exited = true } }
    source.activate()
  }
  deinit { source.cancel() }

  @MainActor
  func qualify(parent: NSRunningApplication, deadline: ContinuousClock.Instant) async throws {
    while !shared.value.withLock({ $0.registered }) {
      guard ContinuousClock.now < deadline else { throw HelperObservationFailure.budget }
      try await Task.sleep(for: .milliseconds(10))
    }
    // activate() alone is not registration acknowledgement. A synthetic ESRCH
    // exit before this live identity recheck never qualifies as observed exit.
    guard ContinuousClock.now < deadline,
      !shared.value.withLock({ $0.exited }),
      try processingIdentity(identity.pid, parent: parent, expected: identity.executable)
        == identity,
      ContinuousClock.now < deadline,
      !shared.value.withLock({ $0.exited })
    else { throw HelperObservationFailure.unknown }
    shared.value.withLock { $0.qualified = true }
  }

  var observedQualifiedExit: Bool {
    shared.value.withLock { $0.registered && $0.qualified && $0.exited }
  }
}
