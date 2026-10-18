// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import Foundation

@main
struct DesktopPreferenceSnapshot {
    static func main() throws {
        let environment = ProcessInfo.processInfo.environment
        guard environment["GITHUB_ACTIONS"] == "true",
              environment["RUNNER_ENVIRONMENT"] == "github-hosted",
              CommandLine.arguments.count == 2,
              let bundle = Bundle(url: URL(fileURLWithPath: CommandLine.arguments[1])),
              let identifier = bundle.bundleIdentifier else {
            throw NSError(domain: "EMLOwnedDesktopObservation", code: 1,
                          userInfo: [NSLocalizedDescriptionKey: "A fresh hosted runner and actual app bundle are required"])
        }
        // This fresh process only reads the actual app's persistent domain. It
        // never sets, removes, synchronizes or translates an owner preference.
        let domain = UserDefaults.standard.persistentDomain(forName: identifier) ?? [:]
        let bytes = try PropertyListSerialization.data(fromPropertyList: domain, format: .xml, options: 0)
        try FileHandle.standardOutput.write(contentsOf: bytes)
    }
}
