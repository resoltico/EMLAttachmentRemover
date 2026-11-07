//! Behavioral contracts over the production controller and supervised stub CLI.
use super::*;
use eml_gui::{MainWindow, Palette};
use i_slint_backend_testing::{ElementHandle, ElementRoot};
use serde_json::{Value, json};
use slint::Model;

fn records(name: &str) -> Vec<Value> {
    fixture(name)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
fn stream(root: &Path, cases: &[(&str, &str)], dry: bool) -> String {
    let mut start = records(if dry {
        "planned-create-unix"
    } else {
        "created"
    })[0]
        .clone();
    start["total"] = cases.len().into();
    let mut output = format!("{start}\n");
    let mut counts = [0; 4];
    let mut interrupted = false;
    for (index, (name, scenario)) in cases.iter().enumerate() {
        let mut item = records(scenario)[1].clone();
        if *scenario == "preview-attachment" {
            unreachable!();
        }
        item["index"] = index.into();
        for (field, path) in [
            ("source", root.join(name)),
            (
                "destination",
                root.join(name).with_extension("no-attachments.eml"),
            ),
        ] {
            item[field] = json!({"display":path.to_str().unwrap(),"native":eml_protocol::NativePathValue::new(&path)});
        }
        let status = item["status"].as_str().unwrap();
        counts[match status {
            "created" => 0,
            "up_to_date" => 1,
            "planned" => 2,
            "failed" => 3,
            _ => panic!("bad fixture"),
        }] += 1;
        interrupted |= item["error"]["code"] == "INTERRUPTED";
        output += &format!("{item}\n");
    }
    let summary = json!({"type":"summary","counts":{"created":counts[0],"up_to_date":counts[1],"planned":counts[2],"failed":counts[3],"not_processed":0},"interrupted":interrupted,"collection_complete":true,"run_error":null,"exit_code":if interrupted {130}else if counts[3]>0 {1}else {0}});
    output + &format!("{summary}\n")
}
fn check_stream(root: &Path, names: &[&str]) -> String {
    let mut values: Vec<Value> = stream(
        root,
        &names
            .iter()
            .map(|n| (*n, "planned-create-unix"))
            .collect::<Vec<_>>(),
        true,
    )
    .lines()
    .map(|l| serde_json::from_str(l).unwrap())
    .collect();
    for item in values.iter_mut().filter(|v| v["type"] == "item") {
        item["removed"] = records("created-attachment")[1]["removed"].clone();
        item["source_bytes"] = 1028.into();
        item["copy_bytes"] = 513.into();
    }
    values.iter().map(|v| format!("{v}\n")).collect()
}
fn emit(stream: &str) -> String {
    if stream.is_empty() {
        return "exit 0\n".into();
    }
    let summary: Value = serde_json::from_str(stream.lines().last().unwrap()).unwrap();
    print(stream) + &format!("exit {}\n", summary["exit_code"].as_i64().unwrap())
}
fn dual(root: &Path, check: &str, create: &str) -> PathBuf {
    script(
        root,
        &format!(
            "request=$(cat)\nprintf '%s\\n' \"$request\" >> requests.jsonl\ncase \"$request\" in *'\"dry_run\":true'*) {};; *) {};; esac\n",
            emit(check),
            emit(create)
        ),
    )
}
fn done(root: &Path, names: &[&str]) -> Rc<DesktopController> {
    let check = check_stream(root, names);
    let create = stream(
        root,
        &names
            .iter()
            .map(|n| (*n, "created-attachment"))
            .collect::<Vec<_>>(),
        false,
    );
    let c = controller(root, dual(root, &check, &create));
    c.stage(vec![root.join("inputs")]).unwrap();
    pump(&c, || !c.is_busy());
    c.start_selection();
    pump(&c, || !c.is_busy());
    c
}
fn gate(root: &Path, ignore: bool) -> Rc<DesktopController> {
    let check = check_stream(root, &["one.eml", "two.eml", "three.eml"]);
    let create = stream(
        root,
        &[
            ("one.eml", "created-attachment"),
            ("two.eml", "created"),
            ("three.eml", "prepublication-failure"),
        ],
        false,
    );
    let (start, rest) = create.split_once('\n').unwrap();
    let trap = if ignore {
        "trap '' INT\n".into()
    } else {
        format!(
            "trap {} INT\n",
            quote(
                &(print(&stream(
                    root,
                    &[("one.eml", "started-preedge-cancellation")],
                    false
                )) + "exit 130\n")
            )
        )
    };
    // Cooperative tests use their own matching-total fixture; this gate is used
    // for create ownership and kill paths, not a fabricated successful interrupt.
    let body = format!(
        "{trap}{}while [ ! -f release ]; do :; done\n{}",
        print(&(start.to_owned() + "\n")),
        emit(rest)
    );
    let cli = script(
        root,
        &format!(
            "request=$(cat)\nprintf '%s\\n' \"$request\" >> requests.jsonl\ncase \"$request\" in *'\"dry_run\":true'*) {};; *) {body};; esac",
            emit(&check)
        ),
    );
    let c = controller(root, cli);
    c.stage(vec![root.join("inputs")]).unwrap();
    pump(&c, || !c.is_busy());
    c.start_selection();
    pump(&c, || {
        c.window()
            .get_view()
            .progress_text
            .starts_with("0 of 3 done")
    });
    c
}
fn requests(root: &Path) -> Vec<Value> {
    fs::read_to_string(root.join("requests.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}
fn labels(ui: &MainWindow) -> Vec<String> {
    ui.root_element()
        .query_descendants()
        .find_all()
        .iter()
        .filter_map(|e| e.accessible_label().map(|s| s.to_string()))
        .collect()
}
fn label(ui: &MainWindow, text: &str) -> ElementHandle {
    ElementHandle::find_by_accessible_label(ui, text)
        .next()
        .unwrap_or_else(|| panic!("Missing element: {text}; {:?}", labels(ui)))
}
fn assert_words(ui: &MainWindow) {
    let forbidden = [
        "request",
        "draft",
        "session",
        "queue",
        "check only",
        "check against",
        "review",
        "close result",
        "technical details",
        "engine",
        "phase",
        "prepared",
        "force stop",
        "settings unavailable",
    ];
    for text in labels(ui) {
        let lower = text.to_lowercase();
        assert!(
            !forbidden.iter().any(|word| lower.contains(word)),
            "Forbidden UI text: {text}"
        );
    }
}
fn select_first(c: &DesktopController) {
    let ui = c.window();
    let key = ui.get_rows().row_data(0).unwrap().key;
    ui.invoke_select_email(key);
    mock_elapsed_time(Duration::from_millis(1));
}

#[test]
fn s2_changes_kill_a_read_only_check_and_collapse_pending_work() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let check = check_stream(root.path(), &["latest.eml"]);
    let start = records("planned-create-unix")[0].to_string() + "\n";
    let cli = script(
        root.path(),
        &format!(
            "trap '' INT\nrequest=$(cat)\nprintf '%s\\n' \"$request\" >> requests.jsonl\nif [ -f started ]; then {}else touch started; {}while :; do :; done; fi",
            print(&check),
            print(&start)
        ),
    );
    let c = controller(root.path(), cli);
    c.stage(vec![root.path().join("old")]).unwrap();
    pump(&c, || c.window().get_view().title == "Checking emails…");
    c.stage(vec![root.path().join("second")]).unwrap();
    c.stage(vec![root.path().join("third")]).unwrap();
    assert_eq!(requests(root.path()).len(), 1);
    mock_elapsed_time(Duration::from_millis(1999));
    c.tick();
    assert!(c.is_busy());
    mock_elapsed_time(Duration::from_millis(1));
    c.tick();
    pump(&c, || !c.is_busy());
    assert_eq!(requests(root.path()).len(), 2);
    assert_eq!(c.window().get_rows().row_count(), 1);
    assert_eq!(
        c.window().get_rows().row_data(0).unwrap().name,
        "latest.eml"
    );
    assert!(requests(root.path()).iter().all(|r| r["dry_run"] == true));
}
#[test]
fn s3_remove_during_check_uses_the_identical_native_snapshot() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("copies");
    fs::create_dir(&destination).unwrap();
    let create = stream(root.path(), &[("one.eml", "created-attachment")], false);
    let start = records("planned-create-unix")[0].to_string() + "\n";
    let cli = script(
        root.path(),
        &format!(
            "trap '' INT\nrequest=$(cat)\nprintf '%s\\n' \"$request\" >> requests.jsonl\ncase \"$request\" in *'\"dry_run\":true'*) {}while :; do :; done;; *) {};; esac",
            print(&start),
            emit(&create)
        ),
    );
    let mut preferences = Preferences::load(root.path().join("preferences.json"));
    preferences
        .set_destination(Some(destination.clone()))
        .unwrap();
    let c = controller(root.path(), cli);
    let inputs = vec![root.path().join("one.eml")];
    c.stage(inputs.clone()).unwrap();
    pump(&c, || c.window().get_view().title == "Checking emails…");
    c.start_selection();
    mock_elapsed_time(Duration::from_secs(2));
    c.tick();
    pump(&c, || !c.is_busy());
    let seen = requests(root.path());
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0]["dry_run"], true);
    assert_eq!(seen[1]["dry_run"], false);
    let expected =
        eml_app::RequestSnapshot::new(inputs, Some(destination), eml_core::RunMode::Create)
            .unwrap();
    assert_eq!(
        seen[1],
        serde_json::from_slice::<Value>(expected.encoded()).unwrap()
    );
    assert_eq!(c.window().get_view().title, "1 copy saved");
}
#[test]
fn s4_s15_adds_are_refused_during_create_and_documents_start_a_new_batch_afterward() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = gate(root.path(), true);
    let ui = c.window();
    let before = ui.get_rows().row_count();
    c.open_documents(vec![root.path().join("refused.eml")]);
    c.tick();
    assert_eq!(ui.get_rows().row_count(), before);
    assert!(ui.get_view().notice.starts_with("Wait for this batch"));
    assert_eq!(requests(root.path()).len(), 2);
    fs::write(root.path().join("release"), b"go").unwrap();
    pump(&c, || !c.is_busy());
    select_first(&c);
    ui.invoke_toggle_problems();
    assert!(ui.get_view().filtered);
    c.open_documents(vec![root.path().join("replacement.eml")]);
    assert!(!ui.get_detail().visible);
    assert!(!ui.get_view().filtered);
    assert_eq!(ui.get_rows().row_count(), 0);
    pump(&c, || !c.is_busy());
}
#[test]
fn s5_all_receipt_statuses_and_natural_stream_order_use_native_selection() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let cases = [
        ("mail10.eml", "created-attachment"),
        ("mail2.eml", "up-to-date-attachment"),
        ("mail1.eml", "created"),
        ("mail3.eml", "published-with-error"),
        ("mail4.eml", "existing-sync-failure"),
        ("mail5.eml", "unknown-publication"),
        ("mail6.eml", "prepublication-failure"),
    ];
    let c = controller(
        root.path(),
        dual(
            root.path(),
            &check_stream(root.path(), &["mail10.eml", "mail2.eml", "mail1.eml"]),
            &stream(root.path(), &cases, false),
        ),
    );
    c.stage(vec![root.path().join("input")]).unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_rows().row_data(0).unwrap().name, "mail1.eml");
    assert_eq!(c.window().get_rows().row_data(1).unwrap().name, "mail2.eml");
    assert_eq!(
        c.window().get_rows().row_data(2).unwrap().name,
        "mail10.eml"
    );
    c.start_selection();
    pump(&c, || !c.is_busy());
    let actual: Vec<_> = c
        .window()
        .get_rows()
        .iter()
        .map(|r| r.status.to_string())
        .collect();
    assert_eq!(
        actual,
        vec![
            "Copied as is",
            "Copy already exists",
            "Saved, needs checking",
            "Saved, needs checking",
            "Result unknown",
            "Not a valid email",
            "Copy saved"
        ]
    );
}
#[test]
fn s6_s7_headers_filter_and_selection_come_from_authoritative_rows() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = gate(root.path(), true);
    let ui = c.window();
    assert_eq!(ui.get_view().title, "Removing attachments…");
    assert_eq!(
        ui.get_view().summary,
        "0 of 3 done · your originals are not changed"
    );
    assert!(ui.get_view().problems.is_empty());
    assert!(!ui.get_view().can_filter);
    fs::write(root.path().join("release"), b"go").unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(ui.get_view().title, "2 copies saved");
    assert!(ui.get_view().summary.contains("515 B removed"));
    assert!(ui.get_view().summary.contains("1 copied as is"));
    assert_eq!(ui.get_view().problems, "1 can’t be copied");
    select_first(&c);
    ui.invoke_toggle_problems();
    assert_eq!(ui.get_rows().row_count(), 1);
    assert!(!ui.get_detail().visible);
    select_first(&c);
    ui.invoke_toggle_problems();
    assert!(ui.get_detail().visible);
}
#[test]
fn s8_stop_continue_reuses_exact_request_and_existing_copy_wording() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let stopped = stream(
        root.path(),
        &[("one.eml", "started-preedge-cancellation")],
        false,
    );
    let (start, rest) = stopped.split_once('\n').unwrap();
    let body = format!(
        "if [ -f continued ]; then {}else touch continued; trap {} INT; {}while :; do :; done; fi",
        print(&stream(root.path(), &[("one.eml", "up-to-date")], false)),
        quote(&(print(rest) + "exit 130")),
        print(&(start.to_owned() + "\n"))
    );
    let cli = script(
        root.path(),
        &format!(
            "request=$(cat)\nprintf '%s\\n' \"$request\" >> requests.jsonl\ncase \"$request\" in *'\"dry_run\":true'*) {};; *) {body};; esac",
            print(&check_stream(root.path(), &["one.eml"]))
        ),
    );
    let c = controller(root.path(), cli);
    c.stage(vec![root.path().join("one.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    c.start_selection();
    pump(&c, || {
        c.window().get_view().progress_text.starts_with("0 of 1")
    });
    c.stop();
    assert!(!c.window().get_view().can_stop);
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_view().state, 4);
    assert_eq!(c.window().get_view().primary, "Continue");
    assert_words(&c.window());
    c.start_selection();
    pump(&c, || !c.is_busy());
    let seen = requests(root.path());
    assert_eq!(seen[1], seen[2]);
    assert_eq!(
        c.window().get_rows().row_data(0).unwrap().status,
        "Copy already exists"
    );
}
#[test]
fn s9_quit_confirmation_cancel_and_delayed_force_keep_process_ownership() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = gate(root.path(), true);
    shortcut(&c.window(), ".");
    assert!(!c.window().get_view().can_stop);
    c.stop();
    mock_elapsed_time(Duration::from_secs(10));
    assert!(!c.request_quit());
    assert!(c.window().get_view().quit);
    c.cancel_quit();
    assert!(!c.window().get_view().quit);
    assert!(c.is_busy());
    c.request_quit();
    c.stop_and_quit();
    mock_elapsed_time(Duration::from_millis(4999));
    c.tick();
    assert!(!c.window().get_view().quit_now);
    mock_elapsed_time(Duration::from_millis(1));
    c.tick();
    assert!(c.window().get_view().quit_now);
    c.quit_now();
    pump(&c, || c.should_exit());
    assert!(!c.is_busy());
}
#[test]
fn s9_check_only_quit_has_no_dialog_and_reaps_before_exit() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let start = records("planned-create-unix")[0].to_string() + "\n";
    let cli = script(
        root.path(),
        &format!(
            "trap '' INT\ncat >/dev/null\n{}while :; do :; done",
            print(&start)
        ),
    );
    let c = controller(root.path(), cli);
    c.stage(vec![root.path().join("a.eml")]).unwrap();
    pump(&c, || c.window().get_view().title == "Checking emails…");
    assert!(!c.request_quit());
    assert!(!c.window().get_view().quit);
    pump(&c, || c.should_exit());
    assert!(!c.is_busy());
}
#[test]
fn s10_details_use_native_paths_and_failed_rows_have_no_show_copy() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let mut values: Vec<Value> =
        stream(root.path(), &[("one.eml", "prepublication-failure")], false)
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
    values[1]["source"]["display"] = "/spoof/not-authority.eml".into();
    values[1]["error"]["code"] = "MALFORMED".into();
    let create = values.iter().map(|v| format!("{v}\n")).collect::<String>();
    let c = controller(
        root.path(),
        dual(
            root.path(),
            &check_stream(root.path(), &["one.eml"]),
            &create,
        ),
    );
    c.stage(vec![root.path().join("one.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    c.start_selection();
    pump(&c, || !c.is_busy());
    select_first(&c);
    let ui = c.window();
    assert!(ui.get_detail().visible);
    assert!(!ui.get_detail().can_show_copy);
    assert!(
        ElementHandle::find_by_accessible_label(&ui, "Show Copy")
            .next()
            .is_none()
    );
    let text = c.clipboard_text().unwrap();
    assert!(text.contains(root.path().join("one.eml").to_str().unwrap()));
    assert!(text.contains("Code: MALFORMED"));
    assert!(text.contains("Phase: Prepare"));
    assert!(!text.contains("not-authority"));
    ui.invoke_clear_selected();
    assert!(!ui.get_detail().visible);
}
#[test]
fn s11_removed_concepts_are_absent_in_ready_removing_done_and_quit() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = gate(root.path(), true);
    assert_words(&c.window());
    c.request_quit();
    assert_words(&c.window());
    c.cancel_quit();
    fs::write(root.path().join("release"), b"go").unwrap();
    pump(&c, || !c.is_busy());
    select_first(&c);
    assert_words(&c.window());
    c.start_over();
    assert_words(&c.window());
    c.stage(vec![root.path().join("again")]).unwrap();
    pump(&c, || !c.is_busy());
    assert_words(&c.window());
}
#[test]
fn s12_invalid_preferences_are_silently_replaced_on_next_save() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("preferences.json");
    fs::write(&path, b"invalid").unwrap();
    let c = controller(root.path(), completed(root.path(), "planned-create-unix"));
    assert_eq!(c.window().get_view().destination, "Next to originals");
    assert!(c.window().get_view().banner.is_empty());
    c.stage(vec![root.path().join("inputs")]).unwrap();
    pump(&c, || !c.is_busy());
    c.set_destination(Some(root.path().to_owned()));
    assert_ne!(fs::read(&path).unwrap(), b"invalid");
    let settings: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(settings["last_destination"], root.path().to_str().unwrap());
}
fn inside(element: &ElementHandle, width: f32, height: f32) {
    let p = element.absolute_position();
    let s = element.size();
    assert!(
        p.x >= -0.1
            && p.y >= -0.1
            && p.x + s.width <= width + 0.1
            && p.y + s.height <= height + 0.1,
        "Outside {width}x{height}: {:?} {:?}, {:?}",
        element.accessible_label(),
        p,
        s
    );
}
#[test]
fn s13_geometry_retains_two_rows_and_visible_actions_at_minimum_and_fullscreen() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let names = ["one.eml", "two.eml", "three.eml", "four.eml", "five.eml"];
    let mut values: Vec<Value> = stream(
        root.path(),
        &names
            .iter()
            .map(|name| (*name, "created-attachment"))
            .collect::<Vec<_>>(),
        false,
    )
    .lines()
    .map(|l| serde_json::from_str(l).unwrap())
    .collect();
    let part = values[1]["removed"][0].clone();
    values[1]["removed"] = (0..40)
        .map(|i| {
            let mut p = part.clone();
            p["section"] = format!("2.{i}").into();
            p["bytes"] = 1.into();
            p
        })
        .collect::<Vec<_>>()
        .into();
    let create = values.iter().map(|v| format!("{v}\n")).collect::<String>();
    let c = controller(
        root.path(),
        dual(root.path(), &check_stream(root.path(), &names), &create),
    );
    c.stage(vec![root.path().join("inputs")]).unwrap();
    pump(&c, || !c.is_busy());
    c.start_selection();
    pump(&c, || !c.is_busy());
    let ui = c.window();
    ui.invoke_select_email(
        ui.get_rows()
            .iter()
            .find(|row| row.name == "one.eml")
            .unwrap()
            .key,
    );
    assert_eq!(ui.get_detail().parts.row_count(), 40);
    for (width, height) in [(760.0, 520.0), (2560.0, 1440.0)] {
        for font in [13.0, 18.0] {
            ui.set_ui_size(font);
            ui.window().set_size(slint::LogicalSize::new(width, height));
            mock_elapsed_time(Duration::from_millis(1));
            for text in [
                "Start Over",
                eml_gui::platform::file_manager_label(),
                "Show Original",
                "Show Copy",
                "Copy Details",
            ] {
                inside(&label(&ui, text), width, height);
            }
            let list = ElementHandle::find_by_accessible_label(&ui, "Emails")
                .find(|e| {
                    e.accessible_role() == Some(i_slint_backend_testing::AccessibleRole::List)
                })
                .unwrap();
            assert!(
                list.size().height >= 2.0 * (44.0_f32.max(font * 2.0 + 12.0)),
                "two visible rows: {:?}",
                list.size()
            );
            assert!(list.size().width <= 980.1);
            if width >= 900.0 {
                let column = label(&ui, "Size");
                let cell = label(&ui, "1.0 kB → 513 B");
                assert!(
                    (column.absolute_position().x - cell.absolute_position().x).abs() < 1.1,
                    "Size header/cell alignment: {:?}, {:?}",
                    column.absolute_position(),
                    cell.absolute_position()
                );
            }

            assert!(
                (list.absolute_position().x + list.size().width / 2.0 - width / 2.0).abs() < 2.1
            );
        }
    }
}
#[test]
fn s16_export_is_exact_settled_bytes_and_stale_panels_are_refused() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let check = check_stream(root.path(), &["one.eml"]);
    let create = stream(root.path(), &[("one.eml", "created-attachment")], false);
    let c = controller(root.path(), dual(root.path(), &check, &create));
    c.stage(vec![root.path().join("one.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    let target = root.path().join("report.jsonl");
    c.start_export(target.clone());
    pump(&c, || !c.is_busy());
    assert_eq!(fs::read(&target).unwrap(), check.as_bytes());
    let old = c.capture_export().unwrap();
    c.start_over();
    c.start_captured_export(old, root.path().join("stale.jsonl"));
    assert!(!root.path().join("stale.jsonl").exists());
    c.stage(vec![root.path().join("one.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    c.start_selection();
    pump(&c, || !c.is_busy());
    c.start_export(target.clone());
    pump(&c, || !c.is_busy());
    assert_eq!(fs::read(target).unwrap(), create.as_bytes());
}
#[test]
fn s17_keyboard_navigation_and_shared_action_predicates() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = done(root.path(), &["one.eml", "two.eml", "three.eml"]);
    let ui = c.window();
    ElementHandle::find_by_accessible_label(&ui, "one.eml, Copy saved")
        .next()
        .unwrap()
        .invoke_accessible_default_action();
    let first = ui.get_rows().row_data(0).unwrap().key;
    key(&ui, slint::platform::Key::DownArrow);
    assert_ne!(
        ui.get_rows().iter().find(|r| r.selected).unwrap().key,
        first
    );
    key(&ui, slint::platform::Key::End);
    assert!(ui.get_rows().row_data(2).unwrap().selected);
    key(&ui, slint::platform::Key::Home);
    assert!(ui.get_rows().row_data(0).unwrap().selected);
    key(&ui, slint::platform::Key::PageDown);
    assert!(ui.get_rows().row_data(2).unwrap().selected);
    key(&ui, slint::platform::Key::PageUp);
    assert!(ui.get_rows().row_data(0).unwrap().selected);
    key(&ui, slint::platform::Key::Escape);
    assert!(!ui.get_detail().visible);
    assert!(!ui.get_view().can_start);
    c.start_selection();
    assert_eq!(requests(root.path()).len(), 2);
}
#[test]
fn s18_blocking_batch_errors_and_worker_failure_have_plain_banners() {
    init_no_event_loop();
    for (name, word) in [
        ("no-inputs", "No .eml files"),
        ("collection-limit", "Too many files"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let cli = script(
            root.path(),
            &("cat >/dev/null\n".to_owned()
                + &print(&fixture(name).replace("\"dry_run\":false", "\"dry_run\":true"))
                + "exit 1\n"),
        );
        let c = controller(root.path(), cli);
        c.stage(vec![root.path().join("inputs")]).unwrap();
        pump(&c, || !c.is_busy());
        assert!(
            c.window().get_view().banner.contains(word),
            "{}",
            c.window().get_view().banner
        );
        assert!(!c.window().get_view().can_start);
    }
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), root.path().join("absent-cli"));
    c.stage(vec![root.path().join("inputs")]).unwrap();
    assert!(c.window().get_view().banner.contains("couldn’t start"));
}
#[test]
#[ignore = "Manual production-view captures; set GUI_CAPTURE_DIR"]
fn s20_capture_one_list_states() {
    let directory =
        std::env::var("GUI_CAPTURE_DIR").expect("Set GUI_CAPTURE_DIR for capture output");
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            mock_time: true,
            renderer_name: Some("software".into()),
            ..Default::default()
        },
    )))
    .unwrap();
    let directory = PathBuf::from(directory);
    fs::create_dir_all(&directory).unwrap();
    for (theme, scheme) in [
        ("light", slint::language::ColorScheme::Light),
        ("dark", slint::language::ColorScheme::Dark),
    ] {
        let root = tempfile::tempdir().unwrap();
        let c = gate(root.path(), true);
        let ui = c.window();
        ui.window().set_size(slint::LogicalSize::new(1080.0, 700.0));
        ui.global::<Palette>().set_color_scheme(scheme);
        capture(&ui, &directory, &format!("removing-{theme}"));
        fs::write(root.path().join("release"), b"go").unwrap();
        pump(&c, || !c.is_busy());
        select_first(&c);
        capture(&ui, &directory, &format!("done-{theme}"));
        c.start_over();
        capture(&ui, &directory, &format!("empty-{theme}"));
        c.stage(vec![root.path().join("inputs")]).unwrap();
        pump(&c, || !c.is_busy());
        select_first(&c);
        capture(&ui, &directory, &format!("ready-{theme}"));
        if theme == "light" {
            ui.window().set_size(slint::LogicalSize::new(760.0, 520.0));
            capture(&ui, &directory, "ready-light-narrow");
        }
    }
}
fn capture(ui: &MainWindow, directory: &Path, name: &str) {
    use std::io::Write;
    mock_elapsed_time(Duration::from_millis(1));
    let pixels = ui.window().take_snapshot().unwrap();
    let mut file =
        std::io::BufWriter::new(fs::File::create(directory.join(format!("{name}.ppm"))).unwrap());
    writeln!(file, "P6\n{} {}\n255", pixels.width(), pixels.height()).unwrap();
    for pixel in pixels.as_slice() {
        file.write_all(&[pixel.r, pixel.g, pixel.b]).unwrap();
    }
}

