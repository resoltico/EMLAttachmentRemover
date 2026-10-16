import Foundation

/// Unlike decodeIfPresent, this wrapper requires the key even when its value is null.
@propertyWrapper
public struct RequiredNullable<Value: Codable & Sendable>: Codable, Sendable {
  public var wrappedValue: Value?

  public init(wrappedValue: Value?) { self.wrappedValue = wrappedValue }

  public init(from decoder: any Decoder) throws {
    let container = try decoder.singleValueContainer()
    wrappedValue = container.decodeNil() ? nil : try container.decode(Value.self)
  }

  public func encode(to encoder: any Encoder) throws {
    var container = encoder.singleValueContainer()
    if let wrappedValue { try container.encode(wrappedValue) } else { try container.encodeNil() }
  }
}

public struct NativePath: Codable, Equatable, Sendable {
  public let encoding: String
  public let base64: String

  public func bytes() throws -> Data {
    guard let bytes = Data(base64Encoded: base64),
      bytes.base64EncodedString() == base64
    else {
      throw ProtocolFailure.malformedEvent
    }
    switch encoding {
    case "unix-bytes":
      guard bytes.count <= 32_768, !bytes.contains(0) else {
        throw ProtocolFailure.malformedEvent
      }
    case "windows-utf16le":
      guard bytes.count <= 65_536, bytes.count.isMultiple(of: 2) else {
        throw ProtocolFailure.malformedEvent
      }
      var cursor = bytes.startIndex
      while cursor < bytes.endIndex {
        let next = bytes.index(after: cursor)
        guard bytes[cursor] != 0 || bytes[next] != 0 else {
          throw ProtocolFailure.malformedEvent
        }
        cursor = bytes.index(after: next)
      }
    default: throw ProtocolFailure.malformedEvent
    }
    return bytes
  }
}

public struct ReportPath: Codable, Sendable {
  public let display: String
  public let native: NativePath
}

public struct NativeIdentity: Codable, Sendable {
  public let kind: String
  public let value: String

  func validate() throws {
    guard ["posix", "windows"].contains(kind), !value.isEmpty,
      value.utf8.count <= 128, value.utf8.allSatisfy({ $0 >= 0x21 && $0 < 0x7F })
    else { throw ProtocolFailure.invalidReceipt }
  }
}

public enum AddressState: String, Codable, Sendable {
  case notChecked = "not_checked"
  case verified, changed, unknown
}
public enum SyncState: String, Codable, Sendable {
  case notAttempted = "not_attempted"
  case succeeded, unsupported, failed
}
public enum SyncMethod: String, Codable, Sendable {
  case notRequested = "not_requested"
  case fsync
  case fsyncFullfsync = "fsync_fullfsync"
  case fsyncFullfsyncUnsupported = "fsync_fullfsync_unsupported"
  case flushFileBuffers = "flush_file_buffers"
}
public struct SyncReceipt: Codable, Sendable {
  enum CodingKeys: String, CodingKey {
    case method, state
    case nativeCode = "native_code"
  }
  public let method: SyncMethod
  public let state: SyncState
  @RequiredNullable public var nativeCode: Int64?

  func validate() throws {
    guard (state == .notAttempted) == (method == .notRequested),
      state != .notAttempted || nativeCode == nil,
      state != .succeeded || nativeCode == nil || method == .fsyncFullfsyncUnsupported
    else {
      throw ProtocolFailure.invalidReceipt
    }
  }
}
public struct NamespaceReceipt: Codable, Sendable {
  enum CodingKeys: String, CodingKey {
    case method, state
    case nativeCode = "native_code"
    case requiredEdges = "required_edges"
    case completedEdges = "completed_edges"
    case failedEdgeIndex = "failed_edge_index"
  }
  public let method: SyncMethod
  public let state: SyncState
  @RequiredNullable public var nativeCode: Int64?
  public let requiredEdges: Int
  public let completedEdges: Int
  @RequiredNullable public var failedEdgeIndex: Int?

