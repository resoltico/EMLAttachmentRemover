// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import Foundation
import CoreFoundation

#if !PREFERENCE_OBSERVATION_TESTS
@main
#endif
struct DesktopPreferenceSnapshot {
    // JSON cannot directly carry plist Data/Date and must not round integers or
    // conflate NSNumber booleans with numeric values. Preserve native value types.
    static func observation(_ value: Any) throws -> Any {
        switch value {
        case let value as Data:
            return ["type": "data", "base64": value.base64EncodedString()]
        case let value as Date:
            return ["type": "date", "reference_seconds_bits": String(value.timeIntervalSinceReferenceDate.bitPattern)]
        case let value as NSNumber:
            if CFGetTypeID(value) == CFBooleanGetTypeID() {
                return ["type": "boolean", "value": value.boolValue]
            }
            let representation = String(cString: value.objCType)
            if representation == "f" || representation == "d" {
                return ["type": "real", "representation": representation, "bits": String(value.doubleValue.bitPattern)]
            }
            return ["type": "integer", "representation": representation, "value": value.stringValue]
        case let value as String:
            return ["type": "string", "value": value]
        case let value as [Any]:
            return ["type": "array", "values": try value.map(observation)]
        case let value as [String: Any]:
            return ["type": "dictionary", "values": try value.mapValues(observation)]
        default:
            throw NSError(domain: "EMLOwnedDesktopObservation", code: 2,
                          userInfo: [NSLocalizedDescriptionKey: "Unsupported native preference value type"])
        }
    }
    static func main() throws {
        let environment = ProcessInfo.processInfo.environment
        guard environment["GITHUB_ACTIONS"] == "true",
              environment["RUNNER_ENVIRONMENT"] == "github-hosted",
              CommandLine.arguments.count == 3,
              let bundle = Bundle(url: URL(fileURLWithPath: CommandLine.arguments[1])),
              let identifier = bundle.bundleIdentifier else {
            throw NSError(domain: "EMLOwnedDesktopObservation", code: 1,
                          userInfo: [NSLocalizedDescriptionKey: "A fresh hosted runner and actual app bundle are required"])
        }
        // This fresh process only reads the actual app's persistent domain. It
        // never sets, removes, synchronizes or translates an owner preference.
        let domain = UserDefaults.standard.persistentDomain(forName: identifier) ?? [:]
        let bytes = try PropertyListSerialization.data(fromPropertyList: domain, format: .xml, options: 0)
        try bytes.write(to: URL(fileURLWithPath: CommandLine.arguments[2]), options: .withoutOverwriting)
        let values = try domain.mapValues(observation)
        try FileHandle.standardOutput.write(contentsOf: JSONSerialization.data(withJSONObject: values, options: [.sortedKeys]))
    }
}
