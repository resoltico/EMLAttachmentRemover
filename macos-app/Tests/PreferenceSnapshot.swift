// Fresh-process, read-only observations of the public product preference domains.
// This tool never synchronizes, restores, deletes or writes preference values.
import CoreFoundation
import Foundation

private enum SnapshotFailure: Error {
  case arguments, environment, conversion, nonempty, changed, invalidBaseline
}

@main private struct PreferenceSnapshot {
  static func main() throws {
    let environment = ProcessInfo.processInfo.environment
    guard environment["CI"] == "true", environment["GITHUB_ACTIONS"] == "true",
      environment["RUNNER_ENVIRONMENT"] == "github-hosted"
    else { throw SnapshotFailure.environment }
    guard CommandLine.arguments.count == 3 || CommandLine.arguments.count == 4 else {
      throw SnapshotFailure.arguments
    }
    let mode = CommandLine.arguments[1]
    guard
      (mode == "before" && CommandLine.arguments.count == 3)
        || (mode == "after" && CommandLine.arguments.count == 4)
    else { throw SnapshotFailure.arguments }
    let domain = "io.github.resoltico.emlattachmentremover"
    // A fresh process avoids this reader's own prior CFPreferences cache. Empty
    // dictionaries observe no values; they do not prove physical-file absence or
    // rule out transient writes between these two observations.
    guard
      let anyHost =
        CFPreferencesCopyMultiple(
          nil, domain as CFString, kCFPreferencesCurrentUser, kCFPreferencesAnyHost)
        as? [String: Any],
      let currentHost =
        CFPreferencesCopyMultiple(
          nil, domain as CFString, kCFPreferencesCurrentUser, kCFPreferencesCurrentHost)
        as? [String: Any]
    else { throw SnapshotFailure.conversion }
    guard anyHost.isEmpty, currentHost.isEmpty else { throw SnapshotFailure.nonempty }
    let snapshot: [String: Any] = [
      "domain": domain, "any_host": anyHost, "current_host": currentHost,
    ]
    if mode == "after" {
      let baselineURL = URL(filePath: CommandLine.arguments[3])
      guard (try baselineURL.resourceValues(forKeys: [.fileSizeKey]).fileSize ?? Int.max) <= 16_384
      else {
        throw SnapshotFailure.invalidBaseline
      }
      let baseline = try PropertyListSerialization.propertyList(
        from: Data(contentsOf: baselineURL), options: [], format: nil)
      guard let baseline = baseline as? NSDictionary, baseline.isEqual(to: snapshot) else {
        throw SnapshotFailure.changed
      }
    }
    let bytes = try PropertyListSerialization.data(
      fromPropertyList: snapshot, format: .xml, options: 0)
    try bytes.write(to: URL(filePath: CommandLine.arguments[2]), options: .withoutOverwriting)
    print(
      "Fresh read-only product domain observations: current user, any host and current host; both empty"
    )
  }
}
