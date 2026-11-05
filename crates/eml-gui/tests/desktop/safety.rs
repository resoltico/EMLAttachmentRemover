//! Production toolkit boundary contracts.
use super::*;

#[test]
fn launch_operands_form_one_request_and_psn_or_missing_operands_do_not_add_work() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let controller = controller(root.path(), completed(root.path(), "created"));
    let paths = launch_paths(
        [
            OsString::from("one.eml"),
            OsString::from("-psn_0_42"),
            root.path().join("two.eml").into_os_string(),
        ],
        root.path(),
    )
    .unwrap();
    assert_eq!(
        paths,
        vec![root.path().join("one.eml"), root.path().join("two.eml")]
    );
    controller.stage(paths).unwrap();
    assert_eq!(controller.window().get_runs().row_count(), 0);
    assert_eq!(controller.window().get_selection().row_count(), 2);
    start_reviewed_selection(&controller.window());
    assert_eq!(controller.window().get_runs().row_count(), 1);
    assert!(
        launch_paths([OsString::from("-psn_0_42")], root.path())
            .unwrap()
            .is_empty()
    );
    assert!(launch_paths([OsString::from("one.eml")], Path::new("relative")).is_err());
    assert!(controller.submit(Vec::new()).is_err());
    assert_eq!(controller.window().get_runs().row_count(), 1);
    pump(&controller, || !controller.is_busy());
}

#[test]
fn deferred_drop_batches_are_distinct_and_cancelled_hover_does_not_queue() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let controller = controller(root.path(), completed(root.path(), "created"));
    for name in ["one.eml", "two.eml", "three.eml"] {
        controller.deliver_drop(root.path().join(name));
    }
    assert_eq!(controller.window().get_runs().row_count(), 0);
    mock_elapsed_time(Duration::from_millis(1));
    assert_eq!(controller.window().get_runs().row_count(), 0);
    assert_eq!(controller.window().get_selection().row_count(), 3);
    start_reviewed_selection(&controller.window());
    assert_eq!(controller.window().get_runs().row_count(), 1);
    controller.deliver_drop(root.path().join("four.eml"));
    mock_elapsed_time(Duration::from_millis(1));
    assert_eq!(controller.window().get_selection().row_count(), 1);
    assert_eq!(controller.window().get_waiting_count() as usize, 0);
    start_reviewed_selection(&controller.window());
    assert_eq!(controller.window().get_waiting_count() as usize, 1);
    controller.deliver_drop(root.path().join("cancelled.eml"));
    controller.cancel_drop();
    mock_elapsed_time(Duration::from_millis(1));
    assert_eq!(controller.window().get_waiting_count() as usize, 1);
    pump(&controller, || !controller.is_busy());
    assert_eq!(controller.window().get_runs().row_count(), 2);
}

#[test]
fn stop_and_confirmed_force_stop_show_settled_states_and_cancel_keeps_work_running() {
    init_no_event_loop();
    for force in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let controller = controller(root.path(), controlled(root.path(), force));
        controller
            .submit(vec![root.path().join("source.eml")])
            .unwrap();
        wait_started(&controller);
        let ui = controller.window();
        if force {
            force_prompt(&controller);
            assert!((ui.get_confirmation() == Confirmation::ForceStop));
            assert!(controller.is_busy());
            click(&ui, "Cancel force stop");
            assert!(!(ui.get_confirmation() == Confirmation::ForceStop));
            assert!(controller.is_busy());
            force_prompt(&controller);
            click(&ui, "Force stop now");
        } else {
            click(&ui, "Stop");
        }
        pump(&controller, || !controller.is_busy());
        assert_eq!(
            ui.get_run_status().as_str(),
            if force { "Force stopped" } else { "Stopped" }
        );
        if force {
            assert!(!has_label(&ui, "Not processed 1"));
            assert!(
                has_label(&ui, "No per-message results are available in this view."),
                "A settled report must not promise future results"
            );
        }
    }
}

#[test]
fn s11_idle_quit_is_immediate_while_busy_quit_requires_a_real_decision() {
    init_no_event_loop();
    {
        let root = tempfile::tempdir().unwrap();
        let controller = controller(root.path(), completed(root.path(), "created"));
        assert!(controller.request_quit());
        assert!(controller.should_exit());
        assert!(!(controller.window().get_confirmation() == Confirmation::Quit));
    }
    let root = tempfile::tempdir().unwrap();
    let controller = controller(root.path(), controlled(root.path(), false));
    controller
        .submit(vec![root.path().join("source.eml")])
        .unwrap();
    wait_started(&controller);
    assert!(!controller.request_quit());
    assert!(!controller.should_exit());
    assert!((controller.window().get_confirmation() == Confirmation::Quit));
    ElementHandle::find_by_accessible_label(&controller.window(), "New Request")
        .next()
        .unwrap()
        .mock_single_click(slint::platform::PointerEventButton::Left);
    mock_elapsed_time(Duration::from_millis(1));
    assert!(
        !controller.window().get_reviewing(),
        "Confirmation must block background New request input"
    );
    click(&controller.window(), "Cancel quit");
    assert!(!(controller.window().get_confirmation() == Confirmation::Quit));
    assert!(!controller.should_exit());
    assert!(!controller.request_quit());
    click(&controller.window(), "Stop and wait");
    pump(&controller, || controller.should_exit());
    assert!(!controller.is_busy());
    assert_eq!(controller.window().get_run_status().as_str(), "Stopped");
}