  func validate() throws {
    let receipt = SyncReceipt(method: method, state: state, nativeCode: nativeCode)
    try receipt.validate()
    guard (0...513).contains(requiredEdges), (0...requiredEdges).contains(completedEdges),
      failedEdgeIndex.map({ (0..<requiredEdges).contains($0) && $0 <= 512 }) ?? true,
      state != .notAttempted || (completedEdges == 0 && failedEdgeIndex == nil),
      state != .succeeded || failedEdgeIndex == nil
    else { throw ProtocolFailure.invalidReceipt }
  }
}
public enum CleanupState: String, Codable, Sendable {
  case notNeeded = "not_needed"
  case complete, residual, unknown
}
public struct CleanupReceipt: Codable, Sendable {
  enum CodingKeys: String, CodingKey {
    case state
    case residualName = "residual_name"
    case nativeCode = "native_code"
  }
  public let state: CleanupState
  @RequiredNullable public var residualName: NativePath?
  @RequiredNullable public var nativeCode: Int64?

  func validate() throws {
    if let residualName {
      let bytes = try residualName.bytes()
      guard state == .residual || state == .unknown, !bytes.isEmpty else {
        throw ProtocolFailure.invalidReceipt
      }
      if residualName.encoding == "unix-bytes" {
        guard bytes.count <= 255, !bytes.contains(47) else {
          throw ProtocolFailure.invalidReceipt
        }
      } else {
        guard bytes.count <= 510 else { throw ProtocolFailure.invalidReceipt }
        for offset in stride(from: 0, to: bytes.count, by: 2) {
          guard bytes[offset + 1] != 0 || ![47, 92].contains(bytes[offset]) else {
            throw ProtocolFailure.invalidReceipt
          }
        }
      }
    }
    guard state != .complete || residualName == nil,
      state != .notNeeded || (residualName == nil && nativeCode == nil)
    else { throw ProtocolFailure.invalidReceipt }
  }
}
public enum PublicationState: String, Codable, Sendable {
  case notPublished = "not_published"
  case published, unknown
}
public struct Publication: Codable, Sendable {
  enum CodingKeys: String, CodingKey {
    case state, address, cleanup
    case candidateIdentity = "candidate_identity"
    case fileSync = "file_sync"
    case namespaceSync = "namespace_sync"
  }
  public let state: PublicationState
  @RequiredNullable public var candidateIdentity: NativeIdentity?
  public let address: AddressState
  public let fileSync: SyncReceipt
  public let namespaceSync: NamespaceReceipt
  public let cleanup: CleanupReceipt

  func validate() throws {
    try candidateIdentity?.validate()
    try fileSync.validate()
    try namespaceSync.validate()
    try cleanup.validate()
    guard state != .published || candidateIdentity != nil,
      state != .unknown || address != .verified
    else {
      throw ProtocolFailure.invalidReceipt
    }
  }

  var usableCopy: Bool {
    state == .published && candidateIdentity != nil && address == .verified
      && fileSync.state == .succeeded && namespaceSync.state == .succeeded
      && namespaceSync.requiredEdges > 0
      && namespaceSync.completedEdges == namespaceSync.requiredEdges
      && namespaceSync.failedEdgeIndex == nil
      && (cleanup.state == .complete || cleanup.state == .notNeeded)
      && cleanup.nativeCode == nil
  }
}
public struct ExistingObservation: Codable, Sendable {
  enum CodingKeys: String, CodingKey {
    case identity, stable, address
    case bytesEqual = "bytes_equal"
    case fileSync = "file_sync"
    case namespaceSync = "namespace_sync"
  }
  @RequiredNullable public var identity: NativeIdentity?
  @RequiredNullable public var bytesEqual: Bool?
  public let stable: Bool
  public let address: AddressState
  public let fileSync: SyncReceipt
  public let namespaceSync: NamespaceReceipt

  func validate() throws {
    try identity?.validate()
    try fileSync.validate()
    try namespaceSync.validate()
  }

  var usableCopy: Bool {
    identity != nil && bytesEqual == true && stable && address == .verified
      && fileSync.state == .succeeded && namespaceSync.state == .succeeded
      && namespaceSync.requiredEdges > 0
      && namespaceSync.completedEdges == namespaceSync.requiredEdges
      && namespaceSync.failedEdgeIndex == nil
  }
}
public enum ErrorPhase: String, Codable, Sendable {
  case admission, collection, read, prepare, existing, stage, write, sync, publish, address,
    cleanup, delivery
}
public struct ReportError: Codable, Sendable {
  /// Open vocabulary: an unfamiliar code still presents a failure.
  public let code: String
  public let phase: ErrorPhase
  public let detail: String
  @RequiredNullable public var section: String?

