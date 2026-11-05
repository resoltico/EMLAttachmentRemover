//! Production toolkit boundary contracts.
use super::*;

#[test]
fn s1_idle_session_starts_work_after_settled_reports() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "created"));
    // F1 prevents even the third run from starting; the contract covers three settled runs.
    for index in 0..3 {
        c.submit(vec![root.path().join(format!("source-{index}.eml"))])
            .unwrap();
        pump(&c, || !c.is_busy());
    }
    c.submit(vec![root.path().join("fourth.eml")]).unwrap();
    c.tick();
    assert_eq!(
        c.window().get_waiting_count() as usize,
        0,
        "Accepted work must start while idle"
    );
}

#[test]
fn s3_window_constraints_allow_resizing_in_every_view() {
    use i_slint_core::{item_tree::ItemTreeRc, layout::Orientation, window::WindowInner};
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "created"));
    let ui = c.window();
    ui.window().set_size(slint::LogicalSize::new(1100.0, 700.0));
    for settled in [false, true] {
        if settled {
            c.submit(vec![root.path().join("source.eml")]).unwrap();
            pump(&c, || !c.is_busy());
        }
        mock_elapsed_time(Duration::from_millis(5));
        let tree = WindowInner::from_pub(ui.window()).component();
        let component = ItemTreeRc::borrow_pin(&tree);
        for axis in [Orientation::Horizontal, Orientation::Vertical] {
            let info = component.as_ref().layout_info(axis);
            assert!(info.max >= 4000.0, "settled={settled}, {axis:?}: {info:?}");
        }
        assert_eq!(ui.window().size(), slint::PhysicalSize::new(1100, 700));
        ui.window().set_size(slint::LogicalSize::new(1300.0, 800.0));
        mock_elapsed_time(Duration::from_millis(5));
        assert_eq!(ui.window().size(), slint::PhysicalSize::new(1300, 800));
        ui.window().set_size(slint::LogicalSize::new(1100.0, 700.0));
    }
}

#[test]
fn s4_interrupted_message_is_not_an_attention_failure() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(
        root.path(),
        script(
            root.path(),
            &("cat >/dev/null\n".to_owned()
                + &print(&fixture("started-preedge-cancellation"))
                + "exit 130\n"),
        ),
    );
    c.submit(vec![root.path().join("source.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    let ui = c.window();
    assert_eq!(ui.get_run_status(), "Stopped");
    assert!(
        !ui.get_items().row_data(0).unwrap().attention,
        "User Stop is not an attention failure"
    );
    assert!(!ui.get_chips().iter().any(|chip| chip.filter == 4));
    assert_eq!(
        ui.get_chips()
            .iter()
            .find(|chip| chip.filter == 5)
            .unwrap()
            .value,
        1
    );
    assert!(ui.get_run_title().contains("Stopped — 0 of 1 processed"));
}

#[test]
fn s18_results_and_inspector_never_intersect_at_supported_sizes() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let (c, _) = mixed_check_after_empty_active_layout(root.path());
    let ui = c.window();
    ui.invoke_filter_results(0);
    for (width, height) in [
        (780.0, 520.0),
        (880.0, 600.0),
        (1100.0, 700.0),
        (1300.0, 800.0),
    ] {
        ui.window().set_size(slint::LogicalSize::new(width, height));
        let mut collapsed_table_bounds = None;
        for expanded in [false, true] {
            ui.set_details_expanded(expanded);
            mock_elapsed_time(Duration::from_millis(5));
            let table = ElementHandle::find_by_accessible_label(&ui, "Message results")
                .next()
                .unwrap();
            let inspector = ElementHandle::find_by_accessible_label(&ui, "Message inspector")
                .next()
                .unwrap();
            let (t, i) = (table.absolute_position(), inspector.absolute_position());
            let bounds = (t, table.size());
            if expanded {
                assert_eq!(
                    Some(bounds),
                    collapsed_table_bounds,
                    "Expanding technical details must not squeeze the message table"
                );
            } else {
                collapsed_table_bounds = Some(bounds);
            }

            assert!(
                t.x + table.size().width <= i.x || t.y + table.size().height <= i.y,
                "{width}x{height}, expanded={expanded}: table {t:?} {:?}, inspector {i:?} {:?}",
                table.size(),
                inspector.size()
            );
            if width < 900.0 {
                assert!(!has_label(&ui, "Folder"));
            }
            if width >= 1100.0 {
                assert!(
                    i.x >= t.x + table.size().width,
                    "Wide inspector must be beside the table"
                );
            } else {
                assert!(
                    i.y >= t.y + table.size().height,
                    "Narrow inspector must move below the table"
                );
            }
        }
    }
}

#[test]
fn automatic_session_reports_drain_queue_and_reload_exact_old_results() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "created"));
    for index in 0..9 {
        c.submit(vec![root.path().join(format!("source-{index}.eml"))])
            .unwrap();
    }
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_runs().row_count(), 9);
    assert_eq!(c.window().get_waiting_count() as usize, 0);
    assert_eq!(c.window().get_queue_notice(), "");
    select_run(&c.window(), 0);
    pump(&c, || !c.window().get_report_loading());
    assert_eq!(c.window().get_run_status(), "Done");
    let folder = root.path().join("long-folder-context-".repeat(8));
    fs::create_dir(&folder).unwrap();
    let output = folder.join("Saved-报告-Rīga.jsonl");
    c.start_export(output.clone());
    pump(&c, || !c.is_busy());
    assert_eq!(fs::read(&output).unwrap(), fixture("created").as_bytes());
    assert!(c.window().get_can_reveal_report());
    assert_eq!(c.saved_report_path().as_deref(), Some(output.as_path()));
    assert!(
        c.window()
            .get_operation_feedback()
            .ends_with("Saved-报告-Rīga.jsonl")
    );
    select_run(&c.window(), 8);
    pump(&c, || !c.window().get_report_loading());
    assert!(!c.window().get_can_reveal_report());
    select_run(&c.window(), 0);
    pump(&c, || !c.window().get_report_loading());
    assert!(!c.window().get_can_reveal_report());
    c.window().invoke_dismiss();
    assert_eq!(c.window().get_runs().row_count(), 8);
}

