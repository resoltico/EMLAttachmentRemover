// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import AppKit
import Darwin
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
  static func main() async throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(
      "eml-fixture-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
    print("Owned fixture API root: \(root.path)")
    var actorFacts = [[String: Any]]()
    var exportFacts = [String: Any]()
    var refusalFacts = [[String: Any]]()
    var eofFacts = [String: Any]()
    let checks: [(name: String, run: @MainActor () async throws -> Void)] = [
      ("failure-sample-retention", { checkFailureSampleRetention() }),
      ("request-decoding", { try checkRequestDecoding() }),
      ("fixture-filesystem", { try checkFixtureFilesystem(root) }),
      ("named-pasteboard-path-lease", { try checkPanelPathClipboard(root) }),
      ("open-pipe-relay", { try checkOpenPipeRelay(root) }),
      ("native-no-replace-and-ack", { try checkNoReplaceAndAcknowledgment(root) }),
      ("actor-wire", { try checkActorWire() }),
      ("parent-actor-transport-and-refusals", { actorFacts = try await checkFixtureActors(root) }),
      ("export-actor-sequence", { exportFacts = try checkExportActors(root) }),
      ("closed-actor-refusals", { refusalFacts = try checkActorRefusals(root) }),
      ("actual-early-eof", { eofFacts = try checkActualActorEOF(root) }),
    ]
    let discovered = checks.map(\.name)
    try FileHandle.standardOutput.write(
      contentsOf: Data(
        ("EML_FIXTURE_API_DISCOVERY " + (try fixtureCanonicalJSON(discovered)).base64EncodedString()
          + "\n").utf8))
    var executed = [String]()
    for check in checks {
      try FileHandle.standardOutput.write(
        contentsOf: Data(("EML_FIXTURE_API_BEGIN " + check.name + "\n").utf8))
      do { try await check.run() } catch {
        let partial: [String: Any] = [
          "scope": "All standalone owner/API functional checks", "discovered": discovered,
          "executed": executed, "failed": [check.name], "skipped": [],
          "not_executed": Array(discovered.dropFirst(executed.count + 1)),
          "error": DesktopFixtureActors.error(error),
        ]
        try FileHandle.standardOutput.write(
          contentsOf: Data(
            ("EML_FIXTURE_API_FAILURE " + (try fixtureCanonicalJSON(partial)).base64EncodedString()
              + "\n").utf8))
        throw error
      }
      executed.append(check.name)
      try FileHandle.standardOutput.write(
        contentsOf: Data(("EML_FIXTURE_API_EXECUTED " + check.name + "\n").utf8))
    }
    let apiFacts: [String: Any] = [
      "checks": [
        "scope": "All standalone owner/API functional checks", "discovered": discovered,
        "executed": executed, "failed": [], "skipped": [], "not_executed": [],
      ],
      "actor_fixtures": actorFacts, "export_fixture": exportFacts, "refusals": refusalFacts,
      "early_eof": eofFacts,
    ]
    try FileHandle.standardOutput.write(
      contentsOf: Data(
        ("EML_FIXTURE_API_RESULT " + (try fixtureCanonicalJSON(apiFacts)).base64EncodedString()
          + "\n").utf8))
    try FileManager.default.removeItem(at: root)
    precondition(!FileManager.default.fileExists(atPath: root.path))
    try FileHandle.standardOutput.write(
      contentsOf: Data("All owner/API checks completed; owned fixture root removed\n".utf8))
  }
  @MainActor
  private static func checkActorWire() throws {
    let actors: [DesktopFixtureActor] =
      [
        .occupyCheckedDestination, .occupyExport, .createExportDirectory, .createExportSymlink,
        .release(operationID: 1), .archive(history: .racingWinner, operationID: 2),
        .retainIndependentReport(Data("{\"opaque\":true}".utf8)),
        .retainChildObservation(Data("{\"case\":\"export-publication\"}".utf8)),
        .raceExportWinner(operationID: UInt64.max),
      ] + DesktopOperationRecipe.allCases.map { .arm($0) }
    for actor in actors {
      let request = DesktopFixtureRequest.fixtureActor(sequence: 1, actor: actor)
      let bytes = Data((try request.wireLine + "\n").utf8)
      for split in [0, 1, bytes.count / 2, bytes.count - 1, bytes.count] {
        let output = DesktopFixtureOutput()
        output.observe(bytes.prefix(split))
        output.observe(bytes.suffix(bytes.count - split))
        precondition(
          output.take() == request && output.take() == nil && output.observedRelayFailure() == nil)
      }
    }
    for line in [
      "EML_FIXTURE_ACTOR", "EML_FIXTURE_ACTOR 01 occupy-export -",
      "EML_FIXTURE_ACTOR 0 occupy-export -", "EML_FIXTURE_ACTOR 33 occupy-export -",
      "EML_FIXTURE_ACTOR 1 write /outside", "EML_FIXTURE_ACTOR 1 occupy-export ../outside",
      "EML_FIXTURE_ACTOR 1 release-operation 0", "EML_FIXTURE_ACTOR 1 release-operation 01",
      "EML_FIXTURE_ACTOR 1 archive-control unknown:1",
      "EML_FIXTURE_ACTOR 1 arm-operation export:unknown",
      "EML_FIXTURE_ACTOR 1 retain-child-observation e30= ",
      "EML_FIXTURE_ACTOR 1 retain-independent-report /w==",
    ] {
      let output = DesktopFixtureOutput()
      output.observe(Data((line + "\n").utf8))
      precondition(output.take() == nil && output.observedRelayFailure() != nil)
    }
    let incomplete = DesktopFixtureOutput()
    incomplete.observe(Data("EML_FIXTURE_ACTOR 1 occupy-export -".utf8))
    incomplete.finish()
    precondition(!incomplete.observedEOF() && incomplete.take() == nil)
    let large = DesktopFixtureOutput()
    large.observe(
      Data(
        (DesktopFixtureActor.prefix
          + String(repeating: "x", count: DesktopFixtureActor.maximumLineBytes) + "\n").utf8))
    precondition(large.observedRelayFailure() != nil)
    do {
      _ = try DesktopFixtureActor.retainIndependentReport(Data(repeating: 97, count: 65537)).line(
        sequence: 1)
      preconditionFailure("Oversize actor payload admitted")
    } catch {}
    let huge = String(repeating: "a\u{301}", count: 3000)
    let bounded = DesktopFixtureActors.error(
      NSError(domain: huge, code: 7, userInfo: [NSLocalizedDescriptionKey: huge]))
    precondition(
      (bounded["domain"] as! String).utf8.count <= 256
        && (bounded["message"] as! String).utf8.count <= 1024)
    let fd = Darwin.open("/dev/null", O_RDONLY | O_CLOEXEC)
    precondition(fd >= 0)
    do {
      _ = try DesktopFixtureActors.checkedDescriptor(fd) { native in
        precondition(Darwin.close(native) == 0)
        return true
      }
      preconditionFailure("Unchecked descriptor close")
    } catch { precondition((error as NSError).code == Int(EBADF)) }
  }
  @MainActor
  private static func actorFixture(_ root: URL, _ name: String, scenario: String) throws -> (
    URL, DesktopFixtureActors, [String: Any]
  ) {
    let fixture = root.appendingPathComponent(name)
    try FileManager.default.createDirectory(
      at: fixture, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
    let repository = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
      .deletingLastPathComponent().deletingLastPathComponent()
    let bytes = try Data(
      contentsOf: repository.appendingPathComponent(
        "tests/contract/mime/inputs/01-gmail-attachment.eml"))
    for name in ["one.eml", "two.eml"] {
      try bytes.write(to: fixture.appendingPathComponent(name), options: .withoutOverwriting)
    }
    for name in ["chosen-destination", "operation-control", "control-history"] {
      try FileManager.default.createDirectory(
        at: fixture.appendingPathComponent(name), withIntermediateDirectories: false,
        attributes: [.posixPermissions: 0o700])
    }
    let filesystem = DesktopInteractionOwner.observeFixtureFilesystem(fixture)
    let actor = try DesktopFixtureActors(
      fixture: fixture, scenario: scenario,
      originals: [(fixture.appendingPathComponent("two.eml"), bytes)],
      filesystemRoot: filesystem["root"] as! [String: Any])
    return (fixture, actor, filesystem)
  }
  @MainActor
  private static func checkFixtureActors(_ root: URL) async throws -> [[String: Any]] {
    var observations = [[String: Any]]()
    let (fixture, actor, filesystem) = try actorFixture(
      root, "actor-transport", scenario: "workflow")
    let forwarded = fixture.appendingPathComponent("forwarded.log")
    try Data().write(to: forwarded, options: .withoutOverwriting)
    let destination = try FileHandle(forWritingTo: forwarded)
    let pipe = Pipe()
    let output = DesktopFixtureOutput()
    try output.relay(from: pipe.fileHandleForReading, to: destination)
    let saved = dup(STDOUT_FILENO)
    precondition(saved >= 0)
    precondition(dup2(pipe.fileHandleForWriting.fileDescriptor, STDOUT_FILENO) == STDOUT_FILENO)
    let monitor = Task { @MainActor in
      let deadline = ProcessInfo.processInfo.systemUptime + 5
      while ProcessInfo.processInfo.systemUptime < deadline {
        if let request = output.take() {
          try actor.handle(request, mayStart: !output.hasEnded())
          return
        }
        try await Task.sleep(for: .milliseconds(5))
      }
      throw DesktopFixtureActorClient.Failure.deadline
    }
    do {
      try await DesktopFixtureActorClient(fixture: fixture, scenario: "workflow").perform(
        .occupyCheckedDestination)
      try await monitor.value
    } catch {
      monitor.cancel()
      _ = try? await monitor.value
      _ = dup2(saved, STDOUT_FILENO)
      _ = Darwin.close(saved)
      try? pipe.fileHandleForWriting.close()
      throw error
    }
    precondition(dup2(saved, STDOUT_FILENO) == STDOUT_FILENO)
    precondition(Darwin.close(saved) == 0)
    try pipe.fileHandleForWriting.close()
    let drain = ProcessInfo.processInfo.systemUptime + 2
    while !output.hasEnded() && ProcessInfo.processInfo.systemUptime < drain {
      try await Task.sleep(for: .milliseconds(5))
    }
    precondition(output.observedEOF())
    try pipe.fileHandleForReading.close()
    try destination.close()
    let target = expectedCopy(
      for: fixture.appendingPathComponent("two.eml"),
      directory: fixture.appendingPathComponent("chosen-destination"))
    try requireBytes(target, DesktopFixtureActors.checkedSentinel)
    let acknowledgment = try Data(
      contentsOf: fixture.appendingPathComponent("fixture-actor-1.json"))
    let object = try fixtureCanonicalObject(acknowledgment)
    let request = DesktopFixtureActor.occupyCheckedDestination
    let hash =
      fixtureBytesIdentity(Data((try request.line(sequence: 1) + "\n").utf8))["sha256"] as! String
    let boundRoot = try fixtureCanonicalJSON(object["root"]!)
    for (key, value) in [
      ("schema", true as Any), ("sequence", true as Any), ("root_address_verified", 1 as Any),
      ("scenario", "picker" as Any), ("request_sha256", String(repeating: "0", count: 64) as Any),
    ] {
      var bad = object
      bad[key] = value
      do {
        try DesktopFixtureActorClient.validateAcknowledgment(
          try fixtureCanonicalJSON(bad), sequence: 1, actor: request, scenario: "workflow",
          requestHash: hash, root: boundRoot)
        preconditionFailure("Invalid ACK accepted")
      } catch {}
    }
    let duplicate = Data(
      (String(decoding: acknowledgment, as: UTF8.self).dropLast() + ",\"schema\":1}").utf8)
    do {
      try DesktopFixtureActorClient.validateAcknowledgment(
        duplicate, sequence: 1, actor: request, scenario: "workflow", requestHash: hash,
        root: boundRoot)
      preconditionFailure("Duplicate ACK accepted")
    } catch {}
    var wrongRoot = object
    var native = object["root"] as! [String: Any]
    native["inode"] = 1
    wrongRoot["root"] = native
    do {
      try DesktopFixtureActorClient.validateAcknowledgment(
        try fixtureCanonicalJSON(wrongRoot), sequence: 1, actor: request, scenario: "workflow",
        requestHash: hash, root: boundRoot)
      preconditionFailure("Wrong-root ACK accepted")
    } catch {}
    try actor.finish()
    precondition(actor.observation["state"] as? String == "complete")
    observations.append([
      "case": "actual-pipe-client-owner-ack", "fixture_filesystem": filesystem,
      "actor": actor.observation,
    ])

    let (occupied, denied, occupiedFS) = try actorFixture(
      root, "actor-winner", scenario: "workflow")
    let winner = expectedCopy(
      for: occupied.appendingPathComponent("two.eml"),
      directory: occupied.appendingPathComponent("chosen-destination"))
    try Data("unrelated winner".utf8).write(to: winner, options: .withoutOverwriting)
    do {
      try denied.handle(
        .fixtureActor(sequence: 1, actor: .occupyCheckedDestination), mayStart: true)
      preconditionFailure("Existing winner replaced")
    } catch { precondition((error as NSError).code == Int(EEXIST)) }
    try requireBytes(winner, Data("unrelated winner".utf8))
    try denied.finish()
    precondition(denied.observation["state"] as? String == "failed")
    observations.append([
      "case": "actual-no-replace-winner", "fixture_filesystem": occupiedFS,
      "actor": denied.observation,
    ])

    let (rebound, refused, reboundFS) = try actorFixture(
      root, "actor-rebound", scenario: "workflow")
    let held = root.appendingPathComponent("actor-rebound-held")
    try FileManager.default.moveItem(at: rebound, to: held)
    try FileManager.default.createDirectory(
      at: rebound, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
    try Data("replacement".utf8).write(
      to: rebound.appendingPathComponent("sentinel"), options: .withoutOverwriting)
    do {
      try refused.handle(
        .fixtureActor(sequence: 1, actor: .occupyCheckedDestination), mayStart: true)
      preconditionFailure("Rebound root adopted")
    } catch {}
    do {
      try refused.finish()
      preconditionFailure("Rebound root reported verified")
    } catch {}
    let replacementEntries = try FileManager.default.contentsOfDirectory(atPath: rebound.path)
    precondition(replacementEntries == ["sentinel"])
    observations.append([
      "case": "actual-root-substitution-no-ack", "fixture_filesystem": reboundFS,
      "actor": refused.observation,
    ])
    let (ackRoot, ackActor, ackFS) = try actorFixture(
      root, "actor-ack-winner", scenario: "workflow")
    let occupiedAck = ackRoot.appendingPathComponent("fixture-actor-1.json")
    try Data("unrelated ACK winner".utf8).write(to: occupiedAck, options: .withoutOverwriting)
    do {
      try ackActor.handle(
        .fixtureActor(sequence: 1, actor: .occupyCheckedDestination), mayStart: true)
      preconditionFailure("ACK winner replaced")
    } catch { precondition((error as NSError).code == Int(EEXIST)) }
    try requireBytes(occupiedAck, Data("unrelated ACK winner".utf8))
    try requireBytes(
      expectedCopy(
        for: ackRoot.appendingPathComponent("two.eml"),
        directory: ackRoot.appendingPathComponent("chosen-destination")),
      DesktopFixtureActors.checkedSentinel)
    try ackActor.finish()
    let ackRows = ackActor.observation["records"] as! [[String: Any]]
    precondition(ackRows.count == 1 && ackRows[0]["ack_published"] as? Bool == false)
    let completedData = try fixtureCanonicalObject(Data((ackRows[0]["document"] as! String).utf8))
    precondition(
      completedData["state"] as? String == "succeeded"
        && completedData["effect"] as? String == "created")
    observations.append([
      "case": "actual-created-data-but-ACK-winner-preserved", "fixture_filesystem": ackFS,
      "actor": ackActor.observation,
    ])
    let (ackRebound, ackRefused, ackReboundFS) = try actorFixture(
      root, "actor-ack-rebound", scenario: "workflow")
    try ackRefused.handle(
      .fixtureActor(sequence: 1, actor: .occupyCheckedDestination), mayStart: true)
    let prior = try Data(contentsOf: ackRebound.appendingPathComponent("fixture-actor-1.json"))
    try FileManager.default.moveItem(
      at: ackRebound, to: root.appendingPathComponent("actor-ack-rebound-held"))
    try FileManager.default.createDirectory(
      at: ackRebound, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
    try Data("replacement".utf8).write(
      to: ackRebound.appendingPathComponent("sentinel"), options: .withoutOverwriting)
    let refusedAck = ackRefused.publishAcknowledgment(prior, sequence: 2)
    precondition(
      !refusedAck.published && refusedAck.error != nil,
      "Prior publication is not an ACK publication witness")
    do {
      try ackRefused.finish()
      preconditionFailure("ACK root substitution was accepted")
    } catch {}
    let replacement = try FileManager.default.contentsOfDirectory(atPath: ackRebound.path)
    precondition(replacement == ["sentinel"])
    observations.append([
      "case": "actual-root-refusal-after-earlier-success-has-no-ACK-edge",
      "fixture_filesystem": ackReboundFS, "actor": ackRefused.observation,
    ])
    return observations
  }
  @MainActor
  private static func checkExportActors(_ root: URL) throws -> [String: Any] {
    let (fixture, actor, filesystem) = try actorFixture(
      root, "actor-export", scenario: "export-publication")
    var sequence = 0
    func perform(_ action: DesktopFixtureActor) throws {
      sequence += 1
      try actor.handle(.fixtureActor(sequence: sequence, actor: action), mayStart: true)
    }
    try perform(.retainIndependentReport(Data("{\"independent\":true}".utf8)))
    try perform(.occupyExport)
    try perform(.createExportDirectory)
    try perform(.createExportSymlink)
    let linkTarget = try FileManager.default.destinationOfSymbolicLink(
      atPath: fixture.appendingPathComponent("symlink-report.json").path)
    precondition(linkTarget == fixture.appendingPathComponent("occupied-report.json").path)
    for (index, history) in DesktopControlHistory.allCases.enumerated() {
      let recipe: DesktopOperationRecipe = index == 2 ? .exportNamespaceSync : .exportPublication
      try perform(.arm(recipe))
      let process = Process()
      process.executableURL = URL(fileURLWithPath: "/usr/bin/true")
      try process.run()
      let pid = process.processIdentifier
      let deadline = ProcessInfo.processInfo.systemUptime + 5
      while process.isRunning && ProcessInfo.processInfo.systemUptime < deadline {
        Thread.sleep(forTimeInterval: 0.005)
      }
      if process.isRunning { process.terminate() }
      let cleanup = ProcessInfo.processInfo.systemUptime + 1
      while process.isRunning && ProcessInfo.processInfo.systemUptime < cleanup {
        Thread.sleep(forTimeInterval: 0.005)
      }
      precondition(
        !process.isRunning, "Owned API child did not settle; outer controller must retain failure")
      process.waitUntilExit()
      precondition(process.terminationStatus == 0)
      let control = fixture.appendingPathComponent("operation-control")
      let phase = index == 2 ? "namespace-sync" : "publication"
      try DesktopInteractionOwner.acknowledge(
        "operation=export phase=\(phase) index=none operation_id=\(index + 1)\n",
        at: control.appendingPathComponent("ready"))
      try DesktopInteractionOwner.acknowledge(
        "\(pid)\n", at: control.appendingPathComponent("worker-pid"))
      if index == 0 { try perform(.raceExportWinner(operationID: 1)) }
      try perform(.release(operationID: UInt64(index + 1)))
      try perform(.archive(history: history, operationID: UInt64(index + 1)))
    }
    try perform(.arm(.exportAfterPublication))
    try perform(.retainChildObservation(try fixtureCanonicalJSON(["case": "export-publication"])))
    try actor.finish()
    precondition(sequence == 16 && actor.observation["state"] as? String == "complete")
    let rows = actor.observation["records"] as! [[String: Any]]
    precondition(rows.count == 16)
    precondition(
      rows.allSatisfy { $0["ack_published"] as? Bool == true && $0["ack_error"] is NSNull })
    return [
      "case": "controlled-markers-real-files-and-reaped-pids", "fixture_filesystem": filesystem,
      "actor": actor.observation,
    ]
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

    installOriginal()
    let outOfOrder = PanelPathClipboardLease(pasteboard: board)
    let beforeOutOfOrder = board.changeCount
    do {
      try outOfOrder.prepare(path: path, sequence: 2)
      preconditionFailure("An out-of-order prepare must not capture or clear the board")
    } catch PanelPathClipboardLease.Failure.invalidSequence {}
    precondition(board.changeCount == beforeOutOfOrder && !outOfOrder.hasActiveLease)
    requireOriginal()

    let duplicate = PanelPathClipboardLease(pasteboard: board)
    try duplicate.prepare(path: path, sequence: 1)
    let beforeDuplicate = board.changeCount
    do {
      try duplicate.prepare(path: "/owned/other.eml", sequence: 1)
      preconditionFailure("An active duplicate must not replace the prepared path")
    } catch PanelPathClipboardLease.Failure.activeLease {}
    precondition(board.changeCount == beforeDuplicate && board.string(forType: .string) == path)
    try duplicate.restore(phase: .runnerSettled)
    requireOriginal()

    let replay = PanelPathClipboardLease(pasteboard: board)
    try replay.prepare(path: path, sequence: 1)
    try replay.restore(phase: .requested)
    let beforeReplay = board.changeCount
    do {
      try replay.prepare(path: path, sequence: 1)
      preconditionFailure("A restored sequence must not be replayed")
    } catch PanelPathClipboardLease.Failure.invalidSequence {}
    precondition(board.changeCount == beforeReplay && !replay.hasActiveLease)
    requireOriginal()

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
      "Named-pasteboard path lease: empty, complete representations, abandoned runner, newer owner, sequence/replay, oversize and acknowledgment failure controls passed; general clipboard untouched"
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

  private static func checkRequestDecoding() throws {
    let requests: [DesktopFixtureRequest] = [
      .pickerBaseline, .picker, .holdCopy, .restoreCopy, .restorePanelClipboard,
      .preparePanelPath(sequence: 1, path: "/owned/日本語-00/one.eml"),
      .preparePanelPath(sequence: 32, path: "/owned/two.eml"),
    ]
    for request in requests {
      let bytes = Data((try request.wireLine + "\n").utf8)
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
    let prefix = DesktopFixtureRequest.panelPathPrefix
    let maximumPath = "/" + String(repeating: "a", count: 4095)
    let maximum = DesktopFixtureRequest.preparePanelPath(sequence: 32, path: maximumPath)
    let maximumLine = try maximum.wireLine
    precondition(maximumLine.utf8.count == DesktopFixtureRequest.maximumPanelPathLineBytes)
    let decodedMaximum = try DesktopFixtureRequest.decode(maximumLine)
    precondition(decodedMaximum == maximum)
    let malformed = [
      prefix, prefix + " 0 L2E=", prefix + " 33 L2E=", prefix + " 01 L2E=",
      prefix + " +1 L2E=", prefix + " 1 L2E", prefix + " 1 L2E= ",
      prefix + " 1 L2E=\r", prefix + " 1 L2E$", prefix + " 1 /w==",
      prefix + " 1 LwBh", prefix + " 1 cmVsYXRpdmU=", prefix + " 1 ",
      prefix + " 1 " + Data((maximumPath + "a").utf8).base64EncodedString(),
    ]
    for line in malformed {
      let output = DesktopFixtureOutput()
      output.observe(Data((line + "\n").utf8))
      precondition(output.take() == nil && output.observedRelayFailure() != nil)
    }
    for split in [0, prefix.utf8.count - 1, prefix.utf8.count, maximumLine.utf8.count] {
      let output = DesktopFixtureOutput()
      let bytes = Data((maximumLine + "\n").utf8)
      output.observe(bytes.prefix(split))
      output.observe(bytes.suffix(bytes.count - split))
      precondition(output.take() == maximum && output.observedRelayFailure() == nil)
    }
    let incomplete = DesktopFixtureOutput()
    incomplete.observe(Data((prefix + " 1 L2E=").utf8))
    incomplete.finish()
    precondition(incomplete.take() == nil && !incomplete.observedEOF())
    let tooLong = DesktopFixtureOutput()
    tooLong.observe(Data((maximumLine + "x").utf8))
    tooLong.observe(Data("\n".utf8))
    precondition(tooLong.take() == nil && tooLong.observedRelayFailure() != nil)
    let incidental = DesktopFixtureOutput()
    incidental.observe(Data("prefix EML_OWNED_COPY_HOLD suffix\n".utf8))
    precondition(incidental.take() == nil)
    incidental.observe(Data("EML_OWNED_COPY_HOLD".utf8))
    incidental.finish()
    precondition(incidental.take() == nil)
    let oversized = DesktopFixtureOutput()
    oversized.observe(
      Data(
        (String(repeating: "x", count: DesktopFixtureRequest.maximumLineBytes + 1)
          + "EML_OWNED_COPY_HOLD\nEML_OWNED_COPY_RESTORE\n").utf8
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

extension DesktopFixtureTests {
  @MainActor
  private static func checkActorRefusals(_ root: URL) throws -> [[String: Any]] {
    var facts = [[String: Any]]()
    do {
      _ = try actorFixture(root, "actor-unknown", scenario: "blocked-export-unknown")
      preconditionFailure("Unknown scenario admitted")
    } catch {}
    let wrong: [(String, String, DesktopFixtureActor, Int)] = [
      ("actor-order", "workflow", .createExportDirectory, 1),
      ("actor-sequence", "workflow", .occupyCheckedDestination, 2),
      ("actor-recipe", "blocked-processing-preflight", .arm(.exportPublication), 1),
    ]
    for (name, scenario, action, sequence) in wrong {
      let (fixture, actor, filesystem) = try actorFixture(root, name, scenario: scenario)
      do {
        try actor.handle(.fixtureActor(sequence: sequence, actor: action), mayStart: true)
        preconditionFailure("Wrong closed actor admitted")
      } catch {}
      do {
        try actor.handle(.fixtureActor(sequence: 3, actor: .createExportDirectory), mayStart: true)
        preconditionFailure("Actor executed after failure")
      } catch {}
      try actor.finish()
      precondition(
        !FileManager.default.fileExists(
          atPath: fixture.appendingPathComponent("directory-report.json").path))
      precondition(
        !FileManager.default.fileExists(
          atPath: fixture.appendingPathComponent("operation-control/observation").path))
      facts.append(["case": name, "fixture_filesystem": filesystem, "actor": actor.observation])
    }
    let (fixture, sourceActor, sourceFS) = try actorFixture(
      root, "actor-source-substitute", scenario: "workflow")
    let original = fixture.appendingPathComponent("two.eml")
    let bytes = try Data(contentsOf: original)
    try FileManager.default.moveItem(
      at: original, to: fixture.appendingPathComponent("held-original.eml"))
    try bytes.write(to: original, options: .withoutOverwriting)
    do {
      try sourceActor.handle(
        .fixtureActor(sequence: 1, actor: .occupyCheckedDestination), mayStart: true)
      preconditionFailure("Source substitute admitted")
    } catch {}
    try sourceActor.finish()
    precondition(
      !FileManager.default.fileExists(
        atPath: expectedCopy(
          for: original, directory: fixture.appendingPathComponent("chosen-destination")
        ).path))
    facts.append([
      "case": "same-bytes-different-source-identity", "fixture_filesystem": sourceFS,
      "actor": sourceActor.observation,
    ])
    let (duplicateRoot, duplicateActor, duplicateFS) = try actorFixture(
      root, "actor-duplicate-observation", scenario: "blocked-processing-preflight")
    try duplicateActor.handle(
      .fixtureActor(sequence: 1, actor: .arm(.processingPreflight)), mayStart: true)
    let duplicate = Data(
      "{\"case\":\"blocked-processing-preflight\",\"case\":\"blocked-processing-preflight\"}".utf8)
    do {
      try duplicateActor.handle(
        .fixtureActor(sequence: 2, actor: .retainChildObservation(duplicate)), mayStart: true)
      preconditionFailure("Duplicate JSON observation admitted")
    } catch {}
    try duplicateActor.finish()
    precondition(
      !FileManager.default.fileExists(
        atPath: duplicateRoot.appendingPathComponent("child-ui-observation.json").path))
    facts.append([
      "case": "duplicate-observation-before-publication", "fixture_filesystem": duplicateFS,
      "actor": duplicateActor.observation,
    ])
    let (liveRoot, liveActor, liveFS) = try actorFixture(
      root, "actor-live-archive", scenario: "export-publication")
    let prefix: [DesktopFixtureActor] = [
      .retainIndependentReport(Data("{}".utf8)), .occupyExport, .createExportDirectory,
      .createExportSymlink, .arm(.exportPublication),
    ]
    for (index, action) in prefix.enumerated() {
      try liveActor.handle(.fixtureActor(sequence: index + 1, actor: action), mayStart: true)
    }
    let control = liveRoot.appendingPathComponent("operation-control")
    try DesktopInteractionOwner.acknowledge(
      "operation=export phase=publication index=none operation_id=42\n",
      at: control.appendingPathComponent("ready"))
    try DesktopInteractionOwner.acknowledge(
      "\(getpid())\n", at: control.appendingPathComponent("worker-pid"))
    try liveActor.handle(
      .fixtureActor(sequence: 6, actor: .raceExportWinner(operationID: 42)), mayStart: true)
    try liveActor.handle(
      .fixtureActor(sequence: 7, actor: .release(operationID: 42)), mayStart: true)
    do {
      try liveActor.handle(
        .fixtureActor(sequence: 8, actor: .archive(history: .racingWinner, operationID: 42)),
        mayStart: true)
      preconditionFailure("Live process allowed archive")
    } catch {}
    try liveActor.finish()
    precondition(
      FileManager.default.fileExists(atPath: control.path)
        && !FileManager.default.fileExists(
          atPath: liveRoot.appendingPathComponent("control-history/racing-winner").path)
    )
    facts.append([
      "case": "actual-current-process-prevents-archive", "fixture_filesystem": liveFS,
      "actor": liveActor.observation,
    ])
    let (endedRoot, endedActor, endedFS) = try actorFixture(
      root, "actor-ended-relay", scenario: "workflow")
    do {
      try endedActor.handle(
        .fixtureActor(sequence: 1, actor: .occupyCheckedDestination), mayStart: false)
      preconditionFailure("Actor executed after observed relay closure")
    } catch {}
    try endedActor.finish()
    let entries = try FileManager.default.contentsOfDirectory(atPath: endedRoot.path)
    precondition(
      !entries.contains(where: {
        $0.hasPrefix("fixture-actor-") || $0.hasPrefix("fixture-pending-")
      }))
    facts.append([
      "case": "closed-admission-no-ack-or-mutation", "fixture_filesystem": endedFS,
      "actor": endedActor.observation,
    ])
    return facts
  }
}

extension DesktopFixtureTests {
  @MainActor
  private static func checkActualActorEOF(_ root: URL) throws -> [String: Any] {
    let (fixture, actor, filesystem) = try actorFixture(
      root, "actor-actual-eof", scenario: "workflow")
    let forwarded = fixture.appendingPathComponent("early-eof.log")
    try Data().write(to: forwarded, options: .withoutOverwriting)
    let destination = try FileHandle(forWritingTo: forwarded)
    let pipe = Pipe()
    let input = Pipe()
    let relay = DesktopFixtureOutput()
    let child = Process()
    try relay.relay(from: pipe.fileHandleForReading, to: destination)
    child.executableURL = URL(fileURLWithPath: "/bin/sh")
    child.arguments = [
      "-c",
      "printf 'EML_FIXTURE_ACTOR 1 occupy-checked-destination -\\n'; exec 1>&-; read token; test \"$token\" = release",
    ]
    child.standardOutput = pipe
    child.standardInput = input
    try child.run()
    let pid = child.processIdentifier
    let deadline = ProcessInfo.processInfo.systemUptime + 5
    while !relay.hasEnded() && child.isRunning && ProcessInfo.processInfo.systemUptime < deadline {
      Thread.sleep(forTimeInterval: 0.005)
    }
    let early = relay.observedEOF() && child.isRunning
    let request = relay.take()
    precondition(early && request == .fixtureActor(sequence: 1, actor: .occupyCheckedDestination))
    do {
      try actor.handle(request, mayStart: !relay.hasEnded())
      preconditionFailure("Queued actor started after actual EOF")
    } catch {}
    try input.fileHandleForWriting.write(contentsOf: Data("release\n".utf8))
    try input.fileHandleForWriting.close()
    let settlement = ProcessInfo.processInfo.systemUptime + 3
    while child.isRunning && ProcessInfo.processInfo.systemUptime < settlement {
      Thread.sleep(forTimeInterval: 0.005)
    }
    if child.isRunning { child.terminate() }
    let grace = ProcessInfo.processInfo.systemUptime + 1
    while child.isRunning && ProcessInfo.processInfo.systemUptime < grace {
      Thread.sleep(forTimeInterval: 0.005)
    }
    precondition(!child.isRunning, "Owned EOF probe requires outer cleanup")
    child.waitUntilExit()
    precondition(child.terminationStatus == 0)
    try pipe.fileHandleForReading.close()
    try destination.close()
    try actor.finish()
    precondition(
      !FileManager.default.fileExists(
        atPath: fixture.appendingPathComponent("fixture-actor-1.json").path))
    return [
      "case": "actual-early-EOF-with-queued-request-refused", "process_id": pid,
      "exit_code": child.terminationStatus, "runner_alive_at_eof": early,
      "fixture_filesystem": filesystem, "actor": actor.observation,
    ]
  }
}
