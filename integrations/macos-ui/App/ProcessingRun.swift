import Darwin
import Foundation

@MainActor
final class ProcessingRun {
  private var process: Process?
  private var lifetime: FileHandle?
  var isRunning: Bool { process != nil }
  func interrupt() {
    if let process, process.isRunning {
      // The launcher forwards the request; signaling the group sends it twice.
      Darwin.kill(process.processIdentifier, SIGINT)
    }
  }
  func start(
    paths: [String], destination: URL? = nil, version: String,
    updated: @escaping @MainActor @Sendable (ProcessingUpdate) -> Void,
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
    let request = try RequestTransport.frame(paths)
    child.arguments = [
      resources.appendingPathComponent("processing-launcher.sh").path, "--request-stdin",
    ]
    child.environment = try environment(resources: resources, destination: destination)
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
    RequestTransport.send(request, to: owner.fileHandleForWriting)
    // Drain both pipes concurrently. Waiting on one stream first can deadlock.
    let errorReader = DispatchQueue(label: "eml.stderr")
    let group = DispatchGroup()
    let errorBuffer = CapturedOutput()
    group.enter()
    errorReader.async {
      defer { group.leave() }
      errorBuffer.set(
        ProgressStream.read(errors.fileHandleForReading) { progress in
          DispatchQueue.main.async { updated(.progress(progress)) }
        })
    }
    let expectedVersion = version
    DispatchQueue.global(qos: .userInitiated).async {
      let data = output.fileHandleForReading.readDataToEndOfFile()
      child.waitUntilExit()
      group.wait()
      let status = child.terminationStatus
      DispatchQueue.main.async {
        updated(.exited)
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
  private func environment(resources: URL, destination: URL?) throws -> [String: String] {
    var environment = ProcessInfo.processInfo.environment
    if Bundle.main.object(forInfoDictionaryKey: "EMLRuntimeMode") as? String == "bundled" {
      let python = resources.appendingPathComponent("Runtime/bin/python3.14").path
      guard FileManager.default.isExecutableFile(atPath: python) else {
        throw NSError(
          domain: "EMLBundledRuntime", code: 1,
          userInfo: [
            NSLocalizedDescriptionKey: "The bundled CPython runtime is missing or damaged."
          ])
      }
      environment["EML_REMOVER_PYTHON"] = python
    }
    if environment["EML_REMOVER_PYTHON"] == nil {
      if let configured = try RuntimeConfiguration.python() {
        environment["EML_REMOVER_PYTHON"] = configured
      }

    }
    environment["EML_REMOVER_ZIPAPP"] =
      resources.appendingPathComponent("remove-eml-attachments.pyz").path
    environment["EML_REMOVER_UI_OWNER_PIPE"] = "1"
    environment["EML_REMOVER_UI_REQUEST_PIPE"] = "1"
    environment["EML_REMOVER_UI_PROGRESS_PIPE"] = "1"
    environment["EML_REMOVER_OUTPUT_DIR"] = destination?.path
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
