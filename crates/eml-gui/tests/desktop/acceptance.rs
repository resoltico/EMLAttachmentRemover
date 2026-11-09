//! Behavioral contracts over the production controller and supervised stub CLI.
use super::*;
use eml_gui::{BatchState, MainWindow, Palette};
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
pub(super) fn check_stream(root: &Path, names: &[&str]) -> String {
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
pub(super) fn dual(root: &Path, check: &str, create: &str) -> PathBuf {
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
pub(super) fn gate(root: &Path) -> Rc<DesktopController> {
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
    let trap = "trap '' INT\n";
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
pub(super) fn requests(root: &Path) -> Vec<Value> {
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
    let raw = fs::read_to_string(root.path().join("requests.jsonl")).unwrap();
    assert_eq!(raw.lines().nth(1).unwrap().as_bytes(), expected.encoded());
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
    let c = gate(root.path());
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
fn s5_all_receipt_statuses_and_worker_stream_order_use_native_selection() {
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
    assert_eq!(
        c.window().get_rows().row_data(0).unwrap().name,
        "mail10.eml"
    );
    assert_eq!(c.window().get_rows().row_data(1).unwrap().name, "mail2.eml");
    assert_eq!(c.window().get_rows().row_data(2).unwrap().name, "mail1.eml");
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
            "Copy saved",
            "Copy already exists",
            "Copied as is",
            "Saved, needs checking",
            "Saved, needs checking",
            "Result unknown",
            "Not a valid email"
        ]
    );
}
#[test]
fn s6_s7_headers_filter_and_selection_come_from_authoritative_rows() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = gate(root.path());
    let ui = c.window();
    assert_eq!(ui.get_view().title, "Removing attachments…");
    assert_eq!(ui.get_view().summary, "Your originals are not changed");
    assert!(ui.get_view().problems.is_empty());
    assert!(!ui.get_view().can_filter);
    assert!(
        ElementHandle::find_by_element_type_name(&ui, "Chip")
            .next()
            .is_none()
    );
    fs::write(root.path().join("release"), b"go").unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(ui.get_view().title, "2 copies saved");
    assert!(ui.get_view().summary.contains("515 B removed"));
    assert!(ui.get_view().summary.contains("1 copied as is"));
    assert_eq!(ui.get_view().problems, "1 can’t be copied");
    assert!(
        ElementHandle::find_by_element_type_name(&ui, "Chip")
            .next()
            .is_some()
    );
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
    assert_eq!(c.window().get_view().state, BatchState::Stopped);
    assert_eq!(
        c.window().get_view().summary,
        "0 copies saved · 1 not processed"
    );
    assert_eq!(c.window().get_view().primary, "Continue");
    assert_words(&c.window());
    c.start_selection();
    pump(&c, || !c.is_busy());
    let seen = requests(root.path());
    assert_eq!(seen[1], seen[2]);
    let raw = fs::read_to_string(root.path().join("requests.jsonl")).unwrap();
    assert_eq!(raw.lines().nth(1), raw.lines().nth(2));
    assert_eq!(
        c.window().get_rows().row_data(0).unwrap().status,
        "Copy already exists"
    );
}
#[test]
fn s9_quit_confirmation_cancel_and_delayed_force_keep_process_ownership() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = gate(root.path());
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
    assert_eq!(ui.get_view().title, "No copies saved");
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
    assert!(text.contains("Phase: prepare"));
    assert!(!text.contains("not-authority"));
    ui.invoke_clear_selected();
    assert!(!ui.get_detail().visible);
}
#[test]
fn s11_removed_concepts_are_absent_in_ready_removing_done_and_quit() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = gate(root.path());
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
    let destination: eml_protocol::records::NativePathRecord =
        serde_json::from_value(settings["last_destination"].clone()).unwrap();
    assert_eq!(destination.native_path().as_deref(), Some(root.path()));
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
// The contract is two complete rows in the clipping viewport, not a skin's frame height.
fn assert_two_complete_rows(ui: &MainWindow) -> ElementHandle {
    let list = ElementHandle::find_by_accessible_label(ui, "Emails")
        .find(|element| {
            element.accessible_role() == Some(i_slint_backend_testing::AccessibleRole::List)
        })
        .unwrap();
    let viewport = list
        .query_descendants()
        .match_type_name("Flickable")
        .find_first()
        .unwrap();
    let origin = viewport.absolute_position();
    let size = viewport.size();
    let mut rows: Vec<_> = list
        .query_descendants()
        .find_all()
        .into_iter()
        .filter(|element| {
            element.accessible_role() == Some(i_slint_backend_testing::AccessibleRole::ListItem)
        })
        .map(|element| (element.absolute_position(), element.size()))
        .collect();
    rows.sort_by(|a, b| a.0.y.total_cmp(&b.0.y));
    let complete: Vec<_> = rows
        .iter()
        .filter(|(position, row_size)| {
            position.x >= origin.x - 0.1
                && position.y >= origin.y - 0.1
                && position.x + row_size.width <= origin.x + size.width + 0.1
                && position.y + row_size.height <= origin.y + size.height + 0.1
        })
        .collect();
    eprintln!(
        "S13 rows: outer={:?}; viewport={origin:?} {size:?}; row bounds={rows:?}; complete={}",
        list.size(),
        complete.len()
    );
    assert!(
        complete.len() >= 2,
        "Two complete rows must fit: viewport={origin:?} {size:?}, rows={rows:?}"
    );
    let expected_height = 44.0;
    for (_, row_size) in &complete {
        assert!(
            (row_size.height - expected_height).abs() <= 0.1,
            "Declared fixed row height: {row_size:?}, expected {expected_height}"
        );
    }
    for pair in complete.windows(2) {
        assert!(
            pair[0].0.y + pair[0].1.height <= pair[1].0.y + 0.1,
            "Visible rows overlap: {pair:?}"
        );
    }
    list
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
        {
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
            let list = assert_two_complete_rows(&ui);
            assert!(list.size().width <= 980.1);
            assert!(
                label(&ui, "Email details").size().height <= height * 0.4 + 0.1,
                "Detail pane exceeds its height cap"
            );
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
fn s16_exports_exact_settled_bytes_and_keeps_an_explicit_capture_after_reset() {
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
    let pinned = root.path().join("captured-before-reset.jsonl");
    c.start_captured_export(old, pinned.clone());
    pump(&c, || !c.is_busy());
    assert_eq!(fs::read(pinned).unwrap(), check.as_bytes());
    assert_eq!(c.window().get_view().state, BatchState::Empty);
    assert!(c.window().get_view().can_reveal_report);
    assert!(
        ElementHandle::find_by_accessible_label(&c.window(), "Show")
            .next()
            .is_some()
    );
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
        let c = gate(root.path());
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
    assert_eq!(ui.get_rows().row_data(20).unwrap().name, "mail2.eml");
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
        ("created", "No longer found", BatchState::Done),
        (
            "started-preedge-cancellation",
            "Not processed",
            BatchState::Stopped,
        ),
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
                .contains("1.0 kB would be removed")
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
        "No attachments"
    );
    assert_eq!(
        c.window().get_rows().row_data(1).unwrap().status,
        "Copy exists, will verify"
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
        if code == "OUTPUT_OVERLAP" {
            v[1]["run_error"]["phase"] = "admission".into();
        }
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
    assert_eq!(ui.get_view().state, BatchState::Done);
    shortcut(&ui, "2");
    assert!(ui.get_view().filtered);
    assert_eq!(ui.get_rows().row_count(), 1);
    shortcut(&ui, slint::platform::Key::Backspace);
    assert_eq!(ui.get_view().state, BatchState::Empty);
    shortcut(&ui, "s");
    assert_eq!(saved.get(), 1);
    shortcut(&ui, slint::platform::Key::Return);
    assert_eq!(requests(root.path()).len(), 2);
}

#[test]
fn s7_continue_retains_problems_filter_until_successes_replace_prior_results() {
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
            "request=$(cat)\ncase \"$request\" in *'\"dry_run\":true'*) {};; *) if [ -f continued ]; then while [ ! -f release ]; do :; done; {}else touch continued; {}fi;; esac",
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
    assert_eq!(c.window().get_view().state, BatchState::Stopped);
    c.window().invoke_toggle_problems();
    assert!(c.window().get_view().filtered);
    c.start_selection();
    assert!(c.window().get_view().filtered);
    fs::write(root.path().join("release"), b"go").unwrap();
    pump(&c, || !c.is_busy());
    assert!(!c.window().get_view().filtered);
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
    assert_eq!(c.window().get_view().state, BatchState::Empty);
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
    let text = "Cannot add this selection: Too many input paths. Add at most 4096 files or folders at once.";
    assert!(labels(&c.window()).iter().any(|label| label == text));
    mock_elapsed_time(Duration::from_secs(6));
    c.tick();
    assert!(c.window().get_view().notice.is_empty());
    assert!(!labels(&c.window()).iter().any(|label| label == text));
}

#[test]
fn s13_native_resize_constraints_admit_the_declared_minimum_and_large_windows() {
    use i_slint_core::{item_tree::ItemTreeRc, layout::Orientation, window::WindowInner};
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let cli = dual(
        root.path(),
        &check_stream(root.path(), &["one.eml", "two.eml", "three.eml"]),
        &stream(root.path(), &[("one.eml", "created-attachment")], false),
    );
    let c = controller(root.path(), cli);
    let ui = c.window();
    for phase in 0..3 {
        if phase == 1 {
            c.stage(vec![root.path().join("inputs")]).unwrap();
            pump(&c, || !c.is_busy());
        } else if phase == 2 {
            c.start_selection();
            pump(&c, || !c.is_busy());
        }
        mock_elapsed_time(Duration::from_millis(1));
        let tree = WindowInner::from_pub(ui.window()).component();
        let component = ItemTreeRc::borrow_pin(&tree);
        for (axis, minimum) in [
            (Orientation::Horizontal, 760.0),
            (Orientation::Vertical, 520.0),
        ] {
            let info = component.as_ref().layout_info(axis);
            assert!(
                info.min <= minimum && info.max >= 4000.0,
                "phase={phase}, {axis:?}: {info:?}"
            );
        }
    }
}

#[test]
fn s6_ready_summary_counts_existing_copies_separately_from_new_removals() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let mut values: Vec<Value> = stream(
        root.path(),
        &[
            ("new.eml", "planned-create-unix"),
            ("existing.eml", "planned-existing"),
        ],
        true,
    )
    .lines()
    .map(|l| serde_json::from_str(l).unwrap())
    .collect();
    values[1]["removed"] = records("created-attachment")[1]["removed"].clone();
    values[1]["source_bytes"] = 1028.into();
    values[1]["copy_bytes"] = 513.into();
    let check = values.iter().map(|v| format!("{v}\n")).collect::<String>();
    let c = controller(root.path(), dual(root.path(), &check, ""));
    c.stage(vec![root.path().join("inputs")]).unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_view().title, "2 emails");
    assert_eq!(
        c.window().get_view().summary,
        "1 attachment · 515 B would be removed · 1 copy to verify"
    );
}

