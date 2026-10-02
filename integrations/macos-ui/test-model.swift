import Foundation

@main
struct ModelTests {
  static let version = "4.0.0"
  static func envelope(
    _ status: Int, _ itemStatus: String, interrupted: Bool = false, ok: Bool = false
  )
    throws -> Data
  {
    let value: [String: Any] = [
      "process_status": status, "details": [],
      "report": [
        "version": version, "ok": ok, "interrupted": interrupted,
        "summary": [itemStatus: 1, "total": 1],
        "items": [
          [
            "index": 0, "status": itemStatus,
            "source_request": ["display": "/tmp/sample.eml", "text": "/tmp/sample.eml"],
            "warnings": [],
          ]
        ],
      ],
    ]
    return try JSONSerialization.data(withJSONObject: value)
  }

  static func main() throws {
    let success = try UIReceipt.admit(envelope(0, "created", ok: true), status: 0, version: version)
    precondition(success.successful && success.heading == "Your copy is ready")
    let verified = try UIReceipt.admit(
      envelope(0, "existing_verified", ok: true), status: 0, version: version)
    precondition(verified.heading == "A matching copy is already available")
    let planned = try UIReceipt.admit(
      envelope(0, "would_create", ok: true), status: 0, version: version)
    precondition(planned.heading == "Copy plan ready" && planned.report.items[0].acceptedURL == nil)
    let late = try UIReceipt.admit(
      envelope(130, "created", ok: true), status: 130, version: version)
    precondition(late.errorCount == 0 && late.warningCount == 0)
    precondition(
      !late.successful && late.stopped && late.heading == "Processing stopped"
        && late.subtitle == "1 created")
    let flush = try UIReceipt.admit(
      envelope(120, "created", ok: true), status: 120, version: version)
    precondition(flush.errorCount == 1)
    precondition(!flush.successful && flush.heading == "The run couldn’t finish reporting")
    let published = try UIReceipt.admit(
      envelope(5, "published_with_error"), status: 5, version: version)
    precondition(published.errorCount == 1)
    precondition(
      published.report.items[0].acceptedURL == nil
        && published.subtitle.contains("published with an error"))
    for status in ["failed", "cancelled", "not_run", "would_create"] {
      let result = try UIReceipt.admit(envelope(1, status), status: 1, version: version)
      precondition(!result.successful && !result.report.items[0].label.isEmpty)
    }
    do {
      _ = try UIReceipt.admit(envelope(0, "created", ok: true), status: 1, version: version)
      preconditionFailure("status mismatch accepted")
    } catch {}
    do {
      _ = try UIReceipt.admit(envelope(0, "created", ok: true), status: 0, version: "other")
      preconditionFailure("version mismatch accepted")
    } catch {}
    do {
      _ = try UIReceipt.admit(Data("{}".utf8), status: 0, version: version)
      preconditionFailure("incomplete report accepted")
    } catch {}
    try checkInvalidUTF8()
    try checkAddresses()
    print("Native report model checks passed.")
  }

  static func checkInvalidUTF8() throws {
    precondition(hasValidUTF8Encoding(Data([0x65, 0xCC, 0x81])))
    precondition(!hasValidUTF8Encoding(Data([0xA0, 0x9B, 0x9B, 0x8D])))
    var invalid = try envelope(0, "created", ok: true)
    guard let range = invalid.range(of: Data("source_request".utf8)) else {
      preconditionFailure("missing regression field")
    }
    invalid.replaceSubrange(
      range.lowerBound..<(range.lowerBound + 4), with: [0xA0, 0x9B, 0x9B, 0x8D])
    precondition(
      (try? UIReceipt.admit(invalid, status: 0, version: version)) == nil,
      "invalid UTF-8 report accepted")
  }

  static func checkAddresses() throws {
    precondition(safeText("file\nname", multiline: false) == "file\\u{A}name")
    precondition(safeText("file\u{202E}.eml").contains("\\u{202E}"))
    let decoder = JSONDecoder()
    decoder.keyDecodingStrategy = .convertFromSnakeCase
    let native = try decoder.decode(
      Address.self, from: Data("{\"display\":\"native\",\"native_base64\":\"L3RtcC9uYXT/\"}".utf8))
    precondition(native.url != nil)
    let nul = try decoder.decode(
      Address.self, from: Data("{\"display\":\"bad\",\"native_base64\":\"YQA=\"}".utf8))
    precondition(nul.url == nil)
  }
}
