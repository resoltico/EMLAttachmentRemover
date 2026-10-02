import Foundation

enum ReportText {
  static func details(_ admitted: UIReceipt, version: String) -> String {
    var readable = [
      "EML Attachment Remover \(version)", "Invocation status: \(admitted.processStatus)",
      admitted.heading, admitted.subtitle,
    ]
    if let notice = admitted.invocationNotice { readable.append(notice) }
    var readableCount = 0
    for item in admitted.report.items {
      var lines = ["File: " + safeText(item.sourceRequest.display), "Result: " + item.label]
      if let destination = item.destinationRequest {
        lines.append("Output: " + safeText(destination.display))
      }
      if let error = item.error {
        lines.append("Error: " + error.code + "\n" + safeText(error.message))
      }
      for warning in item.warnings {
        lines.append("Warning: " + warning.code + "\n" + safeText(warning.message))
      }
      let block = lines.joined(separator: "\n")
      readableCount += block.count
      readable.append(block)
      if readableCount > 1_000_000 {
        readable.append("[Display truncated. Copy report includes the complete receipt.]")
        break
      }
    }
    if let batchError = admitted.report.batchError {
      readable.append("Batch error: " + batchError.code + "\n" + safeText(batchError.message))
    }
    return readable.joined(separator: "\n\n")
  }
}
