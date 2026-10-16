import Foundation

public enum ProtocolFailure: Error, Equatable, Sendable {
  case frameLimit
  case runLimit
  case malformedEvent
  case invalidOrder
  case invalidCounts
  case invalidReceipt
  case incompatibleHelper
  case missingSummary
  case exitDisagreement
}

/// Byte framing only. A partial EOF tail is discarded, never decoded as a record.
/// A callback consumes each frame before the next one is assembled; callers must
/// apply backpressure instead of retaining an unbounded queue of frame copies.
public struct LFFrameSplitter: Sendable {
  public static let maximumFrameBytes = 64 * 1024 * 1024  // Includes LF.
  public static let maximumRunBytes = 256 * 1024 * 1024  // Strict upper bound.
  private var pending = Data()
  private var received = 0
  private var failed = false

  public init() {}

  public mutating func append(
    _ bytes: Data, consume: (Data) throws -> Void
  ) throws {
    guard !failed else { throw ProtocolFailure.malformedEvent }
    do {
      guard bytes.count < Self.maximumRunBytes - received else {
        throw ProtocolFailure.runLimit
      }
      received += bytes.count
      var cursor = bytes.startIndex
      while cursor < bytes.endIndex {
        let newline = bytes[cursor...].firstIndex(of: 0x0A)
        let end = newline ?? bytes.endIndex
        let count = bytes.distance(from: cursor, to: end)
        // Reserving LF here also rejects an unterminated oversized tail.
        guard count < Self.maximumFrameBytes - pending.count else {
          throw ProtocolFailure.frameLimit
        }
        pending.append(contentsOf: bytes[cursor..<end])
        guard let newline else { break }
        guard !pending.isEmpty else { throw ProtocolFailure.malformedEvent }
        try consume(pending)
        pending.removeAll(keepingCapacity: true)
        cursor = bytes.index(after: newline)
      }
    } catch {
      failed = true
      pending.removeAll(keepingCapacity: false)
      throw error
    }
  }

  /// Returns whether an incomplete tail was discarded, for report diagnostics.
  @discardableResult
  public mutating func finish() -> Bool {
    let partial = !pending.isEmpty
    pending.removeAll(keepingCapacity: false)
    return partial
  }
}
