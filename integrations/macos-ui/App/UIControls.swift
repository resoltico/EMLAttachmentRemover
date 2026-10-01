import AppKit

@MainActor
enum UIControls {
  static func label(
    _ text: String, size: CGFloat = 13, weight: NSFont.Weight = .regular,
    color: NSColor = .labelColor
  ) -> NSTextField {
    let field = NSTextField(wrappingLabelWithString: text)
    field.font = .systemFont(ofSize: size, weight: weight)
    field.textColor = color
    field.isSelectable = true
    field.setContentCompressionResistancePriority(.required, for: .vertical)
    return field
  }
  static func button(_ title: String, action: Selector, target: AnyObject) -> NSButton {
    let button = NSButton(title: title, target: target, action: action)
    button.bezelStyle = .rounded
    return button
  }
}

@MainActor final class FlippedView: NSView { override var isFlipped: Bool { true } }
