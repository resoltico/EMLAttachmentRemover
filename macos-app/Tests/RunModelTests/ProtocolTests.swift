import Foundation
import Testing

@testable import RunModel

@Test func presentationUsesSizeDifferenceAndIndependentPublicationEvidence() throws {
  let created = try decode(fixture("created-attachment"), exit: 0)
  #expect(created.items[0].savedBytes == 515)
  #expect(created.items[0].removed[0].bytes == 507)
  #expect(created.items[0].failureGuidance == nil)
  let published = try decode(fixture("published-with-error"), exit: 1)
  #expect(published.items[0].failureGuidance?.contains("Publication was observed") == true)
  #expect(published.items[0].failureGuidance?.contains("did not publish") == false)
  let unknown = try decode(fixture("unknown-publication"), exit: 1)
  #expect(unknown.items[0].failureGuidance?.contains("unconfirmed") == true)
  let notPublished = try decode(fixture("prepublication-failure"), exit: 1)
  #expect(notPublished.items[0].failureGuidance?.contains("did not publish") == true)
  let exported = try #require(
    JSONSerialization.jsonObject(with: created.technicalReport()) as? [String: Any])
  #expect((exported["items"] as? [[String: Any]])?.count == 1)
  #expect(!String(decoding: try created.technicalReport(), as: UTF8.self).contains("Hello Bob"))
}

@Test func transformedAliasesCannotSatisfyMissingWireKeysAndPhasesStayClosed() throws {
  let original = try frames(fixture("created"))
  for (wire, alias) in [
    ("source_bytes", "sourceBytes"), ("copy_bytes", "copyBytes"),
    ("existing_observation", "existingObservation"), ("stripped_fields", "strippedFields"),
  ] {
    var events = original
    events[1][alias] = events[1].removeValue(forKey: wire)
    #expect(throws: ProtocolFailure.malformedEvent) { try decode(encoded(events), exit: 0) }
  }
  for phase in ["", "future_phase"] {
    var events = try frames(fixture("prepublication-failure"))
    var error = try #require(events[1]["error"] as? [String: Any])
    error["phase"] = phase
    events[1]["error"] = error
    #expect(throws: ProtocolFailure.malformedEvent) { try decode(encoded(events), exit: 1) }
  }
}

@Test func displayAndNativeAggregateBoundsAreIndependentAndAllowLossyExpansion() throws {
  let template = try frames(fixture("created"))
  let native = Data(repeating: 255, count: 1024).base64EncodedString()
  let expanded = String(repeating: "\u{FFFD}", count: 1024)
  var start = template[0]
  start["total"] = 4096
  func item(_ index: Int, display: String, raw: String) throws -> ProtocolEvent {
    var value = template[1]
    value["index"] = index
    for key in ["source", "destination"] {
      value[key] = ["display": display, "native": ["encoding": "unix-bytes", "base64": raw]]
    }
    return try ProtocolEvent.decode(JSONSerialization.data(withJSONObject: value))
  }
  var valid = RunReport(expectedVersion: "test-version")
  try valid.accept(ProtocolEvent.decode(JSONSerialization.data(withJSONObject: start)))
  for index in 0..<4096 {
    try valid.accept(item(index, display: expanded, raw: native))
  }
  #expect(valid.items.count == 4096)  // 8MiB native, 24MiB lossy display.
  var invalid = RunReport(expectedVersion: "test-version")
  try invalid.accept(ProtocolEvent.decode(JSONSerialization.data(withJSONObject: start)))
  let long = String(repeating: "x", count: 32768)
  for index in 0..<384 {
    try invalid.accept(item(index, display: long, raw: "YQ=="))
  }
  #expect(throws: ProtocolFailure.runLimit) {
    try invalid.accept(item(384, display: long, raw: "YQ=="))
  }
  #expect(invalid.items.count == 384)
  var nativeOverflow = RunReport(expectedVersion: "test-version")
  try nativeOverflow.accept(ProtocolEvent.decode(JSONSerialization.data(withJSONObject: start)))
  for index in 0..<4095 {
    try nativeOverflow.accept(item(index, display: "a", raw: native))
  }
  var last = template[1]
  last["index"] = 4095
  last["source"] = [
    "display": "a",
    "native": [
      "encoding": "unix-bytes", "base64": Data(repeating: 255, count: 1025).base64EncodedString(),
    ],
  ]
  last["destination"] = ["display": "a", "native": ["encoding": "unix-bytes", "base64": native]]
  #expect(throws: ProtocolFailure.runLimit) {
    try nativeOverflow.accept(ProtocolEvent.decode(JSONSerialization.data(withJSONObject: last)))
  }
  #expect(nativeOverflow.items.count == 4095)  // Exactly 8MiB + one native byte.
}

