import AppKit
import Darwin

@MainActor
final class ReportWindow: NSObject, NSWindowDelegate, NSTableViewDataSource, NSTableViewDelegate {
    var onClose: (() -> Void)?
    var onStopped: (() -> Void)?
    var onChoose: (() -> Void)?
    var isWelcome: Bool { paths.isEmpty }
    var isRunning: Bool { process != nil }
    deinit { trace("report-controller released") }
    private var window: NSWindow!
    private var content: NSStackView!
    private var footerHost: NSStackView!
    private var process: Process?
    private var lifetime: FileHandle?
    private var headline: NSTextField?
    private var receipt: UIReceipt?
    private var rawReport = ""
    private var detailText = ""
    private var stopping = false
    private var quitting = false
    private var version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "Unknown"
    private var items: [Item] = []
    private var card: NSStackView?
    private var detailsView: NSScrollView?
    private var detailsButton: NSButton?
    private var table: NSTableView?
    private var stopButton: NSButton?
    private var paths: [String] = []

    init(paths: [String]) {
        super.init()
        self.paths = paths
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 640, height: 590), styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        let firstName = paths.first.map { safeText(($0 as NSString).lastPathComponent, multiline: false) }
        window.title = firstName.map { $0 + (paths.count > 1 ? " + \(paths.count - 1) more" : "") + " — Processing report" } ?? "Processing report"
        window.minSize = NSSize(width: 560, height: 460)
        window.delegate = self
        window.isReleasedWhenClosed = false
        window.center()
        makeContent()
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        if paths.isEmpty {
            showWelcome()
        } else {
            start()
        }
    }

    private func label(_ text: String, size: CGFloat = 13, weight: NSFont.Weight = .regular, color: NSColor = .labelColor) -> NSTextField {
        let field = NSTextField(wrappingLabelWithString: text)
        field.font = .systemFont(ofSize: size, weight: weight)
        field.textColor = color
        field.isSelectable = true
        field.setContentCompressionResistancePriority(.required, for: .vertical)
        return field
    }
    private func button(_ title: String, action: Selector) -> NSButton {
        let button = NSButton(title: title, target: self, action: action)
        button.bezelStyle = .rounded
        return button
    }
    private func makeContent() {
        trace("make-content")
        let outer = NSView(frame: window.contentView?.bounds ?? NSRect(x: 0, y: 0, width: 640, height: 590))
        window.contentView = outer
        content = NSStackView()
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 16
        content.translatesAutoresizingMaskIntoConstraints = false
        let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 640, height: 450))
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.drawsBackground = false
        scroll.translatesAutoresizingMaskIntoConstraints = false
        let document = FlippedView(frame: NSRect(x: 0, y: 0, width: 640, height: 450))
        document.translatesAutoresizingMaskIntoConstraints = false
        document.addSubview(content)
        scroll.documentView = document
        outer.addSubview(scroll)
        footerHost = NSStackView()
        footerHost.orientation = .horizontal
        footerHost.translatesAutoresizingMaskIntoConstraints = false
        outer.addSubview(footerHost)
        NSLayoutConstraint.activate([
            footerHost.leadingAnchor.constraint(equalTo: outer.leadingAnchor, constant: 28),
            footerHost.trailingAnchor.constraint(equalTo: outer.trailingAnchor, constant: -28),
            footerHost.bottomAnchor.constraint(equalTo: outer.bottomAnchor, constant: -20),
            scroll.leadingAnchor.constraint(equalTo: outer.leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: outer.trailingAnchor),
            scroll.topAnchor.constraint(equalTo: outer.topAnchor),
            scroll.bottomAnchor.constraint(equalTo: footerHost.topAnchor, constant: -16),
            document.widthAnchor.constraint(equalTo: scroll.contentView.widthAnchor),
            content.leadingAnchor.constraint(equalTo: document.leadingAnchor, constant: 28),
            content.trailingAnchor.constraint(equalTo: document.trailingAnchor, constant: -28),
            content.topAnchor.constraint(equalTo: document.topAnchor, constant: 24),
            content.bottomAnchor.constraint(equalTo: document.bottomAnchor, constant: -12)
        ])
        let identity = NSStackView()
        identity.orientation = .horizontal
        identity.spacing = 14
        let icon = NSImageView(image: Artwork.image(.identity))
        icon.setAccessibilityElement(false)
        icon.widthAnchor.constraint(equalToConstant: 40).isActive = true
        icon.heightAnchor.constraint(equalToConstant: 40).isActive = true
        let names = NSStackView(views: [label("EML Attachment Remover", size: 18, weight: .semibold), label("Version \(version)", color: .secondaryLabelColor)])
        names.orientation = .vertical
        names.alignment = .leading
        names.spacing = 4
        identity.addArrangedSubview(icon)
        identity.addArrangedSubview(names)
        add(identity)
        let separator = NSBox(frame: NSRect(x: 0, y: 0, width: 584, height: 64))
        separator.boxType = .separator
        add(separator)
    }
    private func add(_ view: NSView) {
        content.addArrangedSubview(view)
        view.widthAnchor.constraint(equalTo: content.widthAnchor).isActive = true
    }
    private func heading(_ title: String, subtitle: String, artwork: Artwork.Kind, color: NSColor) {
        let row = NSStackView()
        row.orientation = .horizontal
        row.alignment = .top
        row.spacing = 12
        let image = NSImageView(image: Artwork.image(artwork, color: color))
        image.setAccessibilityElement(false)
        image.widthAnchor.constraint(equalToConstant: 28).isActive = true
        image.heightAnchor.constraint(equalToConstant: 28).isActive = true
        headline = label(title, size: 24, weight: .semibold)
        let words = NSStackView(views: [headline!, label(subtitle, color: .secondaryLabelColor)])
        words.orientation = .vertical
        words.alignment = .leading
        words.spacing = 6
        row.addArrangedSubview(image)
        row.addArrangedSubview(words)
        add(row)
    }
    private func start() {
        heading("Creating MIME-pruned copies…", subtitle: "\(paths.count) file\(paths.count == 1 ? "" : "s") selected", artwork: .processing, color: .controlAccentColor)
        let progress = NSProgressIndicator()
        progress.style = .bar
        progress.isIndeterminate = true
        progress.startAnimation(nil)
        add(progress)
        add(label("Stopping preserves any copies already created.", color: .secondaryLabelColor))
        stopButton = button("Stop processing", action: #selector(stop))
        footerHost.addArrangedSubview(stopButton!)
        window.standardWindowButton(.closeButton)?.isEnabled = false
        let child = Process()
        child.executableURL = URL(fileURLWithPath: "/bin/sh")
        guard let resources = Bundle.main.resourceURL else {
            showFailure("Couldn’t start processing", explanation: "The application resources are missing.", diagnostics: "Missing resource directory.")
            return
        }
        child.arguments = [resources.appendingPathComponent("run-from-finder.sh").path] + paths
        var environment = ProcessInfo.processInfo.environment
        if environment["EML_REMOVER_PYTHON"] == nil {
            do {
                if let configured = try configuredPython() { environment["EML_REMOVER_PYTHON"] = configured }
            } catch {
                process = nil
                showFailure("Runtime configuration needs attention", explanation: "Reinstall the application with your CPython 3.14 interpreter to repair its private runtime configuration.", diagnostics: error.localizedDescription)
                return
            }
        }
        environment["EML_REMOVER_HOME"] = resources.path
        environment["EML_REMOVER_ZIPAPP"] = resources.appendingPathComponent("remove-eml-attachments.pyz").path
        environment["EML_REMOVER_UI_REPORT"] = "1"
        environment["EML_REMOVER_UI_OWNER_PIPE"] = "1"
        environment["EML_REMOVER_REVEAL"] = "0"
        child.environment = environment
        let owner = Pipe()
        lifetime = owner.fileHandleForWriting
        child.standardInput = owner.fileHandleForReading
        let output = Pipe()
        let errors = Pipe()
        child.standardOutput = output
        child.standardError = errors
        process = child
        do { try child.run(); owner.fileHandleForReading.closeFile() } catch {
            lifetime?.closeFile()
            lifetime = nil
            process = nil
            showFailure("Couldn’t start processing", explanation: "The processing launcher could not be started.", diagnostics: error.localizedDescription)
            return
        }
        // Drain both pipes concurrently. Waiting on one stream first can deadlock.
        let errorReader = DispatchQueue(label: "eml.stderr")
        let group = DispatchGroup()
        let errorBuffer = LockedData()
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
                self.stopButton?.isEnabled = false
                self.stopButton?.title = "Finishing report…"
                self.headline?.stringValue = "Preparing your report…"
            }
            let stderr = errorBuffer.get()
            let admitted = Result { try UIReceipt.admit(data, status: status, version: expectedVersion) }
            let raw = String(decoding: data, as: UTF8.self)
            DispatchQueue.main.async {
                self.finished(admitted, raw: raw, status: status, stderr: stderr)
            }
        }
    }
    @objc private func stop() {
        guard let process, process.isRunning, !stopping else { return }
        stopping = true
        stopButton?.title = "Stopping…"
        stopButton?.isEnabled = false
        process.interrupt()
    }
    private func finished(_ result: Result<UIReceipt, Error>, raw: String, status: Int32, stderr: Data) {
        trace("finished status=\(status)")
        lifetime?.closeFile()
        lifetime = nil
        process = nil
        if quitting { onStopped?(); return }
        do {
            let admitted = try result.get()
            trace("admitted status=\(admitted.process_status) heading=\(admitted.heading) counts=\(admitted.subtitle)")
            receipt = admitted
            items = admitted.report.items
            rawReport = raw
            var readable = ["EML Attachment Remover \(version)", "Invocation status: \(admitted.process_status)", admitted.heading, admitted.subtitle]
            if let notice = admitted.invocationNotice { readable.append(notice) }
            var readableCount = 0
            for item in items {
                var lines = ["File: " + safeText(item.source_request.display), "Result: " + item.label]
                if let destination = item.destination_request { lines.append("Output: " + safeText(destination.display)) }
                if let error = item.error { lines.append("Error: " + error.code + "\n" + safeText(error.message)) }
                for warning in item.warnings { lines.append("Warning: " + warning.code + "\n" + safeText(warning.message)) }
                let block = lines.joined(separator: "\n")
                readableCount += block.count
                readable.append(block)
                if readableCount > 1_000_000 {
                    readable.append("[Display truncated. Copy details includes the full report.]")
                    break
                }
            }
            if let batchError = admitted.report.batch_error { readable.append("Batch error: " + batchError.code + "\n" + safeText(batchError.message)) }
            detailText = readable.joined(separator: "\n\n")
            makeContent()
            heading(admitted.heading, subtitle: admitted.subtitle, artwork: admitted.successful ? .success : admitted.stopped ? .stopped : .attention, color: admitted.successful ? .systemGreen : admitted.stopped ? .secondaryLabelColor : .systemOrange)
            if let notice = admitted.invocationNotice { add(label(notice)) }
            let selected = admitted.successful ? 0 : items.firstIndex(where: { ["failed", "published_with_error"].contains($0.status) }) ?? items.firstIndex(where: { ["cancelled", "not_run"].contains($0.status) }) ?? 0
            if items.count > 1 { makeTable(selectedRow: selected) }
            card = NSStackView()
            card!.orientation = .vertical
            card!.alignment = .leading
            card!.spacing = 10
            let cardBox = NSBox(frame: NSRect(x: 0, y: 0, width: 584, height: 64))
            cardBox.boxType = .custom
            cardBox.titlePosition = .noTitle
            cardBox.borderWidth = 1
            cardBox.borderColor = .separatorColor
            cardBox.fillColor = .controlBackgroundColor
            cardBox.cornerRadius = 10
            cardBox.contentViewMargins = NSSize(width: 18, height: 16)
            cardBox.contentView = NSView()
            card!.translatesAutoresizingMaskIntoConstraints = false
            cardBox.contentView!.addSubview(card!)
            NSLayoutConstraint.activate([
                card!.leadingAnchor.constraint(equalTo: cardBox.contentView!.leadingAnchor),
                card!.trailingAnchor.constraint(equalTo: cardBox.contentView!.trailingAnchor),
                card!.topAnchor.constraint(equalTo: cardBox.contentView!.topAnchor),
                card!.bottomAnchor.constraint(equalTo: cardBox.contentView!.bottomAnchor)
            ])
            add(cardBox)
            if items.indices.contains(selected) { showItem(items[selected]) }
            makeDetails()
            footer()
            table?.scrollRowToVisible(selected)
            window.standardWindowButton(.closeButton)?.isEnabled = true
        } catch {
            let stopped = stopping || [129, 130, 143].contains(status)
            let diagnostics = String(decoding: stderr, as: UTF8.self)
            let missingPython = status == 9 && (diagnostics.contains("Python 3.14") || diagnostics.contains("CPython 3.14"))
            showFailure(missingPython ? "CPython 3.14 is required" : stopped ? "Processing stopped" : "Couldn’t obtain a verified report", explanation: missingPython ? "Processing could not start. Install CPython 3.14 or configure the runtime described in the application setup guide." : "No complete, validated report is available. Files may already have been created; inspect the destination before running again.", diagnostics: "Process status: \(status)\n\(error.localizedDescription)\n\(diagnostics)")
        }
    }
    private func makeTable(selectedRow: Int) {
        let list = NSTableView()
        for (id, title, width) in [("file", "File", 355.0), ("result", "Result", 210.0)] {
            let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier(id))
            column.title = title
            column.width = width
            list.addTableColumn(column)
        }
        list.delegate = self
        list.dataSource = self
        list.rowHeight = 28
        list.usesAlternatingRowBackgroundColors = true
        list.allowsEmptySelection = false
        let scroll = NSScrollView()
        scroll.hasVerticalScroller = true
        scroll.borderType = .bezelBorder
        scroll.documentView = list
        scroll.heightAnchor.constraint(equalToConstant: 140).isActive = true
        add(scroll)
        table = list
        list.selectRowIndexes(IndexSet(integer: selectedRow), byExtendingSelection: false)
    }
    func numberOfRows(in tableView: NSTableView) -> Int { items.count }
    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let field = label(tableColumn?.identifier.rawValue == "file" ? items[row].name : items[row].label)
        field.maximumNumberOfLines = 1
        field.lineBreakMode = .byTruncatingMiddle
        field.toolTip = safeText(items[row].source_request.display)
        return field
    }
    func tableViewSelectionDidChange(_ notification: Notification) {
        guard let table, items.indices.contains(table.selectedRow) else { return }
        showItem(items[table.selectedRow])
    }
    private func showItem(_ item: Item) {
        guard let card else { return }
        for view in card.arrangedSubviews { card.removeArrangedSubview(view); view.removeFromSuperview() }
        func append(_ view: NSView) {
            card.addArrangedSubview(view)
            view.widthAnchor.constraint(equalTo: card.widthAnchor).isActive = true
        }
        let name = label(item.name, size: 16, weight: .semibold)
        name.maximumNumberOfLines = 2
        name.lineBreakMode = .byTruncatingMiddle
        name.toolTip = safeText(item.source_request.display)
        append(name)
        let folder = item.source_request.url?.deletingLastPathComponent().lastPathComponent ?? "See full path in Details"
        append(label(safeText(folder), color: .secondaryLabelColor))
        let separator = NSBox(frame: NSRect(x: 0, y: 0, width: 584, height: 64))
        separator.boxType = .separator
        append(separator)
        append(label(item.isConflict ? "An existing copy differs" : item.label, weight: .semibold))
        append(label(item.explanation))
        let row = NSStackView()
        row.orientation = .horizontal
        if item.acceptedURL != nil && receipt?.process_status != 120 && receipt?.stopped != true {
            let reveal = button("Show copy in Finder", action: #selector(revealCopy))
            reveal.tag = item.index
            row.addArrangedSubview(reveal)
        } else if item.source_request.url != nil {
            let folderButton = button("Show source folder", action: #selector(revealFolder))
            folderButton.tag = item.index
            row.addArrangedSubview(folderButton)
        }
        append(row)
    }
    private func makeDetails() {
        let errors = receipt?.errorCount ?? 0
        let warnings = receipt?.warningCount ?? 0
        let counts = receipt == nil ? "" : " · \(errors) error\(errors == 1 ? "" : "s") · \(warnings) warning\(warnings == 1 ? "" : "s")"
        detailsButton = button("▸ Details" + counts, action: #selector(toggleDetails))
        detailsButton!.bezelStyle = .inline
        let disclosureRow = NSStackView(views: [detailsButton!])
        disclosureRow.orientation = .horizontal
        disclosureRow.alignment = .leading
        add(disclosureRow)
        let scroll = NSScrollView()
        let text = NSTextView()
        text.isEditable = false
        text.isSelectable = true
        text.font = .monospacedSystemFont(ofSize: 11, weight: .regular)
        let limit = 1_000_000
        text.string = detailText.count > limit ? String(detailText.prefix(limit)) + "\n[Display truncated. Copy details includes the full report.]" : detailText
        text.textContainer?.widthTracksTextView = true
        text.autoresizingMask = [.width]
        text.setAccessibilityLabel("Processing report details")
        scroll.documentView = text
        scroll.hasVerticalScroller = true
        scroll.borderType = .bezelBorder
        scroll.heightAnchor.constraint(equalToConstant: 140).isActive = true
        scroll.isHidden = true
        detailsView = scroll
        add(scroll)
    }
    private func footer() {
        let row = footerHost!
        let copy = button("Copy details", action: #selector(copyDetails))
        row.addArrangedSubview(copy)
        let spacer = NSView()
        spacer.setContentHuggingPriority(.defaultLow, for: .horizontal)
        row.addArrangedSubview(spacer)
        let done = button("Done", action: #selector(done))
        done.keyEquivalent = "\r"
        row.addArrangedSubview(done)
        window.defaultButtonCell = done.cell as? NSButtonCell
        resizeToContent()
    }
    private func configuredPython() throws -> String? {
        let root = NSHomeDirectory() + "/Library/Application Support/EML Attachment Remover UI"
        let file = root + "/runtime.json"
        if !FileManager.default.fileExists(atPath: file) { return nil }
        let directory = Darwin.open(root, O_RDONLY | O_DIRECTORY | O_NOFOLLOW)
        guard directory >= 0 else { throw NSError(domain: "EMLRuntime", code: 1) }
        defer { Darwin.close(directory) }
        var metadata = stat()
        guard fstat(directory, &metadata) == 0, metadata.st_uid == geteuid(), metadata.st_mode & 0o077 == 0 else {
            throw NSError(domain: "EMLRuntime", code: 2)
        }
        let handle = openat(directory, "runtime.json", O_RDONLY | O_NOFOLLOW)
        guard handle >= 0 else { throw NSError(domain: "EMLRuntime", code: 3) }
        defer { Darwin.close(handle) }
        guard fstat(handle, &metadata) == 0, metadata.st_uid == geteuid(), metadata.st_mode & S_IFMT == S_IFREG, metadata.st_mode & 0o077 == 0, metadata.st_size <= 4096 else {
            throw NSError(domain: "EMLRuntime", code: 4)
        }
        let data = FileHandle(fileDescriptor: handle, closeOnDealloc: false).readDataToEndOfFile()
        let configuration = try JSONDecoder().decode([String: String].self, from: data)
        guard let python = configuration["python"], !python.isEmpty, !python.contains("\0") else {
            throw NSError(domain: "EMLRuntime", code: 5)
        }
        return python
    }
    private func showWelcome() {
        makeContent()
        window.title = "EML Attachment Remover"
        heading("Create MIME-pruned EML copies", subtitle: "Choose files to get started.", artwork: .welcome, color: .controlAccentColor)
        add(label("Creates verified working copies while keeping your originals. You can also select EML files in Finder and run your Quick Action."))
        let choose = button("Choose files…", action: #selector(chooseFiles))
        choose.keyEquivalent = "\r"
        footerHost.addArrangedSubview(choose)
        window.defaultButtonCell = choose.cell as? NSButtonCell
        resizeToContent()
    }
    @objc private func chooseFiles() { onChoose?() }
    func closeWelcome() { if isWelcome { window.close() } }

    private func showFailure(_ title: String, explanation: String, diagnostics: String) {
        trace("unverified-report heading=\(title)")
        process = nil
        makeContent()
        heading(title, subtitle: "Review the details before trying again.", artwork: .attention, color: .systemOrange)
        add(label(explanation))
        rawReport = diagnostics
        detailText = safeText(diagnostics)
        makeDetails()
        footer()
        window.standardWindowButton(.closeButton)?.isEnabled = true
    }
    @objc private func revealCopy(_ sender: NSButton) {
        guard let item = items.first(where: { $0.index == sender.tag }), let url = item.acceptedURL else { return }
        NSWorkspace.shared.activateFileViewerSelecting([url])
    }
    @objc private func revealFolder(_ sender: NSButton) {
        guard let item = items.first(where: { $0.index == sender.tag }), let url = item.source_request.url else { return }
        NSWorkspace.shared.open(url.deletingLastPathComponent())
    }
    @objc private func toggleDetails() {
        guard let detailsView else { return }
        detailsView.isHidden.toggle()
        if let title = detailsButton?.title {
            detailsButton?.title = (detailsView.isHidden ? "▸" : "▾") + String(title.dropFirst())
        }
        resizeToContent()
    }
    private func resizeToContent() {
        window.contentView?.layoutSubtreeIfNeeded()
        let maximum = (window.screen ?? NSScreen.main)?.visibleFrame.height ?? 900
        let height = min(maximum - 100, max(460, content.fittingSize.height + 85))
        window.setContentSize(NSSize(width: window.contentView!.bounds.width, height: height))
    }
    @objc private func copyDetails() {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(rawReport, forType: .string)
    }
    @objc private func done() { trace("done event=\(NSApp.currentEvent?.type.rawValue ?? 0)"); window.close() }
    func windowShouldClose(_ sender: NSWindow) -> Bool { trace("window-close requested"); return process == nil }
    func windowWillClose(_ notification: Notification) { trace("window-closed"); onClose?() }
    func requestQuit() {
        quitting = true
        stop()
    }

}

@MainActor final class FlippedView: NSView { override var isFlipped: Bool { true } }

final class LockedData: @unchecked Sendable {
    private let lock = NSLock()
    private var data = Data()
    func set(_ value: Data) { lock.lock(); defer { lock.unlock() }; data = value }
    func get() -> Data { lock.lock(); defer { lock.unlock() }; return data }
}

@MainActor
final class Application: NSObject, NSApplicationDelegate {
    private var windows: [UUID: ReportWindow] = [:]
    private var quitting = false
    private var pendingQuit: Set<UUID> = []
    private var launched = false

    func applicationDidFinishLaunching(_ notification: Notification) {
        trace("app-launched args=\(CommandLine.arguments.count)")
        let menu = NSMenu()
        let root = NSMenuItem()
        let appMenu = NSMenu()
        appMenu.addItem(withTitle: "About EML Attachment Remover", action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: "")
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Hide EML Attachment Remover", action: #selector(NSApplication.hide(_:)), keyEquivalent: "h")
        let hideOthers = appMenu.addItem(withTitle: "Hide Others", action: #selector(NSApplication.hideOtherApplications(_:)), keyEquivalent: "h")
        hideOthers.keyEquivalentModifierMask = [.command, .option]
        appMenu.addItem(withTitle: "Show All", action: #selector(NSApplication.unhideAllApplications(_:)), keyEquivalent: "")
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Quit EML Attachment Remover", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        root.submenu = appMenu
        menu.addItem(root)
        let file = NSMenuItem(title: "File", action: nil, keyEquivalent: "")
        let fileMenu = NSMenu(title: "File")
        let open = fileMenu.addItem(withTitle: "Open EML Files…", action: #selector(chooseFiles(_:)), keyEquivalent: "o")
        open.target = self
        fileMenu.addItem(.separator())
        fileMenu.addItem(withTitle: "Close Report", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        let escape = fileMenu.addItem(withTitle: "Close Report", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "\u{1b}")
        escape.keyEquivalentModifierMask = []
        escape.isHidden = true
        escape.allowsKeyEquivalentWhenHidden = true
        file.submenu = fileMenu
        menu.addItem(file)
        let edit = NSMenuItem(title: "Edit", action: nil, keyEquivalent: "")
        let editMenu = NSMenu(title: "Edit")
        editMenu.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        editMenu.addItem(withTitle: "Select All", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        edit.submenu = editMenu
        menu.addItem(edit)
        let windowItem = NSMenuItem(title: "Window", action: nil, keyEquivalent: "")
        let windowMenu = NSMenu(title: "Window")
        windowMenu.addItem(withTitle: "Minimize", action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m")
        windowMenu.addItem(withTitle: "Bring All to Front", action: #selector(NSApplication.arrangeInFront(_:)), keyEquivalent: "")
        windowItem.submenu = windowMenu
        menu.addItem(windowItem)
        NSApp.windowsMenu = windowMenu
        NSApp.mainMenu = menu
        launched = true
        let arguments = Array(CommandLine.arguments.dropFirst()).filter { !$0.hasPrefix("-psn_") }
        if !arguments.isEmpty { openBatch(arguments) }
        // Finder's file-open event can arrive just after launch.
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
            if self.windows.isEmpty { self.openBatch([]) }
        }
    }
    func application(_ sender: NSApplication, openFiles filenames: [String]) {
        trace("open-files count=\(filenames.count)")
        openBatch(filenames)
        sender.reply(toOpenOrPrint: .success)
    }
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        if launched && !flag { openBatch([]) }
        return true
    }
    private func openBatch(_ paths: [String]) {
        trace("open-batch count=\(paths.count)")
        guard !quitting else { return }
        let welcome = paths.isEmpty ? [] : windows.values.filter { $0.isWelcome }
        let id = UUID()
        let controller = ReportWindow(paths: paths)
        windows[id] = controller
        controller.onChoose = { [weak self] in self?.chooseFiles(nil) }
        for previous in welcome { previous.closeWelcome() }
        controller.onClose = { [weak self] in
            self?.windows.removeValue(forKey: id)
        }
        controller.onStopped = { [weak self] in
            guard let self else { return }
            self.pendingQuit.remove(id)
            if self.pendingQuit.isEmpty { NSApp.reply(toApplicationShouldTerminate: true) }
        }
    }
    @objc private func chooseFiles(_ sender: Any?) {
        let panel = NSOpenPanel()
        panel.title = "Choose EML files"
        panel.prompt = "Create copies"
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = false
        panel.canChooseFiles = true
        panel.begin { [weak self] response in
            if response == .OK { self?.openBatch(panel.urls.map { $0.path }) }
        }
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        trace("last-window closed")
        return windows.values.allSatisfy { !$0.isRunning }
    }
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        trace("app-quit requested")
        pendingQuit = Set(windows.filter { $0.value.isRunning }.map { $0.key })
        guard !pendingQuit.isEmpty else { return .terminateNow }
        quitting = true
        for id in pendingQuit { windows[id]?.requestQuit() }
        return .terminateLater
    }
}

func trace(_ event: String) {
    if ProcessInfo.processInfo.environment["EML_REMOVER_UI_TRACE"] == "1" {
        FileHandle.standardError.write(Data("UI \(ProcessInfo.processInfo.processIdentifier): \(event)\n".utf8))
    }
}

@main
struct Main {
    @MainActor static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.regular)
        let delegate = Application()
        app.delegate = delegate
        withExtendedLifetime(delegate) { app.run() }
    }
}
