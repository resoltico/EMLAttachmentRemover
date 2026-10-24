// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import CryptoKit
import Darwin
import Foundation

// Only the unsandboxed lifetime owner instantiates this closed fixture actor.
@MainActor
final class DesktopFixtureActors {
  enum Failure: Error {
    case invalid(String)
    case unavailable
  }
  private let fixture: URL
  private let scenario: String
  private var rootFD: Int32 = -1
  private var controlFD: Int32 = -1
  private var historyFD: Int32 = -1
  private var historyIdentity: [String: Any]?
  private var initialDestination: [String: Any]?
  private var rootIdentity: [String: Any] = [:]
  private var controlIdentity: [String: Any]?
  private var initialSource: [String: Any]?
  private var expectedSource: Data?
  private var records = [[String: Any]]()
  private var unexecuted = [[String: Any]]()
  private var failure: [String: Any]?
  private var descriptorClosed = false
  private var rootVerified = false
  private var generation = 0
  private var nextSequence = 1
  private var armed: DesktopOperationRecipe?
  private var released = false
  private var admissionClosed = false
  private var phase = "admission"
  private var knownEffect: String?
  private var outputs = [[String: Any]]()
  private var source: Any = NSNull()
  private var worker: Any = NSNull()
  private var pendingCleanup: [String: Any] = [
    "state": "not_needed", "relative_path": NSNull(), "identity": NSNull(), "error": NSNull(),
  ]
  static let scenarios: Set<String> = {
    var names: Set<String> = [
      "owner-control", "workflow", "picker", "pagination", "appearance", "stop", "force-stop",
      "active-quit", "published-error", "export-publication", "export-writer-block",
    ]
    for recipe in DesktopOperationRecipe.allCases
    where recipe != .dualQuit && recipe != .exportRequestBody && recipe != .exportAfterPublication {
      let pieces = recipe.rawValue.split(separator: ":")
      names.insert("blocked-" + pieces[0] + "-" + pieces[1])
    }
    return names
  }()
  static let checkedSentinel = Data("Owned entry created after Check; never replace.\n".utf8)
  static let exportSentinel = Data("Independent report occupancy sentinel\n".utf8)

  init(fixture: URL, scenario: String, originals: [(URL, Data)], filesystemRoot: [String: Any])
    throws
  {
    self.fixture = fixture
    self.scenario = scenario
    guard fixture.path.utf8.count <= 4096, Self.scenarios.contains(scenario) else {
      throw Failure.invalid("Invalid fixture actor admission")
    }
    rootFD = fixture.withUnsafeFileSystemRepresentation {
      $0.map { Darwin.open($0, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC) } ?? -1
    }
    guard rootFD >= 0 else { throw Self.posix() }
    do {
      rootIdentity = try identity(rootFD)
      guard same(rootIdentity, filesystemRoot), rootIdentity["owner_uid"] as? UInt32 == geteuid(),
        (rootIdentity["mode"] as? UInt32).map({ $0 & 0o777 == 0o700 }) == true
      else { throw Failure.invalid("Fixture root identity or ownership differs") }
      try checkRoot()
      if scenario == "workflow" {
        let url = fixture.appendingPathComponent("two.eml")
        guard let bytes = originals.first(where: { $0.0 == url })?.1 else {
          throw Failure.invalid("Missing independent workflow source")
        }
        initialSource = try Self.checkedDescriptor(try openFile(rootFD, "two.eml")) { fd in
          let fact = try fileFact(fd)
          guard try read(fd) == bytes else { throw Failure.invalid("Workflow original differs") }
          return fact
        }
        expectedSource = bytes
        initialDestination = try Self.checkedDescriptor(try directory("chosen-destination")) {
          try identity($0)
        }
      }
      if scenario == "export-publication" {
        historyFD = try directory("control-history")
        historyIdentity = try identity(historyFD)
      }
    } catch {
      var primary = error
      if historyFD >= 0 {
        if Darwin.close(historyFD) != 0 {
          primary = DescriptorCloseFailure(primary: primary, close: Self.posix())
        }
        historyFD = -1
      }
      if Darwin.close(rootFD) != 0 {
        primary = DescriptorCloseFailure(primary: primary, close: Self.posix())
      }
      rootFD = -1
      throw primary
    }
  }