#[test]
fn refused_check_run_again_restores_its_captured_inputs_mode_and_destination() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let cli = script(
        root.path(),
        "cat >/dev/null\nprintf 'fixture refusal' >&2\nexit 2\n",
    );
    let c = controller(root.path(), cli);
    c.window().invoke_mode_changed(true);
    let paths = vec![
        root.path().join("original-folder"),
        root.path().join("second.eml"),
    ];
    c.submit(paths.clone()).unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_run_status(), "Couldn’t start");
    assert!(c.window().get_can_review());
    assert!(
        !c.window().get_can_export(),
        "No protocol stream exists to export"
    );
    let destination = root.path().join("corrected-destination");
    c.set_destination(Some(destination.clone()));
    c.review_request();
    assert!(
        c.window().get_check_only(),
        "Generic recovery must not silently become Create"
    );
    assert!(c.window().get_reviewing());
    assert_eq!(c.window().get_selection().row_count(), 2);
    assert_eq!(c.window().get_destination_label(), "Next to originals");
    c.set_destination(Some(destination.clone()));
    assert_eq!(
        c.window().get_destination_label(),
        "~/corrected-destination"
    );
    assert!(!c.is_busy(), "Review must not automatically launch work");
}

#[test]
fn visible_action_names_distinguish_mode_from_start_and_expose_destination_state() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "created"));
    let ui = c.window();
    c.stage(vec![root.path().join("source.eml")]).unwrap();
    for (role, label) in [
        (AccessibleRole::Tab, "Create copies"),
        (AccessibleRole::Button, "Create Copies"),
    ] {
        assert!(
            ElementHandle::find_by_accessible_label(&ui, label)
                .any(|element| element.accessible_role() == Some(role)),
            "The mode and committing action retain their visible name and distinct roles"
        );
    }
    click(
        &ui,
        &format!("Copy destination: {}", ui.get_destination_label()),
    );
    let beside = ElementHandle::find_by_accessible_label(&ui, "Next to originals")
        .find(|element| element.accessible_role() == Some(AccessibleRole::Button))
        .unwrap();
    assert_eq!(beside.accessible_checkable(), Some(true));
    assert_eq!(beside.accessible_checked(), Some(true));
    click(&ui, "Next to originals");
    let output = root.path().join("copies");
    fs::create_dir(&output).unwrap();
    c.set_destination(Some(output));
    click(
        &ui,
        &format!("Copy destination: {}", ui.get_destination_label()),
    );
    let beside = ElementHandle::find_by_accessible_label(&ui, "Next to originals")
        .find(|element| element.accessible_role() == Some(AccessibleRole::Button))
        .unwrap();
    assert_eq!(beside.accessible_checked(), Some(false));
}

#[test]
fn gui_verdicts_preserve_removal_facts_and_wrong_identity_is_not_success() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    {
        let controller = controller(root.path(), completed(root.path(), "created-attachment"));
        controller
            .submit(vec![root.path().join("source.eml")])
            .unwrap();
        pump(&controller, || !controller.is_busy());
        let ui = controller.window();
        assert_eq!(ui.get_run_status().as_str(), "Done");
        assert_eq!(ui.get_item_status().as_str(), "Created · 1 removed");
        assert!(has_label(&ui, "Created 1"));
        click(&ui, "Technical details");
        let scroller = ElementHandle::find_by_accessible_label(&ui, "Message details")
            .next()
            .unwrap();
        // Virtualized rows appear only after scrolling them into the viewport.
        // Navigate by the actual viewport, independent of font/layout dimensions.
        assert!(scroller.size().height > 0.0);
        for _ in 0..16 {
            if has_label(&ui, vocabulary::removal_reason(RemovalReason::Attachment)) {
                break;
            }
            scroller.scroll(0.0, -scroller.size().height / 2.0);
            mock_elapsed_time(Duration::from_millis(5));
        }
        assert!(has_label(
            &ui,
            vocabulary::removal_reason(RemovalReason::Attachment)
        ));
        ui.invoke_about();
        mock_elapsed_time(Duration::from_millis(1));
        assert!(
            ElementHandle::find_by_accessible_label(&controller.about_window(), "#MadeWithSlint")
                .any(|element| element.accessible_role() == Some(AccessibleRole::Image))
        );
    }
    let cli = script(
        root.path(),
        &("cat >/dev/null\n".to_owned()
            + &print(&fixture("created").replace("emlattachmentremover", "wrong-cli"))),
    );
    let controller = controller(root.path(), cli);
    controller
        .submit(vec![root.path().join("source.eml")])
        .unwrap();
    pump(&controller, || !controller.is_busy());
    assert_eq!(
        controller.window().get_run_status().as_str(),
        "Couldn’t start"
    );
    assert_eq!(controller.window().get_items().row_count(), 0);
    assert_eq!(
        controller.window().get_chips().row_count(),
        0,
        "No trusted start record must not be presented as zero work failures"
    );
    assert!(!has_label(&controller.window(), "Created 1"));
}

#[test]
fn s9_s10_check_conversion_confirms_replacement_and_sends_a_fresh_create_request() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let requests = root.path().join("requests.jsonl");
    let body = format!(
        "request=$(cat)\nprintf '%s\\n' \"$request\" >>{}\ncase \"$request\" in *'\"dry_run\":true'*) {} ;; *) {} ;; esac\n",
        quote(requests.to_str().unwrap()),
        print(&fixture("planned-create-unix")),
        print(&fixture("created"))
    );
    let c = controller(root.path(), script(root.path(), &body));
    let ui = c.window();
    choose_operation(&ui, "Check only");
    let original = root.path().join("source.eml");
    c.submit(vec![original.clone()]).unwrap();
    pump(&c, || !c.is_busy());
    assert!(ui.get_can_create());
    assert!(has_label(&ui, "Create Copies…"));
    let unrelated = root.path().join("unrelated.eml");
    c.stage(vec![unrelated.clone()]).unwrap();
    select_run(&ui, 0);
    click(&ui, "Create Copies…");
    assert_eq!(ui.get_confirmation(), Confirmation::ReplaceDraft);
    click(&ui, "Cancel");
    assert_eq!(
        ui.get_selection().row_data(0).unwrap().path.full.as_str(),
        unrelated.to_str().unwrap()
    );
    assert!(ui.get_check_only());
    click(&ui, "Create Copies…");
    click(&ui, "Replace Draft");
    assert_eq!(ui.get_confirmation(), Confirmation::None);
    assert_eq!(
        ui.get_selection().row_data(0).unwrap().path.full.as_str(),
        original.to_str().unwrap()
    );
    assert!(!ui.get_check_only());
    assert_eq!(ui.get_destination_label(), "Next to originals");
    let output = root.path().join("copies");
    fs::create_dir(&output).unwrap();
    c.set_destination(Some(output.clone()));
    start_reviewed_selection(&ui);
    pump(&c, || !c.is_busy());
    let lines: Vec<serde_json::Value> = fs::read_to_string(requests)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["dry_run"], true);
    assert_eq!(lines[1]["dry_run"], false);
    assert_eq!(lines[0]["paths"], lines[1]["paths"]);
    assert!(lines[0]["output_directory"].is_null());
    assert_eq!(
        lines[1]["output_directory"],
        serde_json::to_value(eml_protocol::NativePathValue::new(&output)).unwrap()
    );
}

