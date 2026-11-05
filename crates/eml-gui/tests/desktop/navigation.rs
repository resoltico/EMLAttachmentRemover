//! Production toolkit boundary contracts.
use super::*;

#[test]
fn long_result_paths_keep_distinguishing_suffixes_and_full_accessible_paths() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let mut records: Vec<serde_json::Value> = fixture("created")
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    records[0]["total"] = 2.into();
    let mut items = Vec::new();
    let mut sources = Vec::new();
    for (index, end) in ["A", "B"].into_iter().enumerate() {
        let source = root
            .path()
            .join(format!(
                "invoice-{}{end}.eml",
                "shared-description-".repeat(10)
            ))
            .to_str()
            .unwrap()
            .to_owned();
        let mut item = records[1].clone();
        item["index"] = index.into();
        item["source"]["display"] = source.clone().into();
        item["source"]["native"]["base64"] = STANDARD.encode(source.as_bytes()).into();
        items.push(item);
        sources.push(source);
    }
    records[2]["counts"]["created"] = 2.into();
    let stream = [&records[0], &items[0], &items[1], &records[2]]
        .into_iter()
        .map(|v| v.to_string() + "\n")
        .collect::<String>();
    let c = controller(
        root.path(),
        script(
            root.path(),
            &("cat >/dev/null\n".to_owned() + &print(&stream)),
        ),
    );
    c.submit(vec![root.path().join("selection")]).unwrap();
    pump(&c, || !c.is_busy());
    let ui = c.window();
    for width in [780.0, 1240.0] {
        ui.window().set_size(slint::LogicalSize::new(width, 760.0));
        mock_elapsed_time(Duration::from_millis(5));
        let a = ui.get_items().row_data(0).unwrap();
        let b = ui.get_items().row_data(1).unwrap();
        assert!(a.path.suffix.ends_with("A.eml"));
        assert!(b.path.suffix.ends_with("B.eml"));
        assert_ne!(a.path.suffix, b.path.suffix);
        let row = ElementHandle::find_by_accessible_label(&ui, a.path.full.as_str())
            .find(|element| element.accessible_role() == Some(AccessibleRole::ListItem))
            .unwrap();
        let filename = row
            .query_descendants()
            .match_type_name("FileName")
            .find_first()
            .unwrap();
        let prefix = filename
            .query_descendants()
            .match_id("FileName::prefix")
            .find_first()
            .unwrap();
        let suffix = filename
            .query_descendants()
            .match_id("FileName::suffix-area")
            .find_first()
            .unwrap();
        assert!(
            prefix.absolute_position().x + prefix.size().width
                <= suffix.absolute_position().x + 0.1,
            "Filename prefix and distinguishing suffix must not overlap"
        );

        assert_eq!(a.path.full.as_str(), sources[0]);
        assert_eq!(b.path.full.as_str(), sources[1]);
        let selected_row = ElementHandle::find_by_accessible_label(&ui, &sources[1])
            .find(|e| e.accessible_role() == Some(AccessibleRole::ListItem))
            .unwrap();
        click(&ui, &sources[1]);
        assert_eq!(ui.get_item_source_path().full.as_str(), sources[1]);
        assert!(
            selected_row.is_valid(),
            "Selecting a row must preserve its control and keyboard focus"
        );
        c.tick();
        mock_elapsed_time(Duration::from_millis(5));
        assert!(
            selected_row.is_valid(),
            "Unchanged refresh must not replace row delegates"
        );
        ui.invoke_select_item(0);
        mock_elapsed_time(Duration::from_millis(5));
        let status_cell =
            ElementHandle::find_by_accessible_label(&ui, "Created · no parts removed")
                .nth(1)
                .unwrap();
        let viewport = ElementHandle::find_by_accessible_label(&ui, "Message results")
            .next()
            .unwrap();
        let overflow = status_cell.absolute_position().y + status_cell.size().height
            - viewport.absolute_position().y
            - viewport.size().height;
        if overflow > 0.0 {
            viewport.scroll(0.0, -overflow - 1.0);
            mock_elapsed_time(Duration::from_millis(5));
        }
        assert!(
            status_cell.absolute_position().y + status_cell.size().height
                <= viewport.absolute_position().y + viewport.size().height + 0.1
        );
        status_cell.mock_single_click(slint::platform::PointerEventButton::Left);
        mock_elapsed_time(Duration::from_millis(5));
        assert_eq!(
            ui.get_item_source_path().full.as_str(),
            sources[1],
            "The whole result row, including its status, must select the message; status {:?} {:?}; all statuses {:?}",
            status_cell.absolute_position(),
            status_cell.size(),
            ElementHandle::find_by_accessible_label(&ui, "Created · no parts removed")
                .map(|e| (e.absolute_position(), e.size()))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn failure_inspector_stays_visible_and_filtered_keyboard_navigation_keeps_original_identity() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde_json::{Value, json};
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let created: Vec<Value> = fixture("created-attachment")
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let failed: Vec<Value> = fixture("prepublication-failure")
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let mut start = created[0].clone();
    start["total"] = json!(16);
    let mut events = vec![start];
    for index in 0..8 {
        let mut item = if index == 7 {
            failed[1].clone()
        } else {
            created[1].clone()
        };
        // Wire indexes deliberately have gaps: selection must use report offsets.
        item["index"] = json!(index * 2);
        for (field, suffix) in [("source", ".eml"), ("destination", ".no-attachments.eml")] {
            let path = format!("/mail/message-{index}{suffix}");
            item[field] = json!({"display":path,"native":{"encoding":"unix-bytes","base64":STANDARD.encode(path.as_bytes())}});
        }
        if index == 7 {
            item["error"] = json!({"code":"DESTINATION_EXISTS","phase":"existing","detail":"A different file already exists at the destination.","section":null});
        }
        events.push(item);
    }
    events.push(json!({"type":"summary","counts":{"created":7,"up_to_date":0,"planned":0,"failed":1,"not_processed":8},"interrupted":false,"collection_complete":true,"run_error":null,"exit_code":1}));
    let bytes = events.iter().map(|v| format!("{v}\n")).collect::<String>();
    let cli = script(
        root.path(),
        &("cat >/dev/null\n".to_owned() + &print(&bytes) + "exit 1\n"),
    );
    let c = controller(root.path(), cli);
    c.submit(vec![root.path().join("source.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    c.submit(vec![root.path().join("second-selection.eml")])
        .unwrap();
    pump(&c, || !c.is_busy());
    let ui = c.window();
    assert_eq!(ui.get_runs().row_count(), 2);
    ui.invoke_filter_results(0);
    assert_eq!(ui.get_items().row_count(), 8);
    toggle_attention(&ui);
    assert_eq!(ui.get_items().row_count(), 1);
    assert_eq!(ui.get_selected_item(), 7);
    assert_eq!(
        ui.get_item_source_path().full.as_str(),
        "/mail/message-7.eml"
    );
    assert_eq!(ui.get_item_status().as_str(), "Name conflict");
    assert!(c.selected_copy().is_none());
    toggle_attention(&ui);
    assert_eq!(ui.get_items().row_count(), 8);
    for (width, font) in [(1060.0, 13.0), (780.0, 13.0), (780.0, 20.0)] {
        ui.set_ui_size(font);
        ui.window().set_size(slint::LogicalSize::new(width, 600.0));
        mock_elapsed_time(Duration::from_millis(5));
        let viewport = ElementHandle::find_by_accessible_label(&ui, "Message details")
            .next()
            .unwrap();
        let heading = ElementHandle::find_by_accessible_label(&ui, "Message inspector")
            .next()
            .unwrap();
        assert!(heading.absolute_position().y < viewport.absolute_position().y);
        let list = ElementHandle::find_by_accessible_label(&ui, "Message results")
            .next()
            .unwrap();
        let result_header = ElementHandle::find_by_accessible_label(&ui, "Outcome")
            .next()
            .unwrap();
        let result_cell = ElementHandle::find_by_accessible_label(&ui, "Name conflict")
            .next()
            .unwrap();
        assert!(
            (result_header.absolute_position().x - result_cell.absolute_position().x).abs() < 1.0,
            "Result column header and cells must align"
        );
        let selected = ElementHandle::find_by_accessible_label(&ui, "/mail/message-7.eml")
            .find(|e| e.accessible_role() == Some(AccessibleRole::ListItem))
            .unwrap();
        assert!(
            selected.absolute_position().y >= list.absolute_position().y - 0.1
                && selected.absolute_position().y + selected.size().height
                    <= list.absolute_position().y + list.size().height + 0.1,
            "{font}: selected filename must remain visible: {:?} {:?}, list {:?} {:?}",
            selected.absolute_position(),
            selected.size(),
            list.absolute_position(),
            list.size()
        );
        ui.set_details_expanded(true);
        mock_elapsed_time(Duration::from_millis(1));
        for text in [
            "A different file already exists at the destination.",
            vocabulary::recovery_hint("DESTINATION_EXISTS").unwrap(),
            "No new copy was published.",
        ] {
            assert_inspector_text_reachable(&ui, text);
        }
        assert!(viewport.absolute_position().y + viewport.size().height < 600.0);
    }
    toggle_attention(&ui);
    assert_eq!(
        ui.get_messages_scroll_y(),
        0.0,
        "Filtering must not retain a stale offset"
    );
    // Filtering the UI must not filter the exported report.
    let exported = root.path().join("all-results.jsonl");
    c.start_export(exported.clone());
    pump(&c, || !c.is_busy());
    assert_eq!(fs::read_to_string(exported).unwrap(), bytes);
    toggle_attention(&ui);
    assert_eq!(ui.get_selected_item(), 7);
    ui.set_ui_size(13.0);
    mock_elapsed_time(Duration::from_millis(5));
    // Focus the failed result, then exercise actual Slint key events.
    click(&ui, "/mail/message-7.eml");
    mock_elapsed_time(Duration::from_millis(5));
    let key = |key: slint::platform::Key| {
        ui.window()
            .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: key.into() });
        ui.window()
            .dispatch_event(slint::platform::WindowEvent::KeyReleased { text: key.into() });
        mock_elapsed_time(Duration::from_millis(5));
    };
    key(slint::platform::Key::Home);
    assert_eq!(ui.get_selected_item(), 0);
    key(slint::platform::Key::Tab);
    key(slint::platform::Key::DownArrow);
    assert_eq!(
        ui.get_selected_item(),
        0,
        "One Tab leaves the result list rather than focusing another unselected row"
    );
    click(&ui, "/mail/message-0.eml");
    key(slint::platform::Key::DownArrow);
    assert_eq!(ui.get_selected_item(), 1);
    key(slint::platform::Key::End);
    assert_eq!(ui.get_selected_item(), 7);
    assert_eq!(
        ui.get_item_source_path().full.as_str(),
        "/mail/message-7.eml"
    );
    assert!(ui.get_messages_scroll_y() < 0.0);
    key(slint::platform::Key::UpArrow);
    assert_eq!(ui.get_selected_item(), 6);
    assert_eq!(
        c.selected_copy().unwrap(),
        Path::new("/mail/message-6.no-attachments.eml")
    );
}

#[test]
fn report_selection_empty_filter_survives_current_task_reselection() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "created"));
    c.submit(vec![root.path().join("source.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    let ui = c.window();
    toggle_attention(&ui);
    assert_eq!(ui.get_items().row_count(), 0);
    assert!(!ui.get_has_item());
    assert!(!ui.get_can_reveal());
    assert!(has_label(
        &ui,
        "No per-message results are available in this view."
    ));
    let selection = ui.get_selected_item();
    assert_sidebar(&ui);
    choose_result(&c, 0);
    assert!(
        ui.get_filter() == 4,
        "Selecting the current task retains its view filter"
    );
    assert_eq!(
        ui.get_selected_item(),
        selection,
        "An empty view retains no message selection"
    );
    assert_eq!(ui.get_items().row_count(), 0);
    assert_eq!(ui.get_selected_visible_row(), -1);
    assert!(
        !ui.get_has_item(),
        "The inspector cannot show a healthy item outside the empty filtered list"
    );
    assert!(!ui.get_can_reveal());
    click(&ui, "New Request");
    assert!(ui.get_reviewing());
    assert_sidebar(&ui);
    choose_result(&c, 0);
    assert!(!ui.get_reviewing());
    assert!(
        ui.get_filter() == 4,
        "An empty draft does not replace the retained report's filter"
    );
    assert_eq!(ui.get_selected_item(), selection);
    assert_eq!(ui.get_items().row_count(), 0);
    assert!(!ui.get_has_item());
    toggle_attention(&ui);
    assert_eq!(ui.get_items().row_count(), 1);
    assert!(ui.get_has_item());
    assert_eq!(ui.get_selected_item(), 0);
}

#[test]
fn report_selection_keeps_explicit_item_on_current_task_reselection() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let (c, inputs) = mixed_check_after_empty_active_layout(root.path());
    let ui = c.window();
    ui.invoke_filter_results(0);
    click(&ui, inputs[2].to_str().unwrap());
    click(&ui, "Technical details");
    assert_eq!(ui.get_selected_item(), 2);
    assert!(ui.get_details_expanded());
    assert_sidebar(&ui);
    choose_result(&c, 0);
    pump(&c, || !ui.get_report_loading());
    assert_eq!(
        ui.get_selected_item(),
        2,
        "Choosing the current task does not replace an explicit item with its default"
    );
    assert!(
        ui.get_details_expanded(),
        "Same-context selection preserves expanded details"
    );
    assert_eq!(ui.get_item_source_path().full, inputs[2].to_str().unwrap());
    click(&ui, "New Request");
    assert!(ui.get_reviewing());
    assert_sidebar(&ui);
    choose_result(&c, 0);
    pump(&c, || !ui.get_report_loading());
    assert_eq!(
        ui.get_selected_item(),
        2,
        "Returning from an empty draft preserves that report's explicit item"
    );
    assert!(ui.get_details_expanded());
}

#[test]
fn report_selection_respects_filter_on_healthy_completion() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let gate = root.path().join("release-healthy-report");
    let cli = script(
        root.path(),
        &format!(
            "cat >/dev/null\nwhile [ ! -f {} ]; do :; done\n{}exit 0\n",
            quote(gate.to_str().unwrap()),
            print(&fixture("created"))
        ),
    );
    let c = controller(root.path(), cli);
    c.submit(vec![root.path().join("source.eml")]).unwrap();
    let ui = c.window();
    mock_elapsed_time(Duration::from_millis(5));
    toggle_attention(&ui);
    assert!(ui.get_filter() == 4);
    assert!(!ui.get_has_item());
    let selection = ui.get_selected_item();
    fs::write(gate, b"release").unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(ui.get_run_status(), "Done");
    assert!(
        ui.get_filter() == 4,
        "Completion does not reset the explicit filter"
    );
    assert_eq!(ui.get_items().row_count(), 0);
    assert_eq!(
        ui.get_selected_item(),
        selection,
        "Completion cannot select a healthy item excluded by the current filter"
    );
    assert!(!ui.get_has_item());
    assert_eq!(ui.get_selected_visible_row(), -1);
    toggle_attention(&ui);
    assert_eq!(ui.get_items().row_count(), 1);
    assert!(
        ui.get_has_item(),
        "Removing the filter restores the real healthy result"
    );
}