private let cases: [(String, Int32, Int)] = [
  ("planned-create-unix", 0, 1), ("planned-create-windows", 0, 1),
  ("created", 0, 1), ("up-to-date", 0, 1), ("planned-existing", 0, 1),
  ("no-inputs", 1, 0), ("prepublication-failure", 1, 1),
  ("existing-sync-failure", 1, 1), ("published-with-error", 1, 1),
  ("unknown-publication", 1, 1), ("failed-root-admission", 1, 1),
  ("collection-limit", 1, 0), ("started-preedge-cancellation", 130, 1),
  ("postedge-cancellation", 130, 1), ("native-control-name", 1, 1),
  ("created-limited-sync", 0, 1), ("partial-namespace-observation", 1, 1),
  ("unattempted-required-edge", 1, 1), ("complete-cleanup-with-error", 1, 1),
  ("created-attachment", 0, 1), ("up-to-date-attachment", 0, 1),
  ("incomplete-item-failure", 1, 1),
]

private func fixture(_ name: String) throws -> Data {
  let root = URL(filePath: #filePath).deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
  let bytes = try Data(contentsOf: root.appending(path: "tests/contract/protocol/\(name).jsonl"))
  return Data(
    String(decoding: bytes, as: UTF8.self).replacingOccurrences(
      of: "@VERSION@", with: "test-version"
    ).utf8)
}

private func decode(_ data: Data, split: Int? = nil, exit: Int32) throws -> RunReport {
  var splitter = LFFrameSplitter()
  var report = RunReport(expectedVersion: "test-version")
  let chunks = split.map { [Data(data.prefix($0)), Data(data.dropFirst($0))] } ?? [data]
  for chunk in chunks {
    try splitter.append(chunk) { try report.accept(ProtocolEvent.decode($0)) }
  }
  let incomplete = splitter.finish()
  #expect(!incomplete)
  try report.finish(exitCode: exit)
  return report
}

@Test(arguments: cases)
func sharedScenariosAtEveryByteBoundary(name: String, exit: Int32, items: Int) throws {
  let bytes = try fixture(name)
  for boundary in 0...bytes.count {
    let report = try decode(bytes, split: boundary, exit: exit)
    #expect(report.items.count == items)
    #expect(report.summary?.exitCode == exit)
  }
}

@Test func gapsPreserveKnownPositionsAndTypedPublicationFacts() throws {
  let gap = try decode(fixture("failed-root-admission"), exit: 1)
  #expect(gap.items.map(\.index) == [1])
  #expect(gap.summary?.counts.notProcessed == 2)
  #expect(gap.items.count != gap.items[0].index + 1)
  let created = try decode(fixture("created"), exit: 0)
  #expect(created.items[0].revealPath != nil)
  let existing = try decode(fixture("up-to-date"), exit: 0)
  #expect(existing.items[0].revealPath != nil)
  for name in [
    "planned-existing", "unknown-publication", "published-with-error", "prepublication-failure",
  ] {
    let report = try decode(fixture(name), exit: name == "planned-existing" ? 0 : 1)
    #expect(report.items[0].revealPath == nil)
  }
  let unknown = try decode(fixture("unknown-publication"), exit: 1)
  #expect(unknown.items[0].publication.state == .unknown)
  let published = try decode(fixture("published-with-error"), exit: 1)
  #expect(published.items[0].publication.state == .published)
  let stopped = try decode(fixture("postedge-cancellation"), exit: 130)
  #expect(stopped.items[0].status == .created)
}

private func frames(_ data: Data) throws -> [[String: Any]] {
  try data.split(separator: 10).map {
    try #require(JSONSerialization.jsonObject(with: Data($0)) as? [String: Any])
  }
}
private func encoded(_ frames: [[String: Any]]) throws -> Data {
  var result = Data()
  for frame in frames {
    result.append(try JSONSerialization.data(withJSONObject: frame, options: [.sortedKeys]))
    result.append(10)
  }
  return result
}

@Test func additiveFieldsAndUnknownErrorCodesRemainFailures() throws {
  var events = try frames(fixture("prepublication-failure"))
  events[0]["additional"] = ["safe": true]
  var error = try #require(events[1]["error"] as? [String: Any])
  error["code"] = "FUTURE_REFUSAL"
  events[1]["error"] = error
  let report = try decode(encoded(events), exit: 1)
  #expect(report.items[0].status == .failed)
  #expect(report.items[0].error?.code == "FUTURE_REFUSAL")
}

@Test func missingRequiredNullsAndUnknownStatusOrReasonAreRejected() throws {
  let original = try frames(fixture("created"))
  for key in ["destination", "source_bytes", "copy_bytes", "plan", "existing_observation", "error"]
  {
    var events = original
    events[1].removeValue(forKey: key)
    #expect(throws: ProtocolFailure.self) { try decode(encoded(events), exit: 0) }
  }
  var events = original
  events[1]["status"] = "published_with_error"
  #expect(throws: ProtocolFailure.self) { try decode(encoded(events), exit: 0) }
  events = original
  events[1]["removed"] = [
    [
      "section": "1", "content_type": "text/plain", "filename": NSNull(), "bytes": 1,
      "reason": "future_reason",
    ]
  ]
  #expect(throws: ProtocolFailure.self) { try decode(encoded(events), exit: 0) }
}

@Test func orderingCountsProtocolAndActualExitAreIndependentChecks() throws {
  let original = try frames(fixture("created"))
  for order in [[1, 0, 2], [0, 0, 1, 2], [0, 1, 1, 2], [0, 1, 2, 2], [0, 2, 1]] {
    #expect(throws: ProtocolFailure.self) {
      try decode(encoded(order.map { original[$0] }), exit: 0)
    }
  }
  for index in [-1, 1, Int.max] {
    var events = original
    events[1]["index"] = index
    #expect(throws: ProtocolFailure.self) { try decode(encoded(events), exit: 0) }
  }
  var events = original
  events[0]["protocol"] = 2
  #expect(throws: ProtocolFailure.self) { try decode(encoded(events), exit: 0) }
  events = original
  events[2]["counts"] = [
    "created": Int.max, "up_to_date": 0, "planned": 0, "failed": 0, "not_processed": 0,
  ]
  #expect(throws: ProtocolFailure.self) { try decode(encoded(events), exit: 0) }
  #expect(throws: ProtocolFailure.exitDisagreement) { try decode(encoded(original), exit: 1) }
  #expect(throws: ProtocolFailure.missingSummary) {
    try decode(encoded(Array(original.prefix(2))), exit: 0)
  }
}