  func validate() throws {
    guard !code.isEmpty, code.utf8.count <= 128,
      detail.unicodeScalars.count <= 1000, (section?.utf8.count ?? 0) <= 319
    else { throw ProtocolFailure.malformedEvent }
  }
}
public enum RemovalReason: String, Codable, Sendable {
  case attachment
  case relatedResource = "related_resource"
  case nonBodyPart = "non_body_part"
}
public struct RemovedPart: Codable, Sendable {
  enum CodingKeys: String, CodingKey {
    case section, filename, bytes, reason
    case contentType = "content_type"
  }
  public let section: String
  public let contentType: String
  @RequiredNullable public var filename: String?
  public let bytes: UInt64
  public let reason: RemovalReason
}
public enum ItemStatus: String, Codable, Sendable {
  case created
  case upToDate = "up_to_date"
  case planned, failed
}
public enum PlanAction: String, Codable, Sendable {
  case create
  case verifyExisting = "verify_existing"
}
public struct ItemEvent: Codable, Identifiable, Sendable {
  enum CodingKeys: String, CodingKey {
    case index, source, status, destination, removed, plan, publication, error
    case sourceBytes = "source_bytes"
    case copyBytes = "copy_bytes"
    case strippedFields = "stripped_fields"
    case existingObservation = "existing_observation"
  }
  public var id: Int { index }
  public let index: Int
  public let source: ReportPath
  public let status: ItemStatus
  @RequiredNullable public var destination: ReportPath?
  @RequiredNullable public var sourceBytes: UInt64?
  @RequiredNullable public var copyBytes: UInt64?
  public let removed: [RemovedPart]
  public let strippedFields: [String]
  @RequiredNullable public var plan: PlanAction?
  public let publication: Publication
  @RequiredNullable public var existingObservation: ExistingObservation?
  @RequiredNullable public var error: ReportError?

  /// Navigation uses an earlier receipt, never a fresh identity qualification.
  public var revealPath: NativePath? {
    guard publication.usableCopy || existingObservation?.usableCopy == true else { return nil }
    return destination?.native
  }
}
public struct StartEvent: Codable, Sendable {
  enum CodingKeys: String, CodingKey {
    case `protocol`, program, version, total
    case dryRun = "dry_run"
    case collectionComplete = "collection_complete"
  }
  public let `protocol`: Int
  public let program: String
  public let version: String
  public let dryRun: Bool
  public let total: Int
  public let collectionComplete: Bool
}
public struct Counts: Codable, Equatable, Sendable {
  enum CodingKeys: String, CodingKey {
    case created, planned, failed
    case upToDate = "up_to_date"
    case notProcessed = "not_processed"
  }
  public var created: Int = 0
  public var upToDate: Int = 0
  public var planned: Int = 0
  public var failed: Int = 0
  public var notProcessed: Int = 0

  public var completed: Int { created + upToDate + planned + failed }
}
public struct SummaryEvent: Codable, Sendable {
  enum CodingKeys: String, CodingKey {
    case counts, interrupted
    case collectionComplete = "collection_complete"
    case runError = "run_error"
    case exitCode = "exit_code"
  }
  public let counts: Counts
  public let interrupted: Bool
  public let collectionComplete: Bool
  @RequiredNullable public var runError: ReportError?
  public let exitCode: Int32
}

public enum ProtocolEvent: Sendable {
  case start(StartEvent)
  case item(ItemEvent)
  case summary(SummaryEvent)

  public static func decode(_ frame: Data) throws -> Self {
    struct Header: Decodable { let type: String }
    let decoder = JSONDecoder()
    do {
      switch try decoder.decode(Header.self, from: frame).type {
      case "start": return .start(try decoder.decode(StartEvent.self, from: frame))
      case "item": return .item(try decoder.decode(ItemEvent.self, from: frame))
      case "summary": return .summary(try decoder.decode(SummaryEvent.self, from: frame))
      default: throw ProtocolFailure.malformedEvent
      }
    } catch let failure as ProtocolFailure { throw failure } catch {
      throw ProtocolFailure.malformedEvent
    }
  }
}