#[test]
fn s6_table_sorts_naturally_preserves_native_selection_and_exports_original_order() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let mail = root.path().join("mail");
    let sources = [
        mail.join("a/n1.eml"),
        mail.join("a/n10.eml"),
        mail.join("a/n2.eml"),
        mail.join("b/n1.eml"),
    ];
    let stream = created_stream(&sources);
    let c = controller(
        root.path(),
        script(
            root.path(),
            &("cat >/dev/null\n".to_owned() + &print(&stream)),
        ),
    );
    c.submit(vec![mail]).unwrap();
    pump(&c, || !c.is_busy());
    let ui = c.window();
    let order = || {
        ui.get_items()
            .iter()
            .map(|row| row.original_index)
            .collect::<Vec<_>>()
    };
    assert_eq!(order(), [0, 2, 1, 3]);
    assert_eq!(ui.get_items().row_data(0).unwrap().folder, "a");
    ui.invoke_select_item(2);
    ui.invoke_sort_results(0);
    assert_eq!(order(), [0, 3, 2, 1]);
    ui.invoke_sort_results(0);
    assert_eq!(order(), [1, 2, 3, 0]);
    assert_eq!(c.selected_path(false), Some(sources[2].clone()));
    ui.invoke_type_select("n10".into());
    assert_eq!(c.selected_path(false), Some(sources[1].clone()));
    let path = root.path().join("sorted.jsonl");
    c.start_export(path.clone());
    pump(&c, || !c.is_busy());
    assert_eq!(fs::read(path).unwrap(), stream.as_bytes());
}

