import Darwin
import Foundation

enum RequestTransport {
  static func frame(_ paths: [String]) throws -> Data {
    guard paths.allSatisfy({ !$0.isEmpty && !$0.contains("\0") }) else {
      throw NSError(
        domain: NSPOSIXErrorDomain, code: Int(EINVAL),
        userInfo: [NSLocalizedDescriptionKey: "The selection contains an invalid file path."])
    }
    let payload = try JSONSerialization.data(withJSONObject: [
      "encoding": "posix-bytes", "paths": paths.map { Data($0.utf8).base64EncodedString() },
    ])
    guard var length = UInt32(exactly: payload.count)?.bigEndian else {
      throw NSError(domain: NSPOSIXErrorDomain, code: Int(E2BIG))
    }
    var result = withUnsafeBytes(of: &length) { Data($0) }
    result.append(payload)
    return result
  }
  static func send(_ frame: Data, to writer: FileHandle) {
    DispatchQueue.global(qos: .userInitiated).async {
      do {
        try writer.write(contentsOf: frame)
      } catch {
        trace("request-pipe write failed")
        try? writer.close()
      }
    }
  }

}
