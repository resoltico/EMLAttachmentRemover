import Foundation

func trace(_ event: String) {
  if ProcessInfo.processInfo.environment["EML_REMOVER_UI_TRACE"] == "1" {
    FileHandle.standardError.write(
      Data("UI \(ProcessInfo.processInfo.processIdentifier): \(event)\n".utf8))
  }
}