#[test]
fn s7_attention_chip_is_default_and_filter_sort_and_selection_survive_a_draft_visit() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let (c, _) = mixed_check_after_empty_active_layout(root.path());
    let ui = c.window();
    assert_eq!(ui.get_filter(), 4);
    assert_eq!(ui.get_items().row_count(), 1);
    assert_eq!(ui.get_selected_item(), 1);
    click(&ui, "All 3");
    assert_eq!(ui.get_filter(), 0);
    assert_eq!(ui.get_items().row_count(), 3);
    click(&ui, "Need attention 1");
    assert_eq!(ui.get_filter(), 4);
    ui.invoke_sort_results(0);
    ui.invoke_sort_results(0);
    let descending = ui.get_sort_descending();
    ui.invoke_new_request();
    select_run(&ui, 0);
    pump(&c, || !ui.get_report_loading());
    assert_eq!(ui.get_filter(), 4);
    assert_eq!(ui.get_selected_item(), 1);
    assert_eq!(ui.get_sort_descending(), descending);
}

#[test]
fn sidebar_keyboard_reveals_first_and_last_runs_and_control_tab_cycles_panes() {
    use i_slint_core::window::WindowInner;
    use slint::platform::Key;
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "created"));
    for n in 0..20 {
        c.submit(vec![root.path().join(format!("message-{n}.eml"))])
            .unwrap();
        pump(&c, || !c.is_busy());
    }
    let ui = c.window();
    ui.window().set_size(slint::LogicalSize::new(1080.0, 700.0));
    mock_elapsed_time(Duration::from_millis(5));
    click_role(&ui, &result_caption(&ui, 19), AccessibleRole::ListItem);
    let viewport = ElementHandle::find_by_accessible_label(&ui, "Session runs")
        .next()
        .unwrap();
    let visible = viewport
        .query_descendants()
        .match_type_name("Flickable")
        .find_first()
        .unwrap();
    let inside = |index| {
        let row = ElementHandle::find_by_accessible_label(&ui, &result_caption(&ui, index))
            .find(|row| row.accessible_role() == Some(AccessibleRole::ListItem))
            .unwrap();
        assert!(row.absolute_position().y >= visible.absolute_position().y - 0.1);
        assert!(
            row.absolute_position().y + row.size().height
                <= visible.absolute_position().y + visible.size().height + 0.1
        );
    };
    inside(19);
    press_key(&ui, Key::Home);
    pump(&c, || !ui.get_report_loading());
    assert_eq!(ui.get_run_label(), "Run 1");
    inside(0);
    press_key(&ui, Key::End);
    pump(&c, || !ui.get_report_loading());
    assert_eq!(ui.get_run_label(), "Run 20");
    inside(19);
    let focus_x = || {
        let item = WindowInner::from_pub(ui.window())
            .focus_item
            .borrow()
            .upgrade()
            .unwrap();
        item.map_to_window(item.geometry().origin).x
    };
    let control = if cfg!(target_os = "macos") {
        Key::Meta
    } else {
        Key::Control
    };
    chord(&ui, control, Key::Tab);
    assert!(
        focus_x() >= 230.0,
        "Control-Tab must focus the main workspace"
    );
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed {
            text: Key::Shift.into(),
        });
    chord(&ui, control, Key::Tab);
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyReleased {
            text: Key::Shift.into(),
        });
    assert!(
        focus_x() < 230.0,
        "Control-Shift-Tab must return to the sidebar"
    );
}

