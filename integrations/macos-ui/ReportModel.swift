import Foundation

struct Address: Decodable, Sendable {
  let display: String
  let text: String?
  let nativeBase64: String?

  var url: URL? {
    if let native = nativeBase64 {
      guard var bytes = Data(base64Encoded: native), !bytes.contains(0) else { return nil }
      bytes.append(0)
      return bytes.withUnsafeBytes { raw in
        guard let address = raw.bindMemory(to: CChar.self).baseAddress else { return nil }
        return URL(
          fileURLWithFileSystemRepresentation: address,
          isDirectory: false, relativeTo: nil)
      }
    }
    guard let text, !text.contains("\0") else { return nil }
    return URL(fileURLWithPath: text)
  }
}

struct Diagnostic: Decodable, Sendable {
  let code: String
  let message: String
}

struct Publication: Decodable, Sendable {
  let addressVerified: Bool?
  let finalAddress: Address?
}

struct Item: Decodable, Sendable {
  let index: Int
  let status: String
  let sourceRequest: Address
  let destinationRequest: Address?
  let publication: Publication?
  let error: Diagnostic?
  let warnings: [Diagnostic]

  var name: String {
    safeText((sourceRequest.display as NSString).lastPathComponent, multiline: false)
  }
  var label: String {
    switch status {
    case "created": return "Copy created"
    case "existing_verified": return "Matching copy already exists"
    case "would_create": return "Copy planned"
    case "cancelled": return "Processing stopped"
    case "not_run": return "Not processed"
    case "published_with_error": return "Copy published with an error"
    default: return "Copy not created"
    }
  }
  var isConflict: Bool {
    error?.code == "OUTPUT_CONFLICT"
  }
  var explanation: String {
    if isConflict {
      return
        "This destination could not be used safely. Nothing was overwritten.\n\nReview the problem details and destination before trying again."
    }
    if status == "published_with_error" {
      return
        "A copy was published, but the run could not confirm successful completion for this file. Review the details before using it."
    }
    if let error { return safeText(error.message) }
    if status == "would_create" { return "A copy was planned; no copy was created for this file." }
    if status == "existing_verified" {
      return
        "The existing copy exactly matches the copy this run would create. Nothing was overwritten."
    }
    if status == "created" {
      return
        "A separate copy was saved after checking its email structure and the content kept from the original. Your original file was not changed."
    }
    if status == "not_run" {
      return "This file was not processed. Review the run details before trying again."
    }
    return "Review the details for this file."
  }
  var acceptedURL: URL? {
    guard ["created", "existing_verified"].contains(status), publication?.addressVerified == true
    else { return nil }
    return publication?.finalAddress?.url
  }
}

struct Report: Decodable, Sendable {
  let version: String
  let ok: Bool
  let interrupted: Bool
  let batchError: Diagnostic?
  let summary: [String: Int]
  let items: [Item]
}

struct UIReceipt: Decodable, Sendable {
  let report: Report
  let processStatus: Int
  let details: [String]

  static func admit(_ data: Data, status: Int32, version: String) throws -> UIReceipt {
    guard hasValidUTF8Encoding(data) else {
      throw NSError(
        domain: "EMLReport", code: 1,
        userInfo: [NSLocalizedDescriptionKey: "The report is not valid UTF-8."])
    }
    let decoder = JSONDecoder()
    decoder.keyDecodingStrategy = .convertFromSnakeCase
    if let failure = try? decoder.decode(LauncherFailure.self, from: data),
      failure.processStatus == Int(status), failure.hasExpectedStatus
    {
      throw failure
    }
    let receipt = try decoder.decode(UIReceipt.self, from: data)
    guard receipt.processStatus == Int(status), receipt.report.version == version else {
      throw NSError(
        domain: "EMLReport", code: 1,
        userInfo: [
          NSLocalizedDescriptionKey:
            "The report version or process status disagrees with this application."
        ])
    }
    return receipt
  }
  var errorCount: Int {
    let itemErrors = report.items.filter { ["failed", "published_with_error"].contains($0.status) }
      .count
    let count = itemErrors + (report.batchError == nil ? 0 : 1)
    return count == 0 && processStatus != 0 && !stopped ? 1 : count
  }
  var warningCount: Int { report.items.reduce(0) { $0 + $1.warnings.count } }
  var stopped: Bool { report.interrupted || [129, 130, 143].contains(processStatus) }
  var successful: Bool { processStatus == 0 && report.ok && !stopped }
  var heading: String {
    if processStatus == 120 { return "The run couldn’t finish reporting" }
    if stopped { return "Processing stopped" }
    if successful {
      if report.items.isEmpty { return "Processing complete" }
      if report.summary["would_create", default: 0] > 0 { return "Copy plan ready" }
      if report.summary["created", default: 0] == 0 {
        return report.items.count == 1
          ? "A matching copy is already available" : "Matching copies are already available"
      }
      return report.summary["created", default: 0] == 1
        ? "Your copy is ready" : "Your copies are ready"
    }
    if report.items.count == 1 && report.items.first?.status == "failed" {
      return "Couldn’t create the copy"
    }
    return "The run needs attention"
  }
  var subtitle: String {
    let labels = [
      ("created", "created"), ("existing_verified", "already available"), ("failed", "failed"),
      ("published_with_error", "published with an error"), ("cancelled", "stopped"),
      ("not_run", "not processed"), ("would_create", "planned"),
    ]
    let counts = labels.compactMap { key, label -> String? in
      let count = report.summary[key, default: 0]
      return count > 0 ? "\(count) \(label)" : nil
    }
    return counts.isEmpty ? "No file outcomes were recorded." : counts.joined(separator: " · ")
  }
  var invocationNotice: String? {
    if processStatus == 120 {
      return
        "File results are available, but the run did not finish successfully. Review Details before using any copies."
    }
    if stopped {
      return
        "Stopping does not undo copies already created. The results below describe what completed."
    }
    if let error = report.batchError { return safeText(error.message) }
    if !successful
      && report.items.allSatisfy({ ["created", "existing_verified"].contains($0.status) })
    {
      return
        "The run did not finish successfully even though file results are available. Review Details."
    }
    return nil
  }
}

func safeText(_ text: String, multiline: Bool = true) -> String {
  var result = ""
  for scalar in text.unicodeScalars {
    switch scalar.properties.generalCategory {
    case .control, .format, .surrogate, .lineSeparator, .paragraphSeparator:
      if multiline && scalar == "\n" {
        result.append("\n")
      } else {
        result.append(String(format: "\\u{%X}", scalar.value))
      }
    default: result.append(String(scalar))
    }
  }
  return result
}

func hasValidUTF8Encoding(_ data: Data) -> Bool {
  String(decoding: data, as: UTF8.self).utf8.elementsEqual(data)
}