#[test]
fn s16_approved_report_replacement_and_busy_export_keeps_quit_confirmation() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let controller = controller(root.path(), completed(root.path(), "created"));
    controller
        .submit(vec![root.path().join("source.eml")])
        .unwrap();
    pump(&controller, || !controller.is_busy());
    let existing = root.path().join("existing.jsonl");
    fs::write(&existing, b"winner").unwrap();
    controller.start_export(existing.clone());
    assert!(controller.is_busy());
    assert!(!controller.request_quit());
    click(&controller.window(), "Cancel quit");
    pump(&controller, || !controller.is_busy());
    assert_eq!(fs::read(&existing).unwrap(), fixture("created").as_bytes());
    assert!(
        controller
            .window()
            .get_operation_feedback()
            .contains("Report saved")
    );
    assert!(controller.window().get_can_reveal_report());
    let saved = root.path().join("saved.jsonl");
    controller.start_export(saved.clone());
    pump(&controller, || !controller.is_busy());
    assert_eq!(fs::read(saved).unwrap(), fixture("created").as_bytes());
    assert!(!controller.should_exit());
}

#[test]
fn production_binary_targets_cannot_alias_on_case_insensitive_filesystems() {
    fn distinct(names: &[String]) -> bool {
        let folded: std::collections::HashSet<_> =
            names.iter().map(|name| name.to_lowercase()).collect();
        folded.len() == names.len()
    }
    assert!(!distinct(&[
        "emlattachmentremover".into(),
        "EMLAttachmentRemover".into()
    ]));
    let output = std::process::Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
            "--locked",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let names: Vec<String> = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|package| {
            ["emlattachmentremover", "eml-gui"].contains(&package["name"].as_str().unwrap())
        })
        .flat_map(|package| package["targets"].as_array().unwrap())
        .filter(|target| {
            target["kind"]
                .as_array()
                .unwrap()
                .iter()
                .any(|kind| kind == "bin")
                && Path::new(target["src_path"].as_str().unwrap()).ends_with("src/main.rs")
        })
        .map(|target| target["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names.len(), 2);
    assert!(distinct(&names), "Cargo binary outputs collide: {names:?}");
}

#[test]
fn about_contents_and_actions_fit_font_and_display_scales() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "created"));
    let about = c.about_window();
    for scale in [1.0, 2.0] {
        about
            .window()
            .dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged {
                scale_factor: scale,
            });
        for font in [13.0_f32, 26.0, 40.0] {
            c.window().set_ui_size(font);
            c.window().invoke_about();
            mock_elapsed_time(Duration::from_millis(5));
            let content = ElementHandle::find_by_element_id(&about, "AboutWindow::about-focus")
                .next()
                .unwrap();
            // The test backend inlines the menu that native windows place outside
            // the client area. Measure it before sizing the actual About content.
            let menu_height = content.absolute_position().y;
            let width = 440.0_f32.max(font * 30.0);
            let height = 490.0_f32.max(font * 32.0) + menu_height;
            about
                .window()
                .set_size(slint::LogicalSize::new(width, height));
            mock_elapsed_time(Duration::from_millis(5));
            let version = format!("Version {}", about.get_version());
            let mut action_geometry = Vec::new();
            for label in [
                "Application icon",
                eml_core::product::DISPLAY_NAME,
                version.as_str(),
                "Create body-only copies of EML messages.\nKeep original messages unchanged.",
                about.get_copyright().as_str(),
                "Project website",
                "Licenses",
                "#MadeWithSlint",
            ] {
                let item = ElementHandle::find_by_accessible_label(&about, label)
                    .next()
                    .unwrap_or_else(|| panic!("Missing About content: {label}"));
                let position = item.absolute_position();
                let size = item.size();
                assert!(
                    position.x >= 0.0
                        && position.y >= menu_height - 0.1
                        && size.width > 0.0
                        && size.height > 0.0
                        && position.x + size.width <= width + 0.1
                        && position.y + size.height <= height + 0.1,
                    "{label} clipped at font {font}, scale {scale}: {position:?}, {size:?}"
                );
                if matches!(label, "Project website" | "Licenses") {
                    assert!(size.height >= 32.0);
                    action_geometry.push((position.y, size.height));
                }
            }
            assert_eq!(action_geometry.len(), 2);
            assert!((action_geometry[0].0 - action_geometry[1].0).abs() < 0.1);
            assert!((action_geometry[0].1 - action_geometry[1].1).abs() < 0.1);
            about.invoke_licenses();
            assert_eq!(c.license_window().get_ui_size(), font);
            c.license_window().invoke_dismiss();
            about.invoke_dismiss();
        }
    }
}

