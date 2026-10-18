import Darwin
// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import Foundation

enum DesktopFixtureRequest: String, CaseIterable, Sendable {
  case pickerBaseline = "EML_NATIVE_PICKER_BASELINE"
  case picker = "EML_NATIVE_PICKER_REQUESTED"
  case holdCopy = "EML_OWNED_COPY_HOLD"
  case restoreCopy = "EML_OWNED_COPY_RESTORE"
  func send() throws {
    // Bypass stdio buffering: the owner must observe this before the UI waits
    // for an acknowledgment or enters the native panel operation.
    try FileHandle.standardOutput.write(contentsOf: Data((rawValue + "\n").utf8))
  }
}

// The pipe callback and lifetime owner share only bounded observations/requests.
// NSLock protects every mutable field; native capabilities stay on the main actor.
final class DesktopFixtureOutput: @unchecked Sendable {
  private let lock = NSLock()
  private var line = Data()
  private var droppingLine = false
  private var pending = [DesktopFixtureRequest]()
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
    lock.lock()
    defer { lock.unlock() }
    for byte in bytes {
      if byte == 10 {
        if !droppingLine,
          let text = String(data: line, encoding: .utf8),
          let request = DesktopFixtureRequest(rawValue: text)
        {
          guard pending.count < 8 else {
            relayFailure = relayFailure ?? "Owned fixture request queue overflow"
            return
          }
          pending.append(request)
        }
        line.removeAll(keepingCapacity: true)
        droppingLine = false
      } else if !droppingLine {
        if line.count < 4096 {
          line.append(byte)
        } else {
          line.removeAll(keepingCapacity: true)
          droppingLine = true
        }
      }
    }

  }
  func take() -> DesktopFixtureRequest? {
    lock.lock()
    defer { lock.unlock() }
    return pending.isEmpty ? nil : pending.removeFirst()
  }
  func relay(from handle: FileHandle, to destination: FileHandle) throws {
    let flags = fcntl(handle.fileDescriptor, F_GETFL)
    guard flags >= 0, fcntl(handle.fileDescriptor, F_SETFL, flags | O_NONBLOCK) == 0 else {
      throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO)
    }
    handle.readabilityHandler = { [self] handle in
      var buffer = [UInt8](repeating: 0, count: 64 * 1024)
      let count = Darwin.read(handle.fileDescriptor, &buffer, buffer.count)
      if count > 0 {
        let bytes = Data(buffer.prefix(count))
        observe(bytes)
        do { try destination.write(contentsOf: bytes) } catch {
          failRelay("Output write failed: \(error)")
        }
      } else if count == 0 {
        handle.readabilityHandler = nil
        finish()
      } else if errno != EAGAIN && errno != EINTR {
        failRelay("Output read failed: errno \(errno)")
        handle.readabilityHandler = nil
        finish()
      }
    }
  }
}
