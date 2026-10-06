import Foundation

enum ReportText {
  static func details(_ admitted: UIReceipt, version: String, build: String) -> String {
    var readable = [
      "EML Attachment Remover \(version) (build \(build))",
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
        lines.append("Problem: " + error.guidance)
        lines.append("Technical problem (" + safeText(error.code) + "): " + safeText(error.message))
      }
      for warning in item.warnings {
        lines.append("Warning: " + warning.warningGuidance)
        lines.append(
          "Technical warning (" + safeText(warning.code) + "): " + safeText(warning.message))
      }
      let block = lines.joined(separator: "\n")
      readableCount += block.count
      readable.append(block)
      if readableCount > 1_000_000 {
        readable.append("[More information is available through Copy technical report.]")
        break
      }
    }
    if let batchError = admitted.report.batchError {
      readable.append("Run problem: " + batchError.guidance)
      readable.append(
        "Technical run problem (" + safeText(batchError.code) + "): " + safeText(batchError.message)
      )
    }
    return readable.joined(separator: "\n\n")
  }
}