#[test]
fn s16_capture_owns_one_save_lifetime_across_batch_changes() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let check = check_stream(root.path(), &["one.eml"]);
    let replacement = check_stream(root.path(), &["replacement.eml"]);
    let cli = script(
        root.path(),
        &format!(
            "cat >/dev/null\nif [ -f checked ]; then {}else touch checked; {}fi",
            emit(&replacement),
            emit(&check)
        ),
    );
    let c = controller(root.path(), cli);
    c.stage(vec![root.path().join("one.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    let old = c.capture_export().unwrap();
    c.start_over();
    c.stage(vec![root.path().join("replacement")]).unwrap();
    pump(&c, || !c.is_busy());
    assert!(
        c.capture_export().is_none(),
        "The open panel still owns the only capture lifetime"
    );
    let target = root.path().join("captured-before-reset.jsonl");
    c.start_captured_export(old, target.clone());
    pump(&c, || !c.is_busy());
    assert_eq!(fs::read(target).unwrap(), check.as_bytes());
    let old = c.capture_export().unwrap();
    c.cancel_export(&old);
    let current = c.capture_export().unwrap();
    c.cancel_export(&old);
    c.start_captured_export(old, root.path().join("stale.jsonl"));
    assert!(!root.path().join("stale.jsonl").exists());
    assert!(
        c.capture_export().is_none(),
        "Stale callbacks cannot release the newer panel"
    );
    c.cancel_export(&current);
}
#[test]
fn s16_quit_invalidates_unsubmitted_saves_even_when_quit_is_cancelled() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = gate(root.path());
    let source = c.capture_export().unwrap();
    c.request_quit();
    c.cancel_quit();
    assert!(
        c.capture_export().is_none(),
        "The invalidated panel owns its lifetime until it settles"
    );
    let target = root.path().join("late.jsonl");
    c.start_captured_export(source, target.clone());
    assert!(!target.exists());
    fs::write(root.path().join("release"), b"go").unwrap();
    pump(&c, || !c.is_busy());
    assert!(c.capture_export().is_some());
}

#[test]
fn s5_worker_failure_does_not_claim_preview_sources_disappeared() {
    init_no_event_loop();
    for launch_failure in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let check = check_stream(root.path(), &["one.eml", "two.eml"]);
        let executable = dual(root.path(), &check, "");
        let cli = root.path().join("owned-launcher");
        fs::copy(executable, &cli).unwrap();
        let c = controller(root.path(), cli.clone());
        c.stage(vec![root.path().join("inputs")]).unwrap();
        pump(&c, || !c.is_busy());
        assert_eq!(c.window().get_rows().row_count(), 2);
        if launch_failure {
            fs::remove_file(cli).unwrap();
        }
        c.start_selection();
        pump(&c, || !c.is_busy());
        let expected = if launch_failure {
            "Couldn’t copy"
        } else {
            "Result unknown"
        };
        assert!(
            c.window()
                .get_rows()
                .iter()
                .all(|row| row.status == expected),
            "Absent create items after worker failure are not evidence of deleted sources"
        );
        if launch_failure {
            assert_eq!(c.window().get_view().problems, "2 can’t be copied");
        }
        assert!(!c.window().get_view().banner.is_empty());
        assert!(c.capture_export().is_some());
    }
}

#[test]
fn s13_banner_and_long_notice_leave_two_rows_and_detail_actions_visible() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let mut values: Vec<Value> = check_stream(root.path(), &["one.eml", "two.eml"])
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let part = values[1]["removed"][0].clone();
    values[1]["removed"] = (0..40)
        .map(|index| {
            let mut part = part.clone();
            part["section"] = format!("2.{index}").into();
            part["bytes"] = 1.into();
            part
        })
        .collect::<Vec<_>>()
        .into();
    let check = values.iter().map(|v| format!("{v}\n")).collect::<String>();
    let c = controller(root.path(), dual(root.path(), &check, ""));
    c.stage(vec![root.path().join("inputs")]).unwrap();
    pump(&c, || !c.is_busy());
    c.start_selection();
    pump(&c, || !c.is_busy());
    let ui = c.window();

    ui.window().set_size(slint::LogicalSize::new(760.0, 520.0));
    select_first(&c);
    c.message(&format!(
        "Couldn’t save the report. {}",
        "long-directory/".repeat(150)
    ));
    c.tick();
    mock_elapsed_time(Duration::from_millis(1));
    for text in [
        "Show Original",
        "Copy Details",
        "Start Over",
        eml_gui::platform::file_manager_label(),
    ] {
        inside(&label(&ui, text), 760.0, 520.0);
    }
    assert_two_complete_rows(&ui);
}

#[test]
fn t2_continue_retains_saved_rows_until_current_results_arrive() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let names = ["one.eml", "two.eml", "three.eml"];
    let check = check_stream(root.path(), &names);
    let created = stream(
        root.path(),
        &[
            ("one.eml", "created-attachment"),
            ("two.eml", "created-attachment"),
            ("three.eml", "created-attachment"),
        ],
        false,
    );
    let prefix = created
        .lines()
        .take(2)
        .map(|l| format!("{l}\n"))
        .collect::<String>();
    let interrupted = "{\"type\":\"summary\",\"counts\":{\"created\":1,\"up_to_date\":0,\"planned\":0,\"failed\":0,\"not_processed\":2},\"interrupted\":true,\"collection_complete\":true,\"run_error\":null,\"exit_code\":130}\n";
    let continued = stream(root.path(), &names.map(|n| (n, "up-to-date")), false);
    let body = format!(
        "request=$(cat)\ncase \"$request\" in *'\"dry_run\":true'*) {};; *) if [ -f continued ]; then while [ ! -f release ]; do :; done; {}else touch continued; trap {} INT; {}while :; do :; done; fi;; esac",
        emit(&check),
        emit(&continued),
        quote(&(print(interrupted) + "exit 130")),
        print(&prefix)
    );
    let c = controller(root.path(), script(root.path(), &body));
    c.stage(vec![root.path().join("inputs")]).unwrap();
    pump(&c, || !c.is_busy());
    c.start_selection();
    pump(&c, || {
        c.window().get_rows().row_data(0).unwrap().status == "Copy saved"
    });
    c.stop();
    pump(&c, || !c.is_busy());
    c.start_selection();
    assert_eq!(
        c.window().get_rows().row_data(0).unwrap().status,
        "Copy saved"
    );
    assert_eq!(c.window().get_rows().row_data(1).unwrap().status, "Waiting");
    assert_eq!(c.window().get_view().progress_text, "Starting…");
    fs::write(root.path().join("release"), "").unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(
        c.window().get_rows().row_data(0).unwrap().status,
        "Copy already exists"
    );
    c.window().invoke_start_over();
    assert_eq!(c.window().get_rows().row_count(), 0);
}

