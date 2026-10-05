import AppKit

@MainActor
final class ApplicationDelegate: NSObject, NSApplicationDelegate, NSMenuItemValidation {
  private var windows: [UUID: ReportWindow] = [:]
  private let fileService = EmailCopyService()
  private var quitting = false
  private var pendingQuit: Set<UUID> = []
  private var launched = false
  private var launchFiles: [String] = []
  private let information = ApplicationInformation()
  private var destination: URL? = UserDefaults.standard.string(forKey: "destinationFolder")
    .map { URL(fileURLWithPath: $0) }

  func applicationDidFinishLaunching(_ notification: Notification) {
    trace("app-launched args=\(CommandLine.arguments.count)")
    configureMenus()
    launched = true
    fileService.onFiles = { [weak self] paths in
      guard let self, !self.quitting else { return false }
      self.openBatch(paths)
      return true
    }
    NSApp.servicesProvider = fileService
    NSUpdateDynamicServices()
    // AppKit delivers launch files through openFiles, including executable arguments.
    if !launchFiles.isEmpty {
      let paths = launchFiles
      launchFiles.removeAll()
      openBatch(paths)
    }
    DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
      if self.windows.isEmpty { self.openBatch([]) }
    }
  }
  private func configureMenus() {
    let menu = NSMenu()
    let root = NSMenuItem()
    let appMenu = NSMenu()
    configureInformation(appMenu)
    let services = NSMenuItem(title: "Services", action: nil, keyEquivalent: "")
    services.submenu = NSMenu(title: "Services")
    appMenu.addItem(services)
    NSApp.servicesMenu = services.submenu
    appMenu.addItem(.separator())
    appMenu.addItem(
      withTitle: "Hide EML Attachment Remover", action: #selector(NSApplication.hide(_:)),
      keyEquivalent: "h")
    let hideOthers = appMenu.addItem(
      withTitle: "Hide Others", action: #selector(NSApplication.hideOtherApplications(_:)),
      keyEquivalent: "h")
    hideOthers.keyEquivalentModifierMask = [.command, .option]
    appMenu.addItem(
      withTitle: "Show All", action: #selector(NSApplication.unhideAllApplications(_:)),
      keyEquivalent: "")
    appMenu.addItem(.separator())
    appMenu.addItem(
      withTitle: "Quit EML Attachment Remover", action: #selector(NSApplication.terminate(_:)),
      keyEquivalent: "q")
    root.submenu = appMenu
    menu.addItem(root)
    configureFileMenu(menu)
    let edit = NSMenuItem(title: "Edit", action: nil, keyEquivalent: "")
    let editMenu = NSMenu(title: "Edit")
    editMenu.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
    editMenu.addItem(
      withTitle: "Select All", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
    edit.submenu = editMenu
    menu.addItem(edit)
    let windowItem = NSMenuItem(title: "Window", action: nil, keyEquivalent: "")
    let windowMenu = NSMenu(title: "Window")
    windowMenu.addItem(
      withTitle: "Minimize", action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m")
    windowMenu.addItem(
      withTitle: "Bring All to Front", action: #selector(NSApplication.arrangeInFront(_:)),
      keyEquivalent: "")
    windowItem.submenu = windowMenu
    menu.addItem(windowItem)
    NSApp.windowsMenu = windowMenu
    NSApp.mainMenu = menu
  }
  private func configureFileMenu(_ menu: NSMenu) {
    let file = NSMenuItem(title: "File", action: nil, keyEquivalent: "")
    let fileMenu = NSMenu(title: "File")
    let open = fileMenu.addItem(
      withTitle: "Open Files or Folders…", action: #selector(chooseFiles(_:)), keyEquivalent: "o")
    open.target = self
    let closeReports = fileMenu.addItem(
      withTitle: "Close Completed Reports", action: #selector(closeCompletedReports),
      keyEquivalent: "")
    closeReports.target = self
    fileMenu.addItem(.separator())
    fileMenu.addItem(
      withTitle: "Close Window", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
    let escape = fileMenu.addItem(
      withTitle: "Close Window", action: #selector(NSWindow.performClose(_:)),
      keyEquivalent: "\u{1b}")
    escape.keyEquivalentModifierMask = []
    escape.isHidden = true
    escape.allowsKeyEquivalentWhenHidden = true
    file.submenu = fileMenu
    menu.addItem(file)
  }
  private func configureInformation(_ menu: NSMenu) {
    let about = menu.addItem(
      withTitle: "About EML Attachment Remover", action: #selector(showAbout), keyEquivalent: "")
    about.target = self
    let license = menu.addItem(
      withTitle: "License…", action: #selector(showLicense), keyEquivalent: "")
    license.target = self
  }
  @objc private func showAbout() { information.showAbout() }
  @objc private func showLicense() { information.showLicense() }

  func application(_ sender: NSApplication, openFiles filenames: [String]) {
    trace("open-files count=\(filenames.count)")
    if launched {
      openBatch(filenames)
    } else {
      launchFiles.append(contentsOf: filenames)
    }
    sender.reply(toOpenOrPrint: .success)
  }
  func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool
  {
    if launched && !flag { openBatch([]) }
    return true
  }
  private func openBatch(_ paths: [String]) {
    trace("open-batch count=\(paths.count)")
    guard !quitting else { return }
    let welcome = paths.isEmpty ? [] : windows.values.filter { $0.isWelcome }
    let id = UUID()
    let controller = ReportWindow(
      paths: paths, destination: destination, frame: welcome.first?.window.frame)
    windows[id] = controller
    controller.onChoose = { [weak self] in self?.chooseFiles(nil) }
    controller.onFiles = { [weak self] in self?.openBatch($0) }
    controller.onChooseDestination = { [weak self, weak controller] in
      guard let controller else { return }
      self?.chooseDestination(for: controller.window)
    }
    controller.onResetDestination = { [weak self] in self?.setDestination(nil) }
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
    panel.title = "Choose EML files or folders"
    panel.prompt = "Create copies"
    panel.allowsMultipleSelection = true
    panel.canChooseDirectories = true
    panel.canChooseFiles = true
    panel.begin { [weak self] response in
      if response == .OK { self?.openBatch(panel.urls.map { $0.path }) }
    }
  }
  @objc private func closeCompletedReports() {
    let completed = windows.values.filter { !$0.isRunning && !$0.isWelcome }
    for controller in completed { controller.window.close() }
  }
  func validateMenuItem(_ menuItem: NSMenuItem) -> Bool {
    if menuItem.action == #selector(closeCompletedReports) {
      return windows.values.contains { !$0.isRunning && !$0.isWelcome }
    }
    return !quitting
  }
  private func chooseDestination(for window: NSWindow) {
    let panel = NSOpenPanel()
    panel.title = "Choose destination folder"
    panel.prompt = "Use folder"
    panel.canChooseDirectories = true
    panel.canChooseFiles = false
    panel.canCreateDirectories = true
    panel.directoryURL = destination
    panel.beginSheetModal(for: window) { [weak self] response in
      if response == .OK, let folder = panel.url { self?.setDestination(folder) }
    }
  }
  private func setDestination(_ folder: URL?) {
    destination = folder
    UserDefaults.standard.set(folder?.path, forKey: "destinationFolder")
    for controller in windows.values { controller.setDestination(folder) }
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
