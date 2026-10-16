import Foundation

extension ItemEvent {
  public var statusLabel: String {
    switch status {
    case .created: "Copy created"
    case .upToDate: "Already up to date"
    case .planned: plan == .create ? "Copy planned" : "Existing copy needs verification"
    case .failed: "Could not complete"
    }
  }

  /// This is the actual file-size difference, not the sum of removed entities.
  public var savedBytes: UInt64? {
    guard let sourceBytes, let copyBytes, copyBytes <= sourceBytes else { return nil }
    return sourceBytes - copyBytes
  }

  public var failureGuidance: String? {
    guard status == .failed else { return nil }
    var text = "The original was not modified by this app. "
    switch publication.state {
    case .published:
      text += "Publication was observed, but a complete usable copy was not fully qualified. "
      switch publication.address {
      case .changed:
        text +=
          "Its requested address changed; do not assume that address still identifies this copy. "
      case .unknown, .notChecked: text += "Its current requested address is unverified. "
      case .verified: break
      }
    case .unknown:
      text += "Publication is unconfirmed; a complete copy might exist. "
    case .notPublished:
      text += "This item did not publish a new copy. "
    }
    if let existingObservation, existingObservation.bytesEqual == true {
      text += "An existing copy matched during comparison, but that alone does not qualify it. "
    }
    switch error?.phase {
    case .admission, .collection, .read:
      text += "Check the selected paths and access, then select the inputs again."
    case .prepare:
      text += "Review the message format or protection details in the technical report."
    case .existing:
      text += "Review the existing destination; this app never replaces it."
    case .stage, .write, .sync, .publish, .address, .cleanup:
      text +=
        "Review destination access, available space and the recorded synchronization and cleanup evidence before retrying."
    case .delivery:
      text += "Reporting was interrupted; retain the completed outcomes shown here."
    case nil:
      text += "Review the technical report before retrying."
    }
    return text
  }
}

extension RunReport {
  /// Explicit export only. Contains structured observations and authored protocol
  /// diagnostics, never source/copy message bytes or retained raw stdout frames.
  public func technicalReport() throws -> Data {
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
    return try encoder.encode(self)
  }
}

extension RunReport: Encodable {
  private enum CodingKeys: String, CodingKey { case start, items, summary, protocolFailure }
  public func encode(to encoder: any Encoder) throws {
    var container = encoder.container(keyedBy: CodingKeys.self)
    try container.encode(start, forKey: .start)
    try container.encode(items, forKey: .items)
    try container.encode(summary, forKey: .summary)
    try container.encode(failure.map(String.init(describing:)), forKey: .protocolFailure)
  }
}
