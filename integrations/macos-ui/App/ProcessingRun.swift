import Foundation

@MainActor
final class ProcessingRun {
  private var process: Process?
  private var lifetime: FileHandle?
  var isRunning: Bool { process != nil }
  func interrupt() { if let process, process.isRunning { process.interrupt() } }
  func start(
    paths: [String], version: String, preparing: @escaping @MainActor @Sendable () -> Void,
    completed:
      @escaping @MainActor @Sendable (Result<UIReceipt, Error>, String, Int32, Data) -> Void
  ) throws {
    let child = Process()
    child.executableURL = URL(fileURLWithPath: "/bin/sh")
    guard let resources = Bundle.main.resourceURL else {
      throw NSError(
        domain: "EMLRuntime", code: 6,
        userInfo: [NSLocalizedDescriptionKey: "The application resources are missing."])
    }
    child.arguments = [resources.appendingPathComponent("run-from-finder.sh").path] + paths
    child.environment = try environment(resources: resources)
    let owner = Pipe()
    lifetime = owner.fileHandleForWriting
    child.standardInput = owner.fileHandleForReading
    let output = Pipe()
    let errors = Pipe()
    child.standardOutput = output
    child.standardError = errors
    process = child
    do {
      try child.run()
      owner.fileHandleForReading.closeFile()
    } catch {
      lifetime?.closeFile()
      lifetime = nil
      process = nil
      throw error
    }
    // Drain both pipes concurrently. Waiting on one stream first can deadlock.
    let errorReader = DispatchQueue(label: "eml.stderr")
    let group = DispatchGroup()
    let errorBuffer = CapturedOutput()
    group.enter()
    errorReader.async {
      errorBuffer.set(errors.fileHandleForReading.readDataToEndOfFile())
      group.leave()
    }
    let expectedVersion = version
    DispatchQueue.global(qos: .userInitiated).async {
      let data = output.fileHandleForReading.readDataToEndOfFile()
      child.waitUntilExit()
      group.wait()
      let status = child.terminationStatus
      DispatchQueue.main.async {
        preparing()
      }
      let stderr = errorBuffer.get()
      let admitted = Result { try UIReceipt.admit(data, status: status, version: expectedVersion) }
      let raw = String(decoding: data, as: UTF8.self)
      DispatchQueue.main.async {
        self.lifetime?.closeFile()
        self.lifetime = nil
        self.process = nil
        completed(admitted, raw, status, stderr)
      }
    }

  }
  private func environment(resources: URL) throws -> [String: String] {
    var environment = ProcessInfo.processInfo.environment
    if environment["EML_REMOVER_PYTHON"] == nil {
      if let configured = try RuntimeConfiguration.python() {
        environment["EML_REMOVER_PYTHON"] = configured
      }

    }
    environment["EML_REMOVER_HOME"] = resources.path
    environment["EML_REMOVER_ZIPAPP"] =
      resources.appendingPathComponent("remove-eml-attachments.pyz").path
    environment["EML_REMOVER_UI_REPORT"] = "1"
    environment["EML_REMOVER_UI_OWNER_PIPE"] = "1"
    environment["EML_REMOVER_REVEAL"] = "0"
    return environment

  }

}

final class CapturedOutput: @unchecked Sendable {
  private let lock = NSLock()
  private var data = Data()
  func set(_ value: Data) {
    lock.lock()
    defer { lock.unlock() }
    data = value
  }
  func get() -> Data {
    lock.lock()
    defer { lock.unlock() }
    return data
  }
}
