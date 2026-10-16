import Foundation
import RunModel
import SwiftUI
import UniformTypeIdentifiers

struct RunWindow: View {
  @Bindable var store: ApplicationStore
  let reveal: (NativePath) -> Void
  @State private var columns: NavigationSplitViewVisibility = .automatic

  var body: some View {
    NavigationSplitView(columnVisibility: $columns) {
      VStack(alignment: .leading, spacing: 0) {
        Button("Create Copies…") { store.showInputImporter = true }
          .disabled(!store.acceptsRequests)
          .padding()
        List(selection: $store.selected) {
          ForEach(store.records) { record in
            VStack(alignment: .leading, spacing: 4) {
              Text(record.label).lineLimit(2).help(record.label)
              Text(sidebarState(record)).font(.caption).foregroundStyle(.secondary)
              if let summary = record.report?.summary {
                Text(
                  "\(summary.counts.created) created · \(summary.counts.upToDate) up to date · \(summary.counts.failed) failed"
                )
                .font(.caption).foregroundStyle(.secondary)
              }
            }
            .tag(record.id)
            .contextMenu {
              if record.waiting {
                Button("Remove Queued Request") { store.removeWaiting(record.id) }
              } else if record.finished {
                Button("Dismiss Report") { store.dismiss(record.id) }
              }
            }
          }
        }
        .accessibilityLabel("Session runs, newest first")
      }
      .navigationTitle("Runs")
      .navigationSplitViewColumnWidth(min: 160, ideal: 200, max: 260)
    } detail: {
      if let record = store.selectedRecord {
        if record.waiting {
          VStack(spacing: 16) {
            Text(record.label).font(.title2)
            Text("Not started").font(.headline)
            Text(
              "This request is waiting for the active run and available report space. Its destination was captured when it was queued."
            )
            .foregroundStyle(.secondary).multilineTextAlignment(.center)
            Button("Remove Queued Request") { store.removeWaiting(record.id) }
          }.padding(32).frame(maxWidth: 560)
        } else {
          RunDetails(
            label: record.label, report: record.report, active: !record.finished,
            stopRequested: record.stopRequests > 0, transportMessage: record.transportMessage,
            termination: record.termination, stderr: record.stderr,
            stop: { store.stop(record.id) }, export: { store.export(record.id) }, reveal: reveal
          )
          .id(record.id)
        }
      } else {
        SelectionView(
          destination: store.destination,
          destinationIsSessionOnly: store.destinationIsSessionOnly,
          destinationNeedsChoice: store.destinationNeedsChoice,
          chooseInputs: { store.showInputImporter = true },
          chooseDestination: { store.showDestinationImporter = true },
          besideOriginals: { store.setDestinationFromPicker(nil) }, admit: store.admit
        )
        .disabled(!store.acceptsRequests)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
      }
    }
    .navigationSplitViewStyle(.balanced)
    .frame(minWidth: 760, minHeight: 520)
    .fileImporter(
      isPresented: $store.showInputImporter, allowedContentTypes: [.emailMessage, .folder],
      allowsMultipleSelection: true
    ) { result in
      switch result {
      case .success(let urls): store.admit(urls)
      case .failure(let error): importFailed(error)
      }
    }
    .fileImporter(
      isPresented: $store.showDestinationImporter, allowedContentTypes: [.folder],
      allowsMultipleSelection: false
    ) { result in
      switch result {
      case .success(let urls):
        if let url = urls.first { store.setDestinationFromPicker(url) }
      case .failure(let error): importFailed(error)
      }
    }
    .fileExporter(
      isPresented: $store.showExporter, document: store.exportDocument, contentType: .json,
      defaultFilename: "EML run report", onCompletion: store.finishExport
    )
    .alert(
      store.alert?.title ?? "EML Attachment Remover",
      isPresented: Binding(
        get: { store.alert != nil }, set: { if !$0 { store.dismissAlert() } }),
      presenting: store.alert
    ) { alert in
      switch alert {
      case .message:
        Button("OK", role: .cancel) { store.dismissAlert() }
      case .reportSpace:
        Button("Keep Waiting", role: .cancel) { store.dismissAlert() }
        Button("Cancel Newest Queued Request", role: .destructive) {
          store.dismissAlert()
          store.cancelNewestWaiting()
        }
      case .confirmQuit:
        Button("Cancel", role: .cancel) { store.cancelQuit() }
        Button("Stop and Quit", role: .destructive) { store.confirmQuit() }
      }
    } message: {
      Text($0.detail)
    }
    .sheet(isPresented: $store.showAbout) {
      VStack(spacing: 16) {
        StatusArtwork(kind: .identity).frame(width: 96, height: 96)
        Text("EML Attachment Remover").font(.title)
        Text(
          Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
            ?? "Version unavailable")
        Text("Creates separate body-only EML copies. The originals are not modified by this app.")
          .multilineTextAlignment(.center)
        Button("Done") { store.showAbout = false }.keyboardShortcut(.defaultAction)
      }.padding(32).frame(width: 440)
    }
    .sheet(isPresented: $store.showLicense) { LicenseView() }
  }

  private func sidebarState(_ record: RunRecord) -> String {
    if record.waiting { return "Not started" }
    if !record.finished { return record.stopRequests > 0 ? "Stopping…" : "Running" }
    if record.transportMessage != nil || record.report?.failure != nil {
      return "Unverifiable report"
    }
    if record.report?.summary?.interrupted == true { return "Stopped" }
    return "Finished"
  }
  private func importFailed(_ error: any Error) {
    let error = error as NSError
    guard error.domain != NSCocoaErrorDomain || error.code != CocoaError.Code.userCancelled.rawValue
    else { return }
    store.showMessage(
      "The selected file URLs could not be imported. Choose local EML files or folders and try again."
    )
  }
}

private struct LicenseView: View {
  @Environment(\.dismiss) private var dismiss
  @State private var text = "Loading license…"

  var body: some View {
    VStack(alignment: .leading, spacing: 16) {
      Text("License and artwork terms").font(.title2)
      ScrollView {
        Text(text).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
      }
      Button("Done") { dismiss() }.keyboardShortcut(.defaultAction)
    }.padding(24).frame(minWidth: 560, minHeight: 420)
      .task {
        text = await blockingIO {
          guard let url = Bundle.main.url(forResource: "LICENSE", withExtension: nil),
            let value = try? String(contentsOf: url, encoding: .utf8)
          else {
            return "License resources are unavailable. Reinstall the complete native app bundle."
          }
          return value
        }
      }
  }
}