#[test]
fn stale_waiting_removal_token_cannot_cancel_another_queued_run() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), controlled(root.path(), false));
    c.submit(vec![root.path().join("first.eml")]).unwrap();
    wait_started(&c);
    c.submit(vec![root.path().join("second.eml")]).unwrap();
    let ui = c.window();
    let second = ui.get_runs().row_data(1).unwrap().token;
    c.stop();
    pump(&c, || {
        ui.get_runs().row_data(1).is_some_and(|run| !run.waiting) && ui.get_work_total() == 1
    });
    c.submit(vec![root.path().join("third.eml")]).unwrap();
    let third = ui.get_runs().row_data(2).unwrap().token;
    ui.invoke_close_session(second);
    assert_eq!(ui.get_waiting_count() as usize, 1);
    assert_eq!(
        ui.get_runs().iter().find(|run| run.waiting).unwrap().token,
        third
    );
    ui.invoke_close_session(third);
    assert_eq!(ui.get_waiting_count() as usize, 0);
    c.force_stop();
    pump(&c, || !c.is_busy());
}

#[test]
fn table_keyboard_typeahead_and_copy_use_the_selected_native_path() {
    use slint::platform::Key;
    use std::cell::RefCell;
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let sources = [
        root.path().join("n1.eml"),
        root.path().join("n2.eml"),
        root.path().join("n10.eml"),
    ];
    let stream = created_stream(&sources);
    let c = controller(
        root.path(),
        script(
            root.path(),
            &("cat >/dev/null\n".to_owned() + &print(&stream)),
        ),
    );
    c.submit(sources.to_vec()).unwrap();
    pump(&c, || !c.is_busy());
    let ui = c.window();
    let copied = Rc::new(RefCell::new(None));
    let result = copied.clone();
    let weak = Rc::downgrade(&c);
    ui.on_copy_source_path(move || {
        if let Some(c) = weak.upgrade() {
            *result.borrow_mut() = c.selected_path(false);
        }
    });
    click_role(&ui, sources[0].to_str().unwrap(), AccessibleRole::ListItem);
    press_key(&ui, "n");
    press_key(&ui, "2");
    assert_eq!(c.selected_path(false), Some(sources[1].clone()));
    chord(&ui, Key::Control, "c");
    assert_eq!(*copied.borrow(), Some(sources[1].clone()));
    press_key(&ui, Key::End);
    assert_eq!(c.selected_path(false), Some(sources[2].clone()));
}

