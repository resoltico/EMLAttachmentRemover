import Darwin
import Foundation

enum RuntimeConfiguration {
  static func python() throws -> String? {
    let root = NSHomeDirectory() + "/Library/Application Support/EML Attachment Remover UI"
    let file = root + "/runtime.json"
    if !FileManager.default.fileExists(atPath: file) { return nil }
    let directory = Darwin.open(root, O_RDONLY | O_DIRECTORY | O_NOFOLLOW)
    guard directory >= 0 else { throw NSError(domain: "EMLRuntime", code: 1) }
    defer { Darwin.close(directory) }
    var metadata = stat()
    guard fstat(directory, &metadata) == 0, metadata.st_uid == geteuid(),
      metadata.st_mode & 0o077 == 0
    else {
      throw NSError(domain: "EMLRuntime", code: 2)
    }
    let handle = openat(directory, "runtime.json", O_RDONLY | O_NOFOLLOW)
    guard handle >= 0 else { throw NSError(domain: "EMLRuntime", code: 3) }
    defer { Darwin.close(handle) }
    guard fstat(handle, &metadata) == 0, metadata.st_uid == geteuid(),
      metadata.st_mode & S_IFMT == S_IFREG, metadata.st_mode & 0o077 == 0, metadata.st_size <= 4096
    else {
      throw NSError(domain: "EMLRuntime", code: 4)
    }
    let data = FileHandle(fileDescriptor: handle, closeOnDealloc: false).readDataToEndOfFile()
    let configuration = try JSONDecoder().decode([String: String].self, from: data)
    guard let python = configuration["python"], !python.isEmpty, !python.contains("\0") else {
      throw NSError(domain: "EMLRuntime", code: 5)
    }
    return python
  }
}