#[test]
fn t3_deep_folder_keeps_its_end_and_full_accessible_label() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let deep = root
        .path()
        .join("one/two/three/four/five/six/seven/eight/nine/ten/eleven/Inbox");
    let check = check_stream(&deep, &["one.eml"]);
    let c = controller(root.path(), dual(root.path(), &check, ""));
    c.stage(vec![deep.clone()]).unwrap();
    pump(&c, || !c.is_busy());
    c.window()
        .window()
        .set_size(slint::LogicalSize::new(760.0, 520.0));
    mock_elapsed_time(Duration::from_millis(1));
    let row = c.window().get_rows().row_data(0).unwrap();
    assert_eq!(row.folder_suffix, "Inbox");
    assert_eq!(
        format!("{}{}", row.folder_prefix, row.folder_suffix),
        row.folder.as_str()
    );
    let folder = label(&c.window(), &row.folder);
    assert!(folder.size().width > 30.0);
    let suffix = label(&c.window(), "Inbox");
    assert!(suffix.size().width >= 30.0, "final folder is clipped");
    assert!(suffix.absolute_position().x >= folder.absolute_position().x);
    assert!(
        suffix.absolute_position().x + suffix.size().width
            <= folder.absolute_position().x + folder.size().width + 0.1
    );
}

#[test]
fn t5_detail_header_columns_and_footer_share_one_left_edge() {
    init_no_event_loop();
    {
        let root = tempfile::tempdir().unwrap();
        let check = check_stream(root.path(), &["one.eml", "two.eml", "three.eml"]);
        let create = stream(root.path(), &[("one.eml", "created-attachment")], false);
        let c = controller(root.path(), dual(root.path(), &check, &create));
        let ui = c.window();

        ui.window().set_size(slint::LogicalSize::new(760.0, 520.0));
        c.stage(vec![root.path().join("inputs")]).unwrap();
        pump(&c, || !c.is_busy());
        select_first(&c);
        let title = label(&ui, "3 emails");
        let x = title.absolute_position().x;
        for e in [
            label(&ui, "one.eml"),
            label(&ui, "Email"),
            label(&ui, "Save copies"),
        ] {
            assert!(
                (e.absolute_position().x - x).abs() <= 1.0,
                "left edge {:?} vs {x}",
                e.absolute_position()
            );
        }
        let ready_y = label(&ui, "Email").absolute_position().y;
        c.start_selection();
        pump(&c, || !c.is_busy());
        mock_elapsed_time(Duration::from_millis(1));
        assert!((label(&ui, "Email").absolute_position().y - ready_y).abs() <= 1.0);
    }
}

#[test]
fn t6_detail_omits_generic_kind_and_preserves_inline_image_kind() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let mut values: Vec<Value> = check_stream(root.path(), &["one.eml"])
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let generic = values.iter().map(|v| format!("{v}\n")).collect::<String>();
    let c = controller(root.path(), dual(root.path(), &generic, ""));
    c.stage(vec![root.path().join("inputs")]).unwrap();
    pump(&c, || !c.is_busy());
    select_first(&c);
    assert_eq!(c.window().get_detail().parts.row_data(0).unwrap().kind, "");
    assert!(!labels(&c.window()).iter().any(|s| s == "Attachment"));
    drop(c);
    values[1]["removed"][0]["reason"] = "related_resource".into();
    values[1]["removed"][0]["content_type"] = "image/png".into();
    let inline = values.iter().map(|v| format!("{v}\n")).collect::<String>();
    let c = controller(root.path(), dual(root.path(), &inline, ""));
    c.stage(vec![root.path().join("inputs")]).unwrap();
    pump(&c, || !c.is_busy());
    select_first(&c);
    assert_eq!(
        c.window().get_detail().parts.row_data(0).unwrap().kind,
        "Inline image"
    );
    label(&c.window(), "Inline image");
}

#[test]
fn t8_filter_announces_visible_count_and_show_all() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let mut cases = vec![
        ("bad1.eml", "prepublication-failure"),
        ("bad2.eml", "prepublication-failure"),
    ];
    let names: Vec<_> = (0..34).map(|n| format!("mail{n}.eml")).collect();
    cases.extend(names.iter().map(|n| (n.as_str(), "planned-create-unix")));
    let check = stream(root.path(), &cases, true);
    let c = controller(root.path(), dual(root.path(), &check, ""));
    c.stage(vec![root.path().join("inputs")]).unwrap();
    pump(&c, || !c.is_busy());
    let original = c.window().get_view().summary;
    c.window().invoke_toggle_problems();
    assert_eq!(c.window().get_rows().row_count(), 2);
    assert!(c.window().get_view().summary.starts_with("Showing 2 of 36"));
    label(&c.window(), "Show all");
    c.window().invoke_toggle_problems();
    assert_eq!(c.window().get_view().summary, original);
    assert_eq!(c.window().get_rows().row_count(), 36);
    label(&c.window(), "2 can’t be copied");
}

#[test]
fn t7_failed_only_disables_button_menu_and_controller_but_checking_stays_enabled() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let failed = stream(root.path(), &[("bad.eml", "prepublication-failure")], true);
    let c = controller(root.path(), dual(root.path(), &failed, ""));
    c.stage(vec![root.path().join("inputs")]).unwrap();
    assert!(c.window().get_view().can_start);
    pump(&c, || !c.is_busy());
    assert!(!c.window().get_view().can_start);
    assert_eq!(
        label(&c.window(), "Remove Attachments").accessible_enabled(),
        Some(false)
    );
    shortcut(&c.window(), slint::platform::Key::Return);
    c.start_selection();
    assert_eq!(requests(root.path()).len(), 1);
    drop(c);
    let root = tempfile::tempdir().unwrap();
    let mixed = stream(
        root.path(),
        &[
            ("bad.eml", "prepublication-failure"),
            ("good.eml", "planned-create-unix"),
        ],
        true,
    );
    let c = controller(root.path(), dual(root.path(), &mixed, ""));
    c.stage(vec![root.path().join("inputs")]).unwrap();
    pump(&c, || !c.is_busy());
    assert!(c.window().get_view().can_start);
    assert_eq!(
        label(&c.window(), "Remove Attachments").accessible_enabled(),
        Some(true)
    );
    shortcut(&c.window(), slint::platform::Key::Return);
    pump(&c, || !c.is_busy());
    assert_eq!(requests(root.path()).len(), 2);
}

#[test]
fn u5_chips_size_and_status_share_row_centre() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let check = check_stream(root.path(), &["one.eml"]);
    let c = controller(root.path(), dual(root.path(), &check, ""));
    c.stage(vec![root.path().join("inputs")]).unwrap();
    pump(&c, || !c.is_busy());
    let ui = c.window();
    let row = label(&ui, "one.eml, Ready");
    let centre = row.absolute_position().y + row.size().height / 2.0;
    for text in ["report.pdf", "1.0 kB → 513 B", "Ready"] {
        let e = label(&ui, text);
        assert!(
            (e.absolute_position().y + e.size().height / 2.0 - centre).abs() <= 1.0,
            "{text} centre differs from row centre {centre}: {:?} {:?}",
            e.absolute_position(),
            e.size()
        );
    }
    let dot = ElementHandle::find_by_element_type_name(&ui, "ToneDot")
        .next()
        .unwrap();
    assert!((dot.absolute_position().y + dot.size().height / 2.0 - centre).abs() <= 1.0);
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/batch.slint")).unwrap();
    let row = source
        .split("for row[")
        .nth(1)
        .unwrap()
        .split("TouchArea")
        .next()
        .unwrap();
    assert!(
        !row.contains("border-width:"),
        "Rows must have no border: {row}"
    );
}

