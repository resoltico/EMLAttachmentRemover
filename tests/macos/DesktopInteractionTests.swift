// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0
import AppKit
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
        let bundle = try XCTUnwrap(Bundle(url: appURL))
        let identifier = try XCTUnwrap(bundle.bundleIdentifier)
        XCTAssertEqual(input["host_scope"], "fresh-github-hosted-runner")
        // This test is admitted only by the Rust harness on an ephemeral host.
        // It never launches an installed owner app or terminates a preexisting one.
        XCTAssertTrue(NSRunningApplication.runningApplications(withBundleIdentifier: identifier).isEmpty)
        let defaults = UserDefaults.standard.persistentDomain(forName: identifier) ?? [:]
        let previousDestination = defaults["copyDestinationBytes"] as? String
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.createsNewApplicationInstance = true
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
                XCTAssertTrue(app.wait(for: .notRunning, timeout: 20), "Owned GUI settlement remains unverified")
                XCTAssertTrue(owned.isTerminated)
            }
        }
        let window = app.windows["EML Attachment Remover"]
        XCTAssertTrue(window.waitForExistence(timeout: 20), app.debugDescription)
        let instances = NSRunningApplication.runningApplications(withBundleIdentifier: identifier)
        XCTAssertEqual(instances.count, 1)
        XCTAssertEqual(instances.first?.processIdentifier, owned.processIdentifier)
        app.activate()
        snapshot(app, "ready")

        let sources = ["one.eml", "two.eml"].map { root.appendingPathComponent($0) }
        let delivery = NSWorkspace.OpenConfiguration()
        delivery.createsNewApplicationInstance = false
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
        let detail = app.textViews.firstMatch
        guard detail.waitForExistence(timeout: 5) else {
            XCTFail("Main report text is unavailable: \(app.debugDescription)")
            return
        }
        print("Observed report value type=\(type(of: detail.value)), value=\(String(describing: detail.value).prefix(2048))")
        let completed = NSPredicate { _, _ in
            detail.exists && (detail.value as? String)?.contains("settled true; reusable true") == true
        }
        let observation = await XCTWaiter.fulfillment(of: [XCTNSPredicateExpectation(predicate: completed, object: nil)], timeout: 20)
        XCTAssertEqual(observation, .completed)
        guard observation == .completed else { return }
        let golden = try Data(contentsOf: repository.appendingPathComponent("tests/contract/mime/golden/01-gmail-attachment.eml"))
        let original = try Data(contentsOf: repository.appendingPathComponent("tests/contract/mime/inputs/01-gmail-attachment.eml"))
        for source in sources {
            XCTAssertEqual(try Data(contentsOf: source), original)
            let copy = source.deletingPathExtension().appendingPathExtension("no-attachments.eml")
            XCTAssertEqual(try Data(contentsOf: copy), golden)
        }
        snapshot(app, "finished")
        app.typeKey("q", modifierFlags: .command)
        XCTAssertTrue(app.buttons["Cancel"].waitForExistence(timeout: 5))
        app.buttons["Cancel"].click()
        XCTAssertFalse(owned.isTerminated)
        app.typeKey("q", modifierFlags: .command)
        XCTAssertTrue(app.buttons["Confirm"].waitForExistence(timeout: 5))
        app.buttons["Confirm"].click()
        XCTAssertTrue(app.wait(for: .notRunning, timeout: 20))
        XCTAssertTrue(owned.isTerminated)
        XCTAssertEqual(UserDefaults.standard.persistentDomain(forName: identifier)?["copyDestinationBytes"] as? String, previousDestination,
                       "Invocation override must not persist a destination")
    }

    @MainActor
    private func snapshot(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
