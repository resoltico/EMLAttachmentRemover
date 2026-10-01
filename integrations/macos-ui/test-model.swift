import Foundation

@main
struct ModelTests {
    static func main() throws {
        let version = "4.0.0"
        func envelope(_ status: Int, _ itemStatus: String, interrupted: Bool = false, ok: Bool = false) -> Data {
            let value: [String: Any] = ["process_status": status, "details": [], "report": ["version": version, "ok": ok, "interrupted": interrupted, "summary": [itemStatus: 1, "total": 1], "items": [["index": 0, "status": itemStatus, "source_request": ["display": "/tmp/sample.eml", "text": "/tmp/sample.eml"], "warnings": []]]]]
            return try! JSONSerialization.data(withJSONObject: value)
        }
        let success = try UIReceipt.admit(envelope(0, "created", ok: true), status: 0, version: version)
        precondition(success.successful && success.heading == "Your copy is ready")
        let verified = try UIReceipt.admit(envelope(0, "existing_verified", ok: true), status: 0, version: version)
        precondition(verified.heading == "Existing copies verified")
        let planned = try UIReceipt.admit(envelope(0, "would_create", ok: true), status: 0, version: version)
        precondition(planned.heading == "Copy plan ready" && planned.report.items[0].acceptedURL == nil)
        let late = try UIReceipt.admit(envelope(130, "created", ok: true), status: 130, version: version)
        precondition(late.errorCount == 0 && late.warningCount == 0)
        precondition(!late.successful && late.stopped && late.heading == "Processing stopped" && late.subtitle == "1 created")
        let flush = try UIReceipt.admit(envelope(120, "created", ok: true), status: 120, version: version)
        precondition(flush.errorCount == 1)
        precondition(!flush.successful && flush.heading == "The run couldn’t finish reporting")
        let published = try UIReceipt.admit(envelope(5, "published_with_error"), status: 5, version: version)
        precondition(published.errorCount == 1)
        precondition(published.report.items[0].acceptedURL == nil && published.subtitle.contains("published with an error"))
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
        precondition(safeText("file\nname", multiline: false) == "file\\u{A}name")
        precondition(safeText("file\u{202E}.eml").contains("\\u{202E}"))
        let native = try JSONDecoder().decode(Address.self, from: Data("{\"display\":\"native\",\"native_base64\":\"L3RtcC9uYXT/\"}".utf8))
        precondition(native.url != nil)
        let nul = try JSONDecoder().decode(Address.self, from: Data("{\"display\":\"bad\",\"native_base64\":\"YQA=\"}".utf8))
        precondition(nul.url == nil)
        print("Native report model checks passed.")
    }
}
