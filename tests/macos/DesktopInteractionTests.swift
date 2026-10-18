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
        var settled = false
        defer {
            if !settled {
                // Request the actual app's owned shutdown path. A failed cleanup
                // remains a failed test; no process-name kill or helper sweep.
                _ = owned.terminate()
                if app.buttons["Confirm"].waitForExistence(timeout: 3) {
                    app.buttons["Confirm"].click()
                }
                XCTAssertTrue(app.wait(for: .notRunning, timeout: 20), "Owned GUI settlement remains unverified")
            }
        }
        let window = app.windows["EML Attachment Remover"]
        XCTAssertTrue(window.waitForExistence(timeout: 20), app.debugDescription)
        let instances = NSRunningApplication.runningApplications(withBundleIdentifier: identifier)
        XCTAssertEqual(instances.count, 1)
        XCTAssertEqual(instances.first?.processIdentifier, owned.processIdentifier)
        snapshot(app, "ready")

        let sources = ["one.eml", "two.eml"].map { root.appendingPathComponent($0) }
        let delivery = NSWorkspace.OpenConfiguration()
        delivery.createsNewApplicationInstance = false
        let recipient = try await NSWorkspace.shared.open(sources, withApplicationAt: appURL, configuration: delivery)
        XCTAssertEqual(recipient.processIdentifier, owned.processIdentifier, "File batch reached another instance")
        let finished = app.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", "2 created")).firstMatch
        XCTAssertTrue(finished.waitForExistence(timeout: 30), app.debugDescription)
        let technical = app.checkBoxes["Technical details"]
        XCTAssertTrue(technical.exists)
        technical.click()
        let detail = app.textViews.matching(NSPredicate(format: "label CONTAINS %@", "technical observations")).firstMatch
        let completed = NSPredicate { _, _ in
            (detail.value as? String)?.contains("settled true; reusable true") == true
        }
        await fulfillment(of: [XCTNSPredicateExpectation(predicate: completed, object: nil)], timeout: 20)
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
        settled = owned.isTerminated
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
