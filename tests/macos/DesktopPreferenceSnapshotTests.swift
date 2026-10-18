// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import Foundation
import Darwin

#if PREFERENCE_OBSERVATION_TESTS
@main
struct DesktopPreferenceSnapshotTests {
    static func main() {
        do {
            func checked(_ input: Any) throws -> [String: Any] {
                let bytes = try JSONSerialization.data(withJSONObject: DesktopPreferenceSnapshot.observation(input))
                guard let value = try JSONSerialization.jsonObject(with: bytes) as? [String: Any] else { throw CocoaError(.coderInvalidValue) }
                return value
            }
            func require(_ passed: Bool, _ name: String) throws {
                guard passed else {
                    try FileHandle.standardError.write(contentsOf: Data("Preference observation failed: \(name)\n".utf8))
                    throw CocoaError(.coderInvalidValue)
                }
            }
            let data = try checked(Data([0, 1, 255]))
            try require(data["type"] as? String == "data" && data["base64"] as? String == "AAH/", "binary bookmark bytes")
            let date = try checked(Date(timeIntervalSinceReferenceDate: 0.25))
            try require(date["type"] as? String == "date" && date["reference_seconds_bits"] as? String == "4598175219545276416", "fractional date")
            let integer = try checked(NSNumber(value: UInt64.max))
            try require(integer["type"] as? String == "integer" && integer["value"] as? String == "18446744073709551615", "full unsigned integer")
            let boolean = try checked(NSNumber(value: true))
            let one = try checked(NSNumber(value: 1))
            try require(boolean["type"] as? String == "boolean" && one["type"] as? String == "integer", "boolean versus numeric one")
            let string = try checked("日本語")
            try require(string["type"] as? String == "string" && string["value"] as? String == "日本語", "Unicode string")
            let nested = try checked(["values": [Data([0, 1, 255]), "日本語"]])
            guard let dictionary = nested["values"] as? [String: Any], let array = dictionary["values"] as? [String: Any], let items = array["values"] as? [[String: Any]] else { throw CocoaError(.coderInvalidValue) }
            try require(items.count == 2 && items[0]["base64"] as? String == "AAH/" && items[1]["value"] as? String == "日本語", "nested property list")
            print("6 native preference observation cases passed; no preference-domain access")
        } catch { Darwin.exit(1) }
    }
}
#endif
