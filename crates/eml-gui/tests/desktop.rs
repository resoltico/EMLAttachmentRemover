//! Eight desktop contracts through the real Slint testing backend and stub CLI processes.
use eml_core::{
    RemovalReason, RunMode,
    vocabulary::{self, CountKind, RunStatus},
};
use eml_gui::{
    Confirmation, DesktopController, MainWindow, ingress::launch_paths, preferences::Preferences,
};
use i_slint_backend_testing::{
    AccessibleRole, ElementHandle, init_no_event_loop, mock_elapsed_time,
};
use slint::{ComponentHandle, Model};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    thread,
    time::{Duration, Instant},
};
trait SubmitSelection {
    fn submit(&self, paths: Vec<PathBuf>) -> Result<eml_app::RunId, eml_app::QueueError>;
}
impl SubmitSelection for DesktopController {
    fn submit(&self, paths: Vec<PathBuf>) -> Result<eml_app::RunId, eml_app::QueueError> {
        self.stage(paths)?;
        self.start_selection()
    }
}
fn force_prompt(ui: &MainWindow) {
    click(ui, "More run actions");
    click(ui, "Force Stop");
}
fn fixture(name: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../tests/contract/protocol/{name}.jsonl")),
    )
    .unwrap()
    .replace("@VERSION@", eml_core::product::VERSION)
}
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
fn print(value: &str) -> String {
    format!("printf '%s' {}\n", quote(value))
}
fn script(root: &Path, body: &str) -> PathBuf {
    let path = root.join("fixture.sh");
    fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
    PathBuf::from(env!("CARGO_BIN_EXE_eml-gui-cli-fixture"))
}
fn completed(root: &Path, name: &str) -> PathBuf {
    script(
        root,
        &("cat >/dev/null\n".to_owned() + &print(&fixture(name)) + "exit 0\n"),
    )
}
fn controlled(root: &Path, ignore: bool) -> PathBuf {
    let records = fixture("started-preedge-cancellation");
    let (start, rest) = records.split_once('\n').unwrap();
    let trap = if ignore {
        "trap '' INT\n".to_owned()
    } else {
        format!("trap {} INT\n", quote(&(print(rest) + "exit 130")))
    };
    script(
        root,
        &(trap + "cat >/dev/null\n" + &print(&(start.to_owned() + "\n")) + "while :; do :; done\n"),
    )
}
fn controller(root: &Path, cli: PathBuf) -> Rc<DesktopController> {
    let controller = DesktopController::new(
        cli,
        root.to_owned(),
        Preferences::load(root.join("preferences.json")),
    )
    .unwrap();
    controller
        .window()
        .window()
        .set_size(slint::LogicalSize::new(1060.0, 760.0));
    controller.window().show().unwrap();
    mock_elapsed_time(Duration::from_millis(1));
    controller
}
fn pump(controller: &DesktopController, predicate: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        controller.tick();
        mock_elapsed_time(Duration::from_millis(5));
        if predicate() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "Desktop fixture did not reach its expected state"
        );
        thread::sleep(Duration::from_millis(2));
    }
}
fn click(ui: &MainWindow, label: &str) {
    mock_elapsed_time(Duration::from_millis(1));
    ElementHandle::find_by_accessible_label(ui, label)
        .find(|element| {
            matches!(
                element.accessible_role(),
                Some(AccessibleRole::Button | AccessibleRole::Checkbox)
            )
        })
        .unwrap_or_else(|| panic!("Missing accessible control: {label}"))
        .invoke_accessible_default_action();
}
fn has_label(ui: &MainWindow, label: &str) -> bool {
    ElementHandle::find_by_accessible_label(ui, label)
        .next()
        .is_some()
}
fn wait_started(controller: &DesktopController) {
    pump(controller, || {
        has_label(
            &controller.window(),
            &vocabulary::count_label(CountKind::KnownItems, 1),
        )
    });
}

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
    click(&controller.window(), "Start selection");
    assert_eq!(controller.window().get_runs().row_count(), 1);
    assert_eq!(
        controller
            .window()
            .get_runs()
            .row_data(0)
            .unwrap()
            .selections
            .value,
        2
    );
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
    click(&controller.window(), "Start selection");
    assert_eq!(controller.window().get_runs().row_count(), 1);
    assert_eq!(
        controller
            .window()
            .get_runs()
            .row_data(0)
            .unwrap()
            .selections
            .value,
        3
    );
    controller.deliver_drop(root.path().join("four.eml"));
    mock_elapsed_time(Duration::from_millis(1));
    assert_eq!(controller.window().get_selection().row_count(), 1);
    assert_eq!(controller.window().get_waiting().row_count(), 0);
    click(&controller.window(), "Start selection");
    assert_eq!(controller.window().get_waiting().row_count(), 1);
    controller.deliver_drop(root.path().join("cancelled.eml"));
    controller.cancel_drop();
    mock_elapsed_time(Duration::from_millis(1));
    assert_eq!(controller.window().get_waiting().row_count(), 1);
    pump(&controller, || !controller.is_busy());
    assert_eq!(controller.window().get_runs().row_count(), 2);
}
#[test]
fn real_accessible_elements_use_core_vocabulary_and_wrong_identity_is_not_success() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    {
        let controller = controller(root.path(), completed(root.path(), "created-attachment"));
        controller
            .submit(vec![root.path().join("source.eml")])
            .unwrap();
        pump(&controller, || !controller.is_busy());
        let ui = controller.window();
        assert_eq!(ui.get_run_status().as_str(), RunStatus::Completed.label());
        assert_eq!(
            ui.get_item_status().as_str(),
            vocabulary::MeaningKind::Created.label()
        );
        assert!(has_label(
            &ui,
            &vocabulary::count_label(CountKind::Created, 1)
        ));
        click(&ui, "Message technical details");
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
        RunStatus::IdentityFailure.label()
    );
    assert_eq!(controller.window().get_items().row_count(), 0);
    assert!(!has_label(
        &controller.window(),
        &vocabulary::count_label(CountKind::Created, 1)
    ));
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
            force_prompt(&ui);
            assert!((ui.get_confirmation() == Confirmation::ForceStop));
            assert!(controller.is_busy());
            click(&ui, "Keep running");
            assert!(!(ui.get_confirmation() == Confirmation::ForceStop));
            assert!(controller.is_busy());
            force_prompt(&ui);
            click(&ui, "Force Stop now");
        } else {
            click(&ui, "Stop");
        }
        pump(&controller, || !controller.is_busy());
        assert_eq!(
            ui.get_run_status().as_str(),
            if force {
                RunStatus::ForceStopped.label()
            } else {
                RunStatus::Stopped.label()
            }
        );
        if force {
            assert!(!has_label(
                &ui,
                &vocabulary::count_label(CountKind::NotProcessed, 1)
            ));
        }
    }
}
#[test]
fn idle_quit_is_immediate_while_busy_quit_requires_a_real_decision() {
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
    ElementHandle::find_by_accessible_label(&controller.window(), "Check only")
        .next()
        .unwrap()
        .mock_single_click(slint::platform::PointerEventButton::Left);
    mock_elapsed_time(Duration::from_millis(1));
    assert!(
        !controller.window().get_check_only(),
        "Confirmation must block background input"
    );
    click(&controller.window(), "Keep running");
    assert!(!(controller.window().get_confirmation() == Confirmation::Quit));
    assert!(!controller.should_exit());
    assert!(!controller.request_quit());
    click(&controller.window(), "Stop and wait");
    pump(&controller, || controller.should_exit());
    assert!(!controller.is_busy());
    assert_eq!(
        controller.window().get_run_status().as_str(),
        RunStatus::Stopped.label()
    );
}
#[test]
fn create_after_check_sends_a_fresh_request_and_uses_the_current_destination() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let requests = root.path().join("requests.jsonl");
    let body = format!(
        "request=$(cat)\nprintf '%s\\n' \"$request\" >>{}\ncase \"$request\" in *'\"dry_run\":true'*) {} ;; *) {} ;; esac\n",
        quote(requests.to_str().unwrap()),
        print(&fixture("planned-create-unix")),
        print(&fixture("created"))
    );
    let controller = controller(root.path(), script(root.path(), &body));
    let ui = controller.window();
    click(&ui, "Check only");
    assert!(ui.get_check_only());
    controller
        .submit(vec![root.path().join("source.eml")])
        .unwrap();
    pump(&controller, || !controller.is_busy());
    assert_eq!(
        ui.get_item_status().as_str(),
        vocabulary::MeaningKind::Prepared.label()
    );
    assert!(ui.get_can_create());
    let output = root.path().join("copies");
    fs::create_dir(&output).unwrap();
    controller.set_destination(Some(output.clone()));
    click(&ui, "Create copies from this selection");
    assert!(!controller.is_busy());
    assert_eq!(fs::read_to_string(&requests).unwrap().lines().count(), 1);
    assert_eq!(ui.get_selection().row_count(), 1);
    click(&ui, "Start selection");
    pump(&controller, || !controller.is_busy());
    let docs: Vec<serde_json::Value> = fs::read_to_string(requests)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(docs.len(), 2);
    assert_eq!(docs[0]["dry_run"], true);
    assert_eq!(docs[1]["dry_run"], false);
    assert_eq!(docs[0]["paths"], docs[1]["paths"]);
    assert!(docs[0]["output_directory"].is_null());
    assert_eq!(
        docs[1]["output_directory"],
        serde_json::to_value(eml_protocol::NativePathValue::new(&output)).unwrap()
    );
    assert_eq!(ui.get_runs().row_count(), 2);
    assert_eq!(
        ui.get_runs().row_data(0).unwrap().mode.as_str(),
        vocabulary::mode_label(RunMode::DryRun)
    );
    assert_eq!(
        ui.get_runs().row_data(1).unwrap().mode.as_str(),
        vocabulary::mode_label(RunMode::Create)
    );
    assert_eq!(
        ui.get_item_status().as_str(),
        vocabulary::MeaningKind::Created.label()
    );
    assert!(!ui.get_can_create());
}
#[test]
fn export_preserves_existing_bytes_and_busy_export_keeps_quit_confirmation() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let controller = controller(root.path(), completed(root.path(), "created"));
    controller
        .submit(vec![root.path().join("source.eml")])
        .unwrap();
    pump(&controller, || !controller.is_busy());
    let existing = root.path().join("existing.jsonl");
    fs::write(&existing, b"winner").unwrap();
    let modified = fs::metadata(&existing).unwrap().modified().unwrap();
    controller.start_export(existing.clone());
    assert!(controller.is_busy());
    assert!(!controller.request_quit());
    click(&controller.window(), "Keep running");
    pump(&controller, || !controller.is_busy());
    assert_eq!(fs::read(&existing).unwrap(), b"winner");
    assert_eq!(
        fs::metadata(&existing).unwrap().modified().unwrap(),
        modified
    );
    assert!(has_label(
        &controller.window(),
        &format!("Export for Run 1: {}", vocabulary::NO_REPLACEMENT)
    ));
    let saved = root.path().join("saved.jsonl");
    controller.start_export(saved.clone());
    pump(&controller, || !controller.is_busy());
    assert_eq!(fs::read(saved).unwrap(), fixture("created").as_bytes());
    assert!(!controller.should_exit());
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
fn run_failure_without_items_explains_incomplete_discovery_without_failed_items() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let cli = script(
        root.path(),
        &("cat >/dev/null\n".to_owned() + &print(&fixture("collection-limit")) + "exit 1\n"),
    );
    let c = controller(root.path(), cli);
    click(&c.window(), "Check only");
    c.submit(vec![root.path().join("overflow")]).unwrap();
    pump(&c, || !c.is_busy());
    let ui = c.window();
    assert!(has_label(&ui, "Run error: INPUT_LIMIT_EXCEEDED"));
    assert!(has_label(&ui, "Collected work exceeds the item bound"));
    assert!(has_label(
        &ui,
        vocabulary::recovery_hint("INPUT_LIMIT_EXCEEDED").unwrap()
    ));
    assert!(has_label(&ui, vocabulary::COLLECTION_INCOMPLETE_NOTICE));
    assert!(has_label(
        &ui,
        &vocabulary::count_label(CountKind::Failed, 0)
    ));
    assert!(has_label(
        &ui,
        &vocabulary::count_label(CountKind::NotProcessed, 4096)
    ));
    assert_eq!(ui.get_items().row_count(), 0);
    // A complete successful report must clear the run-error and discovery notices.
    completed(root.path(), "created");
    click(&ui, "Create copies");
    c.submit(vec![root.path().join("ordinary.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(ui.get_report_diagnostics().row_count(), 0);
    assert!(has_label(
        &ui,
        &vocabulary::count_label(CountKind::Created, 1)
    ));
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
    force_prompt(&ui);
    click(&ui, "Force Stop now");
    pump(&c, || !c.is_busy());
    assert!(has_label(&ui, vocabulary::FORCE_STOP_NOTICE));
    assert!(has_label(&ui, "earlier CLI diagnostic"));
    c.start_export(root.path().join("first.jsonl"));
    pump(&c, || !c.is_busy());
    assert_eq!(
        ui.get_operation_feedback().as_str(),
        vocabulary::REPORT_SAVED
    );
    assert!(has_label(&ui, vocabulary::FORCE_STOP_NOTICE));
    c.start_export(root.path().join("first.jsonl"));
    pump(&c, || !c.is_busy());
    assert_eq!(
        ui.get_operation_feedback().as_str(),
        vocabulary::NO_REPLACEMENT
    );
    script(
        root.path(),
        &("cat >/dev/null\n".to_owned() + &print(&fixture("collection-limit")) + "exit 1\n"),
    );
    click(&ui, "Check only");
    c.submit(vec![root.path().join("overflow")]).unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(ui.get_operation_feedback().as_str(), "");
    assert!(has_label(&ui, "Run error: INPUT_LIMIT_EXCEEDED"));
    assert!(!has_label(&ui, vocabulary::NO_REPLACEMENT));
    assert!(!has_label(&ui, "earlier CLI diagnostic"));
    click(&ui, "Run 1");
    assert!(has_label(&ui, "earlier CLI diagnostic"));
    assert!(has_label(&ui, vocabulary::FORCE_STOP_NOTICE));
    assert!(!has_label(&ui, "Run error: INPUT_LIMIT_EXCEEDED"));
    click(&ui, "Run 2");
    assert!(has_label(&ui, "Run error: INPUT_LIMIT_EXCEEDED"));
    assert!(!has_label(&ui, vocabulary::FORCE_STOP_NOTICE));
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
        c.set_destination(Some(destination.clone()));
        assert_eq!(
            c.window().get_destination_label().as_str(),
            destination.to_str().unwrap()
        );
        assert_eq!(c.window().get_settings_warning(), warning);
        let ui = c.window();
        ui.window().set_size(slint::LogicalSize::new(780.0, 600.0));
        for name in ["first.eml", "second.eml"] {
            c.submit(vec![root.path().join(name)]).unwrap();
            pump(&c, || !c.is_busy());
        }
        mock_elapsed_time(Duration::from_millis(5));
        for label in ["Dismiss report", "Save JSONL report"] {
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
        let prefix = ElementHandle::find_by_accessible_label(&ui, a.path.prefix.as_str())
            .next()
            .unwrap();
        let suffix = ElementHandle::find_by_accessible_label(&ui, a.path.suffix.as_str())
            .next()
            .unwrap();
        assert!(
            prefix.absolute_position().x + prefix.size().width
                <= suffix.absolute_position().x + 0.1,
            "Filename prefix and distinguishing suffix must not overlap"
        );

        assert_eq!(a.path.full.as_str(), sources[0]);
        assert_eq!(b.path.full.as_str(), sources[1]);
        let selected_row = ElementHandle::find_by_accessible_label(&ui, &sources[1])
            .find(|e| e.accessible_role() == Some(AccessibleRole::Button))
            .unwrap();
        click(&ui, &sources[1]);
        assert_eq!(ui.get_item_source().as_str(), sources[1]);
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
    click(&ui, "Check only");
    assert!(!requests.exists());
    click(&ui, "Start selection");
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
    click(&ui, "Clear selection");
    assert!(!ui.get_reviewing());
    assert_eq!(ui.get_selection().row_count(), 0);
    assert_eq!(ui.get_runs().row_count(), 1);
}

#[test]
fn full_queue_retains_review_and_waiting_snapshots_do_not_follow_new_settings() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "created"));
    let ui = c.window();
    for name in ["first.eml", "second.eml"] {
        c.submit(vec![root.path().join(name)]).unwrap();
        pump(&c, || !c.is_busy());
    }
    for index in 0..eml_app::MAX_WAITING_REQUESTS {
        c.submit(vec![root.path().join(format!("waiting-{index}.eml"))])
            .unwrap();
    }
    assert_eq!(ui.get_waiting().row_count(), eml_app::MAX_WAITING_REQUESTS);
    c.stage(vec![root.path().join("retain-on-refusal.eml")])
        .unwrap();
    assert!(matches!(
        c.start_selection(),
        Err(eml_app::QueueError::Full)
    ));
    assert_eq!(ui.get_selection().row_count(), 1);
    assert!(ui.get_reviewing());
    assert!(!ui.get_can_start());
    click(&ui, "Check only");
    c.set_destination(Some(root.path().join("later-destination")));
    let waiting = ui.get_waiting().row_data(0).unwrap();
    assert_eq!(
        waiting.mode.as_str(),
        vocabulary::mode_label(RunMode::Create)
    );
    assert_eq!(waiting.destination.as_str(), "Beside originals");
    ui.invoke_remove_waiting(0);
    assert!(ui.get_can_start());
    assert_eq!(ui.get_selection().row_count(), 1);
}