#[test]
fn u6_first_row_y_is_stable_when_problems_arrive() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let check = check_stream(root.path(), &["one.eml", "two.eml"]);
    let create = stream(
        root.path(),
        &[
            ("one.eml", "created-attachment"),
            ("two.eml", "prepublication-failure"),
        ],
        false,
    );
    let c = controller(root.path(), dual(root.path(), &check, &create));
    c.stage(vec![root.path().join("inputs")]).unwrap();
    pump(&c, || !c.is_busy());
    let before = label(&c.window(), "one.eml, Ready").absolute_position().y;
    c.start_selection();
    pump(&c, || !c.is_busy());
    let after = label(&c.window(), "one.eml, Copy saved")
        .absolute_position()
        .y;
    assert!(
        (before - after).abs() <= 1.0,
        "first row moved: {before} -> {after}"
    );
}

#[test]
fn u8_selection_does_not_resize_list() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let mut values: Vec<Value> = check_stream(root.path(), &["zero.eml", "one.eml", "five.eml"])
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    values[1]["removed"] = json!([]);
    let part = values[3]["removed"][0].clone();
    values[3]["removed"] = (0..5)
        .map(|n| {
            let mut p = part.clone();
            p["section"] = format!("2.{n}").into();
            p["bytes"] = 1.into();
            p
        })
        .collect::<Vec<_>>()
        .into();
    let check = values.iter().map(|v| format!("{v}\n")).collect::<String>();
    let c = controller(root.path(), dual(root.path(), &check, ""));
    c.stage(vec![root.path().join("inputs")]).unwrap();
    pump(&c, || !c.is_busy());
    let mut heights = Vec::new();
    for name in ["zero.eml", "one.eml", "five.eml"] {
        let row = c
            .window()
            .get_rows()
            .iter()
            .find(|r| r.name == name)
            .unwrap();
        c.window().invoke_select_email(row.key);
        mock_elapsed_time(Duration::from_millis(1));
        heights.push(label(&c.window(), "Emails").size().height);
    }
    assert!(
        heights.iter().all(|h| (h - heights[0]).abs() <= 1.0),
        "list heights: {heights:?}"
    );
    for (pane, separator) in [
        ("detail-pane", "detail-separator"),
        ("footer", "footer-separator"),
    ] {
        let find = |id| {
            ElementHandle::find_by_element_id(&c.window(), &format!("BatchPane::{id}"))
                .next()
                .unwrap()
        };
        assert!(
            (find(pane).absolute_position().y - find(separator).absolute_position().y).abs() <= 1.0
        );
    }
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/batch.slint")).unwrap();
    let detail = source
        .split("if root.detail.visible:")
        .nth(1)
        .unwrap()
        .split("accessible-role:")
        .next()
        .unwrap();
    assert!(!detail.contains("border-width:"));
}

#[test]
fn u10_check_failures_persist_until_the_create_result() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let check = stream(
        root.path(),
        &[
            ("bad.eml", "prepublication-failure"),
            ("good.eml", "planned-create-unix"),
        ],
        true,
    );
    let create = stream(
        root.path(),
        &[
            ("bad.eml", "prepublication-failure"),
            ("good.eml", "created"),
        ],
        false,
    );
    let (start, rest) = create.split_once('\n').unwrap();
    let cli = script(
        root.path(),
        &format!(
            "request=$(cat)\ncase \"$request\" in *'\"dry_run\":true'*) {};; *) {}while [ ! -f release ]; do :; done\n{};; esac",
            emit(&check),
            print(&(start.to_owned() + "\n")),
            emit(rest)
        ),
    );
    let c = controller(root.path(), cli);
    c.stage(vec![root.path().join("inputs")]).unwrap();
    pump(&c, || !c.is_busy());
    let before = c.window().get_view().problems;
    c.start_selection();
    pump(&c, || {
        c.window().get_view().progress_text.starts_with("0 of 2")
    });
    let during = c
        .window()
        .get_rows()
        .iter()
        .find(|r| r.name == "bad.eml")
        .unwrap()
        .status;
    let problems = c.window().get_view().problems;
    fs::write(root.path().join("release"), "").unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(during, "Not a valid email");
    assert_eq!(problems, before);
}

#[test]
fn u9_unnamed_inline_image_is_named_by_kind() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let mut values: Vec<Value> = check_stream(root.path(), &["inline.eml"])
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    values[1]["removed"][0]["filename"] = Value::Null;
    values[1]["removed"][0]["content_type"] = "image/png".into();
    values[1]["removed"][0]["reason"] = "related_resource".into();
    let check = values.iter().map(|v| format!("{v}\n")).collect::<String>();
    let c = controller(root.path(), dual(root.path(), &check, ""));
    c.stage(vec![root.path().join("inputs")]).unwrap();
    pump(&c, || !c.is_busy());
    select_first(&c);
    assert_eq!(
        c.window().get_rows().row_data(0).unwrap().chip1,
        "Inline image"
    );
    assert_eq!(
        c.window().get_detail().parts.row_data(0).unwrap().name,
        "Inline image"
    );
    assert!(!labels(&c.window()).iter().any(|s| s.contains("image/png")));
    assert!(c.clipboard_text().unwrap().contains("image/png"));
}

#[test]
fn u7_secondary_commands_and_soft_selected_chip() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = gate(root.path());
    let ui = c.window();
    assert_eq!(
        ElementHandle::find_by_element_type_name(&ui, "PrimaryButton").count(),
        0
    );
    fs::write(root.path().join("release"), "").unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(
        ElementHandle::find_by_element_type_name(&ui, "PrimaryButton").count(),
        1
    );
    assert_eq!(
        label(&ui, "Start Over").type_name().unwrap(),
        "ActionButton"
    );
    select_first(&c);
    assert_eq!(
        label(&ui, "Copy Details").type_name().unwrap(),
        "ActionButton"
    );
    ui.invoke_toggle_problems();
    let controls =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/controls.slint"))
            .unwrap();
    assert!(controls.contains("background: root.selected ? Theme.accent-soft : Theme.surface"));
    c.start_over();
    c.stage(vec![root.path().join("inputs")]).unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(
        label(&ui, "Start Over").type_name().unwrap(),
        "ActionButton"
    );
    assert_eq!(
        ElementHandle::find_by_element_type_name(&ui, "PrimaryButton").count(),
        1
    );
}

#[test]
fn u12_destination_popup_tracks_control_at_all_widths() {
    use i_slint_core::window::{PopupWindowLocation, WindowInner};
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let check = check_stream(root.path(), &["one.eml"]);
    let c = controller(root.path(), dual(root.path(), &check, ""));
    c.stage(vec![root.path().join("inputs")]).unwrap();
    pump(&c, || !c.is_busy());
    let ui = c.window();
    for width in [760.0, 1080.0, 1920.0] {
        ui.window().set_size(slint::LogicalSize::new(width, 700.0));
        mock_elapsed_time(Duration::from_millis(1));
        let button = label(&ui, "Next to originals ▾");
        let x = button.absolute_position().x;
        button.mock_single_click(slint::platform::PointerEventButton::Left);
        mock_elapsed_time(Duration::from_millis(1));
        let inner = WindowInner::from_pub(ui.window());
        let popups = inner.active_popups();
        let PopupWindowLocation::ChildWindow(position) = &popups.last().unwrap().location else {
            panic!("Testing popup must be in the window");
        };
        assert!(
            (position.x - x).abs() <= 1.0,
            "width {width}: popup x={}, button x={x}",
            position.x
        );
        drop(popups);
        label(&ui, "Next to originals")
            .mock_single_click(slint::platform::PointerEventButton::Left);
    }
}

