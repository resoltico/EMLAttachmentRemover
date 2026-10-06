import Foundation

@_cdecl("LLVMFuzzerTestOneInput")
public func fuzzReceipt(_ bytes: UnsafePointer<UInt8>, _ count: Int) -> Int32 {
  autoreleasepool {
    let data = Data(bytes: bytes, count: count)
    checkTextAndAddresses(data)
    checkReceipt(data)
    checkProgress(data)
    exercisePresentation(structuredReceipt(data))
  }
  return 0
}

private func checkTextAndAddresses(_ data: Data) {
  let text = String(decoding: data, as: UTF8.self)
  let singleLine = safeText(text, multiline: false)
  for forbidden in ["\0", "\u{1B}", "\u{202E}", "\n", "\r"] {
    precondition(!singleLine.contains(forbidden), "Unsafe display text")
  }
  let native = Address(display: text, text: nil, nativeBase64: data.base64EncodedString())
  let ordinary = Address(display: text, text: text, nativeBase64: nil)
  let nativeURL = native.url
  let ordinaryURL = ordinary.url
  checkFileURL(nativeURL)
  checkFileURL(ordinaryURL)
  let encoded = Address(display: text, text: nil, nativeBase64: text)
  if Data(base64Encoded: text) == nil {
    precondition(encoded.url == nil, "Invalid base64 address accepted")
  }
  if data.contains(0) { precondition(nativeURL == nil, "NUL native address accepted") }
  if text.contains("\0") { precondition(ordinaryURL == nil, "NUL text address accepted") }
}

private func checkReceipt(_ data: Data) {
  if !hasValidUTF8Encoding(data) {
    precondition(
      (try? UIReceipt.admit(data, status: 0, version: "fuzz")) == nil,
      "Invalid UTF-8 receipt accepted")
    return
  }
  let decoder = JSONDecoder()
  decoder.keyDecodingStrategy = .convertFromSnakeCase
  guard let decoded = try? decoder.decode(UIReceipt.self, from: data),
    let status = Int32(exactly: decoded.processStatus)
  else {
    precondition(
      (try? UIReceipt.admit(data, status: 0, version: "fuzz")) == nil,
      "Malformed receipt accepted")
    return
  }
  let admitted = try? UIReceipt.admit(data, status: status, version: "fuzz")
  if decoded.report.version != "fuzz" {
    precondition(admitted == nil, "Version mismatch accepted")
    return
  }
  guard let receipt = admitted else { preconditionFailure("Matching receipt refused") }
  precondition(
    (try? UIReceipt.admit(data, status: status ^ 1, version: "fuzz")) == nil,
    "Process status mismatch accepted")
  exercisePresentation(receipt)
}

private func exercisePresentation(_ receipt: UIReceipt) {
  precondition(!receipt.heading.isEmpty && !receipt.subtitle.isEmpty, "Empty outcome text")
  checkDisplayText(receipt.heading, multiline: false)
  checkDisplayText(receipt.subtitle, multiline: false)
  if let notice = receipt.invocationNotice { checkDisplayText(notice, multiline: true) }
  precondition(receipt.errorCount >= 0 && receipt.warningCount >= 0, "Negative diagnostic count")
  if receipt.processStatus != 0 || receipt.report.interrupted {
    precondition(!receipt.successful, "Unsuccessful invocation shown as success")
  }
  for item in receipt.report.items {
    checkDisplayText(item.name, multiline: false)
    checkDisplayText(item.label, multiline: false)
    checkDisplayText(item.explanation, multiline: true)
    checkFileURL(item.sourceRequest.url)
    checkFileURL(item.destinationRequest?.url)
    let accepted = item.acceptedURL
    checkFileURL(accepted)
    if !["created", "existing_verified"].contains(item.status)
      || item.publication?.addressVerified != true
    {
      precondition(accepted == nil, "Unverified output offered for reveal")
    }
  }
}

private func checkDisplayText(_ text: String, multiline: Bool) {
  for scalar in text.unicodeScalars {
    switch scalar.properties.generalCategory {
    case .control, .format, .surrogate, .lineSeparator, .paragraphSeparator:
      precondition(multiline && scalar == "\n", "Unsafe presentation text")
    default: break
    }
  }
}

private func checkFileURL(_ url: URL?) {
  if let url { precondition(url.isFileURL, "Non-file address accepted") }
}

// Reach presentation branches even when byte mutations cannot decode as JSON.
private func structuredReceipt(_ data: Data) -> UIReceipt {
  let bytes = Array(data.prefix(4)) + [0, 0, 0, 0]
  let outcomes = [
    "created", "existing_verified", "would_create", "failed", "published_with_error",
    "cancelled", "not_run", "unknown",
  ]
  let status = outcomes[Int(bytes[0]) % outcomes.count]
  let processStatuses = [0, 1, 120, 129, 130, 143, -1]
  let text = String(decoding: data, as: UTF8.self)
  let address = Address(display: text, text: text, nativeBase64: nil)
  let native = Address(display: text, text: nil, nativeBase64: data.base64EncodedString())
  let diagnostic = Diagnostic(code: "FUZZ", message: text)
  let item = Item(
    index: 0, status: status, sourceRequest: address, destinationRequest: native,
    publication: Publication(addressVerified: bytes[1] & 1 == 1, finalAddress: native),
    error: bytes[1] & 2 == 2 ? diagnostic : nil,
    warnings: bytes[1] & 4 == 4 ? [diagnostic] : [])
  let report = Report(
    version: "fuzz", ok: bytes[2] & 1 == 1, interrupted: bytes[2] & 2 == 2,
    batchError: bytes[2] & 4 == 4 ? diagnostic : nil,
    summary: [status: Int(bytes[3])], items: [item])
  return UIReceipt(
    report: report, processStatus: processStatuses[Int(bytes[2]) % processStatuses.count],
    details: [text])
}

private func checkProgress(_ data: Data) {
  var stream = ProgressStream()
  let update = Data(
    "EML_PROGRESS {\"schema\":1,\"stage\":\"processing\",\"completed\":0,\"total\":1}\n".utf8)
  let split = data.first.map { Int($0) % update.count } ?? 0
  precondition(stream.append(update.prefix(split)).isEmpty)
  let initial = stream.append(update.dropFirst(split))
  precondition(initial.count == 1 && initial[0].completed == 0 && initial[0].total == 1)
  for result in stream.append(Data("EML_PROGRESS ".utf8) + data + Data([10])) {
    precondition(result.schema == 1 && result.total == 1 && (0...1).contains(result.completed))
  }
  precondition(stream.finish().count <= 1024 * 1024)
}
