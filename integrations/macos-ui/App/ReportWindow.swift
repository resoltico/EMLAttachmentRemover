import AppKit

@MainActor
final class ReportWindow: NSObject, NSWindowDelegate {
  var onClose: (() -> Void)?
  var onStopped: (() -> Void)?
  var onChoose: (() -> Void)?
  var onFiles: (([String]) -> Void)?
  var onChooseDestination: (() -> Void)?
  var onResetDestination: (() -> Void)?
  let window: NSWindow
  let layout: ReportLayout
  let run = ProcessingRun()
  let paths: [String]
  private(set) var destination: URL?
  private var destinationLabel: NSTextField?
  private var resetDestinationButton: NSButton?
  let version =
    Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "Unknown"
  let build = Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "Unknown"
  var isWelcome: Bool { paths.isEmpty }
  var isRunning: Bool { run.isRunning }
  private var stopping = false
  private var quitting = false
  private var stopButton: NSButton?
  private var progressBar: NSProgressIndicator?
  private var progressLabel: NSTextField?
  private var itemsView: ReportItems?
  private var details: ReportDetails?
  init(paths: [String], destination: URL? = nil, frame: NSRect? = nil) {
    self.paths = paths
    self.destination = destination
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
    window.minSize = NSSize(width: 560, height: 400)
    window.delegate = self
    window.isReleasedWhenClosed = false
    if let frame { window.setFrame(frame, display: false) } else { window.center() }
    layout.makeContent()
    window.makeKeyAndOrderFront(nil)
    NSApp.activate(ignoringOtherApps: true)
    if paths.isEmpty { showWelcome() } else { start() }
  }
  deinit { trace("report-controller released") }
  private func start() {
    layout.heading(
      "Preparing selected items…",
      subtitle: "\(paths.count) item\(paths.count == 1 ? "" : "s") selected", artwork: .processing,
      tone: .accent)
    showCapturedDestination()
    let progress = NSProgressIndicator()
    progress.style = .bar
    progress.isIndeterminate = true
    progress.startAnimation(nil)
    progressBar = progress
    let progressText = UIControls.label(
      "Collecting and checking selected files…", color: .secondaryLabelColor)
    progressLabel = progressText
    layout.add(progressText)
    layout.add(progress)
    layout.add(
      UIControls.label(
        "Stopping preserves any copies already created.", color: .secondaryLabelColor))
    let stop = UIControls.button("Stop processing", action: #selector(stop), target: self)
    stopButton = stop
    layout.footerHost.addArrangedSubview(stop)
    window.standardWindowButton(.closeButton)?.isEnabled = false
    layout.layoutContent()
    do {
      try run.start(
        paths: paths, destination: destination, version: version,
        updated: { [weak self] update in self?.updateProgress(update) },
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
  private func updateProgress(_ update: ProcessingUpdate) {
    switch update {
    case .exited:
      stopButton?.isEnabled = false
      stopButton?.title = "Finishing report…"
      layout.headline?.stringValue = "Preparing your report…"
    case .progress(let progress):
      guard !stopping else { return }
      layout.subtitle?.stringValue = "\(progress.total) email file\(progress.total == 1 ? "" : "s")"
      if progress.stage == .reporting {
        layout.headline?.stringValue = "Preparing your report…"
        progressLabel?.stringValue = "Collecting the final results…"
        setProgressMode(indeterminate: true)
        progressBar?.startAnimation(nil)
      } else {
        layout.headline?.stringValue = "Creating email copies…"
        progressLabel?.stringValue = "Processed \(progress.completed) of \(progress.total) files"
        setProgressMode(indeterminate: false)
        progressBar?.maxValue = Double(progress.total)
        progressBar?.doubleValue = Double(progress.completed)
      }
    }
  }
  private func setProgressMode(indeterminate: Bool) {
    guard let previous = progressBar, previous.isIndeterminate != indeterminate,
      let index = layout.content.arrangedSubviews.firstIndex(of: previous)
    else { return }
    // Reusing a started indeterminate control can retain its animated rendering layer.
    previous.stopAnimation(nil)
    let replacement = NSProgressIndicator()
    replacement.style = .bar
    replacement.isIndeterminate = indeterminate
    layout.content.removeArrangedSubview(previous)
    previous.removeFromSuperview()
    layout.content.insertArrangedSubview(replacement, at: index)
    replacement.widthAnchor.constraint(equalTo: layout.content.widthAnchor).isActive = true
    progressBar = replacement
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
    defer {
      if !NSApp.isActive { NSApp.requestUserAttention(.informationalRequest) }
    }
    do {
      let admitted = try result.get()
      trace(
        "admitted status=\(admitted.processStatus) heading=\(admitted.heading) counts=\(admitted.subtitle)"
      )
      layout.makeContent()
      layout.heading(
        admitted.heading, subtitle: admitted.subtitle,
        artwork: admitted.successful ? .success : admitted.stopped ? .stopped : .attention,
        tone: admitted.successful ? .success : admitted.stopped ? .neutral : .warning)
      if let notice = admitted.invocationNotice { layout.add(UIControls.label(notice)) }
      showCapturedDestination()
      let items = admitted.report.items
      let selected =
        admitted.successful
        ? 0
        : items.firstIndex(where: { ["failed", "published_with_error"].contains($0.status) })
          ?? items.firstIndex(where: { ["cancelled", "not_run"].contains($0.status) }) ?? 0
      if !["selection", "destination"].contains(admitted.report.batchError?.phase) {
        itemsView = ReportItems(receipt: admitted, layout: layout, selected: selected)
      }
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
      "Remove attachments from email copies",
      subtitle: "Choose EML files or folders to get started.",
      artwork: .identity,
      tone: .accent)
    layout.add(
      UIControls.label(
        "Creates separate EML copies with marked attachments and related embedded content removed. Your original files stay unchanged. In Finder, select EML files or folders and use Services → Create EML Copies Without Attachments.",
        color: .secondaryLabelColor))
    let drop = FileDropView(frame: .zero)
    drop.onFiles = { [weak self] in self?.onFiles?($0) }
    layout.add(drop)
    makeDestinationControls()
    let choose = UIControls.button(
      "Choose files or folders…", action: #selector(chooseFiles), target: self)
    choose.keyEquivalent = "\r"
    let spacer = NSView()
    spacer.setContentHuggingPriority(.defaultLow, for: .horizontal)
    layout.footerHost.addArrangedSubview(spacer)
    layout.footerHost.addArrangedSubview(choose)
    window.defaultButtonCell = choose.cell as? NSButtonCell
    layout.layoutContent()
  }
  @objc private func chooseFiles() { onChoose?() }
  @objc private func chooseDestination() { onChooseDestination?() }
  @objc private func resetDestination() { onResetDestination?() }
  private var destinationDescription: String {
    destination.map { safeText($0.path, multiline: false) } ?? "Beside each original file"
  }
  private func showCapturedDestination() {
    layout.add(
      UIControls.label("Destination: " + destinationDescription, color: .secondaryLabelColor))
  }
  private func makeDestinationControls() {
    let label = UIControls.label(destinationDescription)
    label.maximumNumberOfLines = 1
    label.lineBreakMode = .byTruncatingMiddle
    label.toolTip = destinationDescription
    label.setAccessibilityLabel("Destination folder")
    label.setContentHuggingPriority(.defaultLow, for: .horizontal)
    destinationLabel = label
    let choose = UIControls.button(
      "Choose folder…", action: #selector(chooseDestination), target: self)
    let reset = UIControls.button(
      "Beside originals", action: #selector(resetDestination), target: self)
    reset.isEnabled = destination != nil
    resetDestinationButton = reset
    let row = NSStackView(views: [label, choose, reset])
    row.orientation = .horizontal
    row.spacing = 12
    layout.add(UIControls.label("Save copies to", weight: .semibold))
    layout.add(row)
  }
  func setDestination(_ value: URL?) {
    guard isWelcome else { return }
    destination = value
    destinationLabel?.stringValue = destinationDescription
    destinationLabel?.toolTip = destinationDescription
    resetDestinationButton?.isEnabled = destination != nil
  }
  func closeWelcome() { if isWelcome { window.close() } }
  private func showFailure(_ title: String, explanation: String, diagnostics: String) {
    trace("unverified-report heading=\(title)")
    layout.makeContent()
    layout.heading(
      title, subtitle: "Review the details before trying again.", artwork: .attention,
      tone: .warning)
    layout.add(UIControls.label(explanation))
    showCapturedDestination()
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