#[test]
fn preferences_roundtrip_and_corrupt_or_relative_settings_are_preserved() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("preferences.json");
    let destination = root.path().join("copies");
    let mut preferences = Preferences::load(path.clone());
    assert!(preferences.destination().is_none());
    assert!(preferences.warning().is_none());
    preferences
        .set_destination(Some(destination.clone()))
        .unwrap();
    assert_eq!(
        Preferences::load(path.clone()).destination(),
        Some(destination.as_path())
    );
    for corrupt in [b"not JSON".as_slice(), br#"{"destination":"relative"}"#] {
        fs::write(&path, corrupt).unwrap();
        let mut preferences = Preferences::load(path.clone());
        assert!(preferences.warning().is_some());
        assert!(preferences.destination().is_none());
        assert!(
            preferences
                .set_destination(Some(destination.clone()))
                .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), corrupt);
    }
}

#[test]
fn run_failure_without_items_explains_incomplete_discovery_without_failed_items() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let cli = script(
        root.path(),
        &("cat >/dev/null\n".to_owned() + &print(&fixture("collection-limit")) + "exit 1\n"),
    );
    let c = controller(root.path(), cli);
    choose_operation(&c.window(), "Check only");
    c.submit(vec![root.path().join("overflow")]).unwrap();
    pump(&c, || !c.is_busy());
    let ui = c.window();
    assert!(!ui.get_run_notice().is_empty());
    click(&ui, "Details");
    assert!(has_label(&ui, "Run error: INPUT_LIMIT_EXCEEDED"));
    assert!(has_label(&ui, "Collected work exceeds the item bound"));
    assert!(has_label(
        &ui,
        vocabulary::recovery_hint("INPUT_LIMIT_EXCEEDED").unwrap()
    ));
    assert!(has_label(&ui, vocabulary::COLLECTION_INCOMPLETE_NOTICE));
    assert!(!ui.get_chips().iter().any(|chip| chip.filter == 4));
    assert!(has_label(&ui, "Not processed 4096"));
    assert_eq!(ui.get_items().row_count(), 0);
    // A complete successful report must clear the run-error and discovery notices.
    completed(root.path(), "created");
    click(&ui, "New Request");
    choose_operation(&ui, "Create copies");
    c.submit(vec![root.path().join("ordinary.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(ui.get_report_diagnostics().row_count(), 0);
    assert!(has_label(&ui, "Created 1"));
}

#[test]
fn report_warnings_survive_switching_and_export_feedback() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let records = fixture("started-preedge-cancellation");
    let start = records.split_once('\n').unwrap().0.to_owned() + "\n";
    let cli = script(
        root.path(),
        &("trap '' INT\ncat >/dev/null\nprintf 'earlier CLI diagnostic\\n' >&2\n".to_owned()
            + &print(&start)
            + "while :; do :; done\n"),
    );
    let c = controller(root.path(), cli);
    let ui = c.window();
    c.submit(vec![root.path().join("first.eml")]).unwrap();
    wait_started(&c);
    force_prompt(&c);
    click(&ui, "Force stop now");
    pump(&c, || !c.is_busy());
    assert!(
        ui.get_report_diagnostics()
            .iter()
            .any(|line| line.text == vocabulary::FORCE_STOP_NOTICE)
    );
    assert!(ui.get_report_details().contains("earlier CLI diagnostic"));
    c.start_export(root.path().join("first.jsonl"));
    pump(&c, || !c.is_busy());
    assert!(ui.get_operation_feedback().contains("Report saved"));
    assert!(c.saved_report_path().unwrap().ends_with("first.jsonl"));
    assert!(
        ui.get_report_diagnostics()
            .iter()
            .any(|line| line.text == vocabulary::FORCE_STOP_NOTICE)
    );
    c.start_export(root.path().join("first.jsonl"));
    pump(&c, || !c.is_busy());
    assert!(ui.get_operation_feedback().contains("Report saved"));
    script(
        root.path(),
        &("cat >/dev/null\n".to_owned() + &print(&fixture("collection-limit")) + "exit 1\n"),
    );
    click(&ui, "New Request");
    choose_operation(&ui, "Check only");
    c.submit(vec![root.path().join("overflow")]).unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(ui.get_operation_feedback().as_str(), "");
    assert!(
        ui.get_report_diagnostics()
            .iter()
            .any(|line| line.text == "Run error: INPUT_LIMIT_EXCEEDED")
    );
    assert!(!ui.get_report_details().contains("earlier CLI diagnostic"));
    assert_sidebar(&ui);
    choose_result(&c, 0);
    pump(&c, || !ui.get_report_loading());
    assert_eq!(
        ui.get_run_label(),
        "Run 1",
        "Task drawer must activate the requested result"
    );
    assert!(ui.get_report_details().contains("earlier CLI diagnostic"));
    assert!(
        ui.get_report_diagnostics()
            .iter()
            .any(|line| line.text == vocabulary::FORCE_STOP_NOTICE)
    );
    assert!(
        !ui.get_report_diagnostics()
            .iter()
            .any(|line| line.text == "Run error: INPUT_LIMIT_EXCEEDED")
    );
    assert_sidebar(&ui);
    choose_result(&c, 1);
    pump(&c, || !ui.get_report_loading());
    assert!(
        ui.get_report_diagnostics()
            .iter()
            .any(|line| line.text == "Run error: INPUT_LIMIT_EXCEEDED")
    );
    assert!(
        !ui.get_report_diagnostics()
            .iter()
            .any(|line| line.text == vocabulary::FORCE_STOP_NOTICE)
    );
}

#[test]
fn unavailable_settings_launch_with_preserved_objects_and_session_only_destinations() {
    use std::os::unix::fs::PermissionsExt;
    init_no_event_loop();
    for kind in ["directory", "unreadable", "malformed"] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("preferences.json");
        if kind == "directory" {
            fs::create_dir(&path).unwrap();
        } else {
            fs::write(
                &path,
                if kind == "malformed" {
                    b"{broken".as_slice()
                } else {
                    b"{}"
                },
            )
            .unwrap();
        }
        if kind == "unreadable" {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
            assert!(
                fs::read(&path).is_err(),
                "Fixture requires an unprivileged test user"
            );
        }
        let preferences = Preferences::load(path.clone());
        assert!(preferences.destination().is_none());
        let c = DesktopController::new(
            completed(root.path(), "created"),
            root.path().into(),
            preferences,
        )
        .unwrap();
        c.window().show().unwrap();
        let warning = c.window().get_settings_warning();
        assert!(warning.contains(path.to_str().unwrap()), "{warning}");
        assert!(warning.contains("only to this session"));
        assert!(warning.contains("preserved"));
        let cause = match kind {
            "directory" => "directory",
            "unreadable" => "Permission denied",
            _ => "line 1 column",
        };
        assert!(warning.contains(cause), "Missing reason: {warning}");
        let destination = root.path().join("session-copies");
        fs::create_dir(&destination).unwrap();
        c.set_destination(Some(destination.clone()));
        assert_eq!(
            c.window().get_destination_label().as_str(),
            "~/session-copies"
        );
        assert_eq!(c.window().get_settings_warning(), warning);
        let ui = c.window();
        ui.window().set_size(slint::LogicalSize::new(780.0, 600.0));
        for name in ["first.eml", "second.eml"] {
            c.submit(vec![root.path().join(name)]).unwrap();
            pump(&c, || !c.is_busy());
        }
        mock_elapsed_time(Duration::from_millis(5));
        click(&ui, "More actions");
        for label in ["Close Result", "Save Report…"] {
            let element = ElementHandle::find_by_accessible_label(&ui, label)
                .next()
                .unwrap();
            assert!(element.size().height > 0.0);
            assert!(
                element.absolute_position().y + element.size().height <= 600.0,
                "{kind}: {label} must remain within the supported viewport: position {:?}, size {:?}, window {:?}",
                element.absolute_position(),
                element.size(),
                ui.window().size()
            );
        }

        if kind == "directory" {
            assert!(path.is_dir());
        } else {
            if kind == "unreadable" {
                fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            }
            assert_eq!(
                fs::read(path).unwrap(),
                if kind == "malformed" {
                    b"{broken".as_slice()
                } else {
                    b"{}"
                }
            );
        }
    }
}

