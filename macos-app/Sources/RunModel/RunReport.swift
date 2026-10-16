import Foundation

/// Ordered observations retained for one run. Stop-button state is deliberately
/// outside this reducer: it cannot rewrite a received terminal item or summary.
public struct RunReport: Sendable {
  public private(set) var start: StartEvent?
  public private(set) var items: [ItemEvent] = []
  public private(set) var summary: SummaryEvent?
  public private(set) var failure: ProtocolFailure?
  private var observed = Counts()
  private var removedCount = 0
  private var nativePathBytes = 0
  private var displayPathBytes = 0
  private let expectedVersion: String

  public init(expectedVersion: String) { self.expectedVersion = expectedVersion }

  public mutating func accept(_ event: ProtocolEvent) throws {
    guard failure == nil else { throw failure! }
    do {
      guard summary == nil else { throw ProtocolFailure.invalidOrder }
      switch event {
      case .start(let value):
        guard start == nil, items.isEmpty else { throw ProtocolFailure.invalidOrder }
        guard value.protocol == 1, value.program == "remove-eml-attachments",
          value.version == expectedVersion
        else { throw ProtocolFailure.incompatibleHelper }
        guard (0...4096).contains(value.total) else { throw ProtocolFailure.invalidCounts }
        start = value
      case .item(let item):
        guard let start, (0..<start.total).contains(item.index),
          items.last.map({ $0.index < item.index }) ?? true
        else {
          throw ProtocolFailure.invalidOrder
        }
        try validate(item, dryRun: start.dryRun)
        switch item.status {
        case .created: observed.created += 1
        case .upToDate: observed.upToDate += 1
        case .planned: observed.planned += 1
        case .failed: observed.failed += 1
        }
        items.append(item)
      case .summary(let value):
        guard let start else { throw ProtocolFailure.invalidOrder }
        let c = value.counts
        guard
          [c.created, c.upToDate, c.planned, c.failed, c.notProcessed]
            .allSatisfy({ (0...4096).contains($0) })
        else {
          throw ProtocolFailure.invalidCounts
        }
        var expected = observed
        expected.notProcessed = start.total - observed.completed
        guard c == expected,
          value.collectionComplete == start.collectionComplete
        else {
          throw ProtocolFailure.invalidCounts
        }
        try value.runError?.validate()
        let expectedExit: Int32 =
          value.interrupted
          ? 130
          : (c.failed > 0 || value.runError != nil ? 1 : 0)
        guard value.exitCode == expectedExit,
          value.interrupted || value.runError != nil || c.notProcessed == 0,
          value.interrupted || value.runError != nil || start.total > 0,
          expectedExit != 0 || start.collectionComplete
        else {
          throw ProtocolFailure.invalidCounts
        }
        summary = value
      }
    } catch let value as ProtocolFailure {
      failure = value
      throw value
    } catch {
      failure = .malformedEvent
      throw ProtocolFailure.malformedEvent
    }
  }

  /// Invoke only after stdout EOF, process reaping and all reducer batches.
  public mutating func finish(exitCode: Int32) throws {
    if let failure { throw failure }
    guard let summary else {
      failure = .missingSummary
      throw ProtocolFailure.missingSummary
    }
    guard summary.exitCode == exitCode else {
      failure = .exitDisagreement
      throw ProtocolFailure.exitDisagreement
    }
  }

  private mutating func admit(_ path: ReportPath) throws {
    let raw = try path.native.bytes()
    // Lossy Unix display may expand each invalid byte to a three-byte U+FFFD.
    // It cannot be used to reconstruct or recharge the original native path.
    let displayBytes = path.display.utf8.count
    guard displayBytes <= 3 * 32_768,
      displayBytes <= 3 * 8_388_608 - displayPathBytes,
      raw.count <= 8_388_608 - nativePathBytes
    else {
      throw ProtocolFailure.runLimit
    }
    nativePathBytes += raw.count
    displayPathBytes += displayBytes
  }

  private mutating func validate(_ item: ItemEvent, dryRun: Bool) throws {
    try admit(item.source)
    if let destination = item.destination { try admit(destination) }
    try item.publication.validate()
    try item.existingObservation?.validate()
    try item.error?.validate()
    guard item.removed.count <= 10_000,
      item.removed.count <= 20_000 - removedCount,
      item.strippedFields.count <= 9,
      item.strippedFields.allSatisfy({ $0.utf8.count <= 128 }),
      item.sourceBytes.map({ $0 <= 268_435_456 }) ?? true,
      item.copyBytes.map({ $0 <= 268_435_456 }) ?? true,
      (item.sourceBytes == nil) == (item.copyBytes == nil),
      item.copyBytes.map({ $0 <= item.sourceBytes! }) ?? true
    else {
      throw ProtocolFailure.malformedEvent
    }
    for part in item.removed {
      guard !part.section.isEmpty, part.section.utf8.count <= 319,
        !part.contentType.isEmpty, part.contentType.utf8.count <= 255,
        (part.filename?.unicodeScalars.count ?? 0) <= 255,
        part.bytes <= 268_435_456
      else { throw ProtocolFailure.malformedEvent }
    }
    removedCount += item.removed.count
    let noSync =
      item.publication.fileSync.state == .notAttempted
      && item.publication.namespaceSync.state == .notAttempted
      && (item.existingObservation.map {
        $0.fileSync.state == .notAttempted && $0.namespaceSync.state == .notAttempted
      } ?? true)
    switch item.status {
    case .created:
      guard !dryRun, item.plan == nil, item.error == nil,
        item.sourceBytes != nil, item.destination != nil,
        item.publication.usableCopy, item.existingObservation == nil
      else {
        throw ProtocolFailure.invalidReceipt
      }
    case .upToDate:
      guard !dryRun, item.plan == nil, item.error == nil,
        item.sourceBytes != nil, item.destination != nil,
        item.publication.state == .notPublished,
        item.publication.fileSync.state == .notAttempted,
        item.publication.namespaceSync.state == .notAttempted,
        item.existingObservation?.usableCopy == true
      else {
        throw ProtocolFailure.invalidReceipt
      }
    case .planned:
      guard dryRun, let plan = item.plan, item.error == nil,
        item.sourceBytes != nil, item.destination != nil,
        item.publication.state == .notPublished, noSync
      else {
        throw ProtocolFailure.invalidReceipt
      }
      switch plan {
      case .create:
        guard item.existingObservation == nil else { throw ProtocolFailure.invalidReceipt }
      case .verifyExisting:
        guard let existing = item.existingObservation, existing.identity != nil,
          existing.bytesEqual == true, existing.stable
        else {
          throw ProtocolFailure.invalidReceipt
        }
      }
    case .failed:
      guard item.error != nil, item.plan == nil,
        !dryRun || (item.publication.state == .notPublished && noSync)
      else {
        throw ProtocolFailure.invalidReceipt
      }
    }
  }
}
