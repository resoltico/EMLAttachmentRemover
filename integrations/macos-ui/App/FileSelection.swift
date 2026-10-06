import AppKit

enum FileSelection {
  static func paths(from pasteboard: NSPasteboard) -> [String] {
    let urls =
      pasteboard.readObjects(
        forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL] ?? []
    guard urls.count == pasteboard.pasteboardItems?.count else { return [] }
    let paths = urls.compactMap(localPath)
    return paths.count == urls.count ? paths : []
  }

  private static func localPath(_ url: URL) -> String? {
    guard url.isFileURL, ["", "localhost"].contains(url.host?.lowercased() ?? ""),
      url.user == nil, url.password == nil, url.port == nil,
      url.query == nil, url.fragment == nil,
      let path = url.path(percentEncoded: true).removingPercentEncoding,
      path.hasPrefix("/"), !path.utf8.contains(0)
    else { return nil }
    return path
  }
}
