import CrashEvidence
import Foundation
import Testing

private let image = UUID(uuidString: "1A2B3C4D-1111-2222-3333-123456789ABC")!
private let path = "/owned/App.app/Contents/MacOS/EMLAttachmentRemover"
private let bundle = "io.github.resoltico.emlattachmentremover"
private let base = ISO8601DateFormatter().date(from: "2026-10-17T00:00:00Z")!
private func launch(offset: TimeInterval = 0) -> OwnedLaunch {
  OwnedLaunch(
    pid: 1234, executablePath: path, imageUUID: image, architecture: "x86_64",
    bundleIdentifier: bundle, launchDate: base.addingTimeInterval(0.1234 + offset),
    submittedAt: base, receivedAt: base.addingTimeInterval(1))
}
private func report(mutate: (inout [String: Any]) -> Void = { _ in }) throws -> Data {
  var body: [String: Any] = [
    "pid": 1234, "procName": "EMLAttachmentRemover", "procPath": path,
    "procLaunch": "2026-10-17 00:00:00.1234 +0000", "captureTime": "2026-10-17 00:00:02.0000 +0000",
    "bundleInfo": ["CFBundleIdentifier": bundle],
    "usedImages": [
      [
        "name": "EMLAttachmentRemover", "path": path, "uuid": image.uuidString, "arch": "x86_64",
        "source": "P",
      ]
    ],
  ]
  mutate(&body)
  var data = try JSONSerialization.data(withJSONObject: [
    "bug_type": "309", "slice_uuid": image.uuidString,
  ])
  data.append(10)
  data.append(try JSONSerialization.data(withJSONObject: body))
  return data
}

@Test func exactOwnedLaunchAndMainImageMatch() throws {
  let bytes = try report()
  let match = attribute(bytes, to: launch(), observedEnd: base.addingTimeInterval(3))
  #expect(match?.bytes == bytes)
  #expect(match?.basis.hasPrefix("exact-path") == true)
}
@Test func staleSameBuildAndPIDDoNotIdentifyThisInvocation() throws {
  #expect(
    attribute(try report(), to: launch(offset: 0.1), observedEnd: base.addingTimeInterval(3)) == nil
  )
  let stale = try report { $0["procLaunch"] = "2026-10-16 00:00:00.1234 +0000" }
  #expect(attribute(stale, to: launch(), observedEnd: base.addingTimeInterval(3)) == nil)
}
@Test func changedPIDCaptureIntervalAndConcretePathAreRejected() throws {
  for mutation: ([String: Any]) in [
    ["pid": 1235], ["captureTime": "2026-10-17 00:01:00.0000 +0000"],
    ["procPath": "/different/App"],
  ] {
    let bytes = try report { body in body.merge(mutation) { _, new in new } }
    #expect(attribute(bytes, to: launch(), observedEnd: base.addingTimeInterval(3)) == nil)
  }
}
@Test func wrongUUIDAndFrameworkUUIDCannotStandForMainImage() throws {
  for changes in [
    ["name": "AppKit"], ["uuid": UUID().uuidString], ["arch": "arm64"], ["source": "S"],
  ] {
    let bytes = try report { body in
      var images = body["usedImages"] as! [[String: String]]
      images[0].merge(changes) { _, new in new }
      body["usedImages"] = images
    }
    #expect(attribute(bytes, to: launch(), observedEnd: base.addingTimeInterval(3)) == nil)
  }
}
@Test func redactedPathsRequireStrongIndependentCorroboration() throws {
  let bytes = try report { body in
    body["procPath"] = "/Users/USER/*/EMLAttachmentRemover"
    var images = body["usedImages"] as! [[String: String]]
    images[0]["path"] = "/Users/USER/*/EMLAttachmentRemover"
    body["usedImages"] = images
  }
  #expect(
    attribute(bytes, to: launch(), observedEnd: base.addingTimeInterval(3))?.basis.hasPrefix(
      "explicit-redacted-path") == true)
  #expect(attribute(bytes, to: launch(offset: 1), observedEnd: base.addingTimeInterval(3)) == nil)
}
@Test func ambiguityPartialDataAndWrongReportKindRemainUnobserved() throws {
  let bytes = try report()
  #expect(
    uniqueReport([bytes, bytes], launch: launch(), observedEnd: base.addingTimeInterval(3)) == nil)
  #expect(attribute(bytes.dropLast(), to: launch(), observedEnd: base.addingTimeInterval(3)) == nil)
  var wrong = try JSONSerialization.data(withJSONObject: ["bug_type": "288"])
  wrong.append(10)
  wrong.append(bytes[bytes.index(after: bytes.firstIndex(of: 10)!)...])
  #expect(attribute(wrong, to: launch(), observedEnd: base.addingTimeInterval(3)) == nil)
}

