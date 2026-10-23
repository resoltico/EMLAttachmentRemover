// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import Foundation

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
      relayed.hasEnded() && relayed.observedRelayFailure() == nil,
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
