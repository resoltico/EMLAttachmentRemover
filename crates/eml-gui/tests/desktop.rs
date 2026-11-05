//! Desktop contracts through the real Slint testing backend and stub CLI processes.
use eml_core::{
    RemovalReason, RunMode,
    vocabulary::{self},
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
fn force_prompt(c: &DesktopController) {
    c.stop();
    mock_elapsed_time(Duration::from_secs(5));
    c.tick();
    click(&c.window(), "Force Stop…");
}
fn toggle_attention(ui: &MainWindow) {
    ui.invoke_filter_results(if ui.get_filter() == 4 { 0 } else { 4 });
    mock_elapsed_time(Duration::from_millis(1));
}
fn report_action(ui: &MainWindow, label: &str) {
    click(ui, "More actions");
    click(ui, label);
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
        c.window().get_waiting().row_count(),
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
    assert_eq!(c.window().get_waiting().row_count(), 0);
    assert_eq!(c.window().get_queue_notice(), "");
    c.window().invoke_select_run(0);
    pump(&c, || !c.window().get_report_loading());
    assert_eq!(c.window().get_run_status(), "Done");
    let folder = root.path().join("long-folder-context-".repeat(8));
    fs::create_dir(&folder).unwrap();
    let output = folder.join("Saved-报告-Rīga.jsonl");
    c.start_export(output.clone());
    pump(&c, || !c.is_busy());
    assert_eq!(fs::read(&output).unwrap(), fixture("created").as_bytes());
    assert!(c.window().get_can_reveal_report());
    assert!(
        c.window()
            .get_saved_report_path()
            .full
            .contains("Saved-报告-Rīga.jsonl")
    );
    let receipt = c.window().get_saved_report_path();
    assert_eq!(
        receipt.suffix, "Saved-报告-Rīga.jsonl",
        "The filename must remain visible independently of long folder context"
    );
    assert_eq!(
        receipt.full,
        eml_protocol::TerminalPath::new(&output).to_string()
    );
    c.window().invoke_select_run(8);
    pump(&c, || !c.window().get_report_loading());
    assert!(!c.window().get_can_reveal_report());
    c.window().invoke_select_run(0);
    pump(&c, || !c.window().get_report_loading());
    assert!(c.window().get_can_reveal_report());
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
fn click(ui: &MainWindow, label: &str) {
    mock_elapsed_time(Duration::from_millis(1));
    ElementHandle::find_by_accessible_label(ui, label)
        .find(|element| {
            matches!(
                element.accessible_role(),
                Some(
                    AccessibleRole::Button
                        | AccessibleRole::Checkbox
                        | AccessibleRole::Tab
                        | AccessibleRole::ListItem
                )
            )
        })
        .unwrap_or_else(|| panic!("Missing accessible control: {label}"))
        .invoke_accessible_default_action();
}
fn click_role(ui: &MainWindow, label: &str, role: AccessibleRole) {
    mock_elapsed_time(Duration::from_millis(1));
    ElementHandle::find_by_accessible_label(ui, label)
        .find(|element| element.accessible_role() == Some(role))
        .unwrap_or_else(|| panic!("Missing {role:?}: {label}"))
        .invoke_accessible_default_action();
}
fn start_reviewed_selection(ui: &MainWindow) {
    click_role(ui, ui.get_start_label().as_str(), AccessibleRole::Button);
}
fn choose_operation(ui: &MainWindow, label: &str) {
    click_role(ui, label, AccessibleRole::Tab);
}
fn assert_sidebar(ui: &MainWindow) {
    assert!(
        has_label(ui, "Session navigation"),
        "The session sidebar must remain visible"
    );
}

fn result_caption(ui: &MainWindow, index: usize) -> String {
    let run = ui.get_runs().row_data(index).unwrap();
    format!("{} · {}", run.label, run.mode)
}
fn choose_result(c: &DesktopController, index: usize) {
    let ui = c.window();
    click_role(&ui, &result_caption(&ui, index), AccessibleRole::ListItem);
    pump(c, || !ui.get_report_loading());
}
fn has_label(ui: &MainWindow, label: &str) -> bool {
    ElementHandle::find_by_accessible_label(ui, label)
        .next()
        .is_some()
}
fn wait_started(controller: &DesktopController) {
    pump(controller, || controller.window().get_work_total() == 1);
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
    start_reviewed_selection(&controller.window());
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
    start_reviewed_selection(&controller.window());
    assert_eq!(controller.window().get_waiting().row_count(), 1);
    controller.deliver_drop(root.path().join("cancelled.eml"));
    controller.cancel_drop();
    mock_elapsed_time(Duration::from_millis(1));
    assert_eq!(controller.window().get_waiting().row_count(), 1);
    pump(&controller, || !controller.is_busy());
    assert_eq!(controller.window().get_runs().row_count(), 2);
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
    ui.invoke_select_run(0);
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
    click(&controller.window(), "Cancel quit");
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
    assert!(
        ui.get_operation_feedback()
            .contains(vocabulary::REPORT_SAVED)
    );
    assert!(ui.get_saved_report_path().full.contains("first.jsonl"));
    assert!(
        ui.get_report_diagnostics()
            .iter()
            .any(|line| line.text == vocabulary::FORCE_STOP_NOTICE)
    );
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
    assert!(!has_label(&ui, vocabulary::NO_REPLACEMENT));
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
            ui.get_item_source().as_str(),
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
    choose_operation(&ui, "Check only");
    fs::create_dir(root.path().join("later-destination")).unwrap();
    c.set_destination(Some(root.path().join("later-destination")));
    let waiting = ui.get_waiting().row_data(0).unwrap();
    assert_eq!(
        waiting.mode.as_str(),
        vocabulary::mode_label(RunMode::Create)
    );
    assert_eq!(waiting.destination.as_str(), "Next to originals");
    ui.invoke_remove_waiting(0);
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
    assert_eq!(ui.get_waiting().row_count(), 0);
}

#[test]
fn run_actions_and_the_inspector_have_distinct_workspaces() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.eml");
    let stream = created_stream(std::slice::from_ref(&source));
    let c = controller(
        root.path(),
        script(
            root.path(),
            &("cat >/dev/null\n".to_owned() + &print(&stream)),
        ),
    );
    c.submit(vec![source]).unwrap();
    pump(&c, || !c.is_busy());
    let ui = c.window();
    assert!(has_label(&ui, "More actions"));
    assert!(has_label(&ui, "Show Copy"));
    assert!(
        !has_label(&ui, "Save Report…"),
        "Report actions live in the overflow menu"
    );
    report_action(&ui, "Close Result");
    assert_eq!(ui.get_runs().row_count(), 0);
    assert!(ui.get_reviewing());
    assert_sidebar(&ui);
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
            // One ordinary-button policy across forms and About; reject the old
            // About-only2.5rem height formula without reproducing widget internals.
            let export = c.export_window();
            export.set_ui_size(font);
            export
                .window()
                .set_size(slint::LogicalSize::new(width.max(560.0), height));
            export.show().unwrap();
            mock_elapsed_time(Duration::from_millis(5));
            let export_action = ElementHandle::find_by_accessible_label(&export, "Save report")
                .find(|e| e.accessible_role() == Some(AccessibleRole::Button))
                .unwrap();
            assert!(
                (export_action.size().height - action_geometry[0].1).abs() < 0.1,
                "About and export buttons diverge at font{font}/scale{scale}"
            );
            export.hide().unwrap();
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
        include_str!("../../../LICENSE")
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
    assert_eq!(
        c.window().get_operation_feedback().as_str(),
        eml_app::QueueError::ShuttingDown.to_string()
    );
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
    c.window().invoke_select_run(0);
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
    ui.invoke_select_run(1);
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
    assert_eq!(ui.get_item_source().as_str(), "/mail/message-7.eml");
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
        ] {
            let visible = viewport
                .query_descendants()
                .match_type_name("Flickable")
                .find_first()
                .unwrap();
            let mut reached = false;
            // Scroll through our own bounded inspector content; each wheel gesture
            // advances Slint's pinned 180ms animation on the virtual clock.
            for _ in 0..16 {
                if let Some(line) = ElementHandle::find_by_accessible_label(&ui, text).next() {
                    let top = line.absolute_position().y;
                    let bottom = top + line.size().height;
                    let start = visible.absolute_position().y;
                    let end = start + visible.size().height;
                    if top >= start - 0.1 && bottom <= end + 0.1 {
                        reached = true;
                        break;
                    }
                    let delta = if top < start {
                        start - top
                    } else {
                        end - bottom
                    };
                    viewport.scroll(0.0, delta);
                } else {
                    viewport.scroll(0.0, -visible.size().height / 2.0);
                }
                mock_elapsed_time(Duration::from_millis(180));
            }
            assert!(
                reached,
                "{width}/{font}: inspector text must remain reachable: {text}; scroll {}",
                ui.get_details_scroll_y()
            );
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
    assert_eq!(ui.get_item_source().as_str(), "/mail/message-7.eml");
    assert!(ui.get_messages_scroll_y() < 0.0);
    key(slint::platform::Key::UpArrow);
    assert_eq!(ui.get_selected_item(), 6);
    assert_eq!(
        c.selected_copy().unwrap(),
        Path::new("/mail/message-6.no-attachments.eml")
    );
}

#[test]
fn initial_result_selection_is_visible_without_resizing() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let seed: Vec<serde_json::Value> = fixture("created")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let mut start = seed[0].clone();
    start["total"] = 3.into();
    let mut records = vec![start];
    let mut inputs = Vec::new();
    for index in 0..3 {
        let source = root.path().join(format!("initial-{index}.eml"));
        let destination = root
            .path()
            .join(format!("initial-{index}.no-attachments.eml"));
        let mut item = seed[1].clone();
        item["index"] = index.into();
        for (field, path) in [("source", &source), ("destination", &destination)] {
            item[field] = serde_json::json!({"display":path.to_str().unwrap(), "native":eml_protocol::NativePathValue::new(path)});
        }
        inputs.push(source);
        records.push(item);
    }
    let mut summary = seed[2].clone();
    summary["counts"]["created"] = 3.into();
    records.push(summary);
    let bytes = records
        .iter()
        .map(|record| format!("{record}\n"))
        .collect::<String>();
    let cli = script(
        root.path(),
        &("cat >/dev/null\n".to_owned() + &print(&bytes)),
    );
    let c = DesktopController::new(
        cli,
        root.path().to_owned(),
        Preferences::load(root.path().join("preferences.json")),
    )
    .unwrap();
    c.submit(inputs.clone()).unwrap();
    // Complete I/O before first layout; changing a viewport must not be needed
    // to initialize its geometry. This deliberately avoids the layout pump.
    let deadline = Instant::now() + Duration::from_secs(15);
    while c.is_busy() {
        c.tick();
        assert!(
            Instant::now() < deadline,
            "Fixture did not finish before initial layout"
        );
        thread::sleep(Duration::from_millis(2));
    }
    let ui = c.window();
    assert_eq!(ui.get_items().row_count(), 3);
    ui.window().set_size(slint::LogicalSize::new(960.0, 680.0));
    ui.show().unwrap();
    mock_elapsed_time(Duration::from_millis(5));
    let viewport = ElementHandle::find_by_accessible_label(&ui, "Message results")
        .next()
        .unwrap();
    assert!(viewport.size().height > 0.0);
    let visible = viewport
        .query_descendants()
        .match_type_name("Flickable")
        .find_first()
        .unwrap();
    let selected = ElementHandle::find_by_accessible_label(&ui, inputs[0].to_str().unwrap())
        .find(|e| e.accessible_role() == Some(AccessibleRole::ListItem))
        .unwrap();
    assert_eq!(ui.get_selected_item(), 0);
    assert!(selected.absolute_position().y >= visible.absolute_position().y - 0.1);
    assert!(
        selected.absolute_position().y + selected.size().height
            <= visible.absolute_position().y + visible.size().height + 0.1
    );
}

