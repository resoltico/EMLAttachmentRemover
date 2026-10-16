import Foundation
import RunModel
import SwiftUI

struct ItemDetails: View {
  let item: ItemEvent
  let reveal: (NativePath) -> Void

  var body: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: 16) {
        HStack(spacing: 12) {
          StatusArtwork(kind: artwork, tone: item.status == .failed ? .failure : .success)
            .frame(width: 48, height: 48)
          VStack(alignment: .leading) {
            Text(item.statusLabel).font(.title2)
            if let saved = item.savedBytes {
              Text(
                "\(item.removed.count) removed parts · \(ByteCountFormatter.string(fromByteCount: Int64(saved), countStyle: .file)) saved"
              )
              .foregroundStyle(.secondary)
            }
          }
        }
        path("Source", item.source)
        if let destination = item.destination { path("Destination", destination) }
        Button("Show in Finder") { if let path = item.revealPath { reveal(path) } }
          .disabled(item.revealPath == nil)
          .help(
            "Uses the earlier qualified address. Finder navigation does not reverify the file or its contents."
          )
        if let guidance = item.failureGuidance {
          Text(guidance).fixedSize(horizontal: false, vertical: true)
            .accessibilityLabel("Failure guidance: \(guidance)")
        }
        if !item.removed.isEmpty {
          Text("Removed parts").font(.headline)
          LazyVStack(alignment: .leading, spacing: 12) {
            ForEach(item.removed.indices, id: \.self) { index in
              let part = item.removed[index]
              VStack(alignment: .leading, spacing: 4) {
                Text(part.filename ?? "Unnamed part").fontWeight(.medium)
                Text("Section \(part.section) · \(part.contentType)")
                Text("\(part.bytes) entity bytes · \(part.reason.rawValue)").foregroundStyle(
                  .secondary)
              }
              .textSelection(.enabled)
            }
          }
        }
        if !item.strippedFields.isEmpty {
          Text("Removed stale fields").font(.headline)
          Text(item.strippedFields.joined(separator: ", ")).textSelection(.enabled)
        }
        DisclosureGroup("Publication evidence") {
          VStack(alignment: .leading, spacing: 6) {
            fact("Publication", item.publication.state.rawValue)
            fact("Requested address", item.publication.address.rawValue)
            if let identity = item.publication.candidateIdentity {
              fact("Candidate identity (opaque)", "\(identity.kind): \(identity.value)")
            }
            sync("File synchronization", item.publication.fileSync)
            namespace(item.publication.namespaceSync)
            fact("Cleanup", item.publication.cleanup.state.rawValue)
            if let code = item.publication.cleanup.nativeCode {
              fact("Cleanup native code", "\(code)")
            }
            if let residual = item.publication.cleanup.residualName {
              fact("Residual component (\(residual.encoding), Base64)", residual.base64)
            }
          }
          .frame(maxWidth: .infinity, alignment: .leading)
        }
        if let existing = item.existingObservation {
          DisclosureGroup("Existing copy evidence") {
            VStack(alignment: .leading, spacing: 6) {
              if let identity = existing.identity {
                fact("Identity (opaque)", "\(identity.kind): \(identity.value)")
              }
              fact("Bytes equal", existing.bytesEqual.map { $0 ? "Yes" : "No" } ?? "Unknown")
              fact("Stable during comparison", existing.stable ? "Yes" : "No")
              fact("Requested address", existing.address.rawValue)
              sync("File synchronization", existing.fileSync)
              namespace(existing.namespaceSync)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
          }
        }
        if let error = item.error {
          DisclosureGroup("Technical error") {
            VStack(alignment: .leading, spacing: 6) {
              fact("Code", error.code)
              fact("Phase", error.phase.rawValue)
              if let section = error.section { fact("Section", section) }
              Text(error.detail).textSelection(.enabled)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
          }
        }
      }
      .padding()
      .frame(maxWidth: .infinity, alignment: .leading)
    }
  }

  private var artwork: StatusArtwork.Kind {
    switch item.status {
    case .created, .upToDate: .success
    case .planned: .identity
    case .failed: item.error?.code == "INTERRUPTED" ? .stopped : .attention
    }
  }
  private func path(_ label: String, _ value: ReportPath) -> some View {
    VStack(alignment: .leading, spacing: 4) {
      Text(label).font(.headline)
      Text(value.display).textSelection(.enabled).lineLimit(4).help(value.display)
        .accessibilityLabel("\(label): \(value.display)")
    }
  }
  private func fact(_ label: String, _ value: String) -> some View {
    Text("\(label): \(value)").textSelection(.enabled)
      .fixedSize(horizontal: false, vertical: true)
  }
  private func sync(_ label: String, _ value: SyncReceipt) -> some View {
    fact(
      label,
      "\(value.method.rawValue) · \(value.state.rawValue)\(value.nativeCode.map { " · native \($0)" } ?? "")"
    )
  }
  private func namespace(_ value: NamespaceReceipt) -> some View {
    VStack(alignment: .leading, spacing: 4) {
      fact("Namespace synchronization", "\(value.method.rawValue) · \(value.state.rawValue)")
      fact("Completed required edges", "\(value.completedEdges) of \(value.requiredEdges)")
      if let failed = value.failedEdgeIndex { fact("Failed edge index", "\(failed)") }
      if let code = value.nativeCode { fact("Namespace native code", "\(code)") }
    }
  }
}
