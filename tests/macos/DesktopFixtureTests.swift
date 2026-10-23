// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import AppKit
import Foundation

private final class MissingPanelPathClipboardData: NSObject, NSPasteboardItemDataProvider {
  func pasteboard(
    _ pasteboard: NSPasteboard?, item: NSPasteboardItem,
    provideDataForType type: NSPasteboard.PasteboardType
  ) {}
}

@main
struct DesktopFixtureTests {
  private static func requireBytes(_ url: URL, _ expected: Data) throws {
    let actual = try Data(contentsOf: url)
    precondition(actual == expected)
  }
  @MainActor
  static func main() throws {
    checkFailureSampleRetention()
    checkRequestDecoding()
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(
      "eml-fixture-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
    defer { try? FileManager.default.removeItem(at: root) }
    try checkFixtureFilesystem(root)
    try checkPanelPathClipboard(root)
    try checkPanelPathRequests(root)
    try checkOpenPipeRelay(root)
    try checkNoReplaceAndAcknowledgment(root)
    try FileManager.default.removeItem(at: root)
    precondition(!FileManager.default.fileExists(atPath: root.path))
    try FileHandle.standardOutput.write(
      contentsOf: Data(
        "All request, native no-replace/acknowledgment and prelaunch filesystem observation checks passed\n"
          .utf8))
  }
  @MainActor
  private static func checkPanelPathRequests(_ root: URL) throws {
    let request = root.appendingPathComponent("panel-path-request-control")
    let path = "/owned/日本語-00/one.eml"
    try Data(path.utf8).write(to: request, options: .withoutOverwriting)
    let before = try FileManager.default.attributesOfItem(atPath: request.path)
    let observedPath = try DesktopInteractionOwner.readPanelPath(request)
    precondition(observedPath == path)
    let after = try FileManager.default.attributesOfItem(atPath: request.path)
    precondition(before[.modificationDate] as? Date == after[.modificationDate] as? Date)
    let link = root.appendingPathComponent("panel-path-link-control")
    try FileManager.default.createSymbolicLink(at: link, withDestinationURL: request)
    do {
      _ = try DesktopInteractionOwner.readPanelPath(link)
      preconditionFailure("A request symlink must not be followed")
    } catch {}
    try requireBytes(request, Data(path.utf8))
    let large = root.appendingPathComponent("panel-path-large-control")
    try Data(repeating: 65, count: 4097).write(to: large, options: .withoutOverwriting)
    do {
      _ = try DesktopInteractionOwner.readPanelPath(large)
      preconditionFailure("An oversize request must refuse before clipboard preparation")
    } catch {}
    let invalid = root.appendingPathComponent("panel-path-invalid-control")
    for bytes in [Data(), Data("relative".utf8), Data([47, 0, 97])] {
      try bytes.write(to: invalid)
      do {
        _ = try DesktopInteractionOwner.readPanelPath(invalid)
        preconditionFailure("Incomplete, relative and NUL requests must be refused")
      } catch {}
      try requireBytes(invalid, bytes)
    }
  }

  @MainActor
  private static func checkPanelPathClipboard(_ root: URL) throws {
    let board = NSPasteboard.withUniqueName()
    defer { board.releaseGlobally() }
    let path = "/owned/native-panel/with-00/one.eml"
    let custom = NSPasteboard.PasteboardType("org.eml.tests.binary")
    let empty = NSPasteboard.PasteboardType("org.eml.tests.empty")
    func installOriginal() {
      board.clearContents()
      let first = NSPasteboardItem()
      precondition(first.setString("original text", forType: .string))
      precondition(first.setData(Data([0, 255, 1]), forType: custom))
      let second = NSPasteboardItem()
      precondition(second.setData(Data(), forType: empty))
      precondition(board.writeObjects([first, second]))
    }
    func requireOriginal() {
      let items = board.pasteboardItems ?? []
      precondition(items.count == 2)
      precondition(items[0].types == [.string, custom] && items[1].types == [empty])
      precondition(items[0].string(forType: .string) == "original text")
      precondition(items[0].data(forType: custom) == Data([0, 255, 1]))
      precondition(items[1].data(forType: empty) == Data())
    }
    // A truly empty board is distinct from unavailable declared data.
    board.clearContents()
    let zero = PanelPathClipboardLease(pasteboard: board)
    try zero.prepare(path: path, sequence: 1)
    precondition(board.string(forType: .string) == path)
    try zero.restore(phase: .requested)
    precondition((board.pasteboardItems ?? []).isEmpty && (board.types ?? []).isEmpty)
    precondition(zero.observation["state"] as? String == "restored")
    let zeroOperation = (zero.observation["operations"] as! [[String: Any]])[0]
    precondition(zeroOperation["original_items"] as? Int == 0)
    precondition(zeroOperation["original_types"] as? Int == 0)
    precondition(zeroOperation["original_bytes"] as? Int == 0)

    installOriginal()
    let normal = PanelPathClipboardLease(pasteboard: board)
    try normal.prepare(path: path, sequence: 1)
    try normal.restore(phase: .requested)
    requireOriginal()
    precondition(normal.observation["state"] as? String == "restored")
    // No runner cleanup callback is needed: the parent still holds the active
    // original and restores it after the runner has actually settled.
    try normal.prepare(path: path, sequence: 2)
    try normal.restore(phase: .runnerSettled)
    requireOriginal()
    let operations = normal.observation["operations"] as! [[String: Any]]
    precondition(
      operations.count == 2 && operations[1]["restore_phase"] as? String == "runner_settled")

    let changed = PanelPathClipboardLease(pasteboard: board)
    try changed.prepare(path: path, sequence: 1)
    board.clearContents()
    precondition(board.setString("newer owner", forType: .string))
    let newerCount = board.changeCount
    do {
      try changed.restore(phase: .runnerSettled)
      preconditionFailure("A newer observed pasteboard owner must not be overwritten")
    } catch PanelPathClipboardLease.Failure.ownershipLost {}
    precondition(board.changeCount == newerCount && board.string(forType: .string) == "newer owner")

    board.clearContents()
    let large = NSPasteboardItem()
    let largeBytes = Data(repeating: 7, count: 1_048_576)
    precondition(large.setData(largeBytes, forType: custom))
    precondition(board.writeObjects([large]))
    let largeCount = board.changeCount
    let refused = PanelPathClipboardLease(pasteboard: board)
    do {
      try refused.prepare(path: path, sequence: 1)
      preconditionFailure("Oversize original data must refuse before clearing")
    } catch PanelPathClipboardLease.Failure.snapshotLimit {}
    precondition(board.changeCount == largeCount && board.data(forType: custom) == largeBytes)

    board.clearContents()
    let missingProvider = MissingPanelPathClipboardData()
    let promised = NSPasteboardItem()
    precondition(promised.setDataProvider(missingProvider, forTypes: [custom]))
    precondition(board.writeObjects([promised]))
    let promisedCount = board.changeCount
    let unavailable = PanelPathClipboardLease(pasteboard: board)
    do {
      try unavailable.prepare(path: path, sequence: 1)
      preconditionFailure("Unavailable declared data must not be treated as an empty original")
    } catch PanelPathClipboardLease.Failure.unavailableData {}
    precondition(board.changeCount == promisedCount)

    // A real exclusive acknowledgment write fails after preparation. The
    // parent must still restore, and cleanup must not erase the first failure.
    installOriginal()
    let acknowledgment = root.appendingPathComponent("clipboard-existing-ready")
    try Data("sentinel".utf8).write(to: acknowledgment, options: .withoutOverwriting)
    let failedAcknowledgment = PanelPathClipboardLease(pasteboard: board)
    try failedAcknowledgment.prepare(path: path, sequence: 1)
    do {
      try Data("ready".utf8).write(to: acknowledgment, options: .withoutOverwriting)
      preconditionFailure("Acknowledgment must not replace existing evidence")
    } catch { failedAcknowledgment.recordFailure(error) }
    precondition(failedAcknowledgment.hasActiveLease)
    try failedAcknowledgment.restore(phase: .runnerSettled)
    requireOriginal()
    try requireBytes(acknowledgment, Data("sentinel".utf8))
    precondition(failedAcknowledgment.observation["state"] as? String == "unverified")
    precondition(failedAcknowledgment.observation["failure"] is String)
    print(
      "Named-pasteboard path lease: empty, complete representations, abandoned runner, newer owner, oversize and acknowledgment failure controls passed; general clipboard untouched"
    )
  }

  @MainActor
  private static func checkFixtureFilesystem(_ root: URL) throws {
    let fixture = root.appendingPathComponent("filesystem")
    try FileManager.default.createDirectory(at: fixture, withIntermediateDirectories: false)
    let lower = fixture.appendingPathComponent("one.eml")
    let bytes = Data("owned lowercase source\n".utf8)
    try bytes.write(to: lower, options: .withoutOverwriting)
    let before = try FileManager.default.attributesOfItem(atPath: lower.path)
    let rootBefore = try FileManager.default.attributesOfItem(atPath: fixture.path)
    let observed = DesktopInteractionOwner.observeFixtureFilesystem(fixture)
    precondition(observed["state"] as? String == "observed")
    precondition(observed["phase"] as? String == "prelaunch")
    precondition(observed["descriptor_closed"] as? Bool == true)
    precondition(!(observed["filesystem_type"] as? String ?? "").isEmpty)
    let relation = observed["ascii_alias_relation"] as? String
    precondition(relation == "same_identity" || relation == "absent_enoent")
    let originalEntries = try FileManager.default.contentsOfDirectory(atPath: fixture.path)
    precondition(originalEntries == ["one.eml"])
    try requireBytes(lower, bytes)
    let after = try FileManager.default.attributesOfItem(atPath: lower.path)
    let rootAfter = try FileManager.default.attributesOfItem(atPath: fixture.path)
    precondition(
      String(describing: rootBefore[.modificationDate])
        == String(describing: rootAfter[.modificationDate]))
    try FileHandle.standardOutput.write(
      contentsOf: JSONSerialization.data(withJSONObject: observed, options: [.sortedKeys]))
    try FileHandle.standardOutput.write(contentsOf: Data("\n".utf8))
    for key in [
      FileAttributeKey.systemFileNumber, .systemNumber, .posixPermissions, .size, .modificationDate,
    ] {
      precondition(String(describing: before[key]) == String(describing: after[key]))
    }
    try FileManager.default.removeItem(at: lower)
    let missing = DesktopInteractionOwner.observeFixtureFilesystem(fixture)
    precondition(missing["state"] as? String == "unavailable")
    precondition(missing["filesystem_type"] as? String == observed["filesystem_type"] as? String)
    precondition(missing["descriptor_closed"] as? Bool == true)
    precondition(!(missing["errors"] as? [[String: Any]] ?? []).isEmpty)
    let missingEntries = try FileManager.default.contentsOfDirectory(atPath: fixture.path)
    precondition(missingEntries.isEmpty)
    let receipt = root.appendingPathComponent("fixture-filesystem.json")
    try DesktopInteractionOwner.writeFixtureFilesystem(missing, to: receipt)
    let retained = try Data(contentsOf: receipt)
    let decoded = try JSONSerialization.jsonObject(with: retained) as? [String: Any]
    precondition(decoded?["state"] as? String == "unavailable")
    do {
      try DesktopInteractionOwner.writeFixtureFilesystem(observed, to: receipt)
      preconditionFailure("Prelaunch telemetry must not replace earlier evidence")
    } catch {}
    try requireBytes(receipt, retained)
    let link = root.appendingPathComponent("filesystem-link")
    try FileManager.default.createSymbolicLink(at: link, withDestinationURL: fixture)
    let linked = DesktopInteractionOwner.observeFixtureFilesystem(link)
    precondition(linked["state"] as? String == "unavailable")
    precondition(linked["filesystem_type"] is NSNull)
    precondition(!(linked["errors"] as? [[String: Any]] ?? []).isEmpty)
  }

  @MainActor
  private static func checkFailureSampleRetention() {
    let expected = DesktopInteractionOwner.SampleTarget(
      pid: 42, launchDate: Date(timeIntervalSince1970: 123), bundlePath: "/owned/Fixture.app")
    func accepted(
      _ after: DesktopInteractionOwner.SampleTarget?, settled: Bool = true, bytes: UInt64 = 1
    ) -> Bool {
      DesktopInteractionOwner.mayRetainFailureSample(
        expected: expected, before: expected, after: after, settled: settled, bytes: bytes)
    }
    precondition(accepted(expected, bytes: 5 * 1024 * 1024))
    precondition(!accepted(nil))
    precondition(!accepted(expected, settled: false))
    precondition(!accepted(expected, bytes: 0))
    precondition(!accepted(expected, bytes: 5 * 1024 * 1024 + 1))
    for changed in [
      DesktopInteractionOwner.SampleTarget(
        pid: 43, launchDate: expected.launchDate, bundlePath: expected.bundlePath),
      DesktopInteractionOwner.SampleTarget(
        pid: expected.pid, launchDate: Date(timeIntervalSince1970: 124),
        bundlePath: expected.bundlePath),
      DesktopInteractionOwner.SampleTarget(
        pid: expected.pid, launchDate: expected.launchDate, bundlePath: "/other/Fixture.app"),
    ] { precondition(!accepted(changed)) }
    precondition(
      !DesktopInteractionOwner.mayRetainFailureSample(
        expected: expected, before: nil, after: expected, settled: true, bytes: 1))
  }

  private static func checkRequestDecoding() {
    for request in DesktopFixtureRequest.allCases {
      let bytes = Data("\(request.rawValue)\n".utf8)
      for split in 0...bytes.count {
        let output = DesktopFixtureOutput()
        precondition(output.take() == nil)
        output.observe(bytes.prefix(split))
        let first = output.take()
        output.observe(bytes.suffix(bytes.count - split))
        let second = output.take()
        precondition([first, second].compactMap { $0 } == [request], "Lost/replayed split request")
        precondition(output.take() == nil)
      }
    }
    let incidental = DesktopFixtureOutput()
    incidental.observe(Data("prefix EML_OWNED_COPY_HOLD suffix\n".utf8))
    precondition(incidental.take() == nil)
    incidental.observe(Data("EML_OWNED_COPY_HOLD".utf8))
    incidental.finish()
    precondition(incidental.take() == nil)
    let oversized = DesktopFixtureOutput()
    oversized.observe(
      Data(
        (String(repeating: "x", count: 4097) + "EML_OWNED_COPY_HOLD\nEML_OWNED_COPY_RESTORE\n").utf8
      ))
    precondition(oversized.take() == .restoreCopy && oversized.take() == nil)
    let ordered = DesktopFixtureOutput()
    ordered.observe(Data("EML_OWNED_COPY_HOLD\nEML_OWNED_COPY_RESTORE\n".utf8))
    precondition(
      ordered.take() == .holdCopy && ordered.take() == .restoreCopy && ordered.take() == nil)
    let rejected = DesktopFixtureOutput()
    rejected.observe(Data("EML_NATIVE_PICKER_REQUEST suffix".utf8))
    precondition(rejected.take() == nil && !rejected.hasEnded())
    rejected.failRelay("first read failure")
    rejected.failRelay("later write failure")
    rejected.finish()
    precondition(!rejected.observedEOF(), "An error-finished stream is not successful EOF")
    precondition(rejected.hasEnded() && rejected.observedRelayFailure() == "first read failure")
    let overflow = DesktopFixtureOutput()
    overflow.observe(Data(String(repeating: "EML_OWNED_COPY_HOLD\n", count: 9).utf8))
    precondition(overflow.observedRelayFailure() == "Owned fixture request queue overflow")
  }
  private static func checkOpenPipeRelay(_ root: URL) throws {
    // The real pipe must deliver a complete request while its writer remains
    // open and forward the exact bytes to an ordinary output handle. Waiting
    // for EOF here would deadlock an acknowledgment round trip.
    let pipe = Pipe()
    let relayed = DesktopFixtureOutput()
    let forwarded = root.appendingPathComponent("forwarded-output")
    try Data().write(to: forwarded, options: .withoutOverwriting)
    let destinationHandle = try FileHandle(forWritingTo: forwarded)
    defer { try? destinationHandle.close() }
    let requestBytes = Data("EML_OWNED_COPY_HOLD\n".utf8)
    try relayed.relay(from: pipe.fileHandleForReading, to: destinationHandle)
    try pipe.fileHandleForWriting.write(contentsOf: requestBytes)
    var received: DesktopFixtureRequest?
    let requestDeadline = Date().addingTimeInterval(2)
    while received == nil && Date() < requestDeadline {
      received = relayed.take()
      Thread.sleep(forTimeInterval: 0.01)
    }
    precondition(
      received == .holdCopy && !relayed.hasEnded(),
      "Open pipe request: received=\(String(describing: received)), ended=\(relayed.hasEnded()), relay failure=\(String(describing: relayed.observedRelayFailure()))"
    )
    try pipe.fileHandleForWriting.close()
    let eofDeadline = Date().addingTimeInterval(2)
    while !relayed.hasEnded() && Date() < eofDeadline { Thread.sleep(forTimeInterval: 0.01) }
    precondition(
      relayed.observedEOF() && relayed.observedRelayFailure() == nil,
      "Closed pipe EOF: ended=\(relayed.hasEnded()), relay failure=\(String(describing: relayed.observedRelayFailure()))"
    )
    try requireBytes(forwarded, requestBytes)
    try destinationHandle.close()
    try pipe.fileHandleForReading.close()
  }
  @MainActor
  private static func checkNoReplaceAndAcknowledgment(_ root: URL) throws {
    // Exercise the real Darwin no-replace edge and acknowledgment publication
    // using only a freshly minted temporary directory, without launching an app.
    let source = root.appendingPathComponent("source")
    let destination = root.appendingPathComponent("held")
    let complete = Data("complete synthetic copy\n".utf8)
    try complete.write(to: source, options: .withoutOverwriting)
    try DesktopInteractionOwner.moveWithoutReplacement(from: source, to: destination)
    precondition(!FileManager.default.fileExists(atPath: source.path))
    try requireBytes(destination, complete)
    try DesktopInteractionOwner.moveWithoutReplacement(from: destination, to: source)
    let sentinel = Data("unexpected existing entry\n".utf8)
    try sentinel.write(to: destination, options: .withoutOverwriting)
    var refused = false
    do { try DesktopInteractionOwner.moveWithoutReplacement(from: source, to: destination) } catch {
      refused = true
    }
    precondition(refused)
    try requireBytes(source, complete)
    try requireBytes(destination, sentinel)
    let acknowledgment = root.appendingPathComponent("acknowledgment")
    try DesktopInteractionOwner.acknowledge("complete acknowledgment\n", at: acknowledgment)
    try requireBytes(acknowledgment, Data("complete acknowledgment\n".utf8))
    refused = false
    do { try DesktopInteractionOwner.acknowledge("replacement\n", at: acknowledgment) } catch {
      refused = true
    }
    precondition(refused)
    try requireBytes(acknowledgment, Data("complete acknowledgment\n".utf8))
  }

}