fn mixed_check_after_empty_active_layout(root: &Path) -> (Rc<DesktopController>, Vec<PathBuf>) {
    let planned: Vec<serde_json::Value> = fixture("planned-create-unix")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let failed: Vec<serde_json::Value> = fixture("prepublication-failure")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let mut start = planned[0].clone();
    start["total"] = 3.into();
    let mut records = vec![start];
    let mut inputs = Vec::new();
    for index in 0..3 {
        let source = root.join(format!("mixed-{index}.eml"));
        let destination = root.join(format!("mixed-{index}.no-attachments.eml"));
        let mut item = if index == 1 {
            failed[1].clone()
        } else {
            planned[1].clone()
        };
        item["index"] = index.into();
        if index == 1 {
            item["error"]["code"] = "PROTECTED".into();
            item["error"]["detail"] = "Digitally signed message cannot be changed".into();
        }
        for (field, path) in [("source", &source), ("destination", &destination)] {
            item[field] = serde_json::json!({"display":path.to_str().unwrap(),"native":eml_protocol::NativePathValue::new(path)});
        }
        inputs.push(source);
        records.push(item);
    }
    let mut summary = planned[2].clone();
    summary["counts"]["planned"] = 2.into();
    summary["counts"]["failed"] = 1.into();
    summary["exit_code"] = 1.into();
    records.push(summary);
    let bytes = records
        .iter()
        .map(|record| format!("{record}\n"))
        .collect::<String>();
    let gate = root.join("release-mixed-report");
    let cli = script(
        root,
        &format!(
            "cat >/dev/null\nwhile [ ! -f {} ]; do :; done\n{}exit 1\n",
            quote(gate.to_str().unwrap()),
            print(&bytes)
        ),
    );
    let c = DesktopController::new(
        cli,
        root.to_owned(),
        Preferences::load(root.join("preferences.json")),
    )
    .unwrap();
    let ui = c.window();
    ui.window().set_size(slint::LogicalSize::new(960.0, 680.0));
    ui.show().unwrap();
    mock_elapsed_time(Duration::from_millis(5));
    ui.invoke_mode_changed(true);
    c.submit(inputs.clone()).unwrap();
    // Render the active report's empty list before the controlled producer
    // releases its three results, matching discovery followed by completion.
    mock_elapsed_time(Duration::from_millis(5));
    assert_eq!(ui.get_items().row_count(), 0);
    fs::write(gate, b"release").unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while c.is_busy() {
        c.tick();
        assert!(
            Instant::now() < deadline,
            "Mixed fixture did not finish after its discovery layout"
        );
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(ui.get_filter(), 4);
    assert_eq!(ui.get_items().row_count(), 1);
    assert_eq!(
        ui.get_selected_item(),
        1,
        "Initial attention selects the protected middle row"
    );
    mock_elapsed_time(Duration::from_millis(5));
    (c, inputs)
}

#[test]
fn fitted_mixed_result_keeps_all_rows_visible_with_a_middle_selection() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let (c, inputs) = mixed_check_after_empty_active_layout(root.path());
    let ui = c.window();
    ui.invoke_filter_results(0);
    let viewport = ElementHandle::find_by_accessible_label(&ui, "Message results")
        .next()
        .unwrap();
    assert!(
        viewport.size().height >= 102.0 - 0.1,
        "All three fixed 34px rows fit"
    );
    assert_eq!(
        ui.get_messages_scroll_y(),
        0.0,
        "Fitted3-row viewport {} must not scroll past its first row",
        viewport.size().height
    );
    for path in inputs {
        let row = ElementHandle::find_by_accessible_label(&ui, path.to_str().unwrap())
            .find(|e| e.accessible_role() == Some(AccessibleRole::ListItem))
            .unwrap();
        assert!(row.absolute_position().y >= viewport.absolute_position().y - 0.1);
        assert!(
            row.absolute_position().y + row.size().height
                <= viewport.absolute_position().y + viewport.size().height + 0.1
        );
    }
}