@Test func mixedPrecisionCannotPlaceCaptureBeforeTheKnownLaunch() throws {
  let bytes = try report { body in
    body["procLaunch"] = "2026-10-17 00:00:00 +0000"
    body["captureTime"] = "2026-10-17 00:00:00.050000 +0000"
  }
  #expect(attribute(bytes, to: launch(), observedEnd: base.addingTimeInterval(3)) == nil)
}

@Test func nonfiniteOrInvertedObservationDatesRemainUnattributable() throws {
  let bytes = try report { body in
    body["procLaunch"] = "2026-10-17 00:00:00 +0000"
  }
  #expect(
    attribute(bytes, to: launch(), observedEnd: Date(timeIntervalSince1970: .infinity)) == nil)
  let inverted = OwnedLaunch(
    pid: 1234, executablePath: path, imageUUID: image, architecture: "x86_64",
    bundleIdentifier: bundle, launchDate: base.addingTimeInterval(0.1234),
    submittedAt: base.addingTimeInterval(0.2), receivedAt: base.addingTimeInterval(0.1))
  #expect(attribute(bytes, to: inverted, observedEnd: base.addingTimeInterval(3)) == nil)
}

@Test func redactionDoesNotHideVisibleAppOrExecutableContradictions() throws {
  for value in [
    "unrelated<private>garbage", "/Users/USER/Other.app/Contents/MacOS/Other",
    "/Users/USER/*/Other.app/Contents/MacOS/EMLAttachmentRemover",
  ] {
    let bytes = try report { $0["procPath"] = value }
    #expect(attribute(bytes, to: launch(), observedEnd: base.addingTimeInterval(3)) == nil)
  }
}

@Test func timezoneEquivalentDatesMatchAndInvalidCalendarDatesRefuse() throws {
  let equivalent = try report { body in
    body["procLaunch"] = "2026-10-17 01:00:00.1234 +0100"
    body["captureTime"] = "2026-10-17 01:00:02.0000 +0100"
  }
  #expect(attribute(equivalent, to: launch(), observedEnd: base.addingTimeInterval(3)) != nil)
  let invalid = try report { $0["procLaunch"] = "2026-02-30 00:00:00.1234 +0000" }
  #expect(attribute(invalid, to: launch(), observedEnd: base.addingTimeInterval(3)) == nil)
}

@Test func conflictingHeaderProductIdentityRefuses() throws {
  let bytes = try report()
  let body = bytes[bytes.index(after: bytes.firstIndex(of: 10)!)...]
  for changes in [["bundleID": "different.bundle"], ["app_name": "Other"]] {
    var header: [String: Any] = ["bug_type": "309", "slice_uuid": image.uuidString]
    header.merge(changes) { _, new in new }
    var changed = try JSONSerialization.data(withJSONObject: header)
    changed.append(10)
    changed.append(body)
    #expect(attribute(changed, to: launch(), observedEnd: base.addingTimeInterval(3)) == nil)
  }
}
