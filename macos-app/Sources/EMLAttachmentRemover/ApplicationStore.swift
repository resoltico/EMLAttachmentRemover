import Foundation
import Observation
import RunModel

@MainActor @Observable
final class RunRecord: Identifiable {
  let id: UUID
  let label: String
  let destination: NativePath?
  let control: HelperRunControl
  var report: RunReport?
  var waiting = true
  var finished = false
  var stopRequests = 0
  var transportMessage: String?
  var stderr = Data()
  var termination: HelperExit?

  init(_ request: QueuedRequest) {
    id = request.id
    label = request.label
    destination = request.destination
    control = HelperRunControl(id: request.id)
  }
}

@MainActor @Observable
final class ApplicationStore {
  private var queue = RunQueue()
  private let supervisor = HelperProcess()
  private var running: Task<Void, Never>?
  var records: [RunRecord] = []
  var selected: UUID?
  var destination: NativePath?
  let destinationIsSessionOnly: Bool
  var destinationNeedsChoice = false
  var alertMessage: String?
  var needsReportSpace = false
  var showInputImporter = false
  var showDestinationImporter = false
  var showAbout = false
  var showLicense = false
  var exportDocument: ReportDocument?
  var showExporter = false
  var preparingExport = false

  init() {
    destinationIsSessionOnly = UserDefaults.standard
      .volatileDomain(forName: UserDefaults.argumentDomain)
      .keys.contains(DestinationPreference.key)
    do {
      let value: String
      if let object = UserDefaults.standard.object(forKey: DestinationPreference.key) {
        guard let string = object as? String else {
          throw RequestFailure.invalidDestinationPreference
        }
        value = string
      } else {
        value = ""
      }
      destination = try DestinationPreference.decode(value)
    } catch {
      destinationNeedsChoice = true
      alertMessage =
        "The selected copy destination could not be decoded. Choose an existing folder or Beside originals."
    }
  }

  var hasWork: Bool { queue.active != nil || !queue.waiting.isEmpty }
  var activeID: UUID? { queue.active }
  var selectedRecord: RunRecord? { records.first { $0.id == selected } }

  @discardableResult
  func admit(_ urls: [URL]) -> Bool {
    guard !destinationNeedsChoice else {
      alertMessage =
        "Choose an existing destination folder or Beside originals before creating copies."
      return false
    }
    do {
      let request = try QueuedRequest(fileURLs: urls, destination: destination)
      try queue.enqueue(request)
      records.insert(RunRecord(request), at: 0)
      selected = request.id
      launchNext()
      return true
    } catch RequestFailure.queueFull {
      alertMessage =
        "Eight requests are already waiting. Remove a queued request or wait for it to start."
    } catch {
      alertMessage =
        "Select local EML files or folders within the request limits. Remote URLs and invalid file addresses cannot be processed."
    }
    return false
  }

  func setDestinationFromPicker(_ url: URL?) {
    do {
      let selected = try url.map(NativePath.init(fileURL:))
      let value = try DestinationPreference.encode(selected)
      destination = selected
      destinationNeedsChoice = false
      if !destinationIsSessionOnly {
        UserDefaults.standard.set(value, forKey: DestinationPreference.key)
      }
    } catch {
      alertMessage = "Choose a local destination folder. The previous destination remains selected."
    }
  }

  private func launchNext() {
    guard running == nil else { return }
    let request: QueuedRequest
    do {
      guard let next = try queue.next() else { return }
      request = next
    } catch {
      needsReportSpace = true
      return
    }
    guard let record = records.first(where: { $0.id == request.id }) else { return }
    record.waiting = false
    running = Task {
      // Queue admission already froze this destination. A later picker action
      // cannot redirect it. Core still validates it again at the real boundary.
      let destinationAvailable = await blockingIO {
        guard let destination = request.destination else { return true }
        do {
          return try destination.fileURL().resourceValues(forKeys: [.isDirectoryKey]).isDirectory
            == true
        } catch { return false }
      }
      if record.stopRequests > 0 {
        record.transportMessage = "Stopped before the helper started. No input was processed."
      } else if !destinationAvailable {
        record.transportMessage =
          "The selected destination is unavailable. Choose an existing folder or Beside originals, then select the inputs again."
      } else {
        let completion = await supervisor.run(request: request, control: record.control) { report in
          record.report = report
        }
        record.report = completion.report
        record.stderr = completion.stderr
        record.termination = completion.termination
        record.transportMessage = completion.failure.map(Self.explain)
      }
      record.finished = true
      queue.complete(request.id)
      running = nil
      launchNext()
    }
  }