#[test]
fn fitted_mixed_result_attention_filter_keeps_its_only_row_visible() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let (c, inputs) = mixed_check_after_empty_active_layout(root.path());
    let ui = c.window();
    ui.invoke_filter_results(0);
    toggle_attention(&ui);
    mock_elapsed_time(Duration::from_millis(5));
    assert_eq!(ui.get_items().row_count(), 1);
    assert_eq!(
        ui.get_selected_item(),
        1,
        "Filtering retains original report identity"
    );
    assert_eq!(ui.get_selected_visible_row(), 0);
    let viewport = ElementHandle::find_by_accessible_label(&ui, "Message results")
        .next()
        .unwrap();
    assert!(viewport.size().height > 0.0);
    assert_eq!(
        ui.get_messages_scroll_y(),
        0.0,
        "One-row filtered viewport {} must not contain a stale negative offset",
        viewport.size().height
    );
    let row = ElementHandle::find_by_accessible_label(&ui, inputs[1].to_str().unwrap())
        .find(|e| e.accessible_role() == Some(AccessibleRole::ListItem))
        .unwrap_or_else(|| {
            panic!(
                "Filtered model has one row but its AX list is empty: y{}, viewport{}",
                ui.get_messages_scroll_y(),
                viewport.size().height
            )
        });
    assert!(row.absolute_position().y >= viewport.absolute_position().y - 0.1);
    assert!(
        row.absolute_position().y + row.size().height
            <= viewport.absolute_position().y + viewport.size().height + 0.1
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
    assert_eq!(ui.get_item_source(), inputs[2].to_str().unwrap());
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
            assert!(has_label(
                &ui,
                "A complete copy was published; qualification did not complete. Inspect the destination before retrying."
            ));
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
    assert_eq!(ui.get_waiting().row_count(), 0);
    c.stop();
    pump(&c, || !c.is_busy());
    assert_eq!(fs::read(prefs).unwrap(), b"{broken owned settings");
}

