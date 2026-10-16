import Foundation
import RunModel
import SwiftUI

struct RunDetails: View {
  let label: String
  let report: RunReport?
  let active: Bool
  let stopRequested: Bool
  let transportMessage: String?
  let termination: HelperExit?
  let stderr: Data
  let stop: () -> Void
  let export: () -> Void
  let reveal: (NativePath) -> Void
  @State private var selectedIndex: Int?

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      HStack(spacing: 12) {
        StatusArtwork(kind: artwork, tone: failed ? .failure : .success).frame(
          width: 44, height: 44)
        VStack(alignment: .leading, spacing: 3) {
          Text(label).font(.title2).lineLimit(2).help(label)
          Text(stateLabel).foregroundStyle(.secondary)
        }
        Spacer()
        if active {
          Button(
            stopRequested ? "Force Stop" : "Stop", role: stopRequested ? .destructive : nil,
            action: stop
          )
          .help(
            stopRequested
              ? "Send the second stop request. Complete copies may remain unreported."
              : "Stop new work and retain completed results.")
        }
      }
      if active, let start = report?.start {
        if start.total > 0 {
          ProgressView(value: Double(report?.items.count ?? 0), total: Double(start.total))
            .accessibilityLabel("Reported items")
            .accessibilityValue("\(report?.items.count ?? 0) of \(start.total) known inputs")
        } else {
          Text("No known work items").foregroundStyle(.secondary)
        }
      } else if active {
        ProgressView("Scanning files…")
      }
      if let summary = report?.summary {
        Text(
          "Created \(summary.counts.created) · Up to date \(summary.counts.upToDate) · Planned \(summary.counts.planned) · Failed \(summary.counts.failed) · Not processed \(summary.counts.notProcessed)"
        )
        .font(.callout).fixedSize(horizontal: false, vertical: true)
        if let error = summary.runError {
          Text("\(error.code): \(error.detail)").foregroundStyle(.red)
            .textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
        }
      }
      if let transportMessage {
        Text(transportMessage).foregroundStyle(.red).fixedSize(horizontal: false, vertical: true)
      }
      VSplitView {
        Table(report?.items ?? [], selection: $selectedIndex) {
          TableColumn("File") { item in
            Text((item.source.display as NSString).lastPathComponent)
              .lineLimit(2).help(item.source.display)
          }.width(min: 90, ideal: 150)
          TableColumn("Source folder") { item in
            Text((item.source.display as NSString).deletingLastPathComponent)
              .lineLimit(2).help(item.source.display)
          }.width(min: 90, ideal: 150)
          TableColumn("Outcome") { item in
            Text(item.statusLabel).lineLimit(2).help(item.failureGuidance ?? item.statusLabel)
          }.width(min: 130, ideal: 180)
        }
        .frame(minHeight: 140)
        .accessibilityLabel("Reported input outcomes")
        if let selected = report?.items.first(where: { $0.index == selectedIndex }) {
          ItemDetails(item: selected, reveal: reveal).frame(minHeight: 140)
        } else {
          Text("Select an item to inspect its parts and evidence.")
            .foregroundStyle(.secondary).frame(maxWidth: .infinity, minHeight: 80)
        }
      }
      if !active, report?.summary == nil, let start = report?.start {
        Text(
          "\(start.total - (report?.items.count ?? 0)) known inputs have no reported outcome. Complete unreported copies may exist after a hard stop or reporting failure."
        )
        .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
      }
      DisclosureGroup("Technical report") {
        VStack(alignment: .leading, spacing: 6) {
          if let start = report?.start {
            Text("Protocol \(start.protocol) · \(start.program) \(start.version)")
            Text(
              "Known inputs: \(start.total) · Collection complete: \(start.collectionComplete ? "Yes" : "No")"
            )
          }
          if let summary = report?.summary {
            Text(
              "Summary exit: \(summary.exitCode) · Interrupted: \(summary.interrupted ? "Yes" : "No")"
            )
          }
          if let termination {
            Text(
              termination.normal
                ? "Actual helper exit: \(termination.code)"
                : "Actual helper signal: \(termination.code)")
          }
          if !stderr.isEmpty {
            Text("Captured helper diagnostic:").font(.headline)
            Text(String(decoding: stderr, as: UTF8.self)).textSelection(.enabled)
          }
          Text(
            "Export the structured run record, including item/receipt details and captured helper diagnostics. Message contents are not included."
          )
          .foregroundStyle(.secondary)
          Button("Export Report…", action: export).disabled(report == nil)
        }
        .textSelection(.enabled)
        .frame(maxWidth: .infinity, alignment: .leading)
      }
    }
    .padding()
    .accessibilityElement(children: .contain)
    .accessibilityIdentifier("run-details")
  }

  private var failed: Bool {
    transportMessage != nil || report?.failure != nil || (report?.summary?.counts.failed ?? 0) > 0
      || report?.summary?.runError != nil
  }
  private var stateLabel: String {
    if active {
      return stopRequested
        ? "Stopping…" : (report?.start == nil ? "Scanning files…" : "Creating copies…")
    }
    if transportMessage != nil || report?.failure != nil { return "Report could not be verified" }
    if report?.summary?.interrupted == true {
      return failed ? "Stopped with reported errors" : "Stopped"
    }
    if failed { return "Finished with errors" }
    return report?.summary == nil ? "Incomplete report" : "Finished"
  }
  private var artwork: StatusArtwork.Kind {
    if active { return .processing }
    if report?.summary?.interrupted == true { return .stopped }
    if failed { return .attention }
    return report?.summary == nil ? .attention : .success
  }
}