#[test]
fn reviewed_selection_does_not_spawn_and_start_captures_current_settings() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let requests = root.path().join("requests.jsonl");
    let body = format!(
        "cat >{}\n{}",
        quote(requests.to_str().unwrap()),
        print(&fixture("planned-create-unix"))
    );
    let c = controller(root.path(), script(root.path(), &body));
    let ui = c.window();
    let first = root.path().join("one.eml");
    c.stage(vec![first.clone(), first.clone()]).unwrap();
    c.tick();
    assert!(!requests.exists());
    assert!(!c.is_busy());
    assert_eq!(ui.get_runs().row_count(), 0);
    assert_eq!(ui.get_selection().row_count(), 1);
    assert!(c.stage(vec![PathBuf::from("relative.eml")]).is_err());
    assert_eq!(ui.get_selection().row_count(), 1);
    let destination = root.path().join("reviewed-copies");
    fs::create_dir(&destination).unwrap();
    c.set_destination(Some(destination.clone()));
    choose_operation(&ui, "Check only");
    assert!(!requests.exists());
    start_reviewed_selection(&ui);
    pump(&c, || !c.is_busy());
    let request: serde_json::Value = serde_json::from_slice(&fs::read(requests).unwrap()).unwrap();
    assert_eq!(request["dry_run"], true);
    assert_eq!(
        request["output_directory"],
        serde_json::to_value(eml_protocol::NativePathValue::new(&destination)).unwrap()
    );
    assert_eq!(ui.get_selection().row_count(), 0);
    assert!(ui.get_run_context().contains(destination.to_str().unwrap()));
    c.stage(vec![first]).unwrap();
    click(&ui, "Clear");
    assert!(
        ui.get_reviewing(),
        "Clearing inputs keeps an empty draft usable beside results"
    );
    assert!(has_label(&ui, "Drop email files or folders here"));
    assert_eq!(ui.get_selection().row_count(), 0);
    assert_eq!(ui.get_runs().row_count(), 1);
}

#[test]
fn selection_count_admission_preserves_duplicates_order_and_previous_review() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "created"));
    let first = root.path().join("first.eml");
    c.stage(vec![first.clone(); eml_core::MAX_INPUT_PATHS + 100])
        .unwrap();
    assert_eq!(c.window().get_selection().row_count(), 1);
    let oversized: Vec<_> = (0..10_000)
        .map(|index| root.path().join(format!("item-{index}.eml")))
        .collect();
    let refused = c.stage(oversized.clone());
    assert!(matches!(
        refused,
        Err(eml_app::QueueError::InvalidRequest(
            eml_core::RequestPathError::TooManyInputs
        ))
    ));
    assert_eq!(c.window().get_selection().row_count(), 1);
    let mut invalid = oversized;
    invalid.push(PathBuf::from("relative.eml"));
    assert!(matches!(
        c.stage(invalid),
        Err(eml_app::QueueError::AbsoluteSelectionRequired)
    ));
    c.stage(
        (0..eml_core::MAX_INPUT_PATHS - 1)
            .map(|index| root.path().join(format!("accepted-{index}.eml")))
            .collect(),
    )
    .unwrap();
    let selection = c.window().get_selection();
    assert_eq!(selection.row_count(), eml_core::MAX_INPUT_PATHS);
    assert_eq!(
        selection.row_data(0).unwrap().path.full.as_str(),
        first.to_str().unwrap()
    );
    assert!(
        selection
            .row_data(1)
            .unwrap()
            .path
            .full
            .ends_with("accepted-0.eml")
    );
    assert!(matches!(
        c.stage(vec![root.path().join("one-too-many.eml")]),
        Err(eml_app::QueueError::InvalidRequest(
            eml_core::RequestPathError::TooManyInputs
        ))
    ));
    assert_eq!(
        c.window().get_selection().row_count(),
        eml_core::MAX_INPUT_PATHS
    );
}