fn key(ui: &MainWindow, key: impl Into<slint::SharedString>) {
    let text = key.into();
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyReleased { text });
    mock_elapsed_time(Duration::from_millis(1));
}

#[test]
fn s5_streamed_insertion_retains_native_selection_and_the_visible_selected_row() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let names: Vec<String> = (10..30)
        .map(|n| format!("mail{n}.eml"))
        .chain(std::iter::once("mail2.eml".into()))
        .collect();
    let text = check_stream(
        root.path(),
        &names.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    let lines: Vec<_> = text.lines().collect();
    let prefix = lines[..21].join("\n") + "\n";
    let suffix = lines[21..].join("\n") + "\n";
    let cli = script(
        root.path(),
        &format!(
            "cat >/dev/null\n{}while [ ! -f insert ]; do :; done\n{}while [ ! -f settle ]; do :; done\n{}",
            print(&prefix),
            print(&(lines[21].to_owned() + "\n")),
            emit(
                &suffix
                    .lines()
                    .skip(1)
                    .map(|l| format!("{l}\n"))
                    .collect::<String>()
            )
        ),
    );
    let c = controller(root.path(), cli);
    let ui = c.window();
    ui.window().set_size(slint::LogicalSize::new(760.0, 520.0));
    c.stage(vec![root.path().join("inputs")]).unwrap();
    pump(&c, || ui.get_rows().row_count() == 20);
    ui.invoke_navigate_email(0, 1);
    mock_elapsed_time(Duration::from_millis(1));
    let key = ui.get_rows().row_data(19).unwrap().key;
    let before = label(&ui, "mail29.eml, Ready").absolute_position().y;
    fs::write(root.path().join("insert"), b"go").unwrap();
    pump(&c, || ui.get_rows().row_count() == 21);
    assert_eq!(ui.get_rows().row_data(0).unwrap().name, "mail2.eml");
    assert_eq!(ui.get_rows().iter().find(|r| r.selected).unwrap().key, key);
    mock_elapsed_time(Duration::from_millis(1));
    let after = label(&ui, "mail29.eml, Ready").absolute_position().y;
    assert!(
        (before - after).abs() < 1.1,
        "stream insertion moved selected row: {before} -> {after}"
    );
    fs::write(root.path().join("settle"), b"go").unwrap();
    pump(&c, || !c.is_busy());
}
#[test]
fn s5_missing_unprocessed_waiting_and_preview_statuses_are_distinct() {
    init_no_event_loop();
    for (scenario, expected, state) in [
        ("created", "No longer found", 5),
        ("started-preedge-cancellation", "Not processed", 4),
    ] {
        let root = tempfile::tempdir().unwrap();
        let cli = dual(
            root.path(),
            &check_stream(root.path(), &["one.eml", "missing.eml"]),
            &stream(root.path(), &[("one.eml", scenario)], false),
        );
        let c = controller(root.path(), cli);
        c.stage(vec![root.path().join("inputs")]).unwrap();
        pump(&c, || !c.is_busy());
        assert_eq!(c.window().get_view().title, "2 emails");
        assert!(c.window().get_view().summary.contains("2 attachments"));
        assert!(
            c.window()
                .get_view()
                .summary
                .contains("1.0 kB will be removed")
        );
        c.start_selection();
        pump(&c, || !c.is_busy());
        assert_eq!(c.window().get_view().state, state);
        assert_eq!(
            c.window()
                .get_rows()
                .iter()
                .find(|r| r.name == "missing.eml")
                .unwrap()
                .status,
            expected
        );
    }
    let root = tempfile::tempdir().unwrap();
    let cli = dual(
        root.path(),
        &stream(
            root.path(),
            &[
                ("none.eml", "planned-create-unix"),
                ("exists.eml", "planned-existing"),
            ],
            true,
        ),
        "",
    );
    let c = controller(root.path(), cli);
    c.stage(vec![root.path().join("inputs")]).unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(
        c.window().get_rows().row_data(0).unwrap().status,
        "Copy exists, will verify"
    );
    assert_eq!(
        c.window().get_rows().row_data(1).unwrap().status,
        "No attachments"
    );
    assert!(!c.window().get_view().can_filter);
}
#[test]
fn s18_overlap_empty_admission_and_missing_summary_disable_or_explain_as_required() {
    init_no_event_loop();
    for code in ["OUTPUT_OVERLAP", "INPUT_ADMISSION_FAILED"] {
        let root = tempfile::tempdir().unwrap();
        let mut v = records("no-inputs");
        v[0]["dry_run"] = true.into();
        v[1]["run_error"]["code"] = code.into();
        let text = v.iter().map(|v| format!("{v}\n")).collect::<String>();
        let cli = script(root.path(), &("cat >/dev/null\n".to_owned() + &emit(&text)));
        let c = controller(root.path(), cli);
        c.stage(vec![root.path().join("inputs")]).unwrap();
        pump(&c, || !c.is_busy());
        assert!(!c.window().get_view().can_start);
        assert!(!c.window().get_view().banner.contains(code));
        assert!(!c.window().get_view().banner.is_empty());
    }
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), script(root.path(), "cat >/dev/null\nexit 0\n"));
    c.stage(vec![root.path().join("inputs")]).unwrap();
    pump(&c, || !c.is_busy());
    assert!(
        c.window()
            .get_view()
            .banner
            .contains("stopped unexpectedly")
    );
}