  static func unavailable(scenario: String) -> [String: Any] {
    [
      "state": "unavailable", "scenario": scenario, "actor_uid": geteuid(), "root": NSNull(),
      "root_address_verified": NSNull(), "descriptor_closed": NSNull(), "generation": 0,
      "records": [], "unexecuted": [], "failure": error(Failure.unavailable),
    ]
  }
  var observation: [String: Any] {
    [
      "state": failure == nil ? (records.isEmpty ? "unused" : "complete") : "failed",
      "scenario": scenario, "actor_uid": geteuid(), "root": rootIdentity,
      "root_address_verified": rootVerified, "descriptor_closed": descriptorClosed,
      "generation": generation, "records": records, "unexecuted": unexecuted,
      "failure": failure.map { $0 as Any } ?? NSNull(),
    ]
  }
  private var expectedTags: [String] {
    if scenario == "workflow" { return ["occupy-checked-destination"] }
    if scenario.hasPrefix("blocked-processing-") || scenario.hasPrefix("blocked-export-")
      || scenario == "export-writer-block"
    {
      return ["arm-operation", "retain-child-observation"]
    }
    if scenario == "export-publication" {
      return [
        "retain-independent-report", "occupy-export", "create-export-directory",
        "create-export-symlink", "arm-operation", "race-export-winner", "release-operation",
        "archive-control", "arm-operation", "release-operation", "archive-control", "arm-operation",
        "release-operation", "archive-control", "arm-operation", "retain-child-observation",
      ]
    }
    return []
  }
  func noteUnexecuted(_ request: DesktopFixtureRequest) {
    guard case .fixtureActor(let sequence, let actor) = request else { return }
    admissionClosed = true
    failure =
      failure
      ?? Self.error(
        Failure.invalid("Fixture actor request not executed before runner/output settlement"))
    if unexecuted.count < 8 {
      unexecuted.append([
        "sequence": sequence, "actor": actor.tag,
        "request_sha256": requestHash(sequence, actor),
        "reason": "Owner observed runner/output admission closed",
      ])
    }
  }
  func failAdmission(_ reason: String) {
    admissionClosed = true
    failure = failure ?? Self.error(Failure.invalid(reason))
  }
  func finish() throws {
    admissionClosed = true
    var closeFailure: Error?
    do { try checkRoot() } catch {
      failure = failure ?? Self.error(error)
      closeFailure = error
    }
    if controlFD >= 0 {
      let fd = controlFD
      controlFD = -1
      if Darwin.close(fd) != 0 {
        let e = Self.posix()
        failure = failure ?? Self.error(e)
        closeFailure = closeFailure ?? e
      }
    }
    if historyFD >= 0 {
      let fd = historyFD
      historyFD = -1
      if Darwin.close(fd) != 0 {
        let e = Self.posix()
        failure = failure ?? Self.error(e)
        closeFailure = closeFailure ?? e
      }
    }
    if rootFD >= 0 {
      let fd = rootFD
      rootFD = -1
      if Darwin.close(fd) == 0 {
        descriptorClosed = true
      } else {
        let e = Self.posix()
        failure = failure ?? Self.error(e)
        closeFailure = closeFailure ?? e
      }
    }
    if let closeFailure { throw closeFailure }
  }
  func handle(_ request: DesktopFixtureRequest?, mayStart: Bool) throws {
    guard let request, case .fixtureActor(let sequence, let actor) = request else { return }
    guard mayStart, !admissionClosed else {
      noteUnexecuted(request)
      throw Failure.invalid("Fixture actor admission closed")
    }
    guard records.count < 17 else { throw Failure.invalid("Fixture actor record bound exceeded") }
    phase = "admission"
    knownEffect = nil
    outputs = []
    source = NSNull()
    worker = NSNull()
    pendingCleanup = [
      "state": "not_needed", "relative_path": NSNull(), "identity": NSNull(), "error": NSNull(),
    ]
    let before = generation
    var operationError: Error?
    var effect = "unconfirmed"
    do {
      guard sequence == nextSequence, (1...32).contains(sequence),
        sequence <= expectedTags.count, actor.tag == expectedTags[sequence - 1]
      else { throw Failure.invalid("Unexpected fixture actor sequence or scenario") }
      nextSequence += 1
      try checkRoot()
      effect = try execute(actor)
      phase = "readback"
      try checkRoot()
      phase = "complete"
    } catch {
      operationError = error
      failure = failure ?? Self.error(error)
      admissionClosed = true
    }
    let document: [String: Any] = [
      "schema": 1, "sequence": sequence, "actor": actor.tag, "scenario": scenario,
      "request_sha256": requestHash(sequence, actor), "generation_before": before,
      "generation_after": generation, "state": operationError == nil ? "succeeded" : "failed",
      "phase": phase, "effect": operationError == nil ? effect : (knownEffect ?? "unconfirmed"),
      "root": rootIdentity, "root_address_verified": rootVerified, "outputs": outputs,
      "source": source, "worker": worker, "pending_cleanup": pendingCleanup,
      "error": operationError.map { Self.error($0) as Any } ?? NSNull(),
    ]
    let data = try fixtureCanonicalJSON(document)
    guard data.count <= 16384 else {
      admissionClosed = true
      throw Failure.invalid("Fixture acknowledgment exceeds its bound")
    }
    let acknowledgment = publishAcknowledgment(data, sequence: sequence)
    let ackPublished = acknowledgment.published
    let ackError = acknowledgment.error
    records.append([
      "document": String(decoding: data, as: UTF8.self), "identity": fixtureBytesIdentity(data),
      "ack_published": ackPublished, "ack_pending_cleanup": lastCleanup,
      "ack_error": ackError.map { Self.error($0) as Any } ?? NSNull(),
    ])
    if let operationError { throw operationError }
    if let ackError { throw ackError }
  }

