import AppKit

@MainActor
enum ResultPresentation {
  static func cell(_ item: Item) -> NSView {
    let completed = ["created", "existing_verified", "would_create"].contains(item.status)
    let (kind, tone) =
      !item.warnings.isEmpty && completed
      ? (Artwork.Kind.attention, Artwork.Tone.warning) : artwork(item.status)
    let icon = Artwork.view(kind, tone: tone)
    icon.widthAnchor.constraint(equalToConstant: 18).isActive = true
    icon.heightAnchor.constraint(equalToConstant: 18).isActive = true
    icon.setAccessibilityElement(false)
    let text = UIControls.label(item.label)
    text.maximumNumberOfLines = 1
    text.lineBreakMode = .byTruncatingTail
    text.toolTip = item.label
    text.setContentHuggingPriority(.defaultLow, for: .horizontal)
    let row = NSStackView(views: [icon, text])
    row.orientation = .horizontal
    row.alignment = .centerY
    row.spacing = 6
    row.toolTip = item.label
    return row
  }

  private static func artwork(_ status: String) -> (Artwork.Kind, Artwork.Tone) {
    switch status {
    case "created": return (.success, .success)
    case "existing_verified": return (.success, .accent)
    case "would_create": return (.identity, .accent)
    case "cancelled", "not_run": return (.stopped, .neutral)
    case "published_with_error": return (.attention, .warning)
    default: return (.attention, .failure)
    }
  }

  static func needsReview(_ item: Item) -> Bool {
    !["created", "existing_verified", "would_create"].contains(item.status)
      || !item.warnings.isEmpty
  }

  static func warnings(_ item: Item) -> NSView {
    let labels = item.warnings.prefix(3).map {
      UIControls.label("Warning: " + $0.warningGuidance)
    }
    let column = NSStackView(views: labels)
    column.orientation = .vertical
    column.alignment = .leading
    column.spacing = 6
    if item.warnings.count > 3 {
      column.addArrangedSubview(UIControls.label("More warnings are available in Details."))
    }
    for label in column.arrangedSubviews {
      label.widthAnchor.constraint(equalTo: column.widthAnchor).isActive = true
    }
    return column
  }
}
