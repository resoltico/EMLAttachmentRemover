// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import AppKit
import Darwin
import XCTest

final class DesktopInteractionTests: XCTestCase {
    @MainActor
    func testOwnedFinderBatchAndQuitSettlement() async throws {
        let repository = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let inputURL = repository.appendingPathComponent("target/desktop-interaction/input.json")
        let input = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: inputURL)) as? [String: String])
        let appURL = URL(fileURLWithPath: try XCTUnwrap(input["app"]))
        let root = URL(fileURLWithPath: try XCTUnwrap(input["fixture"]))
        let sources = ["one.eml", "two.eml"].map { root.appendingPathComponent($0) }
        let original = try Data(contentsOf: repository.appendingPathComponent("tests/contract/mime/inputs/01-gmail-attachment.eml"))
        for source in sources { observeSource(source, phase: "before launch") }
        addTeardownBlock { @MainActor in
            for source in sources {
                self.observeSource(source, phase: "after owned app settlement")
                XCTAssertEqual(try Data(contentsOf: source), original)
            }
        }
        let bundle = try XCTUnwrap(Bundle(url: appURL))
        let identifier = try XCTUnwrap(bundle.bundleIdentifier)
        XCTAssertEqual(input["host_scope"], "fresh-github-hosted-runner")
        // This test is admitted only by the Rust harness on an ephemeral host.
        // It never launches an installed owner app or terminates a preexisting one.
        guard NSRunningApplication.runningApplications(withBundleIdentifier: identifier).isEmpty else {
            XCTFail("Refusing to interact with a preexisting application instance")
            return
        }
        let defaults = UserDefaults.standard.persistentDomain(forName: identifier) ?? [:]
        let previousDestination = defaults["copyDestinationBytes"] as? String
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.createsNewApplicationInstance = true
        configuration.addsToRecentItems = false
        configuration.arguments = ["-copyDestinationBytes", ""]
        let owned = try await NSWorkspace.shared.openApplication(at: appURL, configuration: configuration)
        let app = XCUIApplication(url: appURL)
        addTeardownBlock { @MainActor in
            if !owned.isTerminated {
                // Request the actual app's owned shutdown path. A failed cleanup
                // remains a failed test; no process-name kill or helper sweep.
                let accepted = owned.terminate()
                print("Owned termination request accepted: \(accepted); observed state: \(app.state)")
                if !app.buttons["Confirm"].waitForExistence(timeout: 3), !owned.isTerminated {
                    app.typeKey("q", modifierFlags: .command)
                }
                if app.buttons["Confirm"].waitForExistence(timeout: 3) {
                    app.buttons["Confirm"].click()
                }
                let ended = app.wait(for: .notRunning, timeout: 20)
                XCTAssertTrue(ended, "Owned GUI normal settlement remains unverified")
                if !ended && !owned.isTerminated {
                    // Exact NSWorkspace-returned test instance only. A cleanup
                    // fallback cannot turn the failed normal-quit gate green.
                    print("Owned GUI forced cleanup requested: \(owned.forceTerminate())")
                    XCTAssertTrue(app.wait(for: .notRunning, timeout: 5), "Owned GUI forced cleanup remains unverified")
                }
                XCTAssertTrue(owned.isTerminated)
            }
        }
        let launched = NSPredicate { _, _ in owned.isFinishedLaunching && !owned.isTerminated }
        let launchObservation = await XCTWaiter.fulfillment(of: [XCTNSPredicateExpectation(predicate: launched, object: nil)], timeout: 20)
        guard launchObservation == .completed else {
            XCTFail("Owned application did not finish native launch")
            return
        }
        app.activate()
        let window = app.windows["EML Attachment Remover"]
        XCTAssertTrue(window.waitForExistence(timeout: 20), app.debugDescription)
        let instances = NSRunningApplication.runningApplications(withBundleIdentifier: identifier)
        XCTAssertEqual(instances.count, 1)
        XCTAssertEqual(instances.first?.processIdentifier, owned.processIdentifier)
        snapshot(app, "ready")
        app.buttons["About"].click()
        let about = app.windows["About EML Attachment Remover"]
        XCTAssertTrue(about.waitForExistence(timeout: 10), app.debugDescription)
        let version = try XCTUnwrap(bundle.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String)
        let identity = app.descendants(matching: .any).allElementsBoundByIndex.first {
            $0.title == "Product identity and licensing"
        }
        XCTAssertTrue((identity?.value as? String)?.contains("EML Attachment Remover \(version)") == true)
        snapshot(app, "about")
        about.buttons["_XCUI:CloseWindow"].click()

        app.menuBars.menuBarItems["Help"].click()
        app.menuItems["Software and Artwork Licenses…"].click()
        let licenses = app.windows["Software and Artwork Licenses"]
        XCTAssertTrue(licenses.waitForExistence(timeout: 10), app.debugDescription)
        let licenseText = try XCTUnwrap(app.descendants(matching: .any).allElementsBoundByIndex.first {
            $0.title == "Complete software and owner artwork license terms"
        })
        let expectedLicense = try XCTUnwrap(String(data: Data(contentsOf: repository.appendingPathComponent("LICENSE")), encoding: .utf8))
        XCTAssertEqual(licenseText.value as? String, expectedLicense)
        licenseText.click()
        app.typeKey("a", modifierFlags: .command)
        let edit = app.menuBars.menuBarItems["Edit"]
        edit.click()
        XCTAssertTrue(edit.menus.menuItems["Copy"].isEnabled)
        app.typeKey(XCUIKeyboardKey.escape, modifierFlags: [])
        snapshot(app, "licenses")
        licenses.buttons["_XCUI:CloseWindow"].click()

        window.buttons["_XCUI:CloseWindow"].click()
        let hidden = NSPredicate { _, _ in !window.exists }
        let hiddenObservation = await XCTWaiter.fulfillment(of: [XCTNSPredicateExpectation(predicate: hidden, object: nil)], timeout: 10)
        XCTAssertEqual(hiddenObservation, .completed)
        XCTAssertFalse(owned.isTerminated, "Closing the main window terminated the application")
        let reopen = NSWorkspace.OpenConfiguration()
        reopen.createsNewApplicationInstance = false
        reopen.addsToRecentItems = false
        let restored = try await NSWorkspace.shared.openApplication(at: appURL, configuration: reopen)
        XCTAssertEqual(restored.processIdentifier, owned.processIdentifier)
        XCTAssertTrue(window.waitForExistence(timeout: 10), app.debugDescription)
        snapshot(app, "reopened")

        let delivery = NSWorkspace.OpenConfiguration()
        delivery.createsNewApplicationInstance = false
        delivery.addsToRecentItems = false
        let recipient = try await NSWorkspace.shared.open(sources, withApplicationAt: appURL, configuration: delivery)
        XCTAssertEqual(recipient.processIdentifier, owned.processIdentifier, "File batch reached another instance")
        let finished = NSPredicate { _, _ in
            app.buttons.allElementsBoundByIndex.contains { button in
                let text = button.title + "\n" + button.label + "\n" + (button.value as? String ?? "")
                return text.contains("2 created") && text.contains("Finished")
            }
        }
        let finishedObservation = await XCTWaiter.fulfillment(of: [XCTNSPredicateExpectation(predicate: finished, object: nil)], timeout: 30)
        guard finishedObservation == .completed else {
            for button in app.buttons.allElementsBoundByIndex {
                print("Observed button title=\(button.title.debugDescription), label=\(button.label.debugDescription), value=\(String(describing: button.value).prefix(2048))")
            }
            for element in app.descendants(matching: .any).allElementsBoundByIndex {
                if element.title.hasPrefix("Selected message") {
                    print("Observed failed-batch detail: \(String(describing: element.value).prefix(16384))")
                }
            }
            XCTFail("Native batch did not reach its two-created presentation: \(app.debugDescription)")
            return
        }
        for text in window.staticTexts.allElementsBoundByIndex {
            print("Observed static text label=\(text.label.debugDescription), value=\(String(describing: text.value).prefix(2048))")
        }
        let technical = app.checkBoxes["Technical details"]
        guard technical.exists else { XCTFail("Technical details control is unavailable"); return }
        technical.click()
        print("Observed technical disclosure title=\(technical.title.debugDescription), value=\(String(describing: technical.value))")
        let completed = NSPredicate { _, _ in
            app.descendants(matching: .any).allElementsBoundByIndex.contains {
                $0.title == "Selected message, removed parts and technical observations"
                    && ($0.value as? String)?.contains("settled true; reusable true") == true
            }
        }
        let observation = await XCTWaiter.fulfillment(of: [XCTNSPredicateExpectation(predicate: completed, object: nil)], timeout: 20)
        XCTAssertEqual(observation, .completed, "Settled native report is unavailable: \(app.debugDescription)")
        guard observation == .completed else { return }
        let golden = try Data(contentsOf: repository.appendingPathComponent("tests/contract/mime/golden/01-gmail-attachment.eml"))
        for source in sources {
            let copy = source.deletingPathExtension().appendingPathExtension("no-attachments.eml")
            XCTAssertEqual(try Data(contentsOf: copy), golden)
        }
        snapshot(app, "finished")
        window.buttons["_XCUI:CloseWindow"].click()
        let hiddenForQuit = NSPredicate { _, _ in !window.exists }
        let hiddenForQuitObservation = await XCTWaiter.fulfillment(of: [XCTNSPredicateExpectation(predicate: hiddenForQuit, object: nil)], timeout: 10)
        XCTAssertEqual(hiddenForQuitObservation, .completed)
        print("Native hidden-window Quit request accepted: \(owned.terminate())")
        guard app.buttons["Cancel"].waitForExistence(timeout: 5) else {
            XCTFail("Hidden-window Quit did not expose its Cancel control: \(app.debugDescription)")
            return
        }
        XCTAssertTrue(window.exists, "Quit confirmation did not restore the main window")
        app.buttons["Cancel"].click()
        XCTAssertFalse(owned.isTerminated)
        snapshot(app, "quit-cancelled-from-hidden")
        app.typeKey("q", modifierFlags: .command)
        XCTAssertTrue(app.buttons["Confirm"].waitForExistence(timeout: 5))
        app.buttons["Confirm"].click()
        XCTAssertTrue(app.wait(for: .notRunning, timeout: 20))
        XCTAssertTrue(owned.isTerminated)
        XCTAssertEqual(UserDefaults.standard.persistentDomain(forName: identifier)?["copyDestinationBytes"] as? String, previousDestination,
                       "Invocation override must not persist a destination")
    }

    @MainActor
    private func observeSource(_ source: URL, phase: String) {
        var facts = stat()
        let result = source.withUnsafeFileSystemRepresentation { path -> Int32 in
            guard let path else { return -1 }
            return lstat(path, &facts)
        }
        print("Owned source \(phase) \(source.lastPathComponent.debugDescription): stat=\(result); dev=\(facts.st_dev); inode=\(facts.st_ino); uid=\(facts.st_uid); gid=\(facts.st_gid); mode=\(facts.st_mode); size=\(facts.st_size); flags=\(facts.st_flags); mtime=\(facts.st_mtimespec.tv_sec).\(facts.st_mtimespec.tv_nsec); ctime=\(facts.st_ctimespec.tv_sec).\(facts.st_ctimespec.tv_nsec)")
        source.withUnsafeFileSystemRepresentation { path in
            guard let path else { return }
            let count = listxattr(path, nil, 0, XATTR_NOFOLLOW)
            guard count >= 0 && count <= 65536 else {
                print("Owned source xattr name extent unverified: \(count)")
                return
            }
            var names = [CChar](repeating: 0, count: max(1, count))
            let observed = names.withUnsafeMutableBufferPointer { buffer in
                listxattr(path, buffer.baseAddress, buffer.count, XATTR_NOFOLLOW)
            }
            guard observed >= 0 && observed <= names.count else {
                print("Owned source xattr names changed or unavailable: \(observed)")
                return
            }
            let labels = names.prefix(observed).split(separator: 0).map {
                String(decoding: $0.map { UInt8(bitPattern: $0) }, as: UTF8.self)
            }
            print("Owned source xattr names \(phase): \(labels.debugDescription)")
        }
    }

    @MainActor
    private func snapshot(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