#[test]
fn footer_actions_share_geometry_and_about_matches_the_reference_layout() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "created"));
    c.submit(vec![root.path().join("source.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    let ui = c.window();
    for (width, font) in [(780.0, 13.0), (1060.0, 13.0), (780.0, 20.0)] {
        ui.set_ui_size(font);
        ui.window().set_size(slint::LogicalSize::new(width, 600.0));
        mock_elapsed_time(Duration::from_millis(5));
        let mut geometry = Vec::new();
        for name in ["Dismiss report", "Save JSONL report", "Show copy"] {
            let e = ElementHandle::find_by_accessible_label(&ui, name)
                .next()
                .unwrap();
            assert!(e.size().height >= 32.0);
            assert!(e.absolute_position().y + e.size().height <= 600.1);
            assert!(e.absolute_position().x + e.size().width <= width + 0.1);
            geometry.push((e.absolute_position().y, e.size().height));
        }
        assert!(
            geometry
                .iter()
                .all(|(top, height)| (top - geometry[0].0).abs() < 0.1
                    && (height - geometry[0].1).abs() < 0.1),
            "{geometry:?}"
        );
    }
    controlled(root.path(), false);
    c.submit(vec![root.path().join("running.eml")]).unwrap();
    wait_started(&c);
    ui.invoke_select_run(0);
    mock_elapsed_time(Duration::from_millis(5));
    let mut active_geometry = Vec::new();
    for name in [
        "Dismiss report",
        "Save JSONL report",
        "Show copy",
        "More run actions",
        "Stop",
    ] {
        let e = ElementHandle::find_by_accessible_label(&ui, name)
            .next()
            .unwrap();
        assert!(
            e.absolute_position().x + e.size().width <= 780.1,
            "{name}: {:?} {:?}",
            e.absolute_position(),
            e.size()
        );
        assert!(e.absolute_position().y + e.size().height <= 600.1);
        active_geometry.push((e.absolute_position().y, e.size().height));
    }
    assert!(
        active_geometry
            .iter()
            .all(|(top, height)| (top - active_geometry[0].0).abs() < 0.1
                && (height - active_geometry[0].1).abs() < 0.1),
        "{active_geometry:?}"
    );
    click(&ui, "Stop");
    pump(&c, || !c.is_busy());
    assert_eq!(ui.get_run_label().as_str(), "Run 1");
    assert_eq!(ui.get_run_status().as_str(), RunStatus::Completed.label());
    assert!(
        !ElementHandle::find_by_accessible_label(&ui, "About")
            .any(|e| e.accessible_role() == Some(AccessibleRole::Button))
    );
    ui.invoke_about();
    mock_elapsed_time(Duration::from_millis(5));
    let about = c.about_window();
    // Native menus sit outside the client area; the testing backend inlines
    // them. Reserve that measured menu height, then verify the reference content.
    let focus = ElementHandle::find_by_element_id(&about, "AboutWindow::about-focus")
        .next()
        .unwrap();
    let menu_height = focus.absolute_position().y;
    about
        .window()
        .set_size(slint::LogicalSize::new(420.0, 410.0 + menu_height));
    mock_elapsed_time(Duration::from_millis(5));
    assert_eq!(focus.size(), slint::LogicalSize::new(420.0, 410.0));
    for label in ["Project website", "Licenses"] {
        assert!(
            ElementHandle::find_by_accessible_label(&about, label)
                .any(|e| e.accessible_role() == Some(AccessibleRole::Button))
        );
    }
    about.invoke_licenses();
    assert!(
        c.license_window()
            .get_licenses()
            .contains("Mozilla Public License")
    );
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
    click(&c.window(), "Check only");
    c.submit(vec![root.path().join("second.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_run_label().as_str(), "Run 2");
    let output = root.path().join("captured.jsonl");
    c.start_captured_export(saved, output.clone());
    pump(&c, || !c.is_busy());
    assert_eq!(fs::read(output).unwrap(), fixture("created").as_bytes());
    assert_eq!(
        c.window().get_operation_label().as_str(),
        "Export for Run 1"
    );
    // Queue a request behind two retained reports, then explicitly dismiss Run 1.
    // The remaining Run 2 must keep the view when waiting work starts.
    c.submit(vec![root.path().join("third.eml")]).unwrap();
    c.window().invoke_select_run(0);
    c.window().invoke_dismiss();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_run_label().as_str(), "Run 2");
    assert_eq!(c.window().get_runs().row_count(), 2);
}
