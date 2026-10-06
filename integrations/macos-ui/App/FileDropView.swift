import AppKit

@MainActor
final class FileDropView: NSView {
  var onFiles: (([String]) -> Void)?
  private var highlighted = false {
    didSet { needsDisplay = true }
  }

  override init(frame frameRect: NSRect) {
    super.init(frame: frameRect)
    registerForDraggedTypes([.fileURL])
    let words = NSStackView(views: [
      UIControls.label("Drop EML files or folders here", size: 17, weight: .semibold),
      UIControls.label("Or choose files or folders below.", color: .secondaryLabelColor),
    ])
    words.orientation = .vertical
    words.alignment = .centerX
    words.spacing = 8
    words.translatesAutoresizingMaskIntoConstraints = false
    addSubview(words)
    NSLayoutConstraint.activate([
      words.centerXAnchor.constraint(equalTo: centerXAnchor),
      words.centerYAnchor.constraint(equalTo: centerYAnchor),
      words.leadingAnchor.constraint(greaterThanOrEqualTo: leadingAnchor, constant: 20),
      words.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor, constant: -20),
      heightAnchor.constraint(equalToConstant: 180),
    ])
  }

  required init?(coder: NSCoder) { nil }

  override func draw(_ dirtyRect: NSRect) {
    let outline = NSBezierPath(roundedRect: bounds.insetBy(dx: 1, dy: 1), xRadius: 12, yRadius: 12)
    NSColor.controlBackgroundColor.setFill()
    outline.fill()
    (highlighted ? NSColor.controlAccentColor : NSColor.separatorColor).setStroke()
    outline.lineWidth = highlighted ? 2 : 1
    outline.stroke()
  }

  override func viewDidChangeEffectiveAppearance() { needsDisplay = true }

  @discardableResult
  func accept(_ pasteboard: NSPasteboard) -> Bool {
    let paths = FileSelection.paths(from: pasteboard)
    guard let onFiles, !paths.isEmpty else { return false }
    onFiles(paths)
    return true
  }

  override func draggingEntered(_ sender: any NSDraggingInfo) -> NSDragOperation {
    highlighted =
      onFiles != nil && sender.draggingSourceOperationMask.contains(.copy)
      && !FileSelection.paths(from: sender.draggingPasteboard).isEmpty
    return highlighted ? .copy : []
  }

  override func draggingExited(_ sender: (any NSDraggingInfo)?) { highlighted = false }

  override func performDragOperation(_ sender: any NSDraggingInfo) -> Bool {
    highlighted = false
    guard sender.draggingSourceOperationMask.contains(.copy) else { return false }
    return accept(sender.draggingPasteboard)
  }
}
