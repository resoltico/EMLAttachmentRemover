import Foundation
import RunModel
import SwiftUI
import UniformTypeIdentifiers

struct SelectionView: View {
  let destination: NativePath?
  let destinationIsSessionOnly: Bool
  let destinationNeedsChoice: Bool
  let chooseInputs: () -> Void
  let chooseDestination: () -> Void
  let besideOriginals: () -> Void
  let admit: ([URL]) -> Bool
  @State private var targeted = false

  var body: some View {
    VStack(spacing: 20) {
      StatusArtwork(kind: .identity).frame(width: 112, height: 112)
      Text("Create EML copies without attachments").font(.title2).multilineTextAlignment(.center)
      Text(
        "Keep readable plain and HTML bodies in separate copies. Originals are never modified by this app."
      )
      .foregroundStyle(.secondary).multilineTextAlignment(.center)
      VStack(spacing: 12) {
        Text("Drop EML files or folders here").font(.headline)
        Button("Create Copies…", action: chooseInputs)
          .keyboardShortcut("o", modifiers: .command)
      }
      .frame(maxWidth: .infinity, minHeight: 120)
      .background(
        targeted ? Color.accentColor.opacity(0.14) : Color.secondary.opacity(0.08),
        in: RoundedRectangle(cornerRadius: 12)
      )
      .overlay(
        RoundedRectangle(cornerRadius: 12).strokeBorder(
          targeted ? Color.accentColor : Color.secondary.opacity(0.4),
          style: StrokeStyle(lineWidth: 1, dash: [6]))
      )
      .dropDestination(for: URL.self) { urls, _ in
        admit(urls)
      } isTargeted: {
        targeted = $0
      }
      .accessibilityLabel("Drop EML files or folders, or choose Create Copies")
      HStack(alignment: .firstTextBaseline) {
        Text(destinationIsSessionOnly ? "Save copies (for this session):" : "Save copies:")
        Menu {
          Button("Beside originals", action: besideOriginals)
          Button("Choose Folder…", action: chooseDestination)
        } label: {
          Text(destinationLabel).lineLimit(2).help(destinationLabel)
        }
        .accessibilityLabel("Save copies: \(destinationLabel)")
      }
      Text(
        "HTML may retain links, remote references and active rendering content. This app removes authorized parts; it does not sanitize HTML."
      )
      .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
    }
    .padding(32)
    .frame(maxWidth: 620)
  }

  private var destinationLabel: String {
    if destinationNeedsChoice { return "Choose a destination" }
    guard let destination, let bytes = try? destination.bytes() else { return "Beside originals" }
    // Display only. The native destination is frozen in each admitted request.
    return String(decoding: bytes, as: UTF8.self)
  }
}

struct ReportDocument: FileDocument {
  static let readableContentTypes: [UTType] = [.json]
  var bytes: Data

  init(bytes: Data) { self.bytes = bytes }
  init(configuration: ReadConfiguration) throws {
    guard let bytes = configuration.file.regularFileContents else {
      throw CocoaError(.fileReadCorruptFile)
    }
    self.bytes = bytes
  }
  func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper {
    FileWrapper(regularFileWithContents: bytes)
  }
}
