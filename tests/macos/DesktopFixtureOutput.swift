// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import CoreFoundation
import CryptoKit
import Darwin
import Foundation

enum DesktopFixtureRequest: Equatable, Sendable {
  case pickerBaseline, picker, holdCopy, restoreCopy, restorePanelClipboard
  case preparePanelPath(sequence: Int, path: String)
  case fixtureActor(sequence: Int, actor: DesktopFixtureActor)

  static let panelPathPrefix = "EML_NATIVE_PANEL_PATH_PREPARE"
  static let maximumPanelPathLineBytes =
    panelPathPrefix.utf8.count + 1 + 2 + 1 + 4 * ((4096 + 2) / 3)
  static let maximumLineBytes = DesktopFixtureActor.maximumLineBytes
  enum InvalidRequest: Error { case malformed }

  var wireLine: String {
    get throws {
      switch self {
      case .fixtureActor(let sequence, let actor): return try actor.line(sequence: sequence)
      case .pickerBaseline: return "EML_NATIVE_PICKER_BASELINE"
      case .picker: return "EML_NATIVE_PICKER_REQUESTED"
      case .holdCopy: return "EML_OWNED_COPY_HOLD"
      case .restoreCopy: return "EML_OWNED_COPY_RESTORE"
      case .restorePanelClipboard: return "EML_NATIVE_PANEL_CLIPBOARD_RESTORE"
      case .preparePanelPath(let sequence, let path):
        let bytes = Data(path.utf8)
        guard (1...32).contains(sequence), !bytes.isEmpty, bytes.count <= 4096,
          !bytes.contains(0), (path as NSString).isAbsolutePath
        else { throw InvalidRequest.malformed }
        return "\(Self.panelPathPrefix) \(sequence) \(bytes.base64EncodedString())"
      }
    }
  }

  static func decode(_ line: String) throws -> Self? {
    switch line {
    case "EML_NATIVE_PICKER_BASELINE": return .pickerBaseline
    case "EML_NATIVE_PICKER_REQUESTED": return .picker
    case "EML_OWNED_COPY_HOLD": return .holdCopy
    case "EML_OWNED_COPY_RESTORE": return .restoreCopy
    case "EML_NATIVE_PANEL_CLIPBOARD_RESTORE": return .restorePanelClipboard
    default:
      if line.hasPrefix(DesktopFixtureActor.prefix) {
        let (sequence, actor) = try DesktopFixtureActor.decode(line)
        return .fixtureActor(sequence: sequence, actor: actor)
      }
      guard line.hasPrefix(panelPathPrefix) else { return nil }
      let fields = line.split(separator: " ", omittingEmptySubsequences: false)
      guard fields.count == 3, fields[0] == panelPathPrefix,
        let sequence = Int(fields[1]), (1...32).contains(sequence),
        String(sequence) == fields[1], fields[2].utf8.count <= 4 * ((4096 + 2) / 3),
        let bytes = Data(base64Encoded: String(fields[2])), !bytes.isEmpty, bytes.count <= 4096,
        bytes.base64EncodedString() == fields[2], !bytes.contains(0),
        let path = String(data: bytes, encoding: .utf8), (path as NSString).isAbsolutePath
      else { throw InvalidRequest.malformed }
      return .preparePanelPath(sequence: sequence, path: path)
    }
  }