  // Kept as one boundary so actual root refusal cannot reuse a previous edge fact.
  func publishAcknowledgment(_ data: Data, sequence: Int) -> (published: Bool, error: Error?) {
    lastPublicationSucceeded = false
    lastCleanup = [
      "state": "not_needed", "relative_path": NSNull(), "identity": NSNull(), "error": NSNull(),
    ]
    do {
      guard (1...32).contains(sequence), !data.isEmpty, data.count <= 16384 else {
        throw Failure.invalid("Invalid ACK frame bound")
      }
      try checkRoot()
      try publish(rootFD, "fixture-actor-\(sequence).json", data, track: false)
      return (true, nil)
    } catch {
      failure = failure ?? Self.error(error)
      admissionClosed = true
      return (lastPublicationSucceeded, error)
    }
  }

  private func execute(_ actor: DesktopFixtureActor) throws -> String {
    switch actor {
    case .occupyCheckedDestination:
      phase = "source_check"
      return try Self.checkedDescriptor(try openFile(rootFD, "two.eml")) { fd in
        let before = try fileFact(fd)
        guard let initialSource, let expectedSource,
          NSDictionary(dictionary: before).isEqual(to: initialSource),
          try read(fd) == expectedSource
        else { throw Failure.invalid("Workflow source identity/bytes changed") }
        return try Self.checkedDescriptor(try directory("chosen-destination")) { destination in
          guard let initialDestination, same(try identity(destination), initialDestination) else {
            throw Failure.invalid("Checked destination address changed")
          }
          phase = "mutation"
          let name = expectedCopy(for: fixture.appendingPathComponent("two.eml")).lastPathComponent
          try publish(destination, name, Self.checkedSentinel)
          outputs.append(try output(destination, name, relative: "chosen-destination/" + name))
          let after = try fileFact(fd)
          source = ["relative_path": "two.eml", "before": before, "after": after]
          guard NSDictionary(dictionary: before).isEqual(to: after), try read(fd) == expectedSource
          else { throw Failure.invalid("Workflow source changed during actor") }
          return "created"
        }
      }
    case .arm(let recipe):
      try validateRecipe(recipe)
      try holdControl()
      guard armed == nil else { throw Failure.invalid("Control generation already armed") }
      phase = "mutation"
      try publish(controlFD, "observation", Data((recipe.rawValue + "\n").utf8))
      outputs.append(
        try output(controlFD, "observation", relative: "operation-control/observation"))
      armed = recipe
      released = false
      return "created"
    case .release(let id):
      guard scenario == "export-publication", !released else {
        throw Failure.invalid("Unexpected release")
      }
      worker = try observeWorker(id, requireAbsent: false)
      phase = "mutation"
      try publish(controlFD, "release", Data("release\n".utf8))
      outputs.append(try output(controlFD, "release", relative: "operation-control/release"))
      released = true
      return "released"
    case .archive(let history, let id):
      let expected: [DesktopControlHistory] = [.racingWinner, .preEdgeStop, .postEdgeStop]
      guard scenario == "export-publication", generation < 3, history == expected[generation],
        released
      else { throw Failure.invalid("Unexpected archive generation") }
      worker = try observeWorker(id, requireAbsent: true)
      guard historyFD >= 0, let historyIdentity else {
        throw Failure.invalid("No admitted history parent")
      }
      var historyFacts = stat()
      guard fstatat(rootFD, "control-history", &historyFacts, AT_SYMLINK_NOFOLLOW) == 0 else {
        throw Self.posix()
      }
      guard same(Self.native(historyFacts), historyIdentity),
        same(try identity(historyFD), historyIdentity)
      else { throw Failure.invalid("History parent address changed") }
      let parent = historyFD
      phase = "mutation"
      guard
        renameatx_np(rootFD, "operation-control", parent, history.rawValue, UInt32(RENAME_EXCL))
          == 0
      else { throw Self.posix() }
      knownEffect = "archived"
      outputs.append(
        try output(parent, history.rawValue, relative: "control-history/" + history.rawValue))
      guard mkdirat(rootFD, "operation-control", 0o700) == 0 else { throw Self.posix() }
      outputs.append(try output(rootFD, "operation-control", relative: "operation-control"))
      let old = controlFD
      controlFD = -1
      guard Darwin.close(old) == 0 else { throw Self.posix() }
      controlIdentity = nil
      armed = nil
      released = false
      generation += 1
      try holdControl()
      return "archived"
    case .retainIndependentReport(let bytes):
      guard scenario == "export-publication" else {
        throw Failure.invalid("Unexpected report oracle")
      }
      phase = "mutation"
      try publish(rootFD, "independent-report.json", bytes)
      outputs.append(
        try output(rootFD, "independent-report.json", relative: "independent-report.json"))
      return "created"
    case .occupyExport:
      phase = "mutation"
      try publish(rootFD, "occupied-report.json", Self.exportSentinel)
      outputs.append(try output(rootFD, "occupied-report.json", relative: "occupied-report.json"))
      return "created"
    case .createExportDirectory:
      phase = "mutation"
      guard mkdirat(rootFD, "directory-report.json", 0o700) == 0 else { throw Self.posix() }
      knownEffect = "created"
      outputs.append(try output(rootFD, "directory-report.json", relative: "directory-report.json"))
      return "created"
    case .createExportSymlink:
      try Self.checkedDescriptor(try openFile(rootFD, "occupied-report.json")) { occupied in
        guard try read(occupied) == Self.exportSentinel else {
          throw Failure.invalid("Export occupancy oracle changed")
        }
      }
      phase = "mutation"
      let target = fixture.appendingPathComponent("occupied-report.json").path
      guard symlinkat(target, rootFD, "symlink-report.json") == 0 else { throw Self.posix() }
      knownEffect = "created"
      outputs.append(try output(rootFD, "symlink-report.json", relative: "symlink-report.json"))
      return "created"
    case .raceExportWinner(let id):
      guard armed == .exportPublication else { throw Failure.invalid("Race not at publication") }
      worker = try observeWorker(id, requireAbsent: false)
      phase = "mutation"
      try publish(rootFD, "racing-report.json", Self.exportSentinel)
      outputs.append(try output(rootFD, "racing-report.json", relative: "racing-report.json"))
      return "created"
    case .retainChildObservation(let bytes):
      let object = try fixtureCanonicalObject(bytes)
      guard object["case"] as? String == scenario else {
        throw Failure.invalid("Observation case differs")
      }
      phase = "mutation"
      try publish(rootFD, "child-ui-observation.json", bytes)
      outputs.append(
        try output(rootFD, "child-ui-observation.json", relative: "child-ui-observation.json"))
      return "created"
    }
  }