@Test func emptyOrIncompleteDiscoveryCannotManufactureSuccessfulSummary() throws {
  var events = try frames(fixture("no-inputs"))
  events[1]["run_error"] = NSNull()
  events[1]["exit_code"] = 0
  #expect(throws: ProtocolFailure.invalidCounts) { try decode(encoded(events), exit: 0) }
  events[1]["interrupted"] = true
  events[1]["exit_code"] = 130
  _ = try decode(encoded(events), exit: 130)
  events = try frames(fixture("created"))
  events[0]["collection_complete"] = false
  events[2]["collection_complete"] = false
  #expect(throws: ProtocolFailure.invalidCounts) { try decode(encoded(events), exit: 0) }
}

@Test func unqualifiedSuccessAndUnknownExistingEqualityNeverBecomeUsable() throws {
  let original = try frames(fixture("created"))
  for key in ["candidate_identity", "file_sync", "namespace_sync"] {
    var events = original
    var publication = try #require(events[1]["publication"] as? [String: Any])
    publication[key] = NSNull()
    events[1]["publication"] = publication
    #expect(throws: ProtocolFailure.self) { try decode(encoded(events), exit: 0) }
  }
  var events = try frames(fixture("existing-sync-failure"))
  var existing = try #require(events[1]["existing_observation"] as? [String: Any])
  existing["bytes_equal"] = NSNull()
  events[1]["existing_observation"] = existing
  let report = try decode(encoded(events), exit: 1)
  #expect(report.items[0].existingObservation?.bytesEqual == nil)
  #expect(report.items[0].revealPath == nil)
  events[1]["status"] = "up_to_date"
  events[1]["error"] = NSNull()
  #expect(throws: ProtocolFailure.self) { try decode(encoded(events), exit: 0) }
}

@Test func lfOnlyFramingAndIncompleteTail() throws {
  var splitter = LFFrameSplitter()
  var records: [Data] = []
  let bytes = Data("{\"text\":\"a\u{0085}\u{2028}b\"}\npartial".utf8)
  for byte in bytes { try splitter.append(Data([byte])) { records.append($0) } }
  #expect(records.count == 1)
  #expect(String(decoding: records[0], as: UTF8.self).contains("\u{2028}"))
  let incomplete = splitter.finish()
  #expect(incomplete)
  let repeatedFinish = splitter.finish()
  #expect(!repeatedFinish)
  var blank = LFFrameSplitter()
  #expect(throws: ProtocolFailure.malformedEvent) { try blank.append(Data([10])) { _ in } }
  #expect(throws: ProtocolFailure.malformedEvent) { try blank.append(Data("{}\n".utf8)) { _ in } }
}