fn shortcut(ui: &MainWindow, key_text: impl Into<slint::SharedString>) {
    let control: slint::SharedString = slint::platform::Key::Control.into();
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed {
            text: control.clone(),
        });
    key(ui, key_text);
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyReleased { text: control });
}
#[test]
fn s17_menu_shortcuts_obey_the_same_predicates_as_buttons() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let check = stream(
        root.path(),
        &[
            ("one.eml", "planned-create-unix"),
            ("bad.eml", "prepublication-failure"),
        ],
        true,
    );
    let create = stream(
        root.path(),
        &[
            ("one.eml", "created"),
            ("bad.eml", "prepublication-failure"),
        ],
        false,
    );
    let c = controller(root.path(), dual(root.path(), &check, &create));
    let ui = c.window();
    c.stage(vec![root.path().join("inputs")]).unwrap();
    pump(&c, || !c.is_busy());
    let saved = Rc::new(std::cell::Cell::new(0));
    let count = saved.clone();
    ui.on_export_report(move || count.set(count.get() + 1));
    shortcut(&ui, "s");
    assert_eq!(saved.get(), 1);
    shortcut(&ui, slint::platform::Key::Return);
    pump(&c, || !c.is_busy());
    assert_eq!(ui.get_view().state, 5);
    shortcut(&ui, "2");
    assert!(ui.get_view().filtered);
    assert_eq!(ui.get_rows().row_count(), 1);
    shortcut(&ui, slint::platform::Key::Backspace);
    assert_eq!(ui.get_view().state, 0);
    shortcut(&ui, "s");
    assert_eq!(saved.get(), 1);
    shortcut(&ui, slint::platform::Key::Return);
    assert_eq!(requests(root.path()).len(), 2);
}

