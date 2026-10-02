import Foundation

@_cdecl("LLVMFuzzerTestOneInput")
public func fuzzReceipt(_ bytes: UnsafePointer<UInt8>, _ count: Int) -> Int32 {
  autoreleasepool {
    let data = Data(bytes: bytes, count: count)
    checkTextAndAddresses(data)
    checkReceipt(data)
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
  if data.contains(0) { precondition(nativeURL == nil, "NUL native address accepted") }
  if text.contains("\0") { precondition(ordinaryURL == nil, "NUL text address accepted") }
}

private func checkReceipt(_ data: Data) {
  let decoder = JSONDecoder()
  decoder.keyDecodingStrategy = .convertFromSnakeCase
  guard let decoded = try? decoder.decode(UIReceipt.self, from: data),
    let status = Int32(exactly: decoded.processStatus)
  else { return }
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
  _ = receipt.heading
  _ = receipt.subtitle
  _ = receipt.invocationNotice
  _ = receipt.errorCount
  _ = receipt.warningCount
  if receipt.processStatus != 0 || receipt.report.interrupted {
    precondition(!receipt.successful, "Unsuccessful invocation shown as success")
  }
  for item in receipt.report.items {
    _ = item.name
    _ = item.label
    _ = item.explanation
    _ = item.sourceRequest.url
    _ = item.destinationRequest?.url
    let accepted = item.acceptedURL
    if !["created", "existing_verified"].contains(item.status)
      || item.publication?.addressVerified != true
    {
      precondition(accepted == nil, "Unverified output offered for reveal")
    }
  }
}
