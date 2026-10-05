import AppKit

@MainActor
final class EmailCopyService: NSObject {
  var onFiles: (([String]) -> Bool)?

  @objc(createEMLCopies:userData:error:)
  func createEMLCopies(
    _ pasteboard: NSPasteboard, userData _: String?,
    error: AutoreleasingUnsafeMutablePointer<NSString?>?
  ) {
    error?.pointee = nil
    let paths = FileSelection.paths(from: pasteboard)
    guard !paths.isEmpty else {
      error?.pointee = "Select EML files or folders in Finder first."
      return
    }
    guard onFiles?(paths) == true else {
      error?.pointee = "The app is closing. Reopen it and try again."
      return
    }
  }
}
