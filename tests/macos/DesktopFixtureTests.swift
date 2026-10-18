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
    // The real pipe must deliver a complete request while its writer remains
    // open; waiting for EOF here would deadlock an acknowledgment round trip.
    let pipe = Pipe()
    let relayed = DesktopFixtureOutput()
    try relayed.relay(from: pipe.fileHandleForReading, to: FileHandle.nullDevice)
    try pipe.fileHandleForWriting.write(contentsOf: Data("EML_OWNED_COPY_HOLD\n".utf8))
    var received: DesktopFixtureRequest?
    let requestDeadline = Date().addingTimeInterval(2)
    while received == nil && Date() < requestDeadline {
      received = relayed.take()
      Thread.sleep(forTimeInterval: 0.01)
    }
    precondition(received == .holdCopy && !relayed.hasEnded())
    try pipe.fileHandleForWriting.close()
    let eofDeadline = Date().addingTimeInterval(2)
    while !relayed.hasEnded() && Date() < eofDeadline { Thread.sleep(forTimeInterval: 0.01) }
    precondition(relayed.hasEnded() && relayed.observedRelayFailure() == nil)
    try pipe.fileHandleForReading.close()
    // Exercise the real Darwin no-replace edge and acknowledgment publication
    // using only a freshly minted temporary directory, without launching an app.
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(
      "eml-fixture-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
    defer { try? FileManager.default.removeItem(at: root) }
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
    try FileManager.default.removeItem(at: root)
    precondition(!FileManager.default.fileExists(atPath: root.path))
    try FileHandle.standardOutput.write(
      contentsOf: Data(
        "All request splits/order/rejection/EOF/error/overflow and native no-replace/acknowledgment checks passed\n"
          .utf8))
  }
}