#[test]
fn export_naming_retries_exclusively_keeps_capture_and_rejects_stale_folder_choices() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "created"));
    c.submit(vec![root.path().join("one.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    c.begin_export();
    let window = c.export_window();
    window.set_file_name("../escape.jsonl".into());
    window.invoke_create_report();
    assert!(!c.is_busy());
    assert!(window.get_notice().contains("one file name"));
    let stale = c.capture_export_folder().unwrap();
    c.cancel_export();
    c.begin_export();
    let original_folder = window.get_directory();
    c.set_export_directory(&stale, root.path().join("wrong"));
    assert_eq!(window.get_directory(), original_folder);
    let existing = root.path().join("existing.jsonl");
    fs::write(&existing, b"preserved report").unwrap();
    window.set_file_name("existing.jsonl".into());
    window.invoke_create_report();
    pump(&c, || !c.is_busy());
    assert!(!window.get_busy());
    assert!(window.get_notice().contains("never replaced"));
    assert_eq!(fs::read(&existing).unwrap(), b"preserved report");
    completed(root.path(), "created-attachment");
    c.submit(vec![root.path().join("two.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    window.set_file_name("captured.jsonl".into());
    window.invoke_create_report();
    pump(&c, || !c.is_busy());
    assert_eq!(
        fs::read_to_string(root.path().join("captured.jsonl")).unwrap(),
        fixture("created")
    );
    c.begin_export();
    let late = c.capture_export_folder().unwrap();
    assert!(c.request_quit());
    c.set_export_directory(&late, root.path().join("late"));
    window.set_file_name("late.jsonl".into());
    window.invoke_create_report();
    assert!(!root.path().join("late.jsonl").exists());
    assert!(!c.is_busy());
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
    assert_ne!(c.window().get_item_source().as_str(), source);
    assert!(c.window().get_can_copy_source() && c.window().get_can_copy_destination());
    assert!(!c.window().get_can_reveal());
    assert!(c.selected_copy().is_none());
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
    ui.invoke_select_run(1);
    fs::write(gate, b"finish").unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(ui.get_run_label(), "Run 2");
    assert!(ui.get_runs().row_data(2).unwrap().unseen);
    ui.invoke_select_run(2);
    pump(&c, || !ui.get_report_loading());
    assert!(!ui.get_runs().row_data(2).unwrap().unseen);
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

fn capture_window(ui: &MainWindow, directory: &Path, name: &str) {
    use std::io::Write;
    let pixels = ui.window().take_snapshot().unwrap();
    fs::create_dir_all(directory).unwrap();
    let mut file = fs::File::create(directory.join(format!("{name}.ppm"))).unwrap();
    writeln!(file, "P6\n{} {}\n255", pixels.width(), pixels.height()).unwrap();
    for pixel in pixels.as_slice() {
        file.write_all(&[pixel.r, pixel.g, pixel.b]).unwrap();
    }
}

#[test]
#[ignore = "Manual production-view screenshot capture; set GUI_CAPTURE_DIR"]
fn capture_redesign_states_in_light_and_dark() {
    use slint::language::ColorScheme;
    let directory =
        PathBuf::from(std::env::var_os("GUI_CAPTURE_DIR").expect("set GUI_CAPTURE_DIR"));
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            mock_time: true,
            renderer_name: Some("software".into()),
            ..Default::default()
        },
    )))
    .unwrap();
    for (scheme, color) in [("light", ColorScheme::Light), ("dark", ColorScheme::Dark)] {
        let root = tempfile::tempdir().unwrap();
        let c = controller(root.path(), completed(root.path(), "created"));
        let ui = c.window();
        ui.global::<eml_gui::Palette>().set_color_scheme(color);
        assert_eq!(ui.global::<eml_gui::Theme>().get_dark(), scheme == "dark");
        ui.window().set_size(slint::LogicalSize::new(1080.0, 700.0));
        mock_elapsed_time(Duration::from_millis(5));
        capture_window(&ui, &directory, &format!("A-{scheme}"));
        let folder = root.path().join("Inbox");
        fs::create_dir(&folder).unwrap();
        let file = root.path().join("Quarterly invoice.eml");
        fs::write(&file, b"From: sender@example.com\r\n\r\nMessage body\r\n").unwrap();
        c.stage(vec![folder.clone(), file.clone()]).unwrap();
        mock_elapsed_time(Duration::from_millis(5));
        capture_window(&ui, &directory, &format!("B-{scheme}"));
        let mut sources: Vec<_> = (1..=14)
            .map(|n| folder.join(format!("Quarterly report {n}.eml")))
            .collect();
        sources[10] = folder.join("06-signed-message.eml");
        sources[11] = folder.join("09-attachments-only.eml");
        sources[12] = folder.join("14-unusual-format.eml");
        sources.push(file);
        let stream = mixed_create_stream(&sources);
        let lines: Vec<_> = stream.lines().collect();
        let leading = lines[..7].join("\n") + "\n";
        let remaining = lines[7..].join("\n") + "\n";
        let gate = root.path().join("finish-run");
        script(
            root.path(),
            &format!(
                "cat >/dev/null\n{}while [ ! -f {} ]; do :; done\n{}exit 1\n",
                print(&leading),
                quote(gate.to_str().unwrap()),
                print(&remaining)
            ),
        );
        c.start_selection().unwrap();
        pump(&c, || ui.get_work_completed() == 6);
        capture_window(&ui, &directory, &format!("C-{scheme}"));
        fs::write(gate, b"finish").unwrap();
        pump(&c, || !c.is_busy());
        capture_window(&ui, &directory, &format!("D-{scheme}"));
        ui.hide().unwrap();
    }
}

fn created_stream(sources: &[PathBuf]) -> String {
    let records: Vec<serde_json::Value> = fixture("created")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let mut start = records[0].clone();
    start["total"] = sources.len().into();
    let mut stream = format!("{start}\n");
    for (index, source) in sources.iter().enumerate() {
        let mut item = records[1].clone();
        item["index"] = index.into();
        for (field, path) in [
            ("source", source.clone()),
            ("destination", source.with_extension("no-attachments.eml")),
        ] {
            item[field] = serde_json::json!({"display":path.to_str().unwrap(),"native":eml_protocol::NativePathValue::new(&path)});
        }
        stream.push_str(&format!("{item}\n"));
    }
    let mut summary = records[2].clone();
    summary["counts"]["created"] = sources.len().into();
    stream + &format!("{summary}\n")
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
    ui.invoke_select_run(0);
    pump(&c, || !ui.get_report_loading());
    assert_eq!(ui.get_filter(), 4);
    assert_eq!(ui.get_selected_item(), 1);
    assert_eq!(ui.get_sort_descending(), descending);
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

fn press_key(ui: &MainWindow, text: impl Into<slint::SharedString>) {
    let text = text.into();
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyReleased { text });
    mock_elapsed_time(Duration::from_millis(1));
}
fn chord(
    ui: &MainWindow,
    modifier: impl Into<slint::SharedString>,
    key: impl Into<slint::SharedString>,
) {
    let modifier = modifier.into();
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed {
            text: modifier.clone(),
        });
    press_key(ui, key);
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::KeyReleased { text: modifier });
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
    assert_eq!(ui.get_waiting().row_count(), 1);
    assert_eq!(ui.get_waiting().row_data(0).unwrap().token, third);
    ui.invoke_close_session(third);
    assert_eq!(ui.get_waiting().row_count(), 0);
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

fn mixed_create_stream(sources: &[PathBuf]) -> String {
    let created: Vec<serde_json::Value> = fixture("created-attachment")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let failed: Vec<serde_json::Value> = fixture("prepublication-failure")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let mut start = created[0].clone();
    start["total"] = sources.len().into();
    let mut stream = format!("{start}\n");
    for (index, source) in sources.iter().enumerate() {
        let error = match index {
            10 => Some((
                "PROTECTED",
                "Signed or encrypted content cannot be reduced safely",
            )),
            11 => Some(("NO_BODY", "This message has only attachments")),
            12 => Some((
                "UNSUPPORTED_MIME",
                "This message format cannot be reduced safely",
            )),
            _ => None,
        };
        let mut item = if error.is_some() {
            failed[1].clone()
        } else {
            created[1].clone()
        };
        item["index"] = index.into();
        if let Some((code, detail)) = error {
            item["error"]["code"] = code.into();
            item["error"]["detail"] = detail.into();
        }
        for (field, path) in [
            ("source", source.clone()),
            ("destination", source.with_extension("no-attachments.eml")),
        ] {
            item[field] = serde_json::json!({"display":path.to_str().unwrap(),"native":eml_protocol::NativePathValue::new(&path)});
        }
        stream.push_str(&format!("{item}\n"));
    }
    let mut summary = created[2].clone();
    summary["counts"]["created"] = (sources.len() - 3).into();
    summary["counts"]["failed"] = 3.into();
    summary["exit_code"] = 1.into();
    stream + &format!("{summary}\n")
}
