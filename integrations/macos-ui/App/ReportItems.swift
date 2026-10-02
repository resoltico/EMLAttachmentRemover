import AppKit

@MainActor
final class ReportItems: NSObject, NSTableViewDataSource, NSTableViewDelegate {
  let receipt: UIReceipt
  let items: [Item]
  let layout: ReportLayout
  let card = NSStackView()
  var table: NSTableView?
  init(receipt: UIReceipt, layout: ReportLayout, selected: Int) {
    self.receipt = receipt
    self.items = receipt.report.items
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
    let list = NSTableView()
    for (id, title, width) in [("file", "File", 355.0), ("result", "Result", 210.0)] {
      let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier(id))
      column.title = title
      column.width = width
      list.addTableColumn(column)
    }
    list.columnAutoresizingStyle = .lastColumnOnlyAutoresizingStyle
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
    scroll.heightAnchor.constraint(equalToConstant: min(140, CGFloat(items.count) * 28 + 28))
      .isActive = true
    layout.add(scroll)
    table = list
    list.selectRowIndexes(IndexSet(integer: selectedRow), byExtendingSelection: false)
  }
  func numberOfRows(in tableView: NSTableView) -> Int { items.count }
  func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView?
  {
    let field = UIControls.label(
      tableColumn?.identifier.rawValue == "file" ? items[row].name : items[row].label)
    field.maximumNumberOfLines = 1
    field.lineBreakMode = .byTruncatingMiddle
    field.toolTip = safeText(items[row].sourceRequest.display)
    return field
  }
  func tableViewSelectionDidChange(_ notification: Notification) {
    guard let table, items.indices.contains(table.selectedRow) else { return }
    showItem(items[table.selectedRow])
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
    let folder =
      item.sourceRequest.url?.deletingLastPathComponent().lastPathComponent
      ?? "See full path in Details"
    append(UIControls.label(safeText(folder), color: .secondaryLabelColor))
    let separator = NSBox(frame: NSRect(x: 0, y: 0, width: 584, height: 64))
    separator.boxType = .separator
    append(separator)
    append(
      UIControls.label(item.isConflict ? "An existing copy differs" : item.label, weight: .semibold)
    )
    append(UIControls.label(item.explanation))
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
