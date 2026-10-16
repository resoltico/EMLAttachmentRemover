import Foundation
import Synchronization

struct HelperIntent: Sendable {
  let stops: Int
  let inputMayHaveBeenSent: Bool
}

/// One request's monotonic admission intent. Locks cover only tiny state changes,
/// never spawn, pipe I/O, actor calls, callbacks or awaits.
final class HelperRunControl: Sendable {
  let id: UUID
  private struct State: Sendable {
    var stops = 0
    var feeding = false
    var snapshot: HelperIntent { HelperIntent(stops: stops, inputMayHaveBeenSent: feeding) }
  }
  private let state = Mutex(State())

  init(id: UUID) { self.id = id }

  @discardableResult
  func requestStop() -> HelperIntent {
    state.withLock { value in
      value.stops = min(value.stops + 1, 2)
      return value.snapshot
    }
  }

  func snapshot() -> HelperIntent { state.withLock { $0.snapshot } }

  @discardableResult
  func requestCooperativeStop() -> HelperIntent {
    state.withLock { value in
      value.stops = max(value.stops, 1)
      return value.snapshot
    }
  }

  @discardableResult
  func requestForceStop() -> HelperIntent {
    state.withLock { value in
      value.stops = 2
      return value.snapshot
    }
  }

  func beginFeeding() -> Bool {
    state.withLock { value in
      guard value.stops == 0, !value.feeding else { return false }
      value.feeding = true
      return true
    }
  }
}