#[test]
fn full_queue_retains_review_and_waiting_snapshots_do_not_follow_new_settings() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), controlled(root.path(), false));
    let ui = c.window();
    c.submit(vec![root.path().join("first.eml")]).unwrap();
    wait_started(&c);
    for index in 0..eml_app::MAX_WAITING_REQUESTS {
        c.submit(vec![root.path().join(format!("waiting-{index}.eml"))])
            .unwrap();
    }
    assert_eq!(
        ui.get_waiting_count() as usize,
        eml_app::MAX_WAITING_REQUESTS
    );
    c.stage(vec![root.path().join("retain-on-refusal.eml")])
        .unwrap();
    assert!(matches!(
        c.start_selection(),
        Err(eml_app::QueueError::Full)
    ));
    assert_eq!(ui.get_selection().row_count(), 1);
    assert!(ui.get_reviewing());
    assert!(!ui.get_can_start());
    choose_operation(&ui, "Check only");
    fs::create_dir(root.path().join("later-destination")).unwrap();
    c.set_destination(Some(root.path().join("later-destination")));
    let waiting = ui.get_runs().iter().find(|run| run.waiting).unwrap();
    assert_eq!(
        waiting.mode.as_str(),
        vocabulary::mode_label(RunMode::Create)
    );

    ui.invoke_close_session(ui.get_runs().iter().find(|run| run.waiting).unwrap().token);
    assert!(ui.get_can_start());
    assert_eq!(ui.get_selection().row_count(), 1);
    assert!(c.is_busy(), "The active worker owns this queue");
    assert!(
        !c.request_quit(),
        "Waiting accepted requests require a Quit decision"
    );
    c.tick();
    assert!(
        !c.should_exit(),
        "Accepted queued work must not be silently discarded"
    );
    ui.invoke_cancel_confirmation();
    assert!(!c.should_exit());
    assert!(!c.request_quit());
    ui.invoke_stop_and_quit();
    pump(&c, || !c.is_busy());
    assert!(c.should_exit());
    assert_eq!(ui.get_waiting_count() as usize, 0);
}