  private func validateRecipe(_ recipe: DesktopOperationRecipe) throws {
    let wanted: String
    if scenario == "export-publication" {
      wanted =
        generation < 2
        ? "export:publication"
        : generation == 2 ? "export:namespace-sync" : "export:after-publication"
    } else if scenario == "blocked-export-preflight" {
      wanted = "dual-quit"
    } else if scenario == "export-writer-block" {
      wanted = "export:request-body"
    } else if scenario.hasPrefix("blocked-processing-") {
      wanted = "processing:" + scenario.dropFirst("blocked-processing-".count) + ":0"
    } else if scenario.hasPrefix("blocked-export-") {
      wanted = "export:" + scenario.dropFirst("blocked-export-".count)
    } else {
      throw Failure.invalid("Recipe is not admitted for scenario")
    }
    guard recipe.rawValue == wanted else {
      throw Failure.invalid("Recipe does not match current scenario/generation")
    }
  }
  private func observeWorker(_ id: UInt64, requireAbsent: Bool) throws -> [String: Any] {
    try holdControl()
    guard let armed, armed == .exportPublication || armed == .exportNamespaceSync else {
      throw Failure.invalid("No matching export boundary")
    }
    let ready = try Self.checkedDescriptor(try openFile(controlFD, "ready")) {
      try read($0, maximum: 128)
    }
    let pidBytes = try Self.checkedDescriptor(try openFile(controlFD, "worker-pid")) {
      try read($0, maximum: 12)
    }
    let phase = armed == .exportPublication ? "publication" : "namespace-sync"
    guard id > 0,
      ready == Data("operation=export phase=\(phase) index=none operation_id=\(id)\n".utf8),
      let text = String(data: pidBytes, encoding: .utf8), text.hasSuffix("\n"),
      let pid = Int32(text.dropLast()), pid > 0, text == "\(pid)\n"
    else { throw Failure.invalid("Control generation has malformed or mismatched native markers") }
    try checkControl()
    var observation: [String: Any] = [
      "pid": pid, "operation_id": id, "operation": "export", "phase": phase,
      "pid_absent": NSNull(), "absence_errno": NSNull(),
      "control_identity": try identity(controlFD),
    ]
    if requireAbsent {
      let status = Darwin.kill(pid, 0)
      let observed = errno
      observation["pid_absent"] = status == -1 && observed == ESRCH
      observation["absence_errno"] = status == -1 ? observed as Any : NSNull()
      worker = observation
      if status == -1, observed != ESRCH {
        throw NSError(domain: NSPOSIXErrorDomain, code: Int(observed))
      }
      guard status == -1, observed == ESRCH else {
        throw Failure.invalid("Recorded worker is still present; no archive authority")
      }
    }
    return observation
  }

