import Darwin
import Foundation
import Testing

@testable import EMLAttachmentRemover
@testable import RunModel

/// All argument-domain cases are serialized: Foundation shares that volatile
/// domain across instances. Persistent I/O touches only this fresh UUID suite.
@MainActor private final class ApplicationStateFixture {
  private static var argumentDomainAvailable = true
  private var retainUnsettledEvidence = false
  let name = "io.github.resoltico.emlattachmentremover.tests.\(UUID().uuidString)"
  let value: UserDefaults
  let originalArguments: [String: Any]
  let root: URL

  init() throws {
    guard Self.argumentDomainAvailable else { throw ApplicationFixtureFailure.unsettled }
    value = try #require(UserDefaults(suiteName: name))
    #expect(value.persistentDomain(forName: name) == nil)
    originalArguments = value.volatileDomain(forName: UserDefaults.argumentDomain)
    let repository = URL(filePath: #filePath).deletingLastPathComponent()
      .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    root = repository.appending(path: "target/application-state-tests/\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    var arguments = originalArguments
    arguments.removeValue(forKey: DestinationPreference.key)
    value.setVolatileDomain(arguments, forName: UserDefaults.argumentDomain)
  }
  func override(_ selection: Any) {
    var arguments = originalArguments
    arguments[DestinationPreference.key] = selection
    value.setVolatileDomain(arguments, forName: UserDefaults.argumentDomain)
  }
  func cleanup() {
    guard !retainUnsettledEvidence else {
      Issue.record(
        "Unsettled Store task: retained owned fixture and volatile test context at \(root.path)")
      return
    }
    value.setVolatileDomain(originalArguments, forName: UserDefaults.argumentDomain)
    value.removePersistentDomain(forName: name)
    do { try FileManager.default.removeItem(at: root) } catch {
      Issue.record("Owned application fixture cleanup failed: \(error)")
    }
  }
  func waitForCompletion(_ record: RunRecord, store: ApplicationStore) async throws {
    let deadline = ContinuousClock.now.advanced(by: .seconds(3))
    while !record.finished, ContinuousClock.now < deadline {
      do { try await Task.sleep(for: .milliseconds(10)) } catch { break }
    }
    guard !record.finished else { return }
    // Stop admitting later work and try actual Store settlement before teardown.
    store.stop(record.id)
    let cleanupDeadline = ContinuousClock.now.advanced(by: .seconds(3))
    while !record.finished, ContinuousClock.now < cleanupDeadline {
      do { try await Task.sleep(for: .milliseconds(10)) } catch { await Task.yield() }
    }
    if !record.finished {
      retainUnsettledEvidence = true
      Self.argumentDomainAvailable = false
    }
    throw ApplicationFixtureFailure.unsettled
  }
  func selection(_ name: String) throws -> URL {
    let url = root.appending(path: name)
    try Data("Content-Type: text/plain\r\n\r\nowned\r\n".utf8).write(
      to: url, options: [.withoutOverwriting])
    return url
  }
  func folder(_ name: String) throws -> URL {
    let url = root.appending(path: name)
    try FileManager.default.createDirectory(at: url, withIntermediateDirectories: false)
    return url
  }
  func nativeBytes(_ url: URL) throws -> Data {
    try url.withUnsafeFileSystemRepresentation { pointer in
      guard let pointer else { throw RequestFailure.invalidNativePath }
      return Data(bytes: pointer, count: strlen(pointer))
    }
  }
}

private enum ApplicationFixtureFailure: Error { case unsettled }

@Suite(.serialized)
struct ApplicationStateTests {
  @Test @MainActor func destinationPersistenceDistinguishesSavedChoicesAndSessionOverrides() throws
  {
    let owned = try ApplicationStateFixture()
    defer { owned.cleanup() }
    let normal = ApplicationStore(defaults: owned.value)
    #expect(normal.destination == nil)
    #expect(!normal.destinationNeedsChoice && !normal.destinationIsSessionOnly)
    #expect(owned.value.persistentDomain(forName: owned.name) == nil)
    let savedFolder = try owned.folder("saved-folder")
    normal.setDestinationFromPicker(savedFolder)
    let saved = try owned.nativeBytes(savedFolder).base64EncodedString()
    #expect(owned.value.string(forKey: DestinationPreference.key) == saved)
    owned.override("")
    let session = ApplicationStore(defaults: owned.value)
    #expect(session.destinationIsSessionOnly && session.destination == nil)
    let sessionFolder = try owned.folder("session-folder")
    session.setDestinationFromPicker(sessionFolder)
    #expect(try session.destination?.bytes() == owned.nativeBytes(sessionFolder))
    session.setDestinationFromPicker(nil)
    #expect(session.destination == nil)
    #expect(
      owned.value.persistentDomain(forName: owned.name)?[DestinationPreference.key] as? String
        == saved)
  }

  @Test @MainActor func corruptValuesRequireExplicitChoiceWithoutInitializationWrite() throws {
    let owned = try ApplicationStateFixture()
    defer { owned.cleanup() }
    for bad: Any in [Data([1, 2]), ["bad"], ["bad": "value"], 42, "%%%"] {
      owned.value.set(bad, forKey: DestinationPreference.key)
      let before = try #require(owned.value.persistentDomain(forName: owned.name))
      let store = ApplicationStore(defaults: owned.value)
      #expect(store.destinationNeedsChoice)
      #expect(!store.admit([owned.root.appending(path: "unprocessed.eml")]))
      #expect(store.records.isEmpty)
      let after = try #require(owned.value.persistentDomain(forName: owned.name))
      #expect((before as NSDictionary).isEqual(to: after))
      store.setDestinationFromPicker(nil)
      #expect(!store.destinationNeedsChoice)
      #expect(owned.value.string(forKey: DestinationPreference.key) == "")
    }
    owned.override(Data([3, 4]))
    let invalidOverride = ApplicationStore(defaults: owned.value)
    #expect(invalidOverride.destinationIsSessionOnly && invalidOverride.destinationNeedsChoice)
    invalidOverride.setDestinationFromPicker(try owned.folder("explicit-choice"))
    #expect(!invalidOverride.destinationNeedsChoice)
    #expect(
      owned.value.persistentDomain(forName: owned.name)?[DestinationPreference.key] as? String == ""
    )
  }