#[test]
fn u11_create_results_follow_displayed_worker_index_order() {
    use base64::Engine;
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let names = ["mail10.eml", "mail2.eml", "mail1.eml"];
    let check = check_stream(root.path(), &names);
    let held = |names: &[&str]| {
        let records = stream(
            root.path(),
            &names
                .iter()
                .map(|n| (*n, "created-attachment"))
                .collect::<Vec<_>>(),
            false,
        );
        let lines: Vec<_> = records.lines().collect();
        print(&(lines[..2].join("\n") + "\n"))
            + "while [ ! -f first ]; do :; done\n"
            + &print(&(lines[2].to_owned() + "\n"))
            + "while [ ! -f second ]; do :; done\n"
            + &emit(&(lines[3..].join("\n") + "\n"))
    };
    let encoded: Vec<_> = names
        .iter()
        .map(|n| {
            base64::engine::general_purpose::STANDARD
                .encode(root.path().join(n).as_os_str().as_encoded_bytes())
        })
        .collect();
    let cli = script(
        root.path(),
        &format!(
            "request=$(cat)\ncase \"$request\" in *'\"dry_run\":true'*) {};; *) case \"$request\" in *{}*{}*{}*) {};; *) {};; esac;; esac",
            emit(&check),
            quote(&encoded[0]),
            quote(&encoded[1]),
            quote(&encoded[2]),
            held(&names),
            held(&[names[2], names[1], names[0]])
        ),
    );
    let c = controller(root.path(), cli);
    c.stage(names.iter().map(|n| root.path().join(n)).collect())
        .unwrap();
    pump(&c, || !c.is_busy());
    let before: Vec<_> = c
        .window()
        .get_rows()
        .iter()
        .map(|r| r.name.to_string())
        .collect();
    assert_eq!(before, names);
    c.start_selection();
    pump(&c, || {
        c.window()
            .get_rows()
            .iter()
            .any(|r| r.status == "Copy saved")
    });
    let first = c
        .window()
        .get_rows()
        .iter()
        .find(|r| r.name == before[0])
        .unwrap()
        .status;
    fs::write(root.path().join("first"), "").unwrap();
    pump(&c, || {
        c.window()
            .get_rows()
            .iter()
            .filter(|r| r.status == "Copy saved")
            .count()
            == 2
    });
    let second = c
        .window()
        .get_rows()
        .iter()
        .find(|r| r.name == before[1])
        .unwrap()
        .status;
    fs::write(root.path().join("second"), "").unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(first, "Copy saved");
    assert_eq!(second, "Copy saved");
}

#[test]
fn u1_controller_renders_core_outcomes_and_every_error_code() {
    use eml_core::{ErrorCode, vocabulary::Outcome};
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let mut names = vec![
        "ready.eml".to_owned(),
        "none.eml".to_owned(),
        "verify.eml".to_owned(),
    ];
    names.extend(ErrorCode::ALL.iter().map(|c| format!("{}.eml", c.as_str())));
    let mut values: Vec<Value> = check_stream(
        root.path(),
        &names.iter().map(String::as_str).collect::<Vec<_>>(),
    )
    .lines()
    .map(|l| serde_json::from_str(l).unwrap())
    .collect();
    values[2]["removed"] = json!([]);
    let source = values[3]["source"].clone();
    let destination = values[3]["destination"].clone();
    values[3] = records("planned-existing")[1].clone();
    values[3]["index"] = 2.into();
    values[3]["source"] = source;
    values[3]["destination"] = destination;
    for (n, code) in ErrorCode::ALL.iter().enumerate() {
        let i = n + 4;
        let source = values[i]["source"].clone();
        let destination = values[i]["destination"].clone();
        values[i] = records("prepublication-failure")[1].clone();
        values[i]["source"] = source;
        values[i]["destination"] = destination;
        values[i]["index"] = (i - 1).into();
        values[i]["error"]["code"] = code.as_str().into();
        values[i]["error"]["phase"] = code.allowed_phases()[0].as_str().into();
    }
    let summary = values.last_mut().unwrap();
    summary["counts"]["planned"] = 3.into();
    summary["counts"]["failed"] = ErrorCode::ALL.len().into();
    summary["interrupted"] = true.into();
    summary["exit_code"] = 130.into();
    let check = values.iter().map(|v| format!("{v}\n")).collect::<String>();
    let cases = [
        ("saved.eml", "created-attachment", Outcome::CopySaved),
        ("as-is.eml", "created", Outcome::CopiedAsIs),
        ("exists.eml", "up-to-date", Outcome::AlreadyExists),
        ("needs.eml", "published-with-error", Outcome::NeedsChecking),
        (
            "matched.eml",
            "existing-sync-failure",
            Outcome::NeedsChecking,
        ),
        ("unknown.eml", "unknown-publication", Outcome::Unknown),
        (
            "stopped.eml",
            "started-preedge-cancellation",
            Outcome::NotProcessed,
        ),
    ];
    let create = stream(root.path(), &cases.map(|(n, s, _)| (n, s)), false);
    let c = controller(root.path(), dual(root.path(), &check, &create));
    c.stage(vec![root.path().join("inputs")]).unwrap();
    pump(&c, || !c.is_busy());
    let actual = |name: &str| {
        c.window()
            .get_rows()
            .iter()
            .find(|r| r.name == name)
            .unwrap()
            .status
    };
    for (name, outcome) in [
        ("ready.eml", Outcome::Ready),
        ("none.eml", Outcome::NothingToRemove),
        ("verify.eml", Outcome::WillVerify),
    ] {
        assert_eq!(actual(name), outcome.label());
    }
    for code in ErrorCode::ALL {
        let outcome = if *code == ErrorCode::Interrupted {
            Outcome::NotProcessed
        } else {
            Outcome::Failed(*code)
        };
        assert_eq!(actual(&format!("{}.eml", code.as_str())), outcome.label());
    }
    c.start_selection();
    pump(&c, || !c.is_busy());
    for (name, _, outcome) in cases {
        assert_eq!(actual(name), outcome.label());
    }
}

#[test]
fn u6_every_state_family_and_filter_preserves_first_row_y() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let check = check_stream(root.path(), &["one.eml", "two.eml"]);
    let create = stream(
        root.path(),
        &[
            ("one.eml", "created-attachment"),
            ("two.eml", "prepublication-failure"),
        ],
        false,
    );
    let check_lines: Vec<_> = check.lines().collect();
    let create_lines: Vec<_> = create.lines().collect();
    let cli = script(
        root.path(),
        &format!(
            "request=$(cat)\ncase \"$request\" in *'\"dry_run\":true'*) {}while [ ! -f checked ]; do :; done\n{};; *) {}while [ ! -f created ]; do :; done\n{};; esac",
            print(&(check_lines[..2].join("\n") + "\n")),
            emit(&(check_lines[2..].join("\n") + "\n")),
            print(&(create_lines[0].to_owned() + "\n")),
            emit(&(create_lines[1..].join("\n") + "\n"))
        ),
    );
    let c = controller(root.path(), cli);
    let ui = c.window();
    c.stage(vec![root.path().join("inputs")]).unwrap();
    pump(&c, || ui.get_rows().row_count() == 1);
    assert_eq!(ui.get_view().state, BatchState::Checking);
    let y = label(&ui, "one.eml, Ready").absolute_position().y;
    let same = |label_text: &str| {
        assert!(
            (label(&ui, label_text).absolute_position().y - y).abs() <= 1.0,
            "state {:?} shifted first row",
            ui.get_view().state
        )
    };
    fs::write(root.path().join("checked"), "").unwrap();
    pump(&c, || !c.is_busy());
    same("one.eml, Ready");
    c.start_selection();
    pump(&c, || ui.get_view().progress_text.starts_with("0 of 2"));
    same("one.eml, Waiting");
    assert!(
        (label(&ui, &ui.get_view().progress_text)
            .absolute_position()
            .x
            - label(&ui, "Removing attachments…").absolute_position().x)
            .abs()
            <= 1.0
    );
    fs::write(root.path().join("created"), "").unwrap();
    pump(&c, || !c.is_busy());
    same("one.eml, Copy saved");
    assert!(
        (label(
            &ui,
            "Your originals are unchanged. Drop more emails to start a new batch."
        )
        .absolute_position()
        .x - label(&ui, &ui.get_view().title).absolute_position().x)
            .abs()
            <= 1.0
    );
    ui.invoke_toggle_problems();
    same("two.eml, Not a valid email");
    drop(c);
    let root = tempfile::tempdir().unwrap();
    let check = stream(
        root.path(),
        &[
            ("one.eml", "planned-create-unix"),
            ("two.eml", "prepublication-failure"),
        ],
        true,
    );
    let create = stream(
        root.path(),
        &[
            ("one.eml", "started-preedge-cancellation"),
            ("two.eml", "prepublication-failure"),
        ],
        false,
    );
    let c = controller(root.path(), dual(root.path(), &check, &create));
    let ui = c.window();
    c.stage(vec![root.path().join("inputs")]).unwrap();
    pump(&c, || !c.is_busy());
    assert!((label(&ui, "one.eml, No attachments").absolute_position().y - y).abs() <= 1.0);
    ui.invoke_toggle_problems();
    assert!(
        (label(&ui, "two.eml, Not a valid email")
            .absolute_position()
            .y
            - y)
            .abs()
            <= 1.0
    );
    ui.invoke_toggle_problems();
    c.start_selection();
    pump(&c, || !c.is_busy());
    assert_eq!(ui.get_view().state, BatchState::Stopped);
    assert!((label(&ui, "one.eml, Not processed").absolute_position().y - y).abs() <= 1.0);
}

