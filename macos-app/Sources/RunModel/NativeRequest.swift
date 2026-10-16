import Foundation

public enum RequestFailure: Error, Equatable, Sendable {
  case localFileRequired
  case invalidNativePath
  case invalidDestinationPreference
  case requestLimit
  case queueFull
  case reportSpaceRequired
}

extension NativePath {
  /// Foundation's native filesystem representation is the operational value.
  /// The decoded URL path is consulted only to reject an embedded NUL which
  /// cannot be represented by that NUL-terminated API.
  public init(fileURL url: URL) throws {
    guard url.isFileURL, url.scheme == "file", url.host == nil || url.host == "",
      url.user == nil, url.password == nil, url.port == nil,
      url.query == nil, url.fragment == nil,
      !url.path(percentEncoded: false).contains("\0")
    else {
      throw RequestFailure.localFileRequired
    }
    let bytes = url.withUnsafeFileSystemRepresentation { pointer -> Data? in
      guard let pointer else { return nil }
      var length = 0
      while pointer[length] != 0 { length += 1 }
      return Data(bytes: pointer, count: length)
    }
    guard let bytes, bytes.first == 47, bytes.count <= 32_768 else {
      throw RequestFailure.invalidNativePath
    }
    self.init(encoding: "unix-bytes", base64: bytes.base64EncodedString())
  }

  /// A relative CLI fixture is valid protocol data but is not an app navigation
  /// address. Actual app requests use absolute native file URLs throughout.
  public func fileURL() throws -> URL {
    let raw = try bytes()
    guard encoding == "unix-bytes", raw.first == 47 else {
      throw RequestFailure.invalidNativePath
    }
    var terminated = raw
    terminated.append(0)
    return terminated.withUnsafeBytes { buffer in
      // The NSURL initializer retains non-UTF8 native bytes. The modern
      // URL initializer replaces them with U+FFFD on current Foundation.
      // Bridging this native-backed NSURL preserves its representation.
      NSURL(
        fileURLWithFileSystemRepresentation: buffer.baseAddress!.assumingMemoryBound(
          to: CChar.self),
        isDirectory: false, relativeTo: nil) as URL
    }
  }
}

/// One String representation serves both argument-domain and saved defaults.
/// This type performs no defaults I/O or migration.
public enum DestinationPreference {
  public static let key = "copyDestinationBytes"

  public static func decode(_ value: String) throws -> NativePath? {
    if value.isEmpty { return nil }
    let path = NativePath(encoding: "unix-bytes", base64: value)
    do { _ = try path.fileURL() } catch { throw RequestFailure.invalidDestinationPreference }
    return path
  }

  public static func encode(_ path: NativePath?) throws -> String {
    guard let path else { return "" }
    _ = try path.fileURL()
    return path.base64
  }
}

public struct QueuedRequest: Identifiable, Sendable {
  public let id: UUID
  public let paths: [NativePath]
  public let destination: NativePath?
  public let label: String

  public init(fileURLs: [URL], destination: NativePath?) throws {
    guard !fileURLs.isEmpty, fileURLs.count <= 4096 else { throw RequestFailure.requestLimit }
    let paths = try fileURLs.map(NativePath.init(fileURL:))
    if let destination { _ = try destination.fileURL() }
    let total = try paths.reduce(0) { try $0 + $1.bytes().count }
    guard total <= 4_194_304 else { throw RequestFailure.requestLimit }
    self.id = UUID()
    self.paths = paths
    self.destination = destination
    self.label =
      fileURLs.count == 1 ? fileURLs[0].lastPathComponent : "\(fileURLs.count) selections"
    _ = try encoded()
  }

  public func encoded() throws -> Data {
    struct Document: Encodable {
      let `protocol` = 1
      let paths: [NativePath]
      let outputDirectory: NativePath?
      let dryRun = false

      enum CodingKeys: String, CodingKey {
        case `protocol`, paths
        case outputDirectory = "output_directory"
        case dryRun = "dry_run"
      }
      func encode(to encoder: any Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(`protocol`, forKey: .protocol)
        try container.encode(paths, forKey: .paths)
        // encodeIfPresent would omit a required null.
        try container.encode(outputDirectory, forKey: .outputDirectory)
        try container.encode(dryRun, forKey: .dryRun)
      }
    }
    let data = try JSONEncoder().encode(Document(paths: paths, outputDirectory: destination))
    guard data.count <= 8_388_608 else { throw RequestFailure.requestLimit }
    return data
  }
}

/// Serial admission and report reservations. Dismissal is explicit and never
/// discards a completed report merely to make a waiting run fit.
public struct RunQueue: Sendable {
  public private(set) var waiting: [QueuedRequest] = []
  public private(set) var active: UUID?
  public private(set) var reportIDs: [UUID] = []

  public init() {}

  public mutating func enqueue(_ request: QueuedRequest) throws {
    guard waiting.count < 8 else { throw RequestFailure.queueFull }
    waiting.append(request)
  }

  public mutating func next() throws -> QueuedRequest? {
    guard active == nil, !waiting.isEmpty else { return nil }
    guard reportIDs.count < 2 else { throw RequestFailure.reportSpaceRequired }
    let request = waiting.removeFirst()
    active = request.id
    reportIDs.append(request.id)
    return request
  }

  public mutating func complete(_ id: UUID) {
    if active == id { active = nil }
  }

  public mutating func dismiss(_ id: UUID) {
    guard active != id else { return }
    reportIDs.removeAll { $0 == id }
  }

  public mutating func removeWaiting(_ id: UUID) {
    waiting.removeAll { $0.id == id }
  }

  public mutating func cancelWaiting() { waiting.removeAll() }
}
