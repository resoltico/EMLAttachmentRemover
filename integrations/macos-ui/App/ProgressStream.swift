import Darwin
import Foundation

struct NativeProgress: Decodable, Sendable {
  enum Stage: String, Decodable, Sendable { case processing, reporting }
  let schema: Int
  let stage: Stage
  let completed: Int
  let total: Int
}

enum ProcessingUpdate: Sendable {
  case progress(NativeProgress)
  case exited
}

struct ProgressStream {
  static func read(_ handle: FileHandle, received: @Sendable (NativeProgress) -> Void) -> Data {
    var stream = ProgressStream()
    var buffer = [UInt8](repeating: 0, count: 4096)
    while true {
      let count = buffer.withUnsafeMutableBytes {
        Darwin.read(handle.fileDescriptor, $0.baseAddress, $0.count)
      }
      if count > 0 {
        for progress in stream.append(Data(buffer.prefix(count))) { received(progress) }
      } else if count == 0 {
        break
      } else if errno != EINTR {
        stream.retain(Data("The progress/diagnostic channel could not be read.".utf8))
        break
      }
    }
    return stream.finish()
  }
  private var pending = Data()
  private var oversized = false
  private var last: NativeProgress?
  private(set) var diagnostics = Data()
  private let prefix = Data("EML_PROGRESS ".utf8)
  private let recordLimit = 1024
  private let diagnosticLimit = 1024 * 1024

  mutating func append(_ bytes: Data) -> [NativeProgress] {
    var updates: [NativeProgress] = []
    for byte in bytes {
      if byte == 10 {
        if !oversized, let update = admit(pending) { updates.append(update) }
        if oversized && !pending.starts(with: prefix) { retain(pending) }
        pending.removeAll(keepingCapacity: true)
        oversized = false
      } else if pending.count < recordLimit {
        pending.append(byte)
      } else {
        oversized = true
      }
    }
    return updates
  }
  mutating func finish() -> Data {
    if !pending.isEmpty && !pending.starts(with: prefix) { retain(pending) }
    return diagnostics
  }
  private mutating func admit(_ line: Data) -> NativeProgress? {
    guard line.starts(with: prefix) else {
      retain(line)
      return nil
    }
    guard String(data: line.dropFirst(prefix.count), encoding: .utf8) != nil else { return nil }
    let payload = line.dropFirst(prefix.count)
    guard hasValidJSONSyntax(payload) else { return nil }
    guard
      let update = try? JSONDecoder().decode(
        NativeProgress.self, from: payload),
      update.schema == 1, (1...4096).contains(update.total),
      (0...update.total).contains(update.completed)
    else { return nil }
    if let last {
      guard update.total == last.total, update.completed >= last.completed,
        last.stage != .reporting || update.stage == .reporting
      else { return nil }
    }
    last = update
    return update
  }
  private mutating func retain(_ line: Data) {
    guard diagnostics.count < diagnosticLimit else { return }
    diagnostics.append(line.prefix(diagnosticLimit - diagnostics.count))
    if diagnostics.count < diagnosticLimit { diagnostics.append(10) }
  }
}