@Test func incompleteReceiptsAreValidObservationsButCannotQualifySuccess() throws {
  let limited = try decode(fixture("created-limited-sync"), exit: 0)
  #expect(limited.items[0].revealPath != nil)
  #expect(limited.items[0].publication.fileSync.nativeCode == 45)
  for name in [
    "partial-namespace-observation", "unattempted-required-edge", "complete-cleanup-with-error",
  ] {
    let valid = try decode(fixture(name), exit: 1)
    #expect(valid.items[0].status == .failed)
    #expect(valid.items[0].revealPath == nil)
    var events = try frames(fixture(name))
    events[1]["status"] = "created"
    events[1]["source_bytes"] = 34
    events[1]["copy_bytes"] = 34
    events[1]["error"] = NSNull()
    var publication = try #require(events[1]["publication"] as? [String: Any])
    publication["state"] = "published"
    publication["candidate_identity"] = ["kind": "posix", "value": "1:2"]
    publication["address"] = "verified"
    publication["file_sync"] = ["method": "fsync", "state": "succeeded", "native_code": NSNull()]
    events[1]["publication"] = publication
    #expect(throws: ProtocolFailure.invalidReceipt) { try decode(encoded(events), exit: 0) }
    // The same incomplete namespace cannot qualify an existing copy either.
    if name != "complete-cleanup-with-error" {
      events[1]["status"] = "up_to_date"
      var prior = try frames(fixture("up-to-date"))[1]
      var existing = try #require(prior["existing_observation"] as? [String: Any])
      existing["namespace_sync"] = publication["namespace_sync"]
      events[1]["existing_observation"] = existing
      prior = try frames(fixture("up-to-date"))[1]
      events[1]["publication"] = prior["publication"]
      #expect(throws: ProtocolFailure.invalidReceipt) { try decode(encoded(events), exit: 0) }
    }
  }
}

@Test func frameAndRunBoundsIncludeLf() throws {
  var exact = LFFrameSplitter()
  var bytes = Data(repeating: 32, count: LFFrameSplitter.maximumFrameBytes - 1)
  bytes.append(10)
  var count = 0
  try exact.append(bytes) { frame in
    count += 1
    #expect(frame.count == bytes.count - 1)
  }
  #expect(count == 1)
  var over = LFFrameSplitter()
  #expect(throws: ProtocolFailure.frameLimit) {
    try over.append(Data(repeating: 32, count: LFFrameSplitter.maximumFrameBytes)) { _ in }
  }
  bytes.removeAll(keepingCapacity: false)
  var run = LFFrameSplitter()
  let block = Data(repeating: 32, count: 1023) + Data([10])
  for _ in 0..<(LFFrameSplitter.maximumRunBytes / 1024 - 1) {
    try run.append(block) { _ in }
  }
  try run.append(Data(repeating: 32, count: 1023)) { _ in }
  #expect(throws: ProtocolFailure.runLimit) { try run.append(Data([10])) { _ in } }
}

@Test func twoMaximalMetadataReportsRetainAllPartsAndRejectOneMore() throws {
  let template = try frames(fixture("created-attachment"))
  var start = template[0]
  start["total"] = 2
  var item = template[1]
  let part: [String: Any] = [
    "section": Array(repeating: "9999", count: 64).joined(separator: "."),
    "content_type": String(repeating: "t", count: 127) + "/" + String(repeating: "s", count: 127),
    "filename": String(repeating: "🦀", count: 255), "bytes": 268_435_456,
    "reason": "related_resource",
  ]
  // Contract dimension maxima form a conservative metadata envelope. They need
  // not all coexist in one MIME input and are not an exact heap-size promise.
  item["removed"] = Array(repeating: part, count: 10_000)
  item["index"] = 0
  var second = item
  second["index"] = 1
  var summary = template[2]
  summary["counts"] = [
    "created": 2, "up_to_date": 0, "planned": 0, "failed": 0, "not_processed": 0,
  ]
  let data = try encoded([start, item, second, summary])
  let firstReport = try decode(data, exit: 0)
  let secondReport = try decode(data, exit: 0)
  #expect(firstReport.items.reduce(0) { $0 + $1.removed.count } == 20_000)
  #expect(secondReport.items.reduce(0) { $0 + $1.removed.count } == 20_000)
  #expect(firstReport.items[1].removed[9999].filename == String(repeating: "🦀", count: 255))
  start["total"] = 3
  var report = RunReport(expectedVersion: "test-version")
  for frame in [start, item, second] {
    try report.accept(ProtocolEvent.decode(JSONSerialization.data(withJSONObject: frame)))
  }
  var extra = template[1]
  extra["index"] = 2
  #expect(throws: ProtocolFailure.malformedEvent) {
    try report.accept(ProtocolEvent.decode(JSONSerialization.data(withJSONObject: extra)))
  }
  #expect(report.items.count == 2)
}
