import SwiftUI

@main
struct EMLAttachmentRemoverApp: App {
  @NSApplicationDelegateAdaptor(PlatformIntegration.self) private var platform
  @State private var store = ApplicationStore.shared

  var body: some Scene {
    Window("EML Attachment Remover", id: "main") {
      MainWindowContent(store: store, platform: platform)
    }
    .defaultSize(width: 1000, height: 680)
    .windowResizability(.contentMinSize)
    .defaultLaunchBehavior(.presented)
    .restorationBehavior(.disabled)
    .commands {
      CommandGroup(replacing: .appInfo) {
        Button("About EML Attachment Remover") {
          platform.showMainWindow?()
          store.showAbout = true
        }
        .disabled(!store.acceptsRequests)
      }
      CommandGroup(after: .appInfo) {
        Button("License…") {
          platform.showMainWindow?()
          store.showLicense = true
        }
        .disabled(!store.acceptsRequests)
      }
      CommandGroup(replacing: .newItem) {
        Button("Create Copies…") {
          platform.showMainWindow?()
          store.showInputImporter = true
        }
        .keyboardShortcut("o", modifiers: .command)
        .disabled(!store.acceptsRequests)
        Button("Dismiss Completed Reports") { store.dismissCompleted() }
          .disabled(!store.records.contains(where: \.finished) || !store.acceptsRequests)
      }
    }
  }
}

private struct MainWindowContent: View {
  let store: ApplicationStore
  let platform: PlatformIntegration
  @Environment(\.openWindow) private var openWindow

  var body: some View {
    RunWindow(store: store, reveal: platform.reveal)
      .onAppear {
        let action = openWindow
        platform.showMainWindow = { action(id: "main") }
      }
  }
}
