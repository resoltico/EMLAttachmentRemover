// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import AppKit
import Foundation

// Qualification-only input transport. The admitted parent retains the original
// representations across runner failure; neither XCTest nor product code owns it.
@MainActor
final class PanelPathClipboardLease {
  enum RestorePhase: String {
    case requested
    case runnerSettled = "runner_settled"
  }
  enum Failure: Error, Equatable {
    case activeLease, invalidSequence, invalidPath, unavailableData, snapshotLimit,
      unstableCapture, ownershipLost, writeFailed, readbackMismatch, noPreparedLease
  }
  private struct Representation: Equatable {
    let type: NSPasteboard.PasteboardType
    let data: Data
  }
  private struct Snapshot {
    let items: [[Representation]]
    let count: Int
    let types: Int
    let bytes: Int
  }
  private let board: NSPasteboard
  private var active: (original: Snapshot, index: Int, count: Int?)?
  private var operations: [[String: Any]] = []
  private var failure: String?

  init(pasteboard: NSPasteboard) { board = pasteboard }

  var hasActiveLease: Bool { active != nil }
  var observation: [String: Any] {
    let state =
      failure == nil
      ? (operations.isEmpty ? "unused" : (active == nil ? "restored" : "unverified")) : "unverified"
    return [
      "state": state, "operations": operations, "failure": failure.map { $0 as Any } ?? NSNull(),
    ]
  }
  static var unusedObservation: [String: Any] {
    ["state": "unused", "operations": [], "failure": NSNull()]
  }

  func recordFailure(_ error: Error) {
    let description = String(describing: error)
    failure = failure ?? description
    if let index = active?.index ?? operations.indices.last, operations[index]["failure"] is NSNull
    {
      operations[index]["failure"] = description
    }
  }

  private func materialize(_ items: [NSPasteboardItem]) throws -> ([[Representation]], Int, Int) {
    guard items.count <= 64 else { throw Failure.snapshotLimit }
    var result: [[Representation]] = []
    var typeCount = 0
    var bytes = 0
    for item in items {
      let types = item.types
      guard !types.isEmpty, Set(types).count == types.count else { throw Failure.unavailableData }
      var representations: [Representation] = []
      for type in types {
        let nameBytes = type.rawValue.utf8.count
        guard nameBytes > 0, nameBytes <= 256, typeCount < 256 else { throw Failure.snapshotLimit }
        // Provider materialization can block or allocate before its size is known.
        // This bounds admitted retained data, not universal provider time/peak use.
        guard let data = item.data(forType: type) else { throw Failure.unavailableData }
        guard data.count <= 1_048_576, bytes <= 1_048_576 - data.count,
          bytes + data.count <= 1_048_576 - nameBytes
        else { throw Failure.snapshotLimit }
        bytes += nameBytes + data.count
        typeCount += 1
        representations.append(Representation(type: type, data: data))
      }
      result.append(representations)
    }
    return (result, typeCount, bytes)
  }

  private func capture() throws -> Snapshot {
    let count = board.changeCount
    let items = board.pasteboardItems ?? []
    if items.isEmpty, !(board.types ?? []).isEmpty { throw Failure.unavailableData }
    let (values, types, bytes) = try materialize(items)
    guard board.changeCount == count else { throw Failure.unstableCapture }
    return Snapshot(items: values, count: count, types: types, bytes: bytes)
  }

  func prepare(path: String, sequence: Int) throws {
    do {
      guard active == nil else { throw Failure.activeLease }
      guard sequence == operations.count + 1, sequence <= 32 else { throw Failure.invalidSequence }
      guard !path.isEmpty, path.utf8.count <= 4096, !path.utf8.contains(0),
        (path as NSString).isAbsolutePath
      else { throw Failure.invalidPath }
      let item = NSPasteboardItem()
      guard item.setString(path, forType: .string) else { throw Failure.writeFailed }
      let expected = try materialize([item]).0
      let original = try capture()
      operations.append([
        "sequence": sequence, "original_items": original.items.count,
        "original_types": original.types,
        "original_bytes": original.bytes, "capture_change_count": original.count,
        "clear_change_count": NSNull(), "prepared_change_count": NSNull(),
        "restore_change_count": NSNull(), "restore_phase": NSNull(), "state": "unverified",
        "failure": NSNull(),
      ])
      active = (original, operations.count - 1, nil)
      guard board.changeCount == original.count else { throw Failure.ownershipLost }
      // NSPasteboard has no atomic compare-and-write. Refuse observed ownership
      // loss; this admitted disposable-host fixture cannot exclude a concurrent writer.
      let cleared = board.clearContents()
      active?.count = cleared
      operations[operations.count - 1]["clear_change_count"] = cleared
      guard board.writeObjects([item]) else { throw Failure.writeFailed }
      let prepared = try capture()
      guard prepared.items == expected else { throw Failure.readbackMismatch }
      active?.count = prepared.count
      operations[operations.count - 1]["prepared_change_count"] = prepared.count
      operations[operations.count - 1]["state"] = "prepared"
    } catch {
      recordFailure(error)
      throw error
    }
  }

  func restore(phase: RestorePhase) throws {
    guard let lease = active else { return }
    let index = lease.index
    operations[index]["restore_phase"] = phase.rawValue
    do {
      guard let count = lease.count else { throw Failure.noPreparedLease }
      guard board.changeCount == count else { throw Failure.ownershipLost }
      let restoredItems = try lease.original.items.map { representations in
        let item = NSPasteboardItem()
        for representation in representations {
          guard item.setData(representation.data, forType: representation.type) else {
            throw Failure.writeFailed
          }
        }
        return item
      }
      guard board.changeCount == count else { throw Failure.ownershipLost }
      let cleared = board.clearContents()
      active?.count = cleared
      if !restoredItems.isEmpty, !board.writeObjects(restoredItems) { throw Failure.writeFailed }
      let restored = try capture()
      guard restored.items == lease.original.items else { throw Failure.readbackMismatch }
      operations[index]["restore_change_count"] = restored.count
      operations[index]["state"] = "restored"
      active = nil
    } catch {
      operations[index]["state"] =
        (error as? Failure) == .ownershipLost ? "ownership_lost" : "unverified"
      recordFailure(error)
      throw error
    }
  }
}