#[test]
fn save_dialog_captures_the_report_and_navigation_is_not_stolen_by_waiting_work() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "created"));
    c.submit(vec![root.path().join("first.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    let saved = c.capture_export().unwrap();
    // A different report becomes selected while the first report's dialog is open.
    completed(root.path(), "planned-create-unix");
    click(&c.window(), "New Request");
    choose_operation(&c.window(), "Check only");
    c.submit(vec![root.path().join("second.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_run_label().as_str(), "Run 2");
    let output = root.path().join("captured.jsonl");
    c.start_captured_export(saved, output.clone());
    pump(&c, || !c.is_busy());
    assert_eq!(fs::read(output).unwrap(), fixture("created").as_bytes());
    assert!(c.window().get_operation_feedback().contains("Run 1"));
    // Queue a request behind two retained reports, then explicitly dismiss Run 1.
    // The remaining Run 2 must keep the view when waiting work starts.
    c.submit(vec![root.path().join("third.eml")]).unwrap();
    select_run(&c.window(), 0);
    c.window().invoke_dismiss();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_run_label().as_str(), "Run 2");
    assert_eq!(c.window().get_runs().row_count(), 2);
}

#[test]
fn attention_filter_preserves_publication_qualification_and_valid_sync_success() {
    init_no_event_loop();
    for (name, exit, expected_rows) in [
        ("created-limited-sync", 0, 0),
        ("published-with-error", 1, 1),
    ] {
        let root = tempfile::tempdir().unwrap();
        let bytes = fixture(name);
        let cli = script(
            root.path(),
            &format!("cat >/dev/null\n{}exit {exit}\n", print(&bytes)),
        );
        let c = controller(root.path(), cli);
        c.submit(vec![root.path().join("source.eml")]).unwrap();
        pump(&c, || !c.is_busy());
        let ui = c.window();
        assert_eq!(ui.get_items().row_count(), 1);
        toggle_attention(&ui);
        assert_eq!(ui.get_items().row_count(), expected_rows, "{name}");
        if expected_rows == 1 {
            assert_eq!(ui.get_item_status(), "Created — check folder");
            ui.set_details_expanded(true);
            mock_elapsed_time(Duration::from_millis(1));
            assert_inspector_text_reachable(
                &ui,
                "A complete copy was published; qualification did not complete. Inspect the destination before retrying.",
            );
            assert!(
                !ui.get_can_reveal(),
                "Incomplete qualification must not grant Show Copy"
            );
        } else {
            assert!(
                !ui.get_has_item(),
                "Successful fsync fallback is not a fabricated failure"
            );
        }
        toggle_attention(&ui);
        assert_eq!(ui.get_items().row_count(), 1);
    }
}

#[test]
fn queue_drawer_keeps_removal_and_settings_recovery_visible() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let prefs = root.path().join("preferences.json");
    fs::write(&prefs, b"{broken owned settings").unwrap();
    let c = controller(root.path(), controlled(root.path(), false));
    c.submit(vec![root.path().join("one.eml")]).unwrap();
    wait_started(&c);
    let queued = c.submit(vec![root.path().join("waiting.eml")]).unwrap();
    let ui = c.window();
    ui.invoke_new_request();
    for (width, height, font) in [
        (1060.0, 760.0, 14.0),
        (780.0, 600.0, 14.0),
        (780.0, 600.0, 20.0),
    ] {
        ui.set_ui_size(font);
        ui.window().set_size(slint::LogicalSize::new(width, height));
        mock_elapsed_time(Duration::from_millis(5));
        let warning = ElementHandle::find_by_accessible_label(
            &ui,
            "Settings unavailable — changes won't be saved",
        )
        .next()
        .unwrap();
        assert!(warning.size().height > 0.0 && warning.absolute_position().y >= 0.0);
        assert!(warning.absolute_position().y + warning.size().height < height);
        assert_sidebar(&ui);
        let viewport = ElementHandle::find_by_accessible_label(&ui, "Session runs")
            .next()
            .unwrap();
        assert!(
            viewport.size().height >= 60.0,
            "Queue has a real scroll viewport"
        );
        let label = format!("Remove Run {queued}");
        for _ in 0..8 {
            if ElementHandle::find_by_accessible_label(&ui, &label).any(|button| {
                button.absolute_position().y >= viewport.absolute_position().y
                    && button.absolute_position().y + button.size().height
                        <= viewport.absolute_position().y + viewport.size().height + 0.1
            }) {
                break;
            }
            viewport.scroll(0.0, -viewport.size().height / 2.0);
            mock_elapsed_time(Duration::from_millis(5));
        }
        let remove = ElementHandle::find_by_accessible_label(&ui, &label)
            .next()
            .unwrap();
        assert!(remove.size().height >= 32.0);
        assert!(remove.absolute_position().y >= viewport.absolute_position().y - 0.1);
        assert!(
            remove.absolute_position().y + remove.size().height
                <= viewport.absolute_position().y + viewport.size().height + 0.1,
            "{width}/{font}: queued removal must be reachable in its drawer"
        );
        assert!(remove.absolute_position().x + remove.size().width <= width + 0.1);
        ui.window()
            .dispatch_event(slint::platform::WindowEvent::KeyPressed {
                text: slint::platform::Key::Escape.into(),
            });
        ui.window()
            .dispatch_event(slint::platform::WindowEvent::KeyReleased {
                text: slint::platform::Key::Escape.into(),
            });
        mock_elapsed_time(Duration::from_millis(5));
        assert!(has_label(&ui, "Session runs"));
    }
    click(&ui, "Details… — destination settings");
    mock_elapsed_time(Duration::from_millis(5));
    let details =
        ElementHandle::find_by_accessible_label(&ui, "Settings error and recovery details")
            .next()
            .unwrap();
    assert!(
        details.size().height > 0.0
            && details.absolute_position().y >= 0.0
            && details.absolute_position().y + details.size().height <= 600.1
    );
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed {
            text: slint::platform::Key::Escape.into(),
        });
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyReleased {
            text: slint::platform::Key::Escape.into(),
        });
    mock_elapsed_time(Duration::from_millis(5));
    assert_sidebar(&ui);
    click(&ui, &format!("Remove Run {queued}"));
    assert_eq!(ui.get_waiting_count() as usize, 0);
    c.stop();
    pump(&c, || !c.is_busy());
    assert_eq!(fs::read(prefs).unwrap(), b"{broken owned settings");
}

#[test]
fn native_save_capture_replacement_cancellation_and_late_callbacks() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "created"));
    c.submit(vec![root.path().join("one.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    let stale = c.capture_export().unwrap();
    assert!(!c.window().get_can_dismiss());
    assert!(!c.window().get_can_export());
    assert!(stale.suggested_name().starts_with("EML Run 1 "));
    assert!(stale.suggested_name().ends_with(".jsonl"));
    c.cancel_export(&stale);
    let source = c.capture_export().unwrap();
    c.cancel_export(&stale);
    assert!(
        !c.window().get_can_export(),
        "stale cancellation must not clear a new panel"
    );
    let existing = root.path().join("existing.jsonl");
    fs::write(&existing, b"approved replacement").unwrap();
    completed(root.path(), "created-attachment");
    c.submit(vec![root.path().join("two.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    c.start_captured_export(source, existing.clone());
    pump(&c, || !c.is_busy());
    assert_eq!(fs::read(&existing).unwrap(), fixture("created").as_bytes());
    assert_eq!(c.saved_report_path(), Some(existing));
    assert!(c.window().get_operation_feedback().contains("Run 1"));
    assert_eq!(c.window().get_operation_tone(), eml_gui::Tone::Ok);
    mock_elapsed_time(Duration::from_secs(5));
    c.tick();
    assert!(c.saved_report_path().is_none());
    let source = c.capture_export().unwrap();
    assert!(c.request_quit());
    c.start_captured_export(source, root.path().join("late.jsonl"));
    assert!(!root.path().join("late.jsonl").exists());
    assert!(!c.is_busy());
}

#[test]
fn known_prepublication_causes_are_specific_and_published_failures_keep_qualification() {
    init_no_event_loop();
    for (code, phase, label) in [
        ("MALFORMED", "prepare", "Damaged message"),
        ("NO_BODY", "prepare", "No message text"),
        ("PROTECTED", "prepare", "Signed or encrypted"),
        ("UNSUPPORTED_MIME", "prepare", "Unsupported format"),
        ("INTERRUPTED", "prepare", "Stopped"),
        ("STAGE_FAILED", "stage", "Couldn't write"),
        ("WRITE_FAILED", "write", "Couldn't write"),
        ("SOURCE_CHANGED", "read", "Changed during run"),
        ("FUTURE_ERROR", "prepare", "Not copied"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut records: Vec<serde_json::Value> = fixture("prepublication-failure")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        records[1]["error"]["code"] = code.into();
        records[1]["error"]["phase"] = phase.into();
        let stream = records
            .iter()
            .map(|record| format!("{record}\n"))
            .collect::<String>();
        let c = controller(
            root.path(),
            script(
                root.path(),
                &("cat >/dev/null\n".to_owned() + &print(&stream) + "exit 1\n"),
            ),
        );
        c.submit(vec![root.path().join("source.eml")]).unwrap();
        pump(&c, || !c.is_busy());
        assert_eq!(c.window().get_item_status().as_str(), label, "{code}");
        assert!(!c.window().get_can_reveal());
    }
    let existing_root = tempfile::tempdir().unwrap();
    let mut existing: Vec<serde_json::Value> = fixture("existing-sync-failure")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    existing[1]["error"]["code"] = "INTERRUPTED".into();
    existing[1]["error"]["phase"] = "existing".into();
    existing[2]["interrupted"] = true.into();
    existing[2]["exit_code"] = 130.into();
    let stream = existing
        .iter()
        .map(|record| format!("{record}\n"))
        .collect::<String>();
    let c = controller(
        existing_root.path(),
        script(
            existing_root.path(),
            &("cat >/dev/null\n".to_owned() + &print(&stream) + "exit 130\n"),
        ),
    );
    c.submit(vec![existing_root.path().join("existing.eml")])
        .unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_item_status().as_str(), "Stopped");
    assert!(!c.window().get_can_reveal());
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "published-with-error"));
    c.submit(vec![root.path().join("source.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(
        c.window().get_item_status().as_str(),
        "Created — check folder"
    );
}

#[test]
fn s5_background_completion_badge_does_not_steal_the_current_view() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "created"));
    for name in ["first.eml", "second.eml"] {
        c.submit(vec![root.path().join(name)]).unwrap();
        pump(&c, || !c.is_busy());
    }
    let gate = root.path().join("finish-third");
    let records = fixture("created");
    let (start, rest) = records.split_once('\n').unwrap();
    script(
        root.path(),
        &format!(
            "cat >/dev/null\n{}while [ ! -f {} ]; do :; done\n{}",
            print(&(start.to_owned() + "\n")),
            quote(gate.to_str().unwrap()),
            print(rest)
        ),
    );
    c.submit(vec![root.path().join("third.eml")]).unwrap();
    let ui = c.window();
    select_run(&ui, 1);
    fs::write(gate, b"finish").unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(ui.get_run_label(), "Run 2");
    assert!(ui.get_runs().row_data(2).unwrap().unseen);
    select_run(&ui, 2);
    pump(&c, || !ui.get_report_loading());
    assert!(!ui.get_runs().row_data(2).unwrap().unseen);
}

#[test]
fn s17_old_preferences_are_preserved_and_new_schema_remembers_three_recent_folders() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("preferences.json");
    let old = serde_json::to_vec(&serde_json::json!({"destination":root.path().join("previous")}))
        .unwrap();
    fs::write(&path, &old).unwrap();
    let c = controller(root.path(), completed(root.path(), "created"));
    assert_eq!(c.window().get_destination_label(), "Next to originals");
    assert!(has_label(
        &c.window(),
        "Settings unavailable — changes won't be saved"
    ));
    c.set_destination(Some(root.path().join("session-only")));
    assert_eq!(fs::read(&path).unwrap(), old);
    let fresh = root.path().join("fresh.json");
    let mut prefs = Preferences::load(fresh.clone());
    for name in ["a", "b", "c", "a", "d"] {
        prefs.set_destination(Some(root.path().join(name))).unwrap();
    }
    prefs
        .set_report_folder(root.path().join("reports"))
        .unwrap();
    let loaded = Preferences::load(fresh);
    assert_eq!(
        loaded.recent_destinations(),
        &[
            root.path().join("d"),
            root.path().join("a"),
            root.path().join("c")
        ]
    );
    assert_eq!(
        loaded.last_report_folder(),
        Some(root.path().join("reports").as_path())
    );
}

#[test]
fn nonexistent_destination_blocks_start_with_an_actionable_reason() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "created"));
    c.stage(vec![root.path().join("source.eml")]).unwrap();
    let folder = root.path().join("copies");
    c.set_destination(Some(folder.clone()));
    assert!(!c.window().get_can_start());
    assert_eq!(
        c.window().get_start_reason(),
        "Choose an existing folder to save copies"
    );
    assert!(matches!(
        c.start_selection(),
        Err(eml_app::QueueError::DestinationUnavailable)
    ));
    fs::create_dir(&folder).unwrap();
    c.set_destination(Some(folder));
    assert!(c.window().get_can_start());
}