  private func holdControl() throws {
    if controlFD < 0 {
      controlFD = try directory("operation-control")
      controlIdentity = try identity(controlFD)
    }
    try checkControl()
  }
  private func checkControl() throws {
    guard controlFD >= 0, let controlIdentity else {
      throw Failure.invalid("No held control generation")
    }
    var current = stat()
    guard fstatat(rootFD, "operation-control", &current, AT_SYMLINK_NOFOLLOW) == 0 else {
      throw Self.posix()
    }
    guard same(try identity(controlFD), controlIdentity),
      same(Self.native(current), controlIdentity), current.st_mode & S_IFMT == S_IFDIR
    else { throw Failure.invalid("Control directory address changed") }
  }
  private func checkRoot() throws {
    rootVerified = false
    guard rootFD >= 0 else { throw Failure.invalid("Actor root is closed") }
    var current = stat()
    let status = fixture.withUnsafeFileSystemRepresentation { $0.map { lstat($0, &current) } ?? -1 }
    guard status == 0 else { throw Self.posix() }
    guard current.st_mode & S_IFMT == S_IFDIR,
      same(try identity(rootFD), rootIdentity), same(Self.native(current), rootIdentity)
    else { throw Failure.invalid("Actor root requested address changed") }
    rootVerified = true
  }
  private func directory(_ name: String) throws -> Int32 {
    let fd = openat(rootFD, name, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
    guard fd >= 0 else { throw Self.posix() }
    do {
      let facts = try identity(fd)
      guard facts["owner_uid"] as? UInt32 == geteuid(),
        (facts["mode"] as? UInt32).map({ $0 & 0o022 == 0 }) == true
      else { throw Failure.invalid("Unsafe fixture operand directory") }
      return fd
    } catch {
      let primary = error
      if Darwin.close(fd) != 0 {
        throw DescriptorCloseFailure(primary: primary, close: Self.posix())
      }
      throw primary
    }
  }
  private func openFile(_ directory: Int32, _ name: String) throws -> Int32 {
    let fd = openat(directory, name, O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC)
    guard fd >= 0 else { throw Self.posix() }
    do {
      var facts = stat()
      guard fstat(fd, &facts) == 0 else { throw Self.posix() }
      guard facts.st_mode & S_IFMT == S_IFREG else {
        throw Failure.invalid("Fixture operand is not an ordinary file")
      }
      return fd
    } catch {
      let primary = error
      if Darwin.close(fd) != 0 {
        throw DescriptorCloseFailure(primary: primary, close: Self.posix())
      }
      throw primary
    }
  }
  private func read(_ fd: Int32, maximum: Int = 65536) throws -> Data {
    try fixtureReadBytes(fd, maximum: maximum)
  }
  private func identity(_ fd: Int32) throws -> [String: Any] {
    var facts = stat()
    guard fstat(fd, &facts) == 0 else { throw Self.posix() }
    guard facts.st_ino > 0 else { throw Failure.invalid("Missing native fixture identity") }
    return Self.native(facts)
  }
  private static func native(_ facts: stat) -> [String: Any] {
    fixtureNativeIdentity(facts)
  }
  private func same(_ lhs: [String: Any], _ rhs: [String: Any]) -> Bool {
    (["device", "inode", "mode"] + (rhs["owner_uid"] == nil ? [] : ["owner_uid"])).allSatisfy {
      key in
      (lhs[key] as? NSNumber) == (rhs[key] as? NSNumber)
    }
  }
  private func fileFact(_ fd: Int32) throws -> [String: Any] {
    ["identity": try identity(fd), "bytes": fixtureBytesIdentity(try read(fd))]
  }
  private func output(_ dir: Int32, _ name: String, relative: String) throws -> [String: Any] {
    var facts = stat()
    guard fstatat(dir, name, &facts, AT_SYMLINK_NOFOLLOW) == 0 else { throw Self.posix() }
    let kind: String
    let bytes: Any
    let target: Any
    if facts.st_mode & S_IFMT == S_IFREG {
      bytes = try Self.checkedDescriptor(try openFile(dir, name)) { fd in
        guard same(try identity(fd), Self.native(facts)) else {
          throw Failure.invalid("Fixture output identity changed")
        }
        return fixtureBytesIdentity(try read(fd))
      }
      kind = "file"
      target = NSNull()
    } else if facts.st_mode & S_IFMT == S_IFDIR {
      kind = "directory"
      bytes = NSNull()
      target = NSNull()
    } else if facts.st_mode & S_IFMT == S_IFLNK {
      var buffer = [UInt8](repeating: 0, count: 4097)
      let count = readlinkat(dir, name, &buffer, buffer.count)
      guard count > 0, count <= 4096,
        let text = String(data: Data(buffer.prefix(count)), encoding: .utf8)
      else { throw Failure.invalid("Invalid fixture symlink bytes") }
      kind = "symlink"
      bytes = NSNull()
      target = text
    } else {
      throw Failure.invalid("Unexpected fixture output type")
    }
    return [
      "relative_path": relative, "kind": kind, "identity": Self.native(facts), "bytes": bytes,
      "symlink_target": target,
    ]
  }
  private var lastPublicationSucceeded = false
  private var lastCleanup: [String: Any] = [
    "state": "not_needed", "relative_path": NSNull(), "identity": NSNull(), "error": NSNull(),
  ]
  private func publish(_ directory: Int32, _ name: String, _ data: Data, track: Bool = true) throws
  {
    guard data.count <= 65536 else {
      throw Failure.invalid("Fixture publication exceeds its bound")
    }
    lastPublicationSucceeded = false
    lastCleanup = [
      "state": "not_needed", "relative_path": NSNull(), "identity": NSNull(), "error": NSNull(),
    ]
    let pending = "fixture-pending-" + UUID().uuidString
    let fd = openat(directory, pending, O_RDWR | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600)
    guard fd >= 0 else { throw Self.posix() }
    let owned: [String: Any]
    do { owned = try identity(fd) } catch {
      let primary = error
      let closeResult = Darwin.close(fd)
      let closeError: Any = closeResult == 0 ? NSNull() : Self.error(Self.posix())
      lastCleanup = [
        "state": "unknown", "relative_path": pending, "identity": NSNull(), "error": closeError,
      ]
      if track { pendingCleanup = lastCleanup }
      throw primary
    }
    var closed = false
    var closeSucceeded = false
    var cleaned = false
    func cleanup() throws {
      var facts = stat()
      guard fstatat(directory, pending, &facts, AT_SYMLINK_NOFOLLOW) == 0 else {
        throw Self.posix()
      }
      guard same(Self.native(facts), owned) else {
        throw Failure.invalid("Pending fixture identity changed; no deletion authority")
      }
      guard unlinkat(directory, pending, 0) == 0 else { throw Self.posix() }
      cleaned = true
    }
    do {
      try data.withUnsafeBytes { bytes in
        var position = 0
        while position < bytes.count {
          let count = Darwin.write(
            fd, bytes.baseAddress!.advanced(by: position), bytes.count - position)
          if count < 0 {
            if errno == EINTR { continue }
            throw Self.posix()
          }
          guard count > 0 else { throw Failure.invalid("Zero-length fixture write") }
          position += count
        }
      }
      guard try read(fd) == data else { throw Failure.invalid("Pending fixture readback differs") }
      let closeResult = Darwin.close(fd)
      closed = true
      closeSucceeded = closeResult == 0
      guard closeResult == 0 else { throw Self.posix() }
      guard linkat(directory, pending, directory, name, 0) == 0 else { throw Self.posix() }
      lastPublicationSucceeded = true
      if track { knownEffect = name == "release" ? "released" : "created" }
      try Self.checkedDescriptor(try openFile(directory, name)) { published in
        guard same(try identity(published), owned), try read(published) == data else {
          throw Failure.invalid("Published fixture readback differs")
        }
      }
      try cleanup()
      lastCleanup = [
        "state": "complete", "relative_path": pending, "identity": owned, "error": NSNull(),
      ]
      if track { pendingCleanup = lastCleanup }
    } catch {
      let primary = error
      var closeError: Error?
      if !closed {
        let result = Darwin.close(fd)
        closed = true
        closeSucceeded = result == 0
        if result != 0 { closeError = Self.posix() }
      }
      var cleanupError: Error?
      if !cleaned { do { try cleanup() } catch { cleanupError = error } }
      lastCleanup = [
        "state": cleaned && closeSucceeded ? "complete" : "unknown", "relative_path": pending,
        "identity": owned,
        "error": (cleanupError ?? closeError).map { Self.error($0) as Any } ?? NSNull(),
      ]
      if track { pendingCleanup = lastCleanup }
      if let closeError { throw DescriptorCloseFailure(primary: primary, close: closeError) }
      throw primary
    }
  }
  private func requestHash(_ sequence: Int, _ actor: DesktopFixtureActor) -> String {
    let line = (try? actor.line(sequence: sequence)) ?? "invalid"
    return fixtureBytesIdentity(Data((line + "\n").utf8))["sha256"] as! String
  }
  struct DescriptorCloseFailure: Error {
    let primary: Error?
    let close: Error
  }
  static func checkedDescriptor<T>(_ fd: Int32, _ body: (Int32) throws -> T) throws -> T {
    let result = Result { try body(fd) }
    let code = Darwin.close(fd)
    if code != 0 {
      let close = Self.posix()
      if case .failure(let primary) = result {
        throw DescriptorCloseFailure(primary: primary, close: close)
      }
      throw close
    }
    return try result.get()
  }
  private static func posix() -> Error { NSError(domain: NSPOSIXErrorDomain, code: Int(errno)) }
  static func error(_ value: Error) -> [String: Any] {
    func bounded(_ text: String, maximum: Int) -> String {
      var bytes = Data(text.utf8.prefix(maximum))
      while String(data: bytes, encoding: .utf8) == nil { bytes.removeLast() }
      return String(decoding: bytes, as: UTF8.self)
    }
    if let failure = value as? DescriptorCloseFailure {
      var first = error(failure.primary ?? failure.close)
      let close = failure.close as NSError
      first["message"] = bounded(
        (first["message"] as? String ?? "") + "; checked descriptor close failed: " + close.domain
          + ":" + String(close.code), maximum: 1024)
      return first
    }
    let e = value as NSError
    let message = bounded(String(describing: value), maximum: 1024)
    let underlying = e.userInfo[NSUnderlyingErrorKey] as? NSError
    let code: Any
    if e.domain == NSPOSIXErrorDomain {
      code = e.code
    } else if let underlying, underlying.domain == NSPOSIXErrorDomain {
      code = underlying.code
    } else {
      code = NSNull()
    }
    return [
      "domain": bounded(e.domain, maximum: 256), "code": e.code, "native_errno": code,
      "message": message,
    ]
  }
}
