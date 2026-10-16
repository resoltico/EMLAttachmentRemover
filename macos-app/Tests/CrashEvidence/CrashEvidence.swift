import Foundation

public struct OwnedLaunch: Sendable {
  public let pid: Int
  public let executablePath: String
  public let imageUUID: UUID
  public let architecture: String
  public let bundleIdentifier: String
  public let launchDate: Date
  public let submittedAt: Date
  public let receivedAt: Date

  public init(
    pid: Int, executablePath: String, imageUUID: UUID, architecture: String,
    bundleIdentifier: String, launchDate: Date, submittedAt: Date, receivedAt: Date
  ) {
    self.pid = pid
    self.executablePath = executablePath
    self.imageUUID = imageUUID
    self.architecture = architecture
    self.bundleIdentifier = bundleIdentifier
    self.launchDate = launchDate
    self.submittedAt = submittedAt
    self.receivedAt = receivedAt
  }
}

public struct AttributedReport: Sendable {
  public let bytes: Data
  public let basis: String
}

private struct Header: Decodable {
  let bugType: String
  let sliceUUID: UUID?
  let bundleID: String?
  let appName: String?
  enum CodingKeys: String, CodingKey {
    case bugType = "bug_type"
    case sliceUUID = "slice_uuid"
    case bundleID
    case appName = "app_name"
  }
}
private struct BundleInfo: Decodable {
  let bundleIdentifier: String
  enum CodingKeys: String, CodingKey { case bundleIdentifier = "CFBundleIdentifier" }
}
private struct Image: Decodable {
  let name: String?
  let path: String?
  let uuid: UUID?
  let arch: String?
  let source: String?
}
private struct Body: Decodable {
  let pid: Int
  let procName: String
  let procPath: String
  let procLaunch: String
  let captureTime: String
  let bundleInfo: BundleInfo
  let usedImages: [Image]
}

private func redacted(_ path: String, executable: String) -> Bool {
  let url = URL(filePath: executable)
  let name = url.lastPathComponent
  let app = url.deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .lastPathComponent
  // Literal supported opaque forms only; '*' is never expanded as a wildcard.
  return [
    "<private>", "/Users/USER/*/\(name)",
    "/Users/USER/*/\(app)/Contents/MacOS/\(name)",
    "/<private>/\(app)/Contents/MacOS/\(name)",
  ].contains(path)
}

private func timestamp(_ text: String) -> (Date, TimeInterval)? {
  let pieces = text.split(separator: " ", omittingEmptySubsequences: false)
  guard pieces.count == 3, pieces[0].count == 10, pieces[2].count == 5 else { return nil }
  let seconds = pieces[1].split(separator: ".", omittingEmptySubsequences: false)
  guard seconds.count <= 2 else { return nil }
  let precision = seconds.count == 2 ? seconds[1].count : 0
  guard precision <= 6,
    seconds.count == 1 || (precision > 0 && seconds[1].utf8.allSatisfy { (48...57).contains($0) })
  else { return nil }
  let formatter = DateFormatter()
  formatter.locale = Locale(identifier: "en_US_POSIX")
  formatter.calendar = Calendar(identifier: .gregorian)
  formatter.isLenient = false
  formatter.dateFormat = "yyyy-MM-dd HH:mm:ss Z"
  guard let base = formatter.date(from: "\(pieces[0]) \(seconds[0]) \(pieces[2])") else {
    return nil
  }
  let fraction = precision == 0 ? 0 : Double("0.\(seconds[1])") ?? -1
  guard fraction >= 0 else { return nil }
  return (base.addingTimeInterval(fraction), 0.5 * pow(10, -Double(precision)) + 0.000001)
}

/// Attribute one bounded immutable report snapshot. This supplies no process
/// capability, helper-absence proof, or conclusion about the cause of exit.
public func attribute(_ bytes: Data, to launch: OwnedLaunch, observedEnd: Date) -> AttributedReport?
{
  guard launch.pid > 0, bytes.count <= 2 * 1024 * 1024,
    [launch.launchDate, launch.submittedAt, launch.receivedAt, observedEnd].allSatisfy({
      $0.timeIntervalSince1970.isFinite
    }),
    launch.submittedAt <= launch.launchDate, launch.launchDate <= launch.receivedAt,
    launch.receivedAt <= observedEnd,
    let newline = bytes.firstIndex(of: 10),
    let header = try? JSONDecoder().decode(Header.self, from: bytes[..<newline]),
    header.bugType == "309",
    let body = try? JSONDecoder().decode(Body.self, from: bytes[bytes.index(after: newline)...]),
    body.pid == launch.pid,
    body.procName == URL(filePath: launch.executablePath).lastPathComponent,
    body.bundleInfo.bundleIdentifier == launch.bundleIdentifier,
    let (started, startTolerance) = timestamp(body.procLaunch),
    let (captured, captureTolerance) = timestamp(body.captureTime),
    abs(started.timeIntervalSince(launch.launchDate)) <= startTolerance,
    started >= launch.submittedAt.addingTimeInterval(-startTolerance),
    started <= launch.receivedAt.addingTimeInterval(startTolerance),
    captured >= started.addingTimeInterval(-captureTolerance),
    captured.addingTimeInterval(captureTolerance) >= launch.launchDate,
    captured <= observedEnd.addingTimeInterval(captureTolerance)
  else { return nil }
  if let uuid = header.sliceUUID, uuid != launch.imageUUID { return nil }
  if let identifier = header.bundleID, identifier != launch.bundleIdentifier { return nil }
  if let name = header.appName, name != URL(filePath: launch.executablePath).lastPathComponent {
    return nil
  }
  let expectedName = URL(filePath: launch.executablePath).lastPathComponent
  let candidates = body.usedImages.filter { $0.name == expectedName && $0.source == "P" }
  guard candidates.count == 1, let main = candidates.first,
    main.uuid == launch.imageUUID, main.arch == launch.architecture, let imagePath = main.path
  else { return nil }
  let exact = body.procPath == launch.executablePath
  guard exact || redacted(body.procPath, executable: launch.executablePath),
    imagePath == launch.executablePath || redacted(imagePath, executable: launch.executablePath)
  else { return nil }
  return AttributedReport(
    bytes: bytes,
    basis: exact && imagePath == launch.executablePath
      ? "exact-path+launch-instance+main-image-uuid-architecture-bundle"
      : "explicit-redacted-path+launch-instance+main-image-uuid-architecture-bundle; path equality unproven"
  )
}

/// Accept exactly one attributable snapshot; duplicates/ambiguity stay unknown.
public func uniqueReport(_ snapshots: [Data], launch: OwnedLaunch, observedEnd: Date)
  -> AttributedReport?
{
  let matches = snapshots.compactMap { attribute($0, to: launch, observedEnd: observedEnd) }
  return matches.count == 1 ? matches[0] : nil
}