  func send() throws {
    // The parent receives bounded payload bytes without granting XCTest write access.
    try FileHandle.standardOutput.write(contentsOf: Data((try wireLine + "\n").utf8))
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
  private func isReserved(_ bytes: Data) -> Bool {
    bytes.starts(with: Data(DesktopFixtureRequest.panelPathPrefix.utf8))
      || bytes.starts(with: Data(DesktopFixtureActor.prefix.utf8))
  }
  func finish() {
    lock.lock()
    if isReserved(line) {
      relayFailure = relayFailure ?? "Incomplete owned fixture request at EOF"
    }
    ended = true
    lock.unlock()
  }
  func hasEnded() -> Bool {
    lock.lock()
    defer { lock.unlock() }
    return ended
  }
  func observedEOF() -> Bool {
    lock.lock()
    defer { lock.unlock() }
    return ended && relayFailure == nil
  }
  func observe(_ bytes: Data) {
    lock.lock()
    defer { lock.unlock() }
    for byte in bytes {
      if byte == 10 {
        if !droppingLine, let text = String(data: line, encoding: .utf8) {
          do {
            if let request = try DesktopFixtureRequest.decode(text) {
              guard pending.count < 8 else {
                relayFailure = relayFailure ?? "Owned fixture request queue overflow"
                return
              }
              pending.append(request)
            }
          } catch {
            relayFailure = relayFailure ?? "Malformed owned fixture request"
          }
        } else if !droppingLine,
          isReserved(line)
        {
          relayFailure = relayFailure ?? "Malformed owned fixture request"
        }
        line.removeAll(keepingCapacity: true)
        droppingLine = false
      } else if !droppingLine {
        if line.count < DesktopFixtureRequest.maximumLineBytes {
          line.append(byte)
        } else {
          if isReserved(line) {
            relayFailure = relayFailure ?? "Oversize owned fixture request"
          }
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

// Independent fixture implementation of storage-design.md's route seed. These
// short Unicode fixtures have an empty logical directory route; no production
// naming function or generated report is used as the expected-path oracle.
func expectedCopy(for source: URL, directory: URL? = nil) -> URL {
  let basename = source.lastPathComponent
  let stem = basename.lowercased().hasSuffix(".eml") ? String(basename.dropLast(4)) : basename
  let human = stem.isEmpty ? "message" : stem
  let logical = Data((human + ".no-attachments.eml").utf8)
  var seed = Data("eml-attachment-remover:destination-route:v1\0".utf8)
  seed.append(contentsOf: [1, 0, 0, 0, 2, 0])
  let count = UInt32(logical.count)
  seed.append(contentsOf: (0..<4).map { UInt8((count >> ($0 * 8)) & 255) })
  seed.append(logical)
  let digest = SHA256.hash(data: seed).prefix(16)
  let bits = digest.map { String($0, radix: 2).leftPadded(to: 8) }.joined() + "00"
  let alphabet = Array("abcdefghijklmnopqrstuvwxyz234567")
  let binary = Array(bits)
  let token = stride(from: 0, to: binary.count, by: 5).map { index in
    alphabet[Int(String(binary[index..<index + 5]), radix: 2)!]
  }
  return (directory ?? source.deletingLastPathComponent()).appendingPathComponent(
    human + "." + String(token) + ".no-attachments.eml")
}
extension String {
  fileprivate func leftPadded(to length: Int) -> String {
    String(repeating: "0", count: max(0, length - count)) + self
  }
}

// This closed fixture protocol carries no filesystem path or general write command.
enum DesktopOperationRecipe: String, CaseIterable, Sendable {
  case processingPreflight = "processing:preflight:0"
  case processingReadback = "processing:readback:0"
  case processingStageWrite = "processing:stage-write:0"
  case processingFileSync = "processing:file-sync:0"
  case processingPublication = "processing:publication:0"
  case processingNamespaceSync = "processing:namespace-sync:0"
  case exportPreflight = "export:preflight"
  case exportReadback = "export:readback"
  case exportStageWrite = "export:stage-write"
  case exportFileSync = "export:file-sync"
  case exportPublication = "export:publication"
  case exportNamespaceSync = "export:namespace-sync"
  case exportRequestBody = "export:request-body"
  case exportAfterPublication = "export:after-publication"
  case dualQuit = "dual-quit"
}
enum DesktopControlHistory: String, CaseIterable, Sendable {
  case racingWinner = "racing-winner"
  case preEdgeStop = "pre-edge-stop"
  case postEdgeStop = "post-edge-stop"
}
enum DesktopFixtureActor: Equatable, Sendable {
  case occupyCheckedDestination, occupyExport, createExportDirectory, createExportSymlink
  case arm(DesktopOperationRecipe)
  case release(operationID: UInt64)
  case archive(history: DesktopControlHistory, operationID: UInt64)
  case retainIndependentReport(Data)
  case retainChildObservation(Data)
  case raceExportWinner(operationID: UInt64)
  static let prefix = "EML_FIXTURE_ACTOR"
  static let maximumPayloadBytes = 65536
  static let maximumLineBytes = 88 * 1024
  var tag: String {
    switch self {
    case .occupyCheckedDestination: return "occupy-checked-destination"
    case .occupyExport: return "occupy-export"
    case .createExportDirectory: return "create-export-directory"
    case .createExportSymlink: return "create-export-symlink"
    case .arm: return "arm-operation"
    case .release: return "release-operation"
    case .archive: return "archive-control"
    case .retainIndependentReport: return "retain-independent-report"
    case .retainChildObservation: return "retain-child-observation"
    case .raceExportWinner: return "race-export-winner"
    }
  }
  func line(sequence: Int) throws -> String {
    guard (1...32).contains(sequence) else { throw DesktopFixtureRequest.InvalidRequest.malformed }
    let operand: String
    switch self {
    case .occupyCheckedDestination, .occupyExport, .createExportDirectory, .createExportSymlink:
      operand = "-"
    case .arm(let recipe): operand = recipe.rawValue
    case .release(let id), .raceExportWinner(let id):
      guard id > 0 else { throw DesktopFixtureRequest.InvalidRequest.malformed }
      operand = String(id)
    case .archive(let history, let id):
      guard id > 0 else { throw DesktopFixtureRequest.InvalidRequest.malformed }
      operand = history.rawValue + ":" + String(id)
    case .retainIndependentReport(let bytes), .retainChildObservation(let bytes):
      guard !bytes.isEmpty, bytes.count <= Self.maximumPayloadBytes,
        String(data: bytes, encoding: .utf8) != nil
      else { throw DesktopFixtureRequest.InvalidRequest.malformed }
      operand = bytes.base64EncodedString()
    }
    return "\(Self.prefix) \(sequence) \(tag) \(operand)"
  }
  static func decode(_ line: String) throws -> (Int, Self) {
    let f = line.split(separator: " ", omittingEmptySubsequences: false).map(String.init)
    guard f.count == 4, f[0] == prefix, let sequence = Int(f[1]),
      (1...32).contains(sequence), String(sequence) == f[1]
    else { throw DesktopFixtureRequest.InvalidRequest.malformed }
    func id(_ text: String) throws -> UInt64 {
      guard let n = UInt64(text), n > 0, String(n) == text else {
        throw DesktopFixtureRequest.InvalidRequest.malformed
      }
      return n
    }
    let actor: Self
    switch f[2] {
    case "occupy-checked-destination" where f[3] == "-": actor = .occupyCheckedDestination
    case "occupy-export" where f[3] == "-": actor = .occupyExport
    case "create-export-directory" where f[3] == "-": actor = .createExportDirectory
    case "create-export-symlink" where f[3] == "-": actor = .createExportSymlink
    case "arm-operation":
      guard let recipe = DesktopOperationRecipe(rawValue: f[3]) else {
        throw DesktopFixtureRequest.InvalidRequest.malformed
      }
      actor = .arm(recipe)
    case "release-operation": actor = .release(operationID: try id(f[3]))
    case "race-export-winner": actor = .raceExportWinner(operationID: try id(f[3]))
    case "archive-control":
      let parts = f[3].split(separator: ":", omittingEmptySubsequences: false).map(String.init)
      guard parts.count == 2, let history = DesktopControlHistory(rawValue: parts[0]) else {
        throw DesktopFixtureRequest.InvalidRequest.malformed
      }
      actor = .archive(history: history, operationID: try id(parts[1]))
    case "retain-independent-report", "retain-child-observation":
      guard f[3].utf8.count <= 4 * ((maximumPayloadBytes + 2) / 3),
        let bytes = Data(base64Encoded: f[3]), !bytes.isEmpty,
        bytes.count <= maximumPayloadBytes, bytes.base64EncodedString() == f[3],
        String(data: bytes, encoding: .utf8) != nil
      else { throw DesktopFixtureRequest.InvalidRequest.malformed }
      actor =
        f[2] == "retain-independent-report"
        ? .retainIndependentReport(bytes) : .retainChildObservation(bytes)
    default: throw DesktopFixtureRequest.InvalidRequest.malformed
    }
    guard try actor.line(sequence: sequence) == line else {
      throw DesktopFixtureRequest.InvalidRequest.malformed
    }
    return (sequence, actor)
  }
}

func fixtureBytesIdentity(_ data: Data) -> [String: Any] {
  [
    "byte_length": data.count,
    "sha256": SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined(),
  ]
}
func fixtureCanonicalJSON(_ value: Any) throws -> Data {
  try JSONSerialization.data(
    withJSONObject: value, options: [.sortedKeys, .withoutEscapingSlashes])
}
func fixtureCanonicalObject(_ data: Data) throws -> [String: Any] {
  guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any],
    try fixtureCanonicalJSON(object) == data
  else { throw DesktopFixtureRequest.InvalidRequest.malformed }
  return object
}

@MainActor
final class DesktopFixtureActorClient {
  enum Failure: Error { case invalidAcknowledgment, deadline, closed }
  private let fixture: URL
  private let scenario: String
  private var nextSequence = 1
  private var closed = false
  private var rootIdentity: Data?
  init(fixture: URL, scenario: String) {
    self.fixture = fixture
    self.scenario = scenario
  }
  func perform(_ actor: DesktopFixtureActor) async throws {
    guard !closed else { throw Failure.closed }
    do {
      let rootFD = fixture.withUnsafeFileSystemRepresentation {
        $0.map { Darwin.open($0, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC) } ?? -1
      }
      guard rootFD >= 0 else { throw NSError(domain: NSPOSIXErrorDomain, code: Int(errno)) }
      let rootHandle = FileHandle(fileDescriptor: rootFD, closeOnDealloc: false)
      var rootClosed = false
      defer { if !rootClosed { try? rootHandle.close() } }
      var nativeRoot = stat()
      guard fstat(rootFD, &nativeRoot) == 0 else {
        throw NSError(domain: NSPOSIXErrorDomain, code: Int(errno))
      }
      guard nativeRoot.st_mode & S_IFMT == S_IFDIR, nativeRoot.st_mode & 0o777 == 0o700,
        nativeRoot.st_uid == geteuid(), nativeRoot.st_ino > 0
      else { throw Failure.invalidAcknowledgment }
      let rootFields = fixtureNativeIdentity(nativeRoot)
      let boundRoot = try fixtureCanonicalJSON(rootFields)
      if let rootIdentity, rootIdentity != boundRoot { throw Failure.invalidAcknowledgment }
      rootIdentity = boundRoot
      let sequence = nextSequence
      let line = try actor.line(sequence: sequence)
      nextSequence += 1
      let ack = fixture.appendingPathComponent("fixture-actor-\(sequence).json")
      var preexisting = stat()
      guard fstatat(rootFD, ack.lastPathComponent, &preexisting, AT_SYMLINK_NOFOLLOW) == -1,
        errno == ENOENT
      else { throw Failure.invalidAcknowledgment }
      try DesktopFixtureRequest.fixtureActor(sequence: sequence, actor: actor).send()
      let expectedHash = fixtureBytesIdentity(Data((line + "\n").utf8))["sha256"] as! String
      let deadline = ProcessInfo.processInfo.systemUptime + 10
      while ProcessInfo.processInfo.systemUptime < deadline {
        let descriptor = openat(
          rootFD, ack.lastPathComponent, O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC)
        if descriptor >= 0 {
          let handle = FileHandle(fileDescriptor: descriptor, closeOnDealloc: false)
          var ackClosed = false
          do {
            var facts = stat()
            guard fstat(descriptor, &facts) == 0, facts.st_mode & S_IFMT == S_IFREG,
              facts.st_size > 0, facts.st_size <= 16384
            else { throw Failure.invalidAcknowledgment }
            let data = try fixtureReadBytes(descriptor, maximum: 16384)
            do {
              try handle.close()
              ackClosed = true
            } catch {
              ackClosed = true
              throw error
            }
            try Self.validateAcknowledgment(
              data, sequence: sequence, actor: actor, scenario: scenario, requestHash: expectedHash,
              root: boundRoot)
            var current = stat()
            let address = fixture.withUnsafeFileSystemRepresentation {
              $0.map { lstat($0, &current) } ?? -1
            }
            guard address == 0,
              try fixtureCanonicalJSON(fixtureNativeIdentity(current)) == boundRoot
            else { throw Failure.invalidAcknowledgment }
            do {
              try rootHandle.close()
              rootClosed = true
            } catch {
              rootClosed = true
              throw error
            }
            return
          } catch {
            if !ackClosed { try? handle.close() }
            throw error
          }
        }
        guard errno == ENOENT else { throw NSError(domain: NSPOSIXErrorDomain, code: Int(errno)) }
        try await Task.sleep(for: .milliseconds(25))
      }
      throw Failure.deadline
    } catch {
      closed = true
      throw error
    }
  }
  static func validateAcknowledgment(
    _ data: Data, sequence: Int, actor: DesktopFixtureActor, scenario: String, requestHash: String,
    root: Data
  ) throws {
    guard !data.isEmpty, data.count <= 16384 else { throw Failure.invalidAcknowledgment }
    let object = try fixtureCanonicalObject(data)
    let fields: Set<String> = [
      "schema", "sequence", "actor", "scenario", "request_sha256", "generation_before",
      "generation_after", "state", "phase", "effect", "root", "root_address_verified", "outputs",
      "source", "worker", "pending_cleanup", "error",
    ]
    guard Set(object.keys) == fields, fixtureInteger(object["schema"]) == 1,
      fixtureInteger(object["sequence"]) == sequence, object["actor"] as? String == actor.tag,
      object["scenario"] as? String == scenario, object["request_sha256"] as? String == requestHash,
      object["state"] as? String == "succeeded", object["phase"] as? String == "complete",
      fixtureBoolean(object["root_address_verified"]) == true, object["error"] is NSNull,
      let acknowledgedRoot = object["root"] as? [String: Any],
      try fixtureCanonicalJSON(acknowledgedRoot) == root
    else { throw Failure.invalidAcknowledgment }
  }

}

func fixtureInteger(_ value: Any?) -> Int? {
  guard let value = value as? NSNumber, CFGetTypeID(value) != CFBooleanGetTypeID() else {
    return nil
  }
  return Int(value.stringValue)
}
func fixtureNativeIdentity(_ facts: stat) -> [String: Any] {
  [
    "device": Int64(facts.st_dev), "inode": UInt64(facts.st_ino), "mode": UInt32(facts.st_mode),
    "owner_uid": facts.st_uid,
  ]
}
func fixtureReadBytes(_ fd: Int32, maximum: Int) throws -> Data {
  var result = Data()
  var buffer = [UInt8](repeating: 0, count: min(8192, maximum + 1))
  while result.count <= maximum {
    let count = buffer.withUnsafeMutableBytes {
      pread(fd, $0.baseAddress!, min($0.count, maximum + 1 - result.count), off_t(result.count))
    }
    if count < 0 {
      if errno == EINTR { continue }
      throw NSError(domain: NSPOSIXErrorDomain, code: Int(errno))
    }
    if count == 0 { return result }
    result.append(contentsOf: buffer.prefix(count))
  }
  throw DesktopFixtureRequest.InvalidRequest.malformed
}

func fixtureBoolean(_ value: Any?) -> Bool? {
  guard let value = value as? NSNumber, CFGetTypeID(value) == CFBooleanGetTypeID() else {
    return nil
  }
  return value.boolValue
}
