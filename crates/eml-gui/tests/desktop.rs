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

fn capture_window(ui: &MainWindow, directory: &Path, name: &str) {
    use std::io::Write;
    let pixels = ui.window().take_snapshot().unwrap();
    fs::create_dir_all(directory).unwrap();
    let mut file =
        std::io::BufWriter::new(fs::File::create(directory.join(format!("{name}.ppm"))).unwrap());
    writeln!(file, "P6\n{} {}\n255", pixels.width(), pixels.height()).unwrap();
    for pixel in pixels.as_slice() {
        file.write_all(&[pixel.r, pixel.g, pixel.b]).unwrap();
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

#[path = "desktop/acceptance.rs"]
mod acceptance;
#[path = "desktop/captures.rs"]
mod captures;
#[path = "desktop/navigation.rs"]
mod navigation;
#[path = "desktop/safety.rs"]
mod safety;

fn select_run(ui: &MainWindow, index: usize) {
    ui.invoke_select_session(ui.get_runs().row_data(index).unwrap().token);
}

// Exercise our scrollable workspace and inspector, rather than requiring every
// platform's font metrics to put technical content in the initial viewport.
fn assert_inspector_text_reachable(ui: &MainWindow, text: &str) {
    let viewport = ElementHandle::find_by_accessible_label(ui, "Message details")
        .next()
        .unwrap();
    let window_height = ui.window().size().height as f32 / ui.window().scale_factor();
    if viewport.absolute_position().y + viewport.size().height > window_height {
        // The title has no nested scroller: this wheel gesture reaches the outer
        // workspace at large text sizes, bringing the stacked inspector into view.
        ElementHandle::find_by_accessible_label(ui, ui.get_run_title().as_str())
            .next()
            .unwrap()
            .scroll(0.0, -window_height);
        mock_elapsed_time(Duration::from_millis(180));
    }
    assert!(
        viewport.absolute_position().y >= 0.0
            && viewport.absolute_position().y + viewport.size().height <= window_height,
        "Inspector viewport must be visible before reading: {:?} {:?}, window {window_height}",
        viewport.absolute_position(),
        viewport.size()
    );
    let visible = viewport
        .query_descendants()
        .match_type_name("Flickable")
        .find_first()
        .unwrap();
    let mut reached = false;
    for _ in 0..16 {
        if let Some(line) = ElementHandle::find_by_accessible_label(ui, text).next() {
            let top = line.absolute_position().y;
            let bottom = top + line.size().height;
            let start = visible.absolute_position().y;
            let end = start + visible.size().height;
            if top >= start - 0.1 && bottom <= end + 0.1 {
                reached = true;
                break;
            }
            viewport.scroll(
                0.0,
                if top < start {
                    start - top
                } else {
                    end - bottom
                },
            );
        } else {
            viewport.scroll(0.0, -visible.size().height / 2.0);
        }
        // Slint 1.18.1 wheel animations use this exact virtual-clock duration.
        mock_elapsed_time(Duration::from_millis(180));
    }
    assert!(
        reached,
        "Inspector text must remain fully reachable: {text}; scroll {}",
        ui.get_details_scroll_y()
    );
}
