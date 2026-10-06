import Foundation

struct LauncherFailure: Decodable, Error, LocalizedError, Sendable {
  enum Kind: String, Decodable, Sendable {
    case pythonUnavailable = "python_unavailable"
    case processorUnavailable = "processor_unavailable"
    case reportStorageUnavailable = "report_storage_unavailable"
    case invalidRequest = "invalid_request"
  }
  let launcherError: Kind
  let processStatus: Int
  enum CodingKeys: CodingKey { case launcherError, processStatus, report }
  init(from decoder: any Decoder) throws {
    let values = try decoder.container(keyedBy: CodingKeys.self)
    guard !values.contains(.report) else {
      throw DecodingError.dataCorruptedError(
        forKey: .report, in: values,
        debugDescription: "Startup errors cannot contain processing results.")
    }
    launcherError = try values.decode(Kind.self, forKey: .launcherError)
    processStatus = try values.decode(Int.self, forKey: .processStatus)
  }
  var hasExpectedStatus: Bool {
    switch launcherError {
    case .pythonUnavailable: return processStatus == 9
    case .processorUnavailable: return processStatus == 3
    case .reportStorageUnavailable: return processStatus == 7
    case .invalidRequest: return processStatus == 2
    }
  }
  var heading: String {
    launcherError == .pythonUnavailable ? "CPython 3.14 is required" : "Couldn’t start processing"
  }
  var errorDescription: String? {
    switch launcherError {
    case .pythonUnavailable:
      return "Install CPython 3.14 or select its executable in the app installer."
    case .processorUnavailable:
      return "The bundled processor is unavailable. Install a verified copy of the app."
    case .reportStorageUnavailable:
      return
        "The app could not create temporary report storage. Check available space and permissions."
    case .invalidRequest: return "The processing request or launch settings are invalid."
    }
  }
}