  func stop(_ id: UUID) {
    guard queue.active == id, let record = records.first(where: { $0.id == id }) else {
      return
    }
    record.stopRequests = record.control.requestStop().stops
    Task { await supervisor.stop(control: record.control) }
  }

  func dismiss(_ id: UUID) {
    guard let record = records.first(where: { $0.id == id }), record.finished else { return }
    queue.dismiss(id)
    records.removeAll { $0.id == id }
    if selected == id { selected = nil }
    needsReportSpace = false
    launchNext()
  }

  func dismissCompleted() {
    let ids = records.filter(\.finished).map(\.id)
    for id in ids { dismiss(id) }
  }

  func removeWaiting(_ id: UUID) {
    guard records.first(where: { $0.id == id })?.waiting == true else { return }
    queue.removeWaiting(id)
    records.removeAll { $0.id == id }
    if selected == id { selected = nil }
    needsReportSpace = false
    launchNext()
  }

  func cancelNewestWaiting() {
    guard let id = queue.waiting.last?.id else { return }
    removeWaiting(id)
  }

  func export(_ id: UUID) {
    guard !preparingExport, !showExporter, let record = records.first(where: { $0.id == id }),
      let report = record.report
    else { return }
    preparingExport = true
    let termination = record.termination
    let message = record.transportMessage
    let stderr = String(decoding: record.stderr, as: UTF8.self)
    Task {
      do {
        let bytes = try await Task.detached(priority: .userInitiated) {
          struct Export: Encodable {
            let report: RunReport
            let helperTermination: HelperExit?
            let helperMessage: String?
            let helperDiagnostic: String
          }
          let encoder = JSONEncoder()
          encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
          return try encoder.encode(
            Export(
              report: report, helperTermination: termination, helperMessage: message,
              helperDiagnostic: stderr))
        }.value
        exportDocument = ReportDocument(bytes: bytes)
        showExporter = true
      } catch {
        alertMessage =
          "The structured report could not be encoded. The retained outcomes remain available."
      }
      preparingExport = false
    }
  }

  func finishExport(_ result: Result<URL, any Error>) {
    exportDocument = nil
    if case .failure = result {
      alertMessage = "The report was not exported. Choose Export Report to try again."
    }
  }

  private static func explain(_ failure: HelperFailure) -> String {
    let prefix =
      "The app did not modify the originals. Earlier reported outcomes remain available. "
    switch failure {
    case .unavailable, .invalidVersion:
      return prefix
        + "The bundled helper is missing or does not match this app. Reinstall the matching native app."
    case .launch(let code):
      return prefix + "The bundled helper could not start (native code \(code))."
    case .transport(let code):
      return prefix
        + "Helper transport failed (native code \(code)). Complete copies may be unreported."
    case .ownership(let code):
      return prefix
        + "Helper process ownership could not be verified (native code \(code)). No further signal was sent using a lost process identity."
    case .invalidStream(let reason):
      return prefix
        + "The helper report could not be verified (\(reason)). Complete copies may be unreported."
    case .signalled(let code):
      return prefix
        + "The helper ended from signal \(code). Its report may be incomplete, and complete unreported copies may exist."
    case .requestRefused:
      return prefix
        + "The helper refused the request before processing. Review destination availability and the technical report."
    case .stoppedBeforeStart:
      return prefix + "Stopped before processing started."
    }
  }
}