#[test]
fn s7_continue_clears_an_unavailable_problems_filter_instead_of_hiding_successes() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let first = stream(
        root.path(),
        &[
            ("bad.eml", "prepublication-failure"),
            ("good.eml", "started-preedge-cancellation"),
        ],
        false,
    );
    let second = stream(
        root.path(),
        &[("bad.eml", "up-to-date"), ("good.eml", "up-to-date")],
        false,
    );
    let cli = script(
        root.path(),
        &format!(
            "request=$(cat)\ncase \"$request\" in *'\"dry_run\":true'*) {};; *) if [ -f continued ]; then {}else touch continued; {}fi;; esac",
            emit(&check_stream(root.path(), &["bad.eml", "good.eml"])),
            emit(&second),
            emit(&first)
        ),
    );
    let c = controller(root.path(), cli);
    c.stage(vec![root.path().join("inputs")]).unwrap();
    pump(&c, || !c.is_busy());
    c.start_selection();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_view().state, 4);
    c.window().invoke_toggle_problems();
    assert!(c.window().get_view().filtered);
    c.start_selection();
    assert!(!c.window().get_view().filtered);
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_rows().row_count(), 2);
    assert!(c.window().get_view().problems.is_empty());
}

#[test]
fn s1_selection_admission_bounds_unique_paths_and_keeps_first_seen_duplicates() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let check = check_stream(root.path(), &["one.eml"]);
    let c = controller(root.path(), dual(root.path(), &check, ""));
    assert!(
        c.stage(
            (0..4097)
                .map(|n| root.path().join(format!("{n}.eml")))
                .collect()
        )
        .is_err()
    );
    assert_eq!(c.window().get_view().state, 0);
    assert!(!root.path().join("requests.jsonl").exists());
    c.stage(vec![root.path().join("one.eml"); 5000]).unwrap();
    pump(&c, || !c.is_busy());
    let seen = requests(root.path());
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0]["paths"].as_array().unwrap().len(), 1);
}

#[test]
fn s18_admission_notice_is_visible_in_the_empty_window_and_expires() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), root.path().join("absent"));
    c.open_documents(
        (0..4097)
            .map(|n| root.path().join(format!("{n}.eml")))
            .collect(),
    );
    c.tick();
    let text = "Too many items at once. Add fewer files or folders.";
    assert!(labels(&c.window()).iter().any(|label| label == text));
    mock_elapsed_time(Duration::from_secs(6));
    c.tick();
    assert!(c.window().get_view().notice.is_empty());
    assert!(!labels(&c.window()).iter().any(|label| label == text));
}