#[test]
fn about_and_licenses_restore_focus_selection_and_scroll_on_reopen() {
    use i_slint_core::{items::TextInput, window::WindowInner};
    use slint::platform::{Key, PointerEventButton, WindowEvent};
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "created"));
    let about = c.about_window();
    let visits = Rc::new(std::cell::Cell::new(0));
    let observed = visits.clone();
    about.on_website(move || observed.set(observed.get() + 1));
    let key = |window: &slint::Window, value: Key| {
        window.dispatch_event(WindowEvent::KeyPressed { text: value.into() });
        window.dispatch_event(WindowEvent::KeyReleased { text: value.into() });
        mock_elapsed_time(Duration::from_millis(5));
    };
    for expected in 1..=2 {
        c.window().invoke_about();
        key(about.window(), Key::Return);
        assert_eq!(visits.get(), expected, "Website has initial keyboard focus");
        ElementHandle::find_by_accessible_label(&about, eml_core::product::DISPLAY_NAME)
            .next()
            .unwrap()
            .mock_single_click(PointerEventButton::Left);
        let focused = WindowInner::from_pub(about.window())
            .focus_item
            .borrow()
            .upgrade()
            .unwrap();
        assert!(
            focused.downcast::<TextInput>().is_some(),
            "Move focus away before reopening"
        );
        about.invoke_dismiss();
        assert!(!about.window().is_visible());
    }
    c.window().invoke_about();
    about.invoke_licenses();
    let licenses = c.license_window();
    mock_elapsed_time(Duration::from_millis(5));
    assert_eq!(
        licenses.get_licenses().as_str(),
        include_str!("../../../../LICENSE")
    );
    let input_item = WindowInner::from_pub(licenses.window())
        .focus_item
        .borrow()
        .upgrade()
        .unwrap();
    let input = input_item
        .downcast::<TextInput>()
        .expect("License text has initial focus");
    let initial_y = input_item.map_to_window(input_item.geometry().origin).y;
    let text =
        ElementHandle::find_by_accessible_label(&licenses, "Software and artwork license terms")
            .next()
            .unwrap();
    text.set_accessible_selection_offsets(3, 20);
    assert_eq!(input.as_pin_ref().anchor_position_byte_offset(), 3);
    assert_eq!(input.as_pin_ref().cursor_position_byte_offset(), 20);
    text.scroll(0.0, -200.0);
    mock_elapsed_time(Duration::from_millis(5));
    assert!(
        input_item.map_to_window(input_item.geometry().origin).y < initial_y - 1.0,
        "Move the license viewport away from the start before reopening"
    );
    licenses.invoke_dismiss();
    assert!(!licenses.window().is_visible());
    about.invoke_licenses();
    mock_elapsed_time(Duration::from_millis(5));
    assert_eq!(input.as_pin_ref().anchor_position_byte_offset(), 0);
    assert_eq!(input.as_pin_ref().cursor_position_byte_offset(), 0);
    assert!((input_item.map_to_window(input_item.geometry().origin).y - initial_y).abs() < 0.1);
    key(licenses.window(), Key::Escape);
    assert!(!licenses.window().is_visible());
    key(about.window(), Key::Escape);
    assert!(!about.window().is_visible());
}