#[test]
fn s7_default_attention_selection_uses_the_first_visible_sorted_row() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let sources = [root.path().join("n10.eml"), root.path().join("n2.eml")];
    let base: Vec<serde_json::Value> = fixture("prepublication-failure")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let mut start = base[0].clone();
    start["total"] = 2.into();
    let mut stream = format!("{start}\n");
    for (index, source) in sources.iter().enumerate() {
        let mut item = base[1].clone();
        item["index"] = index.into();
        item["error"]["code"] = "PROTECTED".into();
        item["error"]["detail"] = "Signed message cannot be safely reduced".into();
        for (field, path) in [
            ("source", source.clone()),
            ("destination", source.with_extension("no-attachments.eml")),
        ] {
            item[field] = serde_json::json!({"display":path.to_str().unwrap(),"native":eml_protocol::NativePathValue::new(&path)});
        }
        stream.push_str(&format!("{item}\n"));
    }
    let mut summary = base[2].clone();
    summary["counts"]["failed"] = 2.into();
    stream.push_str(&format!("{summary}\n"));
    let c = controller(
        root.path(),
        script(
            root.path(),
            &("cat >/dev/null\n".to_owned() + &print(&stream) + "exit 1\n"),
        ),
    );
    c.submit(sources.to_vec()).unwrap();
    pump(&c, || !c.is_busy());
    let ui = c.window();
    assert_eq!(ui.get_items().row_data(0).unwrap().original_index, 1);
    assert_eq!(ui.get_selected_item(), 1);
    assert_eq!(ui.get_selected_visible_row(), 0);
}