#[test]
fn field_attention_results_do_not_claim_that_no_copy_was_saved() {
    init_no_event_loop();
    for scenario in [
        "published-with-error",
        "existing-sync-failure",
        "unknown-publication",
        "",
    ] {
        let root = tempfile::tempdir().unwrap();
        let create = if scenario.is_empty() {
            String::new()
        } else {
            stream(root.path(), &[("one.eml", scenario)], false)
        };
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
        assert_eq!(
            c.window().get_view().title,
            "Results need attention",
            "scenario {scenario}"
        );
        assert_eq!(c.window().get_view().problems, "1 needs attention");
    }
    let root = tempfile::tempdir().unwrap();
    let c = controller(
        root.path(),
        dual(
            root.path(),
            &check_stream(root.path(), &["one.eml"]),
            &stream(root.path(), &[("one.eml", "prepublication-failure")], false),
        ),
    );
    c.stage(vec![root.path().join("one.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    c.start_selection();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_view().title, "No copies saved");
}

#[test]
fn field_source_controls_are_escaped_without_changing_native_selection() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("folder\n\u{202e}");
    let name = "06-evil\n\u{202e}fake.eml";
    let c = controller(
        root.path(),
        dual(root.path(), &check_stream(&folder, &[name]), ""),
    );
    c.stage(vec![folder.clone()]).unwrap();
    pump(&c, || !c.is_busy());
    let row = c.window().get_rows().row_data(0).unwrap();
    assert_eq!(row.name, "06-evil\\n\\u{202e}fake.eml");
    assert!(!row.folder.contains('\n') && !row.folder.contains('\u{202e}'));
    assert!(row.folder.ends_with("folder\\n\\u{202e}"));
    c.window().invoke_select_email(row.key);
    assert_eq!(c.selected_original(), Some(folder.join(name)));
    assert_eq!(c.window().get_detail().name, "06-evil\\n\\u{202e}fake.eml");
}

#[test]
fn core_consequences_reach_selected_details_and_copy_details() {
    init_no_event_loop();
    for (scenario, expected) in [
        (
            "published-with-error",
            "A complete copy was published; qualification did not complete. Inspect the destination before retrying.",
        ),
        (
            "existing-sync-failure",
            "Existing bytes matched; synchronization/address qualification did not complete.",
        ),
        (
            "unknown-publication",
            "A complete copy may exist. Inspect the destination before retrying.",
        ),
        (
            "complete-cleanup-with-error",
            "Cleanup completed, but reported a native error. Inspect the cleanup details.",
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let c = controller(
            root.path(),
            dual(
                root.path(),
                &check_stream(root.path(), &["one.eml"]),
                &stream(root.path(), &[("one.eml", scenario)], false),
            ),
        );
        c.stage(vec![root.path().join("one.eml")]).unwrap();
        pump(&c, || !c.is_busy());
        c.start_selection();
        pump(&c, || !c.is_busy());
        select_first(&c);
        assert!(
            c.window().get_detail().sentence.contains(expected),
            "{scenario}"
        );
        assert!(c.clipboard_text().unwrap().contains(expected), "{scenario}");
    }
}

#[test]
fn core_related_notices_distinguish_preview_created_and_existing() {
    init_no_event_loop();
    for (scenario, dry, expected) in [
        (
            "planned-create-unix",
            true,
            eml_core::vocabulary::PREPARED_RELATED_RESOURCE_NOTICE,
        ),
        (
            "created-attachment",
            false,
            eml_core::vocabulary::RELATED_RESOURCE_NOTICE,
        ),
        (
            "up-to-date-attachment",
            false,
            eml_core::vocabulary::EXISTING_RELATED_RESOURCE_NOTICE,
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut values: Vec<Value> = stream(root.path(), &[("one.eml", scenario)], dry)
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        values[1]["removed"] = records("created-attachment")[1]["removed"].clone();
        values[1]["source_bytes"] = 1028.into();
        values[1]["copy_bytes"] = 513.into();
        values[1]["removed"][0]["reason"] = "related_resource".into();
        values[1]["removed"][0]["content_type"] = "image/png".into();
        let output: String = values.iter().map(|v| format!("{v}\n")).collect();
        let check = if dry {
            output.clone()
        } else {
            check_stream(root.path(), &["one.eml"])
        };
        let c = controller(
            root.path(),
            dual(root.path(), &check, if dry { "" } else { &output }),
        );
        c.stage(vec![root.path().join("one.eml")]).unwrap();
        pump(&c, || !c.is_busy());
        if !dry {
            c.start_selection();
            pump(&c, || !c.is_busy());
        }
        select_first(&c);
        assert!(
            c.window().get_detail().sentence.contains(expected),
            "{scenario}"
        );
        assert!(c.clipboard_text().unwrap().contains(expected), "{scenario}");
    }
}

#[test]
fn failed_preview_does_not_claim_attachments_are_absent() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(
        root.path(),
        dual(
            root.path(),
            &stream(root.path(), &[("bad.eml", "prepublication-failure")], true),
            "",
        ),
    );
    c.stage(vec![root.path().join("bad.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    assert!(!c.window().get_view().summary.contains("No attachments"));
    assert_eq!(c.window().get_view().problems, "1 can’t be copied");
    assert!(!c.window().get_view().can_start);
}

#[test]
fn request_refusal_retains_diagnostic_details_without_changing_report_bytes() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let cli = script(
        root.path(),
        "cat >/dev/null\nprintf 'Cannot use the output folder: permission denied (OS error 13)\\n' >&2\nexit 2\n",
    );
    let c = controller(root.path(), cli);
    c.stage(vec![root.path().join("one.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(
        c.window().get_view().banner,
        "The request was refused before processing any emails."
    );
    assert_eq!(c.window().get_view().title, "Emails not checked");
    label(&c.window(), "Details…").mock_single_click(slint::platform::PointerEventButton::Left);
    let details = label(&c.window(), "Processing details");
    assert!(details.accessible_value().unwrap().contains("OS error 13"));
    let capture = c.capture_export().unwrap();
    let target = root.path().join("refused.jsonl");
    c.start_captured_export(capture, target.clone());
    pump(&c, || !c.is_busy());
    assert_eq!(fs::read(target).unwrap(), b"");
    label(&c.window(), "Hide details").mock_single_click(slint::platform::PointerEventButton::Left);
    assert!(
        ElementHandle::find_by_accessible_label(&c.window(), "Processing details")
            .next()
            .is_none()
    );
}

#[test]
fn residual_cleanup_detail_keeps_the_resource_and_separate_native_cause() {
    use base64::Engine;
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let mut values: Vec<Value> = stream(root.path(), &[("one.eml", "published-with-error")], false)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    values[1]["publication"]["cleanup"] = json!({"state":"residual","residual_name":{"encoding":"unix-bytes","base64":base64::engine::general_purpose::STANDARD.encode(".emlattachmentremover-\n\u{202e}.tmp")},"native_code":13});
    let output: String = values.iter().map(|v| format!("{v}\n")).collect();
    let c = controller(
        root.path(),
        dual(
            root.path(),
            &check_stream(root.path(), &["one.eml"]),
            &output,
        ),
    );
    c.stage(vec![root.path().join("one.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    c.start_selection();
    pump(&c, || !c.is_busy());
    select_first(&c);
    assert!(
        c.window()
            .get_detail()
            .sentence
            .contains("A known owned temporary resource remains. Inspect the cleanup details.")
    );
    let text = c.clipboard_text().unwrap();
    assert!(text.contains("Cleanup: Residual\nNative code: 13\nResidual resource: .emlattachmentremover-\\n\\u{202e}.tmp"), "{text}");
    assert!(!c.window().get_detail().can_show_copy);
}

#[test]
fn unknown_create_does_not_repeat_a_stale_preview_notice() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let mut values: Vec<Value> = check_stream(root.path(), &["one.eml"])
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    values[1]["removed"][0]["reason"] = "related_resource".into();
    let check: String = values.iter().map(|v| format!("{v}\n")).collect();
    let c = controller(root.path(), dual(root.path(), &check, ""));
    c.stage(vec![root.path().join("one.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    select_first(&c);
    assert!(
        c.window()
            .get_detail()
            .sentence
            .contains(eml_core::vocabulary::PREPARED_RELATED_RESOURCE_NOTICE)
    );
    c.start_selection();
    pump(&c, || !c.is_busy());
    assert_eq!(
        c.window().get_detail().sentence,
        eml_core::vocabulary::Outcome::Unknown.explanation()
    );
}

#[test]
fn unobserved_create_results_do_not_claim_zero_copies_were_saved() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), dual(root.path(), "", ""));
    c.stage(vec![root.path().join("one.eml")]).unwrap();
    c.start_selection();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_view().state, BatchState::Done);
    assert_eq!(c.window().get_view().title, "Results need attention");
}

#[test]
fn blocked_collection_does_not_claim_that_no_emails_exist() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let mut values = records("no-inputs");
    values[0]["dry_run"] = true.into();
    values[0]["collection_complete"] = false.into();
    values[1]["collection_complete"] = false.into();
    values[1]["run_error"]["code"] = "OUTPUT_OVERLAP".into();
    values[1]["run_error"]["phase"] = "admission".into();
    values[1]["run_error"]["detail"] =
        "Output directory is reachable below a selected directory".into();
    let output: String = values.iter().map(|v| format!("{v}\n")).collect();
    let c = controller(root.path(), dual(root.path(), &output, ""));
    c.stage(vec![root.path().join("one.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_view().title, "Emails not checked");
    assert!(c.window().get_view().banner.contains("save folder"));
    assert!(!c.window().get_view().can_start);
}

#[test]
fn stopped_summary_includes_core_work_without_preview_rows() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let mut values: Vec<Value> = stream(
        root.path(),
        &[("one.eml", "started-preedge-cancellation")],
        false,
    )
    .lines()
    .map(|l| serde_json::from_str(l).unwrap())
    .collect();
    values[0]["total"] = 5.into();
    values[2]["counts"]["not_processed"] = 4.into();
    let create: String = values.iter().map(|v| format!("{v}\n")).collect();
    let c = controller(root.path(), dual(root.path(), "", &create));
    c.stage(vec![root.path().join("inputs")]).unwrap();
    c.start_selection();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_view().state, BatchState::Stopped);
    assert!(
        c.window().get_view().summary.contains("5 not processed"),
        "{}",
        c.window().get_view().summary
    );
}

#[test]
fn stopped_before_observations_can_continue_the_same_request() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let cli = script(
        root.path(),
        "request=$(cat)\nprintf '%s\\n' \"$request\" >> requests.jsonl\ncase \"$request\" in *'\"dry_run\":true'*) exit 0;; *) exit 130;; esac\n",
    );
    let c = controller(root.path(), cli);
    c.stage(vec![root.path().join("inputs")]).unwrap();
    c.start_selection();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_view().state, BatchState::Stopped);
    assert!(c.window().get_view().can_start);
    c.start_selection();
    pump(&c, || !c.is_busy());
    let created: Vec<_> = requests(root.path())
        .into_iter()
        .filter(|r| r["dry_run"] == false)
        .collect();
    assert_eq!(created.len(), 2);
    assert_eq!(created[0], created[1]);
}

#[test]
#[cfg(unix)]
fn native_path_units_remain_distinct_in_rows_details_and_folder_choices() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join(OsStr::from_bytes(b"source-\xfc"));
    let destination = root.path().join(OsStr::from_bytes(b"copies-\xfe\n"));
    let names = [
        OsStr::from_bytes(b"odd-\xff.eml"),
        OsStr::from_bytes(b"odd-\xfe.eml"),
        OsStr::new("odd-�.eml"),
        OsStr::new("odd-<0xFF>.eml"),
    ];
    let sources: Vec<_> = names.iter().map(|name| folder.join(name)).collect();
    let copies: Vec<_> = (0..names.len())
        .map(|index| {
            destination.join(OsStr::from_bytes(&[
                b'c',
                b'0' + index as u8,
                0xfd,
                b'.',
                b'e',
                b'm',
                b'l',
            ]))
        })
        .collect();
    let native_stream = |text: String| {
        text.lines()
            .map(|line| {
                let mut record: Value = serde_json::from_str(line).unwrap();
                if record["type"] == "item" {
                    let index = record["index"].as_u64().unwrap() as usize;
                    for (field, path) in
                        [("source", &sources[index]), ("destination", &copies[index])]
                    {
                        record[field] =
                            serde_json::to_value(eml_protocol::PathValue::new(path, path)).unwrap();
                    }
                }
                format!("{record}\n")
            })
            .collect::<String>()
    };
    let check = native_stream(check_stream(
        root.path(),
        &["a.eml", "b.eml", "c.eml", "d.eml"],
    ));
    let create = native_stream(stream(
        root.path(),
        &[
            ("a.eml", "created"),
            ("b.eml", "created"),
            ("c.eml", "created"),
            ("d.eml", "created"),
        ],
        false,
    ));
    // Native path presentation does not depend on the host filesystem accepting
    // those names (for example, APFS rejects invalid UTF-8 directory names).
    let mut preferences = Preferences::load(root.path().join("preferences.json"));
    preferences
        .set_destination(Some(destination.clone()))
        .unwrap();
    let c = DesktopController::new(
        dual(root.path(), &check, &create),
        root.path().to_owned(),
        preferences,
    )
    .unwrap();
    c.window().show().unwrap();
    c.stage(vec![folder.clone()]).unwrap();
    pump(&c, || !c.is_busy());
    let ui = c.window();
    let expected = [
        "odd-<0xFF>.eml",
        "odd-<0xFE>.eml",
        "odd-�.eml",
        "odd-<<0xFF>.eml",
    ];
    assert_eq!(ui.get_rows().row_count(), expected.len());
    for (row, expected) in ui.get_rows().iter().zip(expected) {
        assert_eq!(row.name, expected);
        assert_eq!(row.folder, "~/source-<0xFC>");
    }
    assert_eq!(ui.get_view().destination, r"copies-<0xFE>\n");
    assert!(
        ui.get_recent_destinations()
            .row_data(0)
            .unwrap()
            .ends_with(r"/copies-<0xFE>\n")
    );
    select_first(&c);
    assert_eq!(c.selected_original(), Some(sources[0].clone()));
    assert_eq!(ui.get_detail().name, "odd-<0xFF>.eml");
    let details = c.clipboard_text().unwrap();
    assert!(details.contains("source-<0xFC>/odd-<0xFF>.eml"));
    assert!(!details.contains('�'));
    c.start_selection();
    pump(&c, || !c.is_busy());
    assert_eq!(ui.get_detail().path_suffix, "c0<0xFD>.eml");
    assert!(ui.get_detail().path_prefix.ends_with(r"/copies-<0xFE>\n/"));
    assert!(
        ui.get_detail()
            .path
            .ends_with(r"/copies-<0xFE>\n/c0<0xFD>.eml")
    );
    let details = c.clipboard_text().unwrap();
    assert!(details.contains(r"/copies-<0xFE>\n/c0<0xFD>.eml"));
    assert!(!details.contains('�'));
}

fn test_clipboard(window: &slint::Window) -> Option<String> {
    i_slint_core::window::WindowInner::from_pub(window)
        .context()
        .platform()
        .clipboard_text(slint::platform::Clipboard::DefaultClipboard)
}
fn text_shortcut(window: &slint::Window, text: &str) {
    use slint::platform::{Key, WindowEvent};
    window.dispatch_event(WindowEvent::KeyPressed {
        text: Key::Control.into(),
    });
    window.dispatch_event(WindowEvent::KeyPressed { text: text.into() });
    window.dispatch_event(WindowEvent::KeyReleased { text: text.into() });
    window.dispatch_event(WindowEvent::KeyReleased {
        text: Key::Control.into(),
    });
    mock_elapsed_time(Duration::from_millis(1));
}
fn text_element(window: &impl ElementRoot, name: &str) -> ElementHandle {
    ElementHandle::find_by_accessible_label(window, name)
        .next()
        .unwrap_or_else(|| panic!("Missing text target: {name}"))
}
// The testing backend applies initial preferred size but not later native
// minimum constraints. Mirror the native window's minimum-size enforcement.
fn honor_window_minimum(window: &slint::Window) {
    let component = i_slint_core::window::WindowInner::from_pub(window)
        .try_component()
        .unwrap();
    let component = i_slint_core::item_tree::ItemTreeRc::borrow_pin(&component);
    let h = component
        .as_ref()
        .layout_info(i_slint_core::layout::Orientation::Horizontal);
    let v = component
        .as_ref()
        .layout_info(i_slint_core::layout::Orientation::Vertical);
    let size = window.size().to_logical(window.scale_factor());
    window.set_size(slint::LogicalSize::new(
        size.width.max(h.min),
        size.height.max(v.min),
    ));
}
fn focus_text(window: &impl ElementRoot, name: &str, start: i32, end: i32) {
    let editor = text_element(window, name);
    editor.mock_single_click(slint::platform::PointerEventButton::Left);
    editor.set_accessible_selection_offsets(start, end);
    mock_elapsed_time(Duration::from_millis(1));
}
#[test]
fn standard_copy_uses_diagnostic_selection_and_explicit_copy_details_keeps_full_facts() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = done(root.path(), &["one.eml"]);
    select_first(&c);
    let ui = c.window();
    let facts = c.clipboard_text().unwrap();
    let weak = Rc::downgrade(&c);
    ui.on_copy_details(move || {
        let c = weak.upgrade().unwrap();
        let text = c.clipboard_text().unwrap();
        i_slint_core::window::WindowInner::from_pub(c.window().window())
            .context()
            .platform()
            .set_clipboard_text(&text, slint::platform::Clipboard::DefaultClipboard);
    });
    let mut view = ui.get_view();
    view.diagnostics = "diagnostic selection and more".into();
    ui.set_view(view);
    mock_elapsed_time(Duration::from_millis(1));
    label(&ui, "Details…").mock_single_click(slint::platform::PointerEventButton::Left);
    focus_text(&ui, "Processing details", 0, 10);
    text_shortcut(ui.window(), "c");
    assert_eq!(test_clipboard(ui.window()).as_deref(), Some("diagnostic"));
    label(&ui, "Copy Details").mock_single_click(slint::platform::PointerEventButton::Left);
    assert_eq!(test_clipboard(ui.window()).as_deref(), Some(facts.as_str()));
    text_shortcut(ui.window(), "c");
    assert_eq!(test_clipboard(ui.window()).as_deref(), Some(facts.as_str()));
}
#[test]
fn standard_copy_is_enabled_for_a_focused_diagnostic_without_a_selected_email() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = done(root.path(), &["one.eml"]);
    let ui = c.window();
    assert!(c.selected_original().is_none());
    assert!(!ui.get_view().can_copy);
    let mut view = ui.get_view();
    view.diagnostics = "selection without a row".into();
    ui.set_view(view);
    mock_elapsed_time(Duration::from_millis(1));
    label(&ui, "Details…").mock_single_click(slint::platform::PointerEventButton::Left);
    focus_text(&ui, "Processing details", 0, 9);
    text_shortcut(ui.window(), "c");
    assert_eq!(test_clipboard(ui.window()).as_deref(), Some("selection"));
    label(&ui, "Hide details").mock_single_click(slint::platform::PointerEventButton::Left);
    i_slint_core::window::WindowInner::from_pub(ui.window())
        .context()
        .platform()
        .set_clipboard_text(
            "closed editor",
            slint::platform::Clipboard::DefaultClipboard,
        );
    text_shortcut(ui.window(), "c");
    assert_eq!(
        test_clipboard(ui.window()).as_deref(),
        Some("closed editor")
    );
}
#[test]
fn secondary_about_and_license_details_support_copy_and_select_all() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "created"));
    let about = c.about_window();
    about.set_notice("Failed to open website".into());
    about.set_notice_details("about diagnostic selection".into());
    about.show().unwrap();
    text_element(&about, "Details…").mock_single_click(slint::platform::PointerEventButton::Left);
    honor_window_minimum(about.window());
    assert!(
        text_element(&about, "Website opening details")
            .size()
            .height
            >= 100.
    );
    focus_text(&about, eml_core::product::DISPLAY_NAME, 0, 3);
    text_shortcut(about.window(), "c");
    assert_eq!(test_clipboard(about.window()).as_deref(), Some("EML"));
    focus_text(&about, "Website opening details", 0, 5);
    text_shortcut(about.window(), "c");
    assert_eq!(test_clipboard(about.window()).as_deref(), Some("about"));
    text_shortcut(about.window(), "a");
    text_shortcut(about.window(), "c");
    assert_eq!(
        test_clipboard(about.window()).as_deref(),
        Some("about diagnostic selection")
    );
    let licenses = c.license_window();
    licenses.set_licenses("main license terms".into());
    licenses.set_notice("Failed to show notices".into());
    licenses.set_notice_details("license diagnostic selection".into());
    licenses.show().unwrap();
    text_element(&licenses, "Details…")
        .mock_single_click(slint::platform::PointerEventButton::Left);
    focus_text(&licenses, "Software and artwork license terms", 0, 4);
    text_shortcut(licenses.window(), "c");
    assert_eq!(test_clipboard(licenses.window()).as_deref(), Some("main"));
    focus_text(&licenses, "Notices opening details", 0, 7);
    text_shortcut(licenses.window(), "c");
    assert_eq!(
        test_clipboard(licenses.window()).as_deref(),
        Some("license")
    );
    text_shortcut(licenses.window(), "a");
    text_shortcut(licenses.window(), "c");
    assert_eq!(
        test_clipboard(licenses.window()).as_deref(),
        Some("license diagnostic selection")
    );
}

