import Darwin
import Foundation

private enum CollectionFailure: Error { case bounds, ordinaryFile, changed, write, deadline }

private func admit(_ deadline: ContinuousClock.Instant) throws {
  guard ContinuousClock.now < deadline else { throw CollectionFailure.deadline }
}

private func writePrivate(_ data: Data, to url: URL, deadline: ContinuousClock.Instant) throws {
  try admit(deadline)
  let fd = url.withUnsafeFileSystemRepresentation { pointer in
    pointer.map { Darwin.open($0, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600) }
      ?? -1
  }
  guard fd >= 0 else { throw CollectionFailure.write }
  let file = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
  try file.write(contentsOf: data)
  try file.close()
}

private func snapshot(_ url: URL, deadline: ContinuousClock.Instant) throws -> Data {
  try admit(deadline)
  let fd = url.withUnsafeFileSystemRepresentation { pointer in
    pointer.map { Darwin.open($0, O_RDONLY | O_NONBLOCK | O_NOFOLLOW | O_CLOEXEC) } ?? -1
  }
  guard fd >= 0 else { throw CollectionFailure.ordinaryFile }
  let file = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
  var before = stat()
  guard fstat(fd, &before) == 0, before.st_mode & mode_t(S_IFMT) == mode_t(S_IFREG),
    before.st_size >= 0, before.st_size <= 2 * 1024 * 1024
  else {
    try file.close()
    throw CollectionFailure.ordinaryFile
  }
  var data = Data()
  while true {
    try admit(deadline)
    let chunk = try file.read(upToCount: min(65_536, 2 * 1024 * 1024 + 1 - data.count)) ?? Data()
    if chunk.isEmpty { break }
    data.append(chunk)
    guard data.count <= 2 * 1024 * 1024 else {
      try file.close()
      throw CollectionFailure.bounds
    }
  }
  var after = stat()
  let stable =
    fstat(fd, &after) == 0 && before.st_dev == after.st_dev && before.st_ino == after.st_ino
    && before.st_size == after.st_size && before.st_mtimespec.tv_sec == after.st_mtimespec.tv_sec
    && before.st_mtimespec.tv_nsec == after.st_mtimespec.tv_nsec
    && before.st_ctimespec.tv_sec == after.st_ctimespec.tv_sec
    && before.st_ctimespec.tv_nsec == after.st_ctimespec.tv_nsec
  try file.close()
  guard stable, data.count == before.st_size else { throw CollectionFailure.changed }
  return data
}

/// Scan only bounded current-run product report candidates. Missing, delayed,
/// unstable, unrelated or ambiguous reports do not establish normal termination.
/// The deadline is cooperative admission; the outer Controller is the process
/// backstop. A copied report survives a later metadata failure as partial evidence.
/// The caller supplies the existing deadline and preserves its original error.
@MainActor public func collectStartupReport(
  launch: OwnedLaunch, observedEnd: Date, evidenceDirectory: URL,
  deadline: ContinuousClock.Instant
) async throws -> String {
  try admit(deadline)
  let environment = ProcessInfo.processInfo.environment
  guard environment["CI"] == "true", environment["GITHUB_ACTIONS"] == "true",
    environment["RUNNER_ENVIRONMENT"] == "github-hosted"
  else { return "unobserved: hosted-owner context unavailable" }
  let directories = [
    FileManager.default.homeDirectoryForCurrentUser.appending(
      path: "Library/Logs/DiagnosticReports"),
    URL(filePath: "/Library/Logs/DiagnosticReports", directoryHint: .isDirectory),
  ]
  var reports: [String: Data] = [:]
  repeat {
    var entries = 0
    var candidates = 0
    for directory in directories {
      try admit(deadline)
      guard
        let iterator = FileManager.default.enumerator(
          at: directory,
          includingPropertiesForKeys: [.contentModificationDateKey],
          options: [.skipsSubdirectoryDescendants])
      else { continue }
      while let url = iterator.nextObject() as? URL {
        guard ContinuousClock.now < deadline else {
          return "unobserved: collection budget exhausted"
        }
        entries += 1
        guard entries <= 256 else { return "unobserved: candidate directory bound exceeded" }
        guard url.lastPathComponent.hasPrefix("EMLAttachmentRemover-"), url.pathExtension == "ips"
        else { continue }
        candidates += 1
        guard candidates <= 32 else { return "unobserved: report count bound exceeded" }
        guard
          let modified = try? url.resourceValues(forKeys: [.contentModificationDateKey])
            .contentModificationDate,
          modified >= launch.submittedAt
        else { continue }
        guard let bytes = try? snapshot(url, deadline: deadline),
          attribute(bytes, to: launch, observedEnd: observedEnd) != nil
        else { continue }
        if let previous = reports[url.path], previous != bytes {
          return "unobserved: attributed report changed"
        }
        reports[url.path] = bytes
      }
    }
    if reports.count > 1 { return "unobserved: ambiguous attributed reports" }
    if let match = uniqueReport(Array(reports.values), launch: launch, observedEnd: observedEnd) {
      guard ContinuousClock.now < deadline else { return "unobserved: collection budget exhausted" }
      try writePrivate(
        match.bytes, to: evidenceDirectory.appending(path: "startup-crash.ips"), deadline: deadline)
      let metadata: [String: Any] = [
        "basis": match.basis,
        "scope":
          "attributed report snapshot only; no crash-cause verdict, process authority or helper settlement proof",
      ]
      let metadataBytes = try JSONSerialization.data(
        withJSONObject: metadata, options: [.sortedKeys])
      try writePrivate(
        metadataBytes, to: evidenceDirectory.appending(path: "startup-crash-attribution.json"),
        deadline: deadline)
      return "attributed: " + match.basis
    }
    if ContinuousClock.now < deadline { try await Task.sleep(for: .milliseconds(100)) }
  } while ContinuousClock.now < deadline
  return "unobserved: no complete uniquely attributed report within allowance"
}
