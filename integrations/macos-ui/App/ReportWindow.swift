import AppKit

@MainActor
final class ReportWindow: NSObject, NSWindowDelegate {
  var onClose: (() -> Void)?
  var onStopped: (() -> Void)?
  var onChoose: (() -> Void)?
  let window: NSWindow
  let layout: ReportLayout
  let run = ProcessingRun()
  let paths: [String]
  let version =
    Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "Unknown"
  let build = Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "Unknown"
  var isWelcome: Bool { paths.isEmpty }
  var isRunning: Bool { run.isRunning }
  private var stopping = false
  private var quitting = false
  private var stopButton: NSButton?
  private var itemsView: ReportItems?
  private var details: ReportDetails?
  init(paths: [String]) {
    self.paths = paths
    window = NSWindow(
      contentRect: NSRect(x: 0, y: 0, width: 640, height: 590),
      styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false
    )
    layout = ReportLayout(window: window)
    super.init()
    let firstName = paths.first.map {
      safeText(($0 as NSString).lastPathComponent, multiline: false)
    }
    window.title =
      firstName.map {
        $0 + (paths.count > 1 ? " + \(paths.count - 1) more" : "") + " — Processing report"
      } ?? "Processing report"
    window.minSize = NSSize(width: 560, height: 280)
    window.delegate = self
    window.isReleasedWhenClosed = false
    window.center()
    layout.makeContent()
    window.makeKeyAndOrderFront(nil)
    NSApp.activate(ignoringOtherApps: true)
    if paths.isEmpty { showWelcome() } else { start() }
  }
  deinit { trace("report-controller released") }
  private func start() {
    layout.heading(
      "Creating email copies…",
      subtitle: "\(paths.count) file\(paths.count == 1 ? "" : "s") selected", artwork: .processing,
      color: .controlAccentColor)
    let progress = NSProgressIndicator()
    progress.style = .bar
    progress.isIndeterminate = true
    progress.startAnimation(nil)
    layout.add(progress)
    layout.add(
      UIControls.label(
        "Stopping preserves any copies already created.", color: .secondaryLabelColor))
    let stop = UIControls.button("Stop processing", action: #selector(stop), target: self)
    stopButton = stop
    layout.footerHost.addArrangedSubview(stop)
    window.standardWindowButton(.closeButton)?.isEnabled = false
    layout.resizeToContent()
    do {
      try run.start(
        paths: paths, version: version,
        preparing: { [weak self] in
          self?.stopButton?.isEnabled = false
          self?.stopButton?.title = "Finishing report…"
          self?.layout.headline?.stringValue = "Preparing your report…"
        },
        completed: { [weak self] result, raw, status, stderr in
          self?.finished(result, raw: raw, status: status, stderr: stderr)
        })
    } catch {
      showFailure(
        "Couldn’t start processing",
        explanation: launchExplanation(error),
        diagnostics: error.localizedDescription)
    }
  }
  @objc private func stop() {
    guard isRunning, !stopping else { return }
    stopping = true
    stopButton?.title = "Stopping…"
    stopButton?.isEnabled = false
    run.interrupt()
  }
  private func finished(
    _ result: Result<UIReceipt, Error>, raw: String, status: Int32, stderr: Data
  ) {
    trace("finished status=\(status)")
    if quitting {
      onStopped?()
      return
    }
    do {
      let admitted = try result.get()
      trace(
        "admitted status=\(admitted.processStatus) heading=\(admitted.heading) counts=\(admitted.subtitle)"
      )
      window.minSize = NSSize(width: 560, height: 400)
      layout.makeContent()
      layout.heading(
        admitted.heading, subtitle: admitted.subtitle,
        artwork: admitted.successful ? .success : admitted.stopped ? .stopped : .attention,
        color: admitted.successful
          ? .systemGreen : admitted.stopped ? .secondaryLabelColor : .systemOrange)
      if let notice = admitted.invocationNotice { layout.add(UIControls.label(notice)) }
      let items = admitted.report.items
      let selected =
        admitted.successful
        ? 0
        : items.firstIndex(where: { ["failed", "published_with_error"].contains($0.status) })
          ?? items.firstIndex(where: { ["cancelled", "not_run"].contains($0.status) }) ?? 0
      itemsView = ReportItems(receipt: admitted, layout: layout, selected: selected)
      details = ReportDetails(
        raw: raw, text: ReportText.details(admitted, version: version, build: build),
        receipt: admitted,
        layout: layout)
      window.standardWindowButton(.closeButton)?.isEnabled = true
    } catch {
      let stopped = stopping || [129, 130, 143].contains(status)
      let diagnostics = String(decoding: stderr, as: UTF8.self)
      let launcherFailure = error as? LauncherFailure
      let heading = stopped ? "Processing stopped" : "Couldn’t read the processing results"
      showFailure(
        launcherFailure?.heading ?? heading,
        explanation: launcherFailure?.errorDescription
          ?? "The app could not confirm the processing results. Files may already have been created; check the destination before trying again.",
        diagnostics: "Process status: \(status)\n\(error.localizedDescription)\n\(diagnostics)")
    }
  }
  private func showWelcome() {
    window.title = "EML Attachment Remover"
    layout.heading(
      "Remove attachments from email copies", subtitle: "Choose EML files to get started.",
      artwork: .identity,
      color: .controlAccentColor)
    layout.add(
      UIControls.label(
        "Creates separate EML copies with marked attachments and related embedded content removed. Your original files stay unchanged. You can also select EML files in Finder and run your Quick Action.",
        color: .secondaryLabelColor))
    let choose = UIControls.button("Choose files…", action: #selector(chooseFiles), target: self)
    choose.keyEquivalent = "\r"
    let spacer = NSView()
    spacer.setContentHuggingPriority(.defaultLow, for: .horizontal)
    layout.footerHost.addArrangedSubview(spacer)
    layout.footerHost.addArrangedSubview(choose)
    window.defaultButtonCell = choose.cell as? NSButtonCell
    layout.resizeToContent()
  }
  @objc private func chooseFiles() { onChoose?() }
  func closeWelcome() { if isWelcome { window.close() } }
  private func showFailure(_ title: String, explanation: String, diagnostics: String) {
    trace("unverified-report heading=\(title)")
    window.minSize = NSSize(width: 560, height: 320)
    layout.makeContent()
    layout.heading(
      title, subtitle: "Review the details before trying again.", artwork: .attention,
      color: .systemOrange)
    layout.add(UIControls.label(explanation))
    details = ReportDetails(
      raw: diagnostics, text: safeText(diagnostics), receipt: nil, layout: layout)
    window.standardWindowButton(.closeButton)?.isEnabled = true
  }
  func windowShouldClose(_ sender: NSWindow) -> Bool {
    trace("window-close requested")
    return !isRunning
  }
  func windowWillClose(_ notification: Notification) {
    trace("window-closed")
    onClose?()
  }
  func requestQuit() {
    quitting = true
    stop()
  }
}