#[test]
fn v3_all_existing_headline() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let check = check_stream(root.path(), &["one.eml"]);
    let create = stream(root.path(), &[("one.eml", "up-to-date")], false);
    let c = controller(root.path(), dual(root.path(), &check, &create));
    c.stage(vec![root.path().join("inputs")]).unwrap();
    pump(&c, || !c.is_busy());
    c.start_selection();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_view().title, "1 copy already exists");
}

#[test]
fn v4_plain_shared_words() {
    use eml_core::vocabulary::{Consequence::*, *};
    let mut texts: Vec<_> = [
        Published,
        PublicationUnknown,
        NoPublication,
        ExistingUnqualified,
        AddressUnqualified,
        DurabilityUnqualified,
        ResidualCleanup,
        UnknownCleanup,
        CleanupCompletedWithError,
    ]
    .into_iter()
    .map(|c| c.text())
    .collect();
    texts.extend([
        RELATED_RESOURCE_NOTICE,
        EXISTING_RELATED_RESOURCE_NOTICE,
        PREPARED_RELATED_RESOURCE_NOTICE,
    ]);
    texts.extend(
        [
            Outcome::Ready,
            Outcome::NothingToRemove,
            Outcome::CopySaved,
            Outcome::CopiedAsIs,
            Outcome::AlreadyExists,
            Outcome::WillVerify,
            Outcome::NeedsChecking,
            Outcome::Unknown,
            Outcome::NotProcessed,
        ]
        .into_iter()
        .map(|o| o.explanation()),
    );
    texts.extend(
        eml_core::ErrorCode::ALL
            .iter()
            .map(|c| Outcome::Failed(*c).explanation()),
    );
    for text in texts {
        assert!(
            !["publish", "qualif", "namespace", "owned", "prepared"]
                .iter()
                .any(|word| text.to_lowercase().contains(word)),
            "{text}"
        );
    }
}

#[test]
fn v5_detail_actions_share_title_line() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = done(
        root.path(),
        &["one.eml", "two.eml", "three.eml", "four.eml", "five.eml"],
    );
    let ui = c.window();
    ui.window().set_size(slint::LogicalSize::new(1080., 700.));
    ui.invoke_select_email(ui.get_rows().row_data(0).unwrap().key);
    mock_elapsed_time(Duration::from_millis(1));
    let name = label(&ui, "one.eml");
    let action = label(&ui, "Show Original");
    assert!(
        (name.absolute_position().y - action.absolute_position().y).abs() <= 1.,
        "title={:?}, action={:?}",
        name.absolute_position(),
        action.absolute_position()
    );
}

#[test]
fn v6_status_header_aligns_with_text() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = done(root.path(), &["one.eml"]);
    let ui = c.window();
    mock_elapsed_time(Duration::from_millis(1));
    let header = label(&ui, "Status");
    let cell = label(&ui, "Copy saved");
    assert!(
        (header.absolute_position().x - cell.absolute_position().x).abs() <= 1.,
        "header={:?}, cell={:?}",
        header.absolute_position(),
        cell.absolute_position()
    );
}