#[test]
fn draft_has_one_primary_commit_action_when_inputs_exist_and_none_when_empty() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "created"));
    let ui = c.window();
    let count = || ElementHandle::find_by_element_type_name(&ui, "PrimaryButton").count();
    assert_eq!(count(), 0, "The empty drop zone has no action to commit");
    c.stage(vec![root.path().join("source.eml")]).unwrap();
    assert_eq!(count(), 1);
    ui.invoke_mode_changed(true);
    assert_eq!(count(), 1);
    c.set_destination(Some(root.path().join("missing")));
    assert!(!ui.get_can_start());
    assert_eq!(
        count(),
        1,
        "The blocked primary remains visible with its explanation"
    );
    ui.invoke_clear_selection();
    assert_eq!(count(), 0);
}

#[test]
fn s8_force_stop_is_delayed_and_escape_cancels_the_confirmation_without_resuming() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), controlled(root.path(), true));
    c.submit(vec![root.path().join("source.eml")]).unwrap();
    wait_started(&c);
    let ui = c.window();
    assert!(!has_label(&ui, "Force Stop…"));
    click(&ui, "Stop");
    assert!(!ui.get_can_stop());
    assert!(has_label(&ui, "Stopping…"));
    mock_elapsed_time(Duration::from_millis(4999));
    c.tick();
    assert!(!ui.get_can_force());
    ui.invoke_force_stop();
    assert_eq!(ui.get_confirmation(), Confirmation::None);
    mock_elapsed_time(Duration::from_millis(1));
    c.tick();
    assert!(ui.get_can_force());
    click(&ui, "Force Stop…");
    assert_eq!(ui.get_confirmation(), Confirmation::ForceStop);
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed {
            text: slint::platform::Key::Escape.into(),
        });
    assert_eq!(ui.get_confirmation(), Confirmation::None);
    assert!(c.is_busy());
    assert!(ui.get_stop_requested());
    c.force_stop();
    pump(&c, || !c.is_busy());
}

