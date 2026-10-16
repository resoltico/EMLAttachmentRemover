import Foundation
import Testing

@testable import RunModel

@Test func exactInputPathAggregateStillAllowsSeparateDestination() throws {
  let raw = Data("/".utf8) + Data(repeating: 120, count: 32767)
  let url = try NativePath(encoding: "unix-bytes", base64: raw.base64EncodedString()).fileURL()
  let destination = try NativePath(fileURL: URL(filePath: "/tmp/selected-output"))
  let request = try QueuedRequest(
    fileURLs: Array(repeating: url, count: 128), destination: destination)
  #expect(try request.paths.reduce(0) { try $0 + $1.bytes().count } == 4_194_304)
  #expect(request.destination == destination)
  #expect(try request.encoded().count <= 8_388_608)
}

@Test func fileAdmissionPreservesNativeBytesAndRejectsNonlocalUrls() throws {
  let bytes = Data([47, 116, 109, 112, 47, 97, 255, 10, 27, 46, 101, 109, 108])
  let native = NativePath(encoding: "unix-bytes", base64: bytes.base64EncodedString())
  let url = try native.fileURL()
  #expect(try NativePath(fileURL: url).bytes() == bytes)
  #expect(
    try NativePath(fileURL: url).fileURL().withUnsafeFileSystemRepresentation { pointer in
      pointer != nil
    })
  for string in [
    "https://example.invalid/a.eml", "file://remote.invalid/tmp/a.eml",
    "file://user:password@host.invalid/tmp/a.eml", "file:///tmp/a.eml?query",
    "file:///tmp/a.eml#fragment", "file:///tmp/a%00b.eml",
  ] {
    let candidate = try #require(URL(string: string))
    #expect(throws: RequestFailure.self) { try NativePath(fileURL: candidate) }
  }
  let relative = NativePath(
    encoding: "unix-bytes", base64: Data("a.eml".utf8).base64EncodedString())
  #expect(try relative.bytes() == Data("a.eml".utf8))
  #expect(throws: RequestFailure.invalidNativePath) { try relative.fileURL() }
}

@Test func destinationStringDecoderDoesNotConsultOrWriteDefaults() throws {
  #expect(try DestinationPreference.decode("") == nil)
  let chosen = try NativePath(fileURL: URL(filePath: "/tmp/owned-destination"))
  let string = try DestinationPreference.encode(chosen)
  // The identical String goes through one decoder regardless of which domain
  // supplied it. No test reads or writes the owner's persistent defaults.
  #expect(try DestinationPreference.decode(string) == chosen)
  #expect(try DestinationPreference.encode(nil) == "")
  for invalid in ["%%%", "YQ==", "L3RtcAA=", "L3RtcA"] {
    #expect(throws: RequestFailure.invalidDestinationPreference) {
      try DestinationPreference.decode(invalid)
    }
  }
}

@Test func requestHasRequiredNullAndLosslessNativePath() throws {
  let request = try QueuedRequest(fileURLs: [URL(filePath: "/tmp/a.eml")], destination: nil)
  let document = try #require(
    JSONSerialization.jsonObject(with: request.encoded()) as? [String: Any])
  #expect(document["output_directory"] is NSNull)
  #expect(document["protocol"] as? Int == 1)
  #expect(document["dry_run"] as? Bool == false)
  let paths = try #require(document["paths"] as? [[String: String]])
  #expect(
    paths == [["encoding": "unix-bytes", "base64": Data("/tmp/a.eml".utf8).base64EncodedString()]])
  #expect(throws: RequestFailure.requestLimit) { try QueuedRequest(fileURLs: [], destination: nil) }
  #expect(throws: RequestFailure.requestLimit) {
    try QueuedRequest(
      fileURLs: Array(repeating: URL(filePath: "/tmp/a.eml"), count: 4097), destination: nil)
  }
}

@Test func queueReservesOnlyTwoReportsAndEightWaitingRequestsWithoutDiscard() throws {
  var queue = RunQueue()
  let first = try QueuedRequest(fileURLs: [URL(filePath: "/tmp/a.eml")], destination: nil)
  try queue.enqueue(first)
  let launched = try queue.next()
  #expect(launched?.id == first.id)
  #expect(queue.active == first.id)
  #expect(try queue.next() == nil)
  let destination = try NativePath(fileURL: URL(filePath: "/tmp/frozen"))
  var waiting: [QueuedRequest] = []
  for _ in 0..<8 {
    let request = try QueuedRequest(
      fileURLs: [URL(filePath: "/tmp/b.eml")], destination: destination)
    waiting.append(request)
    try queue.enqueue(request)
  }
  #expect(throws: RequestFailure.queueFull) { try queue.enqueue(first) }
  queue.dismiss(first.id)
  #expect(queue.reportIDs == [first.id])  // An active report cannot be dismissed.
  queue.complete(first.id)
  let second = try queue.next()
  #expect(second?.destination == destination)
  #expect(queue.reportIDs == [first.id, waiting[0].id])
  queue.complete(waiting[0].id)
  #expect(throws: RequestFailure.reportSpaceRequired) { try queue.next() }
  #expect(queue.waiting.map(\.id) == Array(waiting.dropFirst()).map(\.id))
  #expect(queue.reportIDs == [first.id, waiting[0].id])
  queue.dismiss(first.id)
  let third = try queue.next()
  #expect(third?.id == waiting[1].id)
  queue.removeWaiting(waiting[2].id)
  #expect(!queue.waiting.contains { $0.id == waiting[2].id })
  queue.cancelWaiting()
  #expect(queue.waiting.isEmpty)
  #expect(queue.active == waiting[1].id)
}