#[test]
fn committed_quit_rejects_late_dialog_exports_and_new_selections() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "created"));
    c.submit(vec![root.path().join("first.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    let captured = c.capture_export().unwrap();
    c.set_destination(Some(root.path().join("chosen")));
    let preferences = fs::read(root.path().join("preferences.json")).unwrap();
    let destination_label = c.window().get_destination_label();
    assert!(c.request_quit());
    c.set_destination(Some(root.path().join("late-folder")));
    assert_eq!(
        fs::read(root.path().join("preferences.json")).unwrap(),
        preferences
    );
    assert_eq!(c.window().get_destination_label(), destination_label);
    assert!(matches!(
        c.stage(vec![root.path().join("late.eml")]),
        Err(eml_app::QueueError::ShuttingDown)
    ));
    assert!(matches!(
        c.start_selection(),
        Err(eml_app::QueueError::ShuttingDown)
    ));
    assert!(c.capture_export().is_none());
    let output = root.path().join("late.jsonl");
    c.start_captured_export(captured, output.clone());
    assert!(!c.is_busy());
    assert!(!output.exists());
    assert_eq!(c.window().get_operation_feedback(), "");
}

#[test]
fn a_closed_force_stop_prompt_cannot_kill_the_next_queued_worker() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let records = fixture("started-preedge-cancellation");
    let (start, rest) = records.split_once('\n').unwrap();
    let gate = root.path().join("finish-first");
    let body = format!(
        "trap '' INT\ncat >/dev/null\n{}if mkdir {} 2>/dev/null; then while [ ! -f {} ]; do :; done; {}exit 130; else while :; do :; done; fi\n",
        print(&(start.to_owned() + "\n")),
        quote(root.path().join("first-worker").to_str().unwrap()),
        quote(gate.to_str().unwrap()),
        print(rest)
    );
    let c = controller(root.path(), script(root.path(), &body));
    c.submit(vec![root.path().join("first.eml")]).unwrap();
    wait_started(&c);
    c.submit(vec![root.path().join("second.eml")]).unwrap();
    select_run(&c.window(), 0);
    force_prompt(&c);
    let ui = c.window();
    assert_eq!(ui.get_confirmation(), Confirmation::ForceStop);
    fs::write(gate, b"finish").unwrap();
    pump(&c, || {
        ui.get_runs()
            .row_data(1)
            .is_some_and(|run| run.status == "0 of 1")
            && ui.get_confirmation() == Confirmation::None
            && c.is_busy()
    });
    select_run(&ui, 1);
    let closed = ui.get_confirmation() == Confirmation::None;
    ui.invoke_confirm_force_stop();
    let next_unchanged = ui.get_run_status() == "Creating copies";
    c.force_stop();
    pump(&c, || !c.is_busy());
    assert!(closed);
    assert!(
        next_unchanged,
        "A stale answer must not stop the next worker"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn macos_raw_launch_is_rejected_before_ui_and_bundle_layout_is_admitted() {
    let root = tempfile::tempdir().unwrap();
    assert!(
        eml_gui::platform::validate_launch(
            &root.path().join("emlattachmentremover-gui"),
            &root.path().join("emlattachmentremover")
        )
        .is_err()
    );
    let contents = root.path().join("EML Attachment Remover.app/Contents");
    fs::create_dir_all(contents.join("MacOS")).unwrap();
    fs::create_dir_all(contents.join("Helpers")).unwrap();
    fs::write(contents.join("Info.plist"), b"owned fixture").unwrap();
    let helper = contents.join("Helpers/emlattachmentremover");
    fs::write(&helper, b"owned fixture").unwrap();
    assert!(
        eml_gui::platform::validate_launch(&contents.join("MacOS/EMLAttachmentRemover"), &helper)
            .is_ok()
    );
}

#[test]
fn copying_paths_uses_native_values_without_granting_unverified_reveal() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let mut records: Vec<serde_json::Value> = fixture("prepublication-failure")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let source = "/owned/日本語\n\u{202e}.eml";
    let destination = "/owned/日本語\n\u{202e}.no-attachments.eml";
    for (field, native) in [("source", source), ("destination", destination)] {
        records[1][field]["display"] = eml_protocol::TerminalPath::new(Path::new(native))
            .to_string()
            .into();
        records[1][field]["native"]["base64"] = STANDARD.encode(native.as_bytes()).into();
    }
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
    assert_eq!(c.selected_path(false).as_deref(), Some(Path::new(source)));
    assert_eq!(
        c.selected_path(true).as_deref(),
        Some(Path::new(destination))
    );
    assert_ne!(c.window().get_item_source_path().full.as_str(), source);
    assert!(c.window().get_can_copy_source() && c.window().get_can_copy_destination());
    assert!(!c.window().get_can_reveal());
    assert!(c.selected_copy().is_none());
}

#[test]
fn s15_pending_confirmation_ignores_new_and_previously_batched_drops() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), controlled(root.path(), true));
    c.submit(vec![root.path().join("running.eml")]).unwrap();
    wait_started(&c);
    c.deliver_drop(root.path().join("before-prompt.eml"));
    assert!(!c.request_quit());
    c.deliver_drop(root.path().join("during-prompt.eml"));
    mock_elapsed_time(Duration::from_millis(5));
    assert_eq!(c.window().get_selection().row_count(), 0);
    assert!(matches!(
        c.stage(vec![root.path().join("late-panel.eml")]),
        Err(eml_app::QueueError::DecisionPending)
    ));
    c.window().invoke_cancel_confirmation();
    c.force_stop();
    pump(&c, || !c.is_busy());
}