  @Test @MainActor func cancelledAndConfirmedPreHelperQuitDoNotLoseOrStartWork() async throws {
    let owned = try ApplicationStateFixture()
    defer { owned.cleanup() }
    let store = ApplicationStore(defaults: owned.value)
    var replies: [Bool] = []
    store.onQuitDecision = { replies.append($0) }
    #expect(store.admit([try owned.selection("active.eml")]))
    #expect(store.admit([try owned.selection("waiting.eml")]))
    let active = try #require(store.records.first { !$0.waiting })
    let first = store.requestQuit()
    #expect(first == .later)
    store.showMessage("Must not replace quit confirmation")
    #expect(store.alert == .confirmQuit)
    store.cancelQuit()
    #expect(replies == [false])
    #expect(store.records.count == 2 && active.control.snapshot().stops == 0)
    let second = store.requestQuit()
    #expect(second == .later)
    store.confirmQuit()
    #expect(replies == [false, true])
    #expect(!store.acceptsRequests)
    #expect(!store.admit([owned.root.appending(path: "rejected.eml")]))
    #expect(store.records.count == 1 && !active.helperSubmitted)
    try await owned.waitForCompletion(active, store: store)
    #expect(active.finished && !active.helperSubmitted)
    #expect(active.report == nil)
    #expect(store.activeID == nil)
    store.confirmQuit()
    let repeated = store.requestQuit()
    #expect(repeated == .now && replies == [false, true])
    #expect(owned.value.persistentDomain(forName: owned.name) == nil)
  }

  @Test @MainActor func visibleAlertsCancelQuitWithoutReplacingTheirPresentation() async throws {
    let owned = try ApplicationStateFixture()
    defer { owned.cleanup() }
    let store = ApplicationStore(defaults: owned.value)
    store.showMessage("Owned message")
    let messageQuit = store.requestQuit()
    #expect(messageQuit == .cancel && store.alert == .message("Owned message"))
    store.dismissAlert()
    for index in 0..<2 {
      #expect(store.admit([try owned.selection("report-\(index).eml")]))
      let record = try #require(store.selectedRecord)
      try await owned.waitForCompletion(record, store: store)
      #expect(record.finished)
    }
    #expect(store.admit([try owned.selection("paused.eml")]))
    #expect(store.alert == .reportSpace)
    let count = store.records.count
    let pausedQuit = store.requestQuit()
    #expect(pausedQuit == .cancel && store.alert == .reportSpace)
    #expect(store.records.count == count && store.records.filter(\.waiting).count == 1)
    #expect(owned.value.persistentDomain(forName: owned.name) == nil)
  }

  @Test func stopIntentLevelsAreMonotonicAndDoNotEscalateAnExistingCooperativeRequest() {
    let control = HelperRunControl(id: UUID())
    #expect(control.requestCooperativeStop().stops == 1)
    #expect(control.requestCooperativeStop().stops == 1)
    #expect(!control.beginFeeding())
    #expect(control.requestForceStop().stops == 2)
    #expect(control.requestCooperativeStop().stops == 2)
    let fed = HelperRunControl(id: UUID())
    #expect(fed.beginFeeding())
    #expect(!fed.beginFeeding())
    #expect(fed.requestStop().stops == 1)
    #expect(fed.snapshot().inputMayHaveBeenSent)
  }
}
