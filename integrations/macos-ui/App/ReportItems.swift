import AppKit

@MainActor
final class ReportItems: NSObject, NSTableViewDataSource, NSTableViewDelegate {
  let receipt: UIReceipt
  let items: [Item]
  let layout: ReportLayout
  let card = NSStackView()
  var table: NSTableView?
  var reviewOnlyButton: NSButton?
  private var visibleItems: [Item]
  private var listHeightConstraint: NSLayoutConstraint?
  init(receipt: UIReceipt, layout: ReportLayout, selected: Int) {
    self.receipt = receipt
    self.items = receipt.report.items
    self.visibleItems = receipt.report.items
    self.layout = layout
    super.init()
    if items.count > 1 { makeTable(selectedRow: selected) }
    card.orientation = .vertical
    card.alignment = .leading
    card.spacing = 10
    let cardBox = NSBox(frame: NSRect(x: 0, y: 0, width: 584, height: 64))
    cardBox.boxType = .custom
    cardBox.titlePosition = .noTitle
    cardBox.borderWidth = 1
    cardBox.borderColor = .separatorColor
    cardBox.fillColor = .controlBackgroundColor
    cardBox.cornerRadius = 10
    cardBox.contentViewMargins = NSSize(width: 18, height: 16)
    let cardContent = NSView()
    cardBox.contentView = cardContent
    card.translatesAutoresizingMaskIntoConstraints = false
    cardContent.addSubview(card)
    NSLayoutConstraint.activate([
      card.leadingAnchor.constraint(equalTo: cardContent.leadingAnchor),
      card.trailingAnchor.constraint(equalTo: cardContent.trailingAnchor),
      card.topAnchor.constraint(equalTo: cardContent.topAnchor),
      card.bottomAnchor.constraint(equalTo: cardContent.bottomAnchor),
    ])
    layout.add(cardBox)
    if items.indices.contains(selected) { showItem(items[selected]) }
    table?.scrollRowToVisible(selected)
  }
  func makeTable(selectedRow: Int) {
    if items.contains(where: ResultPresentation.needsReview) {
      let filter = NSButton(
        checkboxWithTitle: "Needs review only", target: self, action: #selector(filterItems))
      reviewOnlyButton = filter
      layout.add(filter)
    }
    let list = NSTableView()
    for (id, title, width) in [("file", "File", 355.0), ("result", "Result", 210.0)] {
      let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier(id))
      column.title = title
      column.width = width
      list.addTableColumn(column)
    }
    list.columnAutoresizingStyle = .firstColumnOnlyAutoresizingStyle
    list.tableColumns[0].minWidth = 200
    list.tableColumns[1].minWidth = 140
    list.delegate = self
    list.dataSource = self
    list.rowHeight = 28
    list.usesAlternatingRowBackgroundColors = true
    list.allowsEmptySelection = false
    let scroll = NSScrollView()
    scroll.hasVerticalScroller = true
    scroll.hasHorizontalScroller = true
    scroll.autohidesScrollers = true
    scroll.borderType = .bezelBorder
    scroll.documentView = list
    layout.add(scroll)
    table = list
    listHeightConstraint = scroll.heightAnchor.constraint(equalToConstant: 96)
    listHeightConstraint?.isActive = true
    NotificationCenter.default.addObserver(
      self, selector: #selector(windowResized), name: NSWindow.didResizeNotification,
      object: layout.window)
    updateListHeight()
    list.selectRowIndexes(IndexSet(integer: selectedRow), byExtendingSelection: false)
  }
  private func listHeight(_ list: NSTableView) -> CGFloat {
    CGFloat(visibleItems.count) * (list.rowHeight + list.intercellSpacing.height)
      + (list.headerView?.frame.height ?? 28) + 4
  }
  deinit { NotificationCenter.default.removeObserver(self) }
  @objc private func windowResized(_ notification: Notification) { updateListHeight() }
  private func updateListHeight() {
    guard let table, let outer = layout.window.contentView else { return }
    let natural = listHeight(table)
    listHeightConstraint?.constant = min(natural, max(min(96, natural), outer.bounds.height * 0.30))
  }
  @objc private func filterItems(_ sender: NSButton) {
    guard let table else { return }
    let selected =
      visibleItems.indices.contains(table.selectedRow)
      ? visibleItems[table.selectedRow].index : nil
    visibleItems = sender.state == .on ? items.filter(ResultPresentation.needsReview) : items
    updateListHeight()
    table.reloadData()
    let row = visibleItems.firstIndex(where: { $0.index == selected }) ?? 0
    if visibleItems.indices.contains(row) {
      table.selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false)
      table.scrollRowToVisible(row)
      showItem(visibleItems[row])
    }
    layout.layoutContent()
  }
  func numberOfRows(in tableView: NSTableView) -> Int { visibleItems.count }
  func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView?
  {
    let isFilename = tableColumn?.identifier.rawValue == "file"
    let item = visibleItems[row]
    if !isFilename { return ResultPresentation.cell(item) }
    let field = UIControls.label(item.name)
    field.maximumNumberOfLines = 1
    field.lineBreakMode = .byTruncatingMiddle
    field.toolTip = safeText(item.sourceRequest.display)
    return field
  }
  func tableViewSelectionDidChange(_ notification: Notification) {
    guard let table, visibleItems.indices.contains(table.selectedRow) else { return }
    showItem(visibleItems[table.selectedRow])
  }
  func showItem(_ item: Item) {
    for view in card.arrangedSubviews {
      card.removeArrangedSubview(view)
      view.removeFromSuperview()
    }
    func append(_ view: NSView) {
      card.addArrangedSubview(view)
      view.widthAnchor.constraint(equalTo: card.widthAnchor).isActive = true
    }
    let name = UIControls.label(item.name, size: 16, weight: .semibold)
    name.maximumNumberOfLines = 2
    name.lineBreakMode = .byTruncatingMiddle
    name.toolTip = safeText(item.sourceRequest.display)
    append(name)
    let parent = item.sourceRequest.url?.deletingLastPathComponent()
    let sourceFolder = UIControls.label(
      parent.map { "Source folder: " + safeText($0.lastPathComponent, multiline: false) }
        ?? "Source path: See Details", color: .secondaryLabelColor)
    sourceFolder.toolTip = parent.map {
      safeText($0.path, multiline: false)
    }
    append(sourceFolder)
    let separator = NSBox(frame: NSRect(x: 0, y: 0, width: 584, height: 64))
    separator.boxType = .separator
    append(separator)
    append(UIControls.label(item.label, weight: .semibold))
    append(UIControls.label(item.explanation))
    if !item.warnings.isEmpty { append(ResultPresentation.warnings(item)) }
    let row = NSStackView()
    row.orientation = .horizontal
    if item.acceptedURL != nil && receipt.processStatus != 120 && receipt.stopped != true {
      let reveal = UIControls.button(
        "Show copy in Finder", action: #selector(revealCopy), target: self)
      reveal.tag = item.index
      row.addArrangedSubview(reveal)
    } else if item.sourceRequest.url != nil {
      let folderButton = UIControls.button(
        "Show source folder", action: #selector(revealFolder), target: self)
      folderButton.tag = item.index
      row.addArrangedSubview(folderButton)
    }
    append(row)
  }
  @objc private func revealCopy(_ sender: NSButton) {
    guard let item = items.first(where: { $0.index == sender.tag }), let url = item.acceptedURL
    else { return }
    NSWorkspace.shared.activateFileViewerSelecting([url])
  }
  @objc private func revealFolder(_ sender: NSButton) {
    guard let item = items.first(where: { $0.index == sender.tag }),
      let url = item.sourceRequest.url
    else { return }
    NSWorkspace.shared.open(url.deletingLastPathComponent())
  }
}