#[test]
fn menus_share_action_predicates_in_four_representative_states() {
    // The backend presents non-native menus. Verify their actual Slint bindings,
    // then exercise independent state expectations on the production controller.
    // A drifted menu binding fails even if its callback has a boundary guard.
    let source = include_str!("../../ui/main.slint");
    let menus = source
        .split_once("    MenuBar {")
        .unwrap()
        .1
        .split_once("    HorizontalBox { padding:")
        .unwrap()
        .0;
    let bindings = [
        ("New Request", "can-edit"),
        ("Add Files…", "can-edit"),
        ("Add Folder…", "can-edit"),
        ("Copy", "can-copy-menu"),
        ("Select All", "can-select-all"),
        ("root.start-menu-label", "can-start"),
        ("Stop", "can-stop"),
        ("Force Stop…", "can-force"),
        ("root.review-label", "can-review"),
        ("root.reveal-output-label", "can-reveal-output"),
        ("Save Report…", "can-export"),
        ("Close Result", "can-dismiss"),
        ("All Messages", "can-filter"),
        ("Need Attention", "can-filter"),
        ("root.technical-details-label", "can-details"),
    ];
    for (title, predicate) in bindings {
        let entry = menus
            .lines()
            .find(|line| {
                line.contains("MenuItem {")
                    && (line.contains(&format!("title: \"{title}\""))
                        || line.contains(&format!("title: {title};")))
            })
            .unwrap_or_else(|| panic!("missing menu {title}"));
        assert!(
            entry.contains(&format!("enabled: root.{predicate};")),
            "menu {title} diverges from its action predicate"
        );
    }
    for line in menus
        .lines()
        .filter(|line| line.contains("MenuItem {") && !line.contains("enabled:"))
    {
        assert!(
            [
                "Close Window",
                "Minimize",
                "Project Website",
                "Licenses",
                "About"
            ]
            .iter()
            .any(|title| line.contains(title)),
            "unqualified menu item: {line}"
        );
    }
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), controlled(root.path(), false));
    let ui = c.window();
    let assert_state = |start, stop, review, save, filter| {
        assert_eq!(
            (
                ui.get_can_start(),
                ui.get_can_stop(),
                ui.get_can_review(),
                ui.get_can_export(),
                ui.get_can_filter()
            ),
            (start, stop, review, save, filter)
        );
        if ui.get_reviewing() && ui.get_selection().row_count() > 0 {
            let button =
                ElementHandle::find_by_accessible_label(&ui, ui.get_start_label().as_str())
                    .find(|e| e.accessible_role() == Some(AccessibleRole::Button))
                    .unwrap();
            assert_eq!(button.accessible_enabled(), Some(start));
        }
    };
    assert_state(false, false, false, false, false);
    c.stage(vec![root.path().join("message.eml")]).unwrap();
    assert_state(true, false, false, false, false);
    c.start_selection().unwrap();
    wait_started(&c);
    assert_state(false, true, false, false, true);
    c.stop();
    pump(&c, || !c.is_busy());
    assert_state(false, false, true, true, true);
    assert_eq!(
        ElementHandle::find_by_accessible_label(&ui, "Continue…")
            .find(|e| e.accessible_role() == Some(AccessibleRole::Button))
            .unwrap()
            .accessible_enabled(),
        Some(true)
    );
}

#[test]
fn s13_buttons_are_named_chips_are_checkable_and_rows_never_announce_live() {
    use i_slint_backend_testing::{AccessibleLiveness, ElementQuery, ElementRoot};
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), controlled(root.path(), false));
    let ui = c.window();
    let inspect = |allowed_live: Vec<String>| {
        mock_elapsed_time(Duration::from_millis(5));
        let elements = ElementQuery::from_root(&ui).match_descendants().find_all();
        let mut buttons = 0;
        for element in elements {
            if element.accessible_role() == Some(AccessibleRole::Button) {
                buttons += 1;
                assert!(
                    element
                        .accessible_label()
                        .is_some_and(|label| !label.trim().is_empty()),
                    "unnamed {:?}",
                    element.type_name()
                );
            }
            if matches!(
                element.accessible_live_region(),
                Some(AccessibleLiveness::Polite | AccessibleLiveness::Assertive)
            ) {
                let label = element.accessible_label().unwrap_or_default();
                assert!(
                    allowed_live
                        .iter()
                        .any(|expected| expected == label.as_str()),
                    "unexpected live region: {label}"
                );
                assert_ne!(element.accessible_role(), Some(AccessibleRole::ListItem));
            }
        }
        assert!(buttons > 0, "AX traversal must not be empty");
    };
    inspect(vec![]);
    c.stage(vec![root.path().join("message.eml")]).unwrap();
    assert!(has_label(
        &ui,
        "1 item added · folders are searched when you start"
    ));
    for check in [false, true, false] {
        ui.invoke_mode_changed(check);
        for (label, expected) in [("Create copies", !check), ("Check only", check)] {
            let tab = ElementHandle::find_by_accessible_label(&ui, label)
                .find(|element| element.accessible_role() == Some(AccessibleRole::Tab))
                .unwrap();
            assert_eq!(tab.accessible_checked(), Some(expected));
            assert_eq!(tab.accessible_item_selected(), Some(expected));
        }
    }

    inspect(vec![]);
    c.start_selection().unwrap();
    wait_started(&c);
    inspect(vec![ui.get_run_title().into()]);
    c.stop();
    pump(&c, || !c.is_busy());
    c.message("Inspect this report before continuing.");
    inspect(vec![
        ui.get_run_title().into(),
        "Inspect this report before continuing.".into(),
    ]);
    for chip in ui.get_chips().iter() {
        let label = format!("{} {}", chip.label, chip.value);
        let control = ElementHandle::find_by_accessible_label(&ui, &label)
            .find(|element| element.accessible_role() == Some(AccessibleRole::Button))
            .unwrap();
        assert_eq!(control.accessible_checkable(), Some(true));
        assert_eq!(control.accessible_checked(), Some(chip.active));
    }
    c.window().invoke_review_request();
    c.stage(vec![root.path().join("other.eml")]).unwrap();
    select_run(&c.window(), 0);
    c.window().invoke_review_request();
    assert_eq!(ui.get_confirmation(), Confirmation::ReplaceDraft);
    inspect(vec![ui.get_run_title().into()]);
    ui.invoke_cancel_confirmation();
    c.about_window().show().unwrap();
    for window in [
        c.about_window().root_element(),
        c.license_window().root_element(),
    ] {
        for element in window.query_descendants().find_all() {
            if element.accessible_role() == Some(AccessibleRole::Button) {
                assert!(
                    element
                        .accessible_label()
                        .is_some_and(|label| !label.trim().is_empty())
                );
            }
        }
    }
}
