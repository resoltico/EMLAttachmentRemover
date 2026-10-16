import Darwin
import Foundation
import Synchronization

struct HelperExit: Sendable, Equatable, Encodable {
  let code: Int32
  let normal: Bool
}
enum HelperPoll: Sendable {
  case running
  case exited(HelperExit)
  case failed(Int32)
}
struct HelperSpawn: Sendable {
  var child: HelperChild?
  let error: Int?
}
private struct NativeCallFailure: Error { let code: Int32 }

/// This value belongs only to the supervisor actor. Its PID comes directly from
/// posix_spawn and remains non-reusable until this actor consumes terminal wait
/// status. Polling and signaling contain no suspension or background reaper.
final class HelperChild: Sendable {
  private let pid: Mutex<pid_t?>

  private init(_ pid: pid_t) { self.pid = Mutex(pid) }

  var hasSignalAuthority: Bool { pid.withLock { $0 != nil } }

  func poll() -> HelperPoll {
    pid.withLock { pid in
      guard let owned = pid else { return .failed(ECHILD) }
      var status: Int32 = 0
      let result = waitpid(owned, &status, WNOHANG)
      if result == 0 { return .running }
      if result == -1 {
        let code = errno
        if code == EINTR { return .running }
        // Unexpected ownership loss must revoke every future signal authority.
        if code == ECHILD { pid = nil }
        return .failed(code)
      }
      guard result == owned else { return .failed(EPROTO) }
      // Darwin sys/wait.h: stopped status has low byte 0x7f. A traced stop is
      // not a terminal reap; retain ownership and allow the owned hard stop.
      if status & 0xFF == 0x7F { return .running }
      let signal = status & 0x7F
      guard signal == 0 || signal < 0x7F else { return .failed(EPROTO) }
      // Invalidate immediately, before returning or any actor suspension. Only
      // then may another process obtain this numeric PID.
      pid = nil
      return .exited(
        HelperExit(
          code: signal == 0 ? (status >> 8) & 0xFF : signal,
          normal: signal == 0))
    }
  }

  func signal(_ value: Int32) -> Int32? {
    pid.withLock { pid in
      guard let owned = pid else { return nil }
      guard value == SIGINT || value == SIGKILL else { return EINVAL }
      if kill(owned, value) == 0 { return nil }
      return errno
    }
  }

  static func spawn(
    executable: URL, arguments: [String], input: FileHandle, output: FileHandle,
    errors: FileHandle, parentChildEnds: [FileHandle]
  ) -> HelperSpawn {
    var attributes: posix_spawnattr_t?
    var actions: posix_spawn_file_actions_t?
    var attributesReady = false
    var actionsReady = false
    var descriptors: [Int32] = []
    var strings: [UnsafeMutablePointer<CChar>] = []
    var spawned: pid_t?
    var firstError: Int?
    func checked(_ code: Int32) throws {
      if code != 0 { throw NativeCallFailure(code: code) }
    }
    func allocated(_ pointer: UnsafePointer<CChar>) throws -> UnsafeMutablePointer<CChar> {
      guard let copy = strdup(pointer) else { throw NativeCallFailure(code: ENOMEM) }
      strings.append(copy)
      return copy
    }
    func allocated(_ string: String) throws -> UnsafeMutablePointer<CChar> {
      guard !string.utf8.contains(0) else { throw NativeCallFailure(code: EINVAL) }
      return try string.withCString { try allocated($0) }
    }
    do {
      var disposition = sigaction()
      guard sigaction(SIGCHLD, nil, &disposition) == 0 else {
        throw NativeCallFailure(code: errno)
      }
      // Reject ignored/custom/autoreaping dispositions. The application has no
      // other wait-family call or global child handler; do not alter one here.
      guard disposition.__sigaction_u.__sa_handler == nil,
        disposition.sa_flags & SA_NOCLDWAIT == 0
      else {
        throw NativeCallFailure(code: EINVAL)
      }
      try checked(posix_spawnattr_init(&attributes))
      attributesReady = true
      try checked(posix_spawn_file_actions_init(&actions))
      actionsReady = true
      var defaults = sigset_t()
      var mask = sigset_t()
      guard sigemptyset(&defaults) == 0, sigemptyset(&mask) == 0 else {
        throw NativeCallFailure(code: errno)
      }
      for signal in [SIGINT, SIGTERM, SIGHUP, SIGPIPE, SIGCHLD] {
        guard sigaddset(&defaults, signal) == 0 else { throw NativeCallFailure(code: errno) }
      }
      try checked(posix_spawnattr_setsigdefault(&attributes, &defaults))
      try checked(posix_spawnattr_setsigmask(&attributes, &mask))
      let flags = Int16(
        POSIX_SPAWN_SETSIGDEF | POSIX_SPAWN_SETSIGMASK | POSIX_SPAWN_CLOEXEC_DEFAULT)
      try checked(posix_spawnattr_setflags(&attributes, flags))
      for (standard, handle) in [input, output, errors].enumerated() {
        // Parent standard handles may be closed. Stable copies above 2 avoid
        // redirection cycles and descriptor aliasing during the child actions.
        let descriptor = fcntl(handle.fileDescriptor, F_DUPFD_CLOEXEC, 3)
        guard descriptor >= 0 else { throw NativeCallFailure(code: errno) }
        descriptors.append(descriptor)
        try checked(posix_spawn_file_actions_adddup2(&actions, descriptor, Int32(standard)))
        try checked(posix_spawn_file_actions_addclose(&actions, descriptor))
      }
      try executable.withUnsafeFileSystemRepresentation { path in
        guard let path else { throw NativeCallFailure(code: EINVAL) }
        var argv: [UnsafeMutablePointer<CChar>?] = [try allocated(path)]
        for argument in arguments { argv.append(try allocated(argument)) }
        argv.append(nil)
        var environment: [UnsafeMutablePointer<CChar>?] = []
        for (key, value) in ProcessInfo.processInfo.environment {
          environment.append(try allocated(key + "=" + value))
        }
        environment.append(nil)
        var child: pid_t = 0
        let code = argv.withUnsafeMutableBufferPointer { argv in
          environment.withUnsafeMutableBufferPointer { environment in
            posix_spawn(
              &child, path, &actions, &attributes, argv.baseAddress!, environment.baseAddress!)
          }
        }
        try checked(code)  // Spawn APIs return the error number directly.
        guard child > 0 else { throw NativeCallFailure(code: EPROTO) }
        spawned = child
      }
    } catch let failure as NativeCallFailure { firstError = Int(failure.code) } catch {
      firstError = Int(EINVAL)
    }

    // Cleanup is evaluated before building the result. A successfully spawned
    // child must survive as owned evidence even when parent cleanup fails.
    for descriptor in descriptors {
      if close(descriptor) != 0, firstError == nil { firstError = Int(errno) }
    }
    if actionsReady {
      let code = posix_spawn_file_actions_destroy(&actions)
      if code != 0, firstError == nil { firstError = Int(code) }
    }
    if attributesReady {
      let code = posix_spawnattr_destroy(&attributes)
      if code != 0, firstError == nil { firstError = Int(code) }
    }
    for pointer in strings { free(pointer) }
    // These are the parent's child-only ends, not the endpoints the I/O workers
    // will use. Close on success and failure, so readers see EOF and writes EPIPE.
    for handle in parentChildEnds {
      do { try handle.close() } catch {
        if firstError == nil { firstError = (error as NSError).code }
      }
    }
    return HelperSpawn(child: spawned.map(HelperChild.init), error: firstError)
  }
}
