//! Core-backed rendering, actions, lifecycle and retained layout requirements.
use super::*;
use eml_gui::{BatchState, MainWindow, Palette};
use i_slint_backend_testing::{ElementHandle, ElementRoot};
use slint::Model;

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

fn select_first(c: &DesktopController) {
    let row = c.window().get_rows().row_data(0).unwrap();
    if !row.selected {
        c.window().invoke_select_email(row.key);
    }
}

#[test]
fn s13_native_resize_constraints_admit_the_declared_minimum_and_large_windows() {
    use i_slint_core::{item_tree::ItemTreeRc, layout::Orientation, window::WindowInner};
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    for name in ["one.eml", "two.eml", "three.eml"] {
        write(root.path(), name, ATTACHMENT);
    }
    let c = controller(root.path(), eml_core::run);
    let ui = c.window();
    for phase in 0..3 {
        if phase == 1 {
            c.stage(vec![root.path().to_owned()]).unwrap();
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
fn s18_admission_notice_is_visible_in_the_empty_window_and_expires() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), eml_core::run);
    c.open_documents(
        (0..4097)
            .map(|n| root.path().join(format!("{n}.eml")))
            .collect(),
    );
    c.tick();
    let text = "Too many input paths. Add at most 4096 files or folders at once.";
    assert!(labels(&c.window()).iter().any(|label| label == text));
    mock_elapsed_time(Duration::from_secs(6));
    c.tick();
    assert!(c.window().get_view().notice.is_empty());
    assert!(!labels(&c.window()).iter().any(|label| label == text));
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
    let c = controller(root.path(), eml_core::run);
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
    ui.invoke_select_email(
        ui.get_rows()
            .iter()
            .find(|r| r.name == "one.eml")
            .unwrap()
            .key,
    );
    mock_elapsed_time(Duration::from_millis(1));
    let mut detail = ui.get_detail();
    let part = detail.parts.row_data(0).unwrap();
    let mut heights = Vec::new();
    for count in [0, 1, 5] {
        detail.parts = Rc::new(slint::VecModel::from(vec![part.clone(); count])).into();
        ui.set_detail(detail.clone());
        mock_elapsed_time(Duration::from_millis(1));
        let name = label(&ui, "one.eml");
        for action in ["Show Copy", "Show Original", "Copy Details"] {
            assert!(
                (name.absolute_position().y - label(&ui, action).absolute_position().y).abs() <= 1.,
                "{action} is not beside the name"
            );
        }
        let viewport = label(&ui, "Emails")
            .query_descendants()
            .match_type_name("Flickable")
            .find_first()
            .unwrap();
        heights.push(viewport.size().height);
    }
    assert!(
        heights.iter().all(|h| (h - heights[0]).abs() <= 1.),
        "{heights:?}"
    );
    ui.window().set_size(slint::LogicalSize::new(760., 520.));
    mock_elapsed_time(Duration::from_millis(1));
    let list = label(&ui, "Emails");
    let viewport = list
        .query_descendants()
        .match_type_name("Flickable")
        .find_first()
        .unwrap();
    let top = viewport.absolute_position().y;
    let bottom = top + viewport.size().height;
    let complete = list
        .query_descendants()
        .find_all()
        .into_iter()
        .filter(|e| {
            e.accessible_role() == Some(i_slint_backend_testing::AccessibleRole::ListItem)
                && e.absolute_position().y >= top - 0.1
                && e.absolute_position().y + e.size().height <= bottom + 0.1
        })
        .count();
    assert!(
        complete >= 3,
        "Only {complete} complete rows in {:?}",
        viewport.size()
    );
}

#[test]
fn v6_status_header_aligns_with_text() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = done(root.path(), &["one.eml"]);
    let ui = c.window();
    mock_elapsed_time(Duration::from_millis(1));
    for (heading, text) in [
        ("Email", "one"),
        ("Attachments", "report.pdf"),
        ("Size", "1.0 kB → 513 B"),
        ("Status", "Copy saved"),
    ] {
        let header = label(&ui, heading);
        let cell = label(&ui, text);
        assert!(
            (header.absolute_position().x - cell.absolute_position().x).abs() <= 1.,
            "{heading}: header={:?}, cell={:?}",
            header.absolute_position(),
            cell.absolute_position()
        );
    }
}

pub(super) fn gate(root: &Path) -> (Rc<DesktopController>, Release) {
    write(root, "one.eml", ATTACHMENT);
    write(root, "two.eml", PLAIN);
    write(root, "three.eml", b"");
    let (release, held) = latch();
    let c = controller(root, move |r, control, sink| {
        run_with(r, control, sink, |event, target| {
            let start = r.mode == RunMode::Create && matches!(event, Event::Start(_));
            target.emit(event)?;
            if start {
                held.wait();
            }
            Ok(())
        })
    });
    c.stage(vec![root.to_owned()]).unwrap();
    pump(&c, || !c.is_busy());
    c.start_selection();
    pump(&c, || {
        c.window()
            .get_view()
            .progress_text
            .starts_with("0 of 3 done")
    });
    (c, release)
}
#[test]
fn w3_real_core_rows_and_headlines_for_check_done_and_rerun() {
    use eml_core::{SettledItem, vocabulary::HeadlineMode};
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "one.eml", ATTACHMENT);
    write(root.path(), "plain.eml", PLAIN);
    write(root.path(), "invalid.eml", b"");
    let observations = Arc::new(Mutex::new(Vec::new()));
    let runs = observations.clone();
    let c = controller(root.path(), move |r, control, sink| {
        let mut items = Vec::new();
        let outcome = run_with(r, control, sink, |e, t| {
            if let Event::Item(i) = &e {
                items.push(SettledItem::from(*i));
            }
            t.emit(e)
        })?;
        runs.lock().unwrap().push((r.mode, items, outcome.clone()));
        Ok(outcome)
    });
    for _ in 0..2 {
        c.stage(vec![root.path().to_owned()]).unwrap();
        pump(&c, || !c.is_busy());
        for create in [false, true] {
            if create {
                c.start_selection();
                pump(&c, || !c.is_busy());
            }
            let runs = observations.lock().unwrap();
            let (mode, items, outcome) = runs.last().unwrap();
            let h = outcome
                .settled_totals(items)
                .headline(if *mode == RunMode::DryRun {
                    HeadlineMode::Check {
                        total: Some(outcome.total()),
                    }
                } else {
                    HeadlineMode::Done {
                        incomplete_create: false,
                    }
                });
            assert_eq!(c.window().get_view().title, h.verdict);
            assert_eq!(c.window().get_view().summary, h.parts.join(" · "));
            assert_eq!(c.window().get_rows().row_count(), items.len());
            for item in items {
                let name = eml_core::display::DisplayPath::new(Path::new(
                    item.source.file_name().unwrap(),
                ))
                .to_string();
                let row = c
                    .window()
                    .get_rows()
                    .iter()
                    .find(|r| r.name == name)
                    .unwrap();
                assert_eq!(row.status, item.outcome().label());
            }
        }
    }
}
#[test]
fn w6_all_existing_has_no_repeated_part() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = done(root.path(), &["one.eml", "two.eml"]);
    c.stage(vec![root.path().to_owned()]).unwrap();
    pump(&c, || !c.is_busy());
    c.start_selection();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_view().title, "2 copies already exist");
    assert!(c.window().get_view().summary.is_empty());
}
#[test]
fn one_list_s1_add_starts_exactly_one_read_only_request() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let file = write(root.path(), "source.eml", ATTACHMENT);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let called = calls.clone();
    let c = controller(root.path(), move |r, control, sink| {
        called.lock().unwrap().push(r.clone());
        eml_core::run(r, control, sink)
    });
    c.stage(vec![file]).unwrap();
    pump(&c, || !c.is_busy());
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].mode, RunMode::DryRun);
    assert_eq!(calls[0].output, eml_core::OutputLocation::BesideSource);
    assert!(!root.path().join("source.no-attachments.eml").exists());
}
#[test]
fn s2_changes_cancel_a_check_and_coalesce_pending_work_without_blocking_create() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let first = write(root.path(), "first.eml", ATTACHMENT);
    let second = write(root.path(), "second.eml", ATTACHMENT);
    let third = write(root.path(), "third.eml", ATTACHMENT);
    let (release, held) = latch();
    let started = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = started.clone();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let called = calls.clone();
    let c = controller(root.path(), move |r, control, sink| {
        called.lock().unwrap().push(r.clone());
        if r.paths.len() == 1 && r.mode == RunMode::DryRun {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
            held.wait();
        }
        eml_core::run(r, control, sink)
    });
    c.stage(vec![first]).unwrap();
    pump(&c, || started.load(std::sync::atomic::Ordering::SeqCst));
    c.stage(vec![second]).unwrap();
    c.stage(vec![third]).unwrap();
    c.tick();
    assert_eq!(calls.lock().unwrap().len(), 1);
    c.start_selection();
    pump(&c, || c.window().get_view().state == BatchState::Done);
    assert_eq!(calls.lock().unwrap().len(), 2);
    assert_eq!(calls.lock().unwrap()[1].mode, RunMode::Create);
    assert_eq!(calls.lock().unwrap()[1].paths.len(), 3);
    release.release();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_view().title, "3 copies saved");
}
#[test]
fn w5_obsolete_check_late_results_are_ignored() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let old = write(root.path(), "old.eml", ATTACHMENT);
    let current = write(root.path(), "current.eml", PLAIN);
    let (release, held) = latch();
    let entered = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let signal = entered.clone();
    let c = controller(root.path(), move |r, control, sink| {
        if r.paths.iter().any(|p| p.ends_with("old.eml")) {
            signal.store(true, std::sync::atomic::Ordering::SeqCst);
            held.wait();
            // Model a read-only call returning late despite cancellation.
            eml_core::run(r, &RunControl::default(), sink)
        } else {
            eml_core::run(r, control, sink)
        }
    });
    c.stage(vec![old]).unwrap();
    pump(&c, || entered.load(std::sync::atomic::Ordering::SeqCst));
    c.start_over();
    c.stage(vec![current]).unwrap();
    release.release();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_rows().row_count(), 1);
    assert_eq!(
        c.window().get_rows().row_data(0).unwrap().name,
        "current.eml"
    );
    assert_eq!(
        c.window().get_rows().row_data(0).unwrap().status,
        "No attachments"
    );
}
#[test]
fn s3_remove_during_check_keeps_the_native_request() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let input = write(root.path(), "mail.eml", ATTACHMENT);
    let (release, held) = latch();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let called = calls.clone();
    let c = controller(root.path(), move |r, control, sink| {
        called.lock().unwrap().push(r.clone());
        if r.mode == RunMode::DryRun {
            held.wait();
        }
        eml_core::run(r, control, sink)
    });
    c.stage(vec![input.clone()]).unwrap();
    pump(&c, || calls.lock().unwrap().len() == 1);
    c.start_selection();
    pump(&c, || c.window().get_view().state == BatchState::Done);
    let calls = calls.lock().unwrap();
    assert_eq!(calls[0].paths, calls[1].paths);
    assert_eq!(calls[0].output, calls[1].output);
    assert_eq!(calls[1].mode, RunMode::Create);
    drop(calls);
    release.release();
    assert_eq!(fs::read(input).unwrap(), ATTACHMENT);
}
#[test]
fn s4_s15_inputs_are_refused_during_create_and_replace_a_completed_batch() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let (c, release) = gate(root.path());
    let before = c.window().get_rows().row_count();
    let another = write(root.path(), "another.eml", PLAIN);
    c.stage(vec![another.clone()]).unwrap();
    c.open_documents(vec![another.clone()]);
    assert_eq!(c.window().get_rows().row_count(), before);
    assert!(!c.inputs_enabled());
    release.release();
    pump(&c, || !c.is_busy());
    c.open_documents(vec![another]);
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_rows().row_count(), 1);
    assert_eq!(
        c.window().get_rows().row_data(0).unwrap().name,
        "another.eml"
    );
}
#[test]
fn w5_stop_is_cooperative_idempotent_and_continue_uses_core_totals() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let (c, release) = gate(root.path());
    c.stop();
    let stopped = c.window().get_view().stopping;
    assert!(stopped);
    assert!(!c.window().get_view().can_stop);
    mock_elapsed_time(Duration::from_millis(10));
    c.stop();
    release.release();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_view().state, BatchState::Stopped);
    assert_eq!(c.window().get_view().title, "Stopped");
    assert_eq!(c.window().get_view().summary, "3 not processed");
    assert_eq!(c.window().get_view().primary, "Continue");
    c.start_selection();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_view().state, BatchState::Done);
    assert_eq!(c.window().get_view().title, "2 copies saved");
    assert_eq!(fs::read(root.path().join("one.eml")).unwrap(), ATTACHMENT);
}
#[test]
fn s9_stop_and_quit_waits_for_settle_and_quit_now_honors_five_seconds() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let (c, release) = gate(root.path());
    assert!(!c.request_quit());
    assert!(c.window().get_view().quit);
    c.cancel_quit();
    assert!(!c.window().get_view().quit);
    assert!(c.is_busy());
    c.request_quit();
    c.stop_and_quit();
    c.tick();
    assert!(!c.should_exit());
    assert!(!c.window().get_view().quit_now);
    mock_elapsed_time(Duration::from_millis(4999));
    c.tick();
    assert!(!c.window().get_view().quit_now);
    mock_elapsed_time(Duration::from_millis(1));
    c.tick();
    assert!(c.window().get_view().quit_now);
    c.quit_now();
    assert!(c.should_exit());
    release.release();
    pump(&c, || !c.is_busy());
    let root = tempfile::tempdir().unwrap();
    let (c, release) = gate(root.path());
    c.request_quit();
    c.stop_and_quit();
    assert!(!c.should_exit());
    release.release();
    pump(&c, || c.should_exit());
    assert!(!c.is_busy());
}
#[test]
fn s9_check_only_quit_never_awaits_the_read_only_thread() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let input = write(root.path(), "mail.eml", ATTACHMENT);
    let (release, held) = latch();
    let entered = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = entered.clone();
    let c = controller(root.path(), move |r, control, sink| {
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
        held.wait();
        eml_core::run(r, control, sink)
    });
    c.stage(vec![input]).unwrap();
    pump(&c, || entered.load(std::sync::atomic::Ordering::SeqCst));
    assert!(c.request_quit());
    assert!(c.should_exit());
    assert!(!c.window().get_view().quit);
    release.release();
}
#[test]
fn w5_panic_retains_report_prefix_and_a_responsive_controller() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let input = write(root.path(), "mail.eml", ATTACHMENT);
    let c = controller(root.path(), |r, control, sink| {
        run_with(r, control, sink, |e, t| {
            let start = r.mode == RunMode::Create && matches!(e, Event::Start(_));
            t.emit(e)?;
            assert!(!start, "scripted runner panic");
            Ok(())
        })
    });
    c.stage(vec![input]).unwrap();
    pump(&c, || !c.is_busy());
    c.start_selection();
    pump(&c, || !c.is_busy());
    assert_eq!(
        c.window().get_view().banner,
        "Processing stopped unexpectedly. Results shown may be incomplete. Save Report… for details."
    );
    assert_eq!(
        c.window().get_rows().row_data(0).unwrap().status,
        "Result unknown"
    );
    let target = root.path().join("panic.jsonl");
    c.start_export(target.clone());
    pump(&c, || !c.is_busy());
    let bytes = fs::read(target).unwrap();
    assert!(
        !bytes.is_empty(),
        "catch_unwind must preserve the report prefix"
    );
    assert!(bytes.starts_with(b"{\"type\":\"start\""));
    assert_eq!(bytes.iter().filter(|b| **b == b'\n').count(), 1);
    c.start_over();
    assert_eq!(c.window().get_view().state, BatchState::Empty);
    assert!(c.inputs_enabled());
}
#[test]
fn w7_details_come_only_from_typed_whole_run_failure() {
    use eml_core::vocabulary::fields;
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = ready(root.path(), &["good.eml"]);
    assert!(c.window().get_view().diagnostics.is_empty());
    assert!(
        ElementHandle::find_by_accessible_label(&c.window(), "Details…")
            .next()
            .is_none()
    );
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), eml_core::run);
    c.stage(vec![root.path().to_owned()]).unwrap();
    pump(&c, || !c.is_busy());
    let text = c.window().get_view().diagnostics;
    assert_eq!(
        text,
        format!(
            "{}: NO_INPUTS\n{}: collection\n{}: Discovery found no work items\n",
            fields::CODE,
            fields::PHASE,
            fields::DETAIL
        )
    );
    label(&c.window(), "Details…").mock_single_click(slint::platform::PointerEventButton::Left);
    assert_eq!(
        label(&c.window(), "Processing details")
            .accessible_value()
            .unwrap(),
        text
    );
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "mail.eml", ATTACHMENT);
    let invalid = root.path().join("missing-folder");
    let c = controller(root.path(), move |r, control, sink| {
        let mut r = r.clone();
        r.output = eml_core::OutputLocation::Directory(invalid.clone());
        eml_core::run(&r, control, sink)
    });
    c.stage(vec![root.path().join("mail.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    let text = c.window().get_view().diagnostics;
    assert!(text.contains("Code: BIND_FAILED\nPhase: admission\nDetail: Cannot bind the existing output directory. (OS error 2)"),"{text}");
}

#[test]
fn s5_s10_s14_real_missing_invalid_and_qualified_rows_keep_native_selection() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "one.eml", ATTACHMENT);
    write(root.path(), "bad.eml", b"");
    write(root.path(), "plain.eml", PLAIN);
    let c = controller(root.path(), eml_core::run);
    c.stage(vec![root.path().to_owned()]).unwrap();
    pump(&c, || !c.is_busy());
    let good = c
        .window()
        .get_rows()
        .iter()
        .find(|r| r.name == "one.eml")
        .unwrap();
    c.window().invoke_select_email(good.key);
    assert_eq!(c.selected_original(), Some(root.path().join("one.eml")));
    assert!(c.selected_copy().is_none());
    c.start_selection();
    pump(&c, || !c.is_busy());
    assert_eq!(
        c.selected_copy(),
        Some(root.path().join("one.no-attachments.eml"))
    );
    assert!(c.window().get_detail().can_show_copy);
    let bad = c
        .window()
        .get_rows()
        .iter()
        .find(|r| r.name == "bad.eml")
        .unwrap();
    c.window().invoke_select_email(bad.key);
    assert!(!c.window().get_detail().can_show_copy);
    assert!(c.clipboard_text().unwrap().contains("Code: MALFORMED"));
    c.start_over();
    c.stage(vec![root.path().join("missing.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(
        c.window().get_rows().row_data(0).unwrap().status,
        "Can’t be read"
    );
    select_first(&c);
    assert!(c.clipboard_text().unwrap().contains("OS error 2"));
    assert!(!c.window().get_view().can_start);
}
#[test]
fn s6_s7_t8_filter_uses_core_outcomes_and_announces_count() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "bad1.eml", b"");
    write(root.path(), "bad2.eml", b"");
    for n in 0..34 {
        write(root.path(), &format!("mail{n}.eml"), PLAIN);
    }
    let c = controller(root.path(), eml_core::run);
    c.stage(vec![root.path().to_owned()]).unwrap();
    pump(&c, || !c.is_busy());
    let original = c.window().get_view().summary;
    c.window().invoke_toggle_problems();
    assert_eq!(c.window().get_rows().row_count(), 2);
    assert!(c.window().get_view().summary.starts_with("Showing 2 of 36"));
    label(&c.window(), "Show all");
    select_first(&c);
    c.window().invoke_toggle_problems();
    assert_eq!(c.window().get_rows().row_count(), 36);
    assert_eq!(c.window().get_view().summary, original);
    c.start_selection();
    pump(&c, || !c.is_busy());
    c.window().invoke_toggle_problems();
    assert_eq!(c.window().get_rows().row_count(), 2);
    assert_eq!(c.window().get_view().title, "34 copies saved");
}
#[test]
fn s7_filter_stays_until_successes_replace_problems() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "bad.eml", b"");
    write(root.path(), "good.eml", PLAIN);
    let creates = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls = creates.clone();
    let c = controller(root.path(), move |r, control, sink| {
        let first = r.mode == RunMode::Create
            && calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0;
        run_with(r, control, sink, |e, t| {
            let item = matches!(e, Event::Item(_));
            t.emit(e)?;
            if first && item {
                control.request_stop();
            }
            Ok(())
        })
    });
    c.stage(vec![root.path().to_owned()]).unwrap();
    pump(&c, || !c.is_busy());
    c.start_selection();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_view().state, BatchState::Stopped);
    c.window().invoke_toggle_problems();
    assert!(c.window().get_view().filtered);
    write(root.path(), "bad.eml", PLAIN);
    c.start_selection();
    pump(&c, || !c.is_busy());
    assert!(!c.window().get_view().filtered);
    assert_eq!(c.window().get_rows().row_count(), 2);
}

#[test]
fn s11_removed_concepts_are_absent_in_ready_removing_done_and_quit() {
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
    let check = |ui: &MainWindow| {
        for text in labels(ui) {
            assert!(
                !forbidden.iter().any(|w| text.to_lowercase().contains(w)),
                "{text}"
            );
        }
    };
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let (c, release) = gate(root.path());
    check(&c.window());
    c.request_quit();
    check(&c.window());
    c.cancel_quit();
    release.release();
    pump(&c, || !c.is_busy());
    check(&c.window());
    c.stage(vec![root.path().to_owned()]).unwrap();
    pump(&c, || !c.is_busy());
    check(&c.window());
}
#[test]
fn s12_invalid_preferences_default_and_next_save_replaces_them() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("preferences.json");
    fs::write(&path, b"invalid").unwrap();
    let c = ready(root.path(), &["one.eml"]);
    let destination = root.path().join("copies");
    fs::create_dir(&destination).unwrap();
    c.set_destination(Some(destination.clone()));
    pump(&c, || !c.is_busy());
    let preferences = Preferences::load(path);
    assert_eq!(preferences.destination(), Some(destination.as_path()));
    assert_eq!(c.window().get_view().destination, "copies");
}
#[test]
fn s1_selection_admission_preserves_unique_native_operands_and_bounds() {
    use eml_core::{MAX_INPUT_PATHS, RequestPathError};
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let file = write(root.path(), "one.eml", ATTACHMENT);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let captured = calls.clone();
    let c = controller(root.path(), move |r, ctrl, sink| {
        captured.lock().unwrap().push(r.clone());
        eml_core::run(r, ctrl, sink)
    });
    c.stage(vec![file.clone(), file.clone()]).unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(calls.lock().unwrap()[0].paths, [file]);
    assert!(matches!(
        c.stage(vec!["relative.eml".into()]),
        Err(RequestPathError::AbsoluteSelectionRequired)
    ));
    let paths = (0..=MAX_INPUT_PATHS)
        .map(|n| root.path().join(format!("{n}.eml")))
        .collect();
    assert!(matches!(
        c.stage(paths),
        Err(RequestPathError::TooManyInputs)
    ));
    assert_eq!(calls.lock().unwrap().len(), 1);
}
#[test]
fn s17_keyboard_navigation_and_actions_use_current_state() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = done(root.path(), &["one.eml", "two.eml", "three.eml"]);
    let ui = c.window();
    label(&ui, "one.eml, Copy saved").invoke_accessible_default_action();
    let first = ui.get_rows().row_data(0).unwrap().key;
    key(&ui, slint::platform::Key::DownArrow);
    assert_ne!(
        ui.get_rows().iter().find(|r| r.selected).unwrap().key,
        first
    );
    for (key_name, index) in [
        (slint::platform::Key::End, 2),
        (slint::platform::Key::Home, 0),
        (slint::platform::Key::PageDown, 2),
        (slint::platform::Key::PageUp, 0),
    ] {
        key(&ui, key_name);
        assert!(ui.get_rows().row_data(index).unwrap().selected);
    }
    key(&ui, slint::platform::Key::Escape);
    assert!(!ui.get_detail().visible);
    assert!(!ui.get_view().can_start);
    c.start_selection();
    assert!(!c.is_busy());
    assert_eq!(ui.get_view().state, BatchState::Done);
}
#[test]
fn t7_s17_failed_only_disables_button_shortcut_and_controller() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "bad.eml", b"");
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls = count.clone();
    let c = controller(root.path(), move |r, control, sink| {
        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        eml_core::run(r, control, sink)
    });
    c.stage(vec![root.path().to_owned()]).unwrap();
    assert!(c.window().get_view().can_start);
    pump(&c, || !c.is_busy());
    assert!(!c.window().get_view().can_start);
    assert_eq!(
        label(&c.window(), "Remove Attachments").accessible_enabled(),
        Some(false)
    );
    shortcut(&c.window(), slint::platform::Key::Return);
    c.start_selection();
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    c.start_over();
    write(root.path(), "good.eml", PLAIN);
    c.stage(vec![root.path().to_owned()]).unwrap();
    pump(&c, || !c.is_busy());
    assert!(c.window().get_view().can_start);
    shortcut(&c.window(), slint::platform::Key::Return);
    pump(&c, || !c.is_busy());
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 3);
}
#[test]
fn s18_no_inputs_and_output_overlap_are_core_failures() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), eml_core::run);
    c.stage(vec![root.path().to_owned()]).unwrap();
    pump(&c, || !c.is_busy());
    assert!(c.window().get_view().banner.contains("No .eml files"));
    assert!(!c.window().get_view().can_start);
    let root = tempfile::tempdir().unwrap();
    let c = ready(root.path(), &["one.eml"]);
    let inside = root.path().join("copies");
    fs::create_dir(&inside).unwrap();
    c.set_destination(Some(inside));
    pump(&c, || !c.is_busy());
    assert!(c.window().get_view().banner.contains("inside a folder"));
    assert!(!c.window().get_view().can_start);
}
#[test]
fn u8_v5_real_zero_one_and_five_parts_keep_a_stable_viewport() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "zero.eml", PLAIN);
    write(root.path(), "one.eml", ATTACHMENT);
    write(root.path(), "five.eml", &many_parts(5));
    let c = controller(root.path(), eml_core::run);
    c.stage(vec![root.path().to_owned()]).unwrap();
    pump(&c, || !c.is_busy());
    let mut heights = Vec::new();
    for (name, parts) in [("zero.eml", 0), ("one.eml", 1), ("five.eml", 5)] {
        let row = c
            .window()
            .get_rows()
            .iter()
            .find(|r| r.name == name)
            .unwrap();
        c.window().invoke_select_email(row.key);
        assert_eq!(c.window().get_detail().parts.row_count(), parts);
        mock_elapsed_time(Duration::from_millis(1));
        heights.push(label(&c.window(), "Emails").size().height);
    }
    assert!(
        heights.iter().all(|h| (h - heights[0]).abs() <= 1.),
        "{heights:?}"
    );
    for (pane, line) in [
        ("detail-pane", "detail-separator"),
        ("footer", "footer-separator"),
    ] {
        let find = |id| {
            ElementHandle::find_by_element_id(&c.window(), &format!("BatchPane::{id}"))
                .next()
                .unwrap()
        };
        assert!((find(pane).absolute_position().y - find(line).absolute_position().y).abs() <= 1.);
    }
}
fn many_parts(count: usize) -> Vec<u8> {
    let mut text="Subject: Many\r\nContent-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\nContent-Type: text/plain\r\n\r\nBody\r\n".to_owned();
    for n in 0..count {
        text += &format!(
            "--x\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"part{n}.pdf\"\r\n\r\ncontent\r\n"
        );
    }
    text += "--x--\r\n";
    text.into_bytes()
}
#[test]
fn s13_long_detail_and_notice_keep_actions_and_two_full_rows() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "one.eml", &many_parts(40));
    for name in ["two.eml", "three.eml", "four.eml", "five.eml"] {
        write(root.path(), name, ATTACHMENT);
    }
    let c = controller(root.path(), eml_core::run);
    c.stage(vec![root.path().to_owned()]).unwrap();
    pump(&c, || !c.is_busy());
    c.start_selection();
    pump(&c, || !c.is_busy());
    let ui = c.window();
    ui.invoke_select_email(
        ui.get_rows()
            .iter()
            .find(|r| r.name == "one.eml")
            .unwrap()
            .key,
    );
    assert_eq!(ui.get_detail().parts.row_count(), 40);
    for (width, height) in [(760., 520.), (2560., 1440.)] {
        ui.window().set_size(slint::LogicalSize::new(width, height));
        mock_elapsed_time(Duration::from_millis(1));
        for text in [
            "Start Over",
            eml_gui::platform::file_manager_label(),
            "Show Copy",
            "Show Original",
            "Copy Details",
        ] {
            inside(&label(&ui, text), width, height);
        }
        let list = assert_two_complete_rows(&ui);
        assert!(list.size().width <= 980.1);
        assert!(label(&ui, "Email details").size().height <= 200.1);
    }
    let mut view = ui.get_view();
    view.banner="A long warning that must stay visible without covering the actions or hiding the retained list.".into();
    view.notice = "Report saved.".into();
    ui.set_view(view);
    ui.window().set_size(slint::LogicalSize::new(760., 520.));
    mock_elapsed_time(Duration::from_millis(1));
    assert_two_complete_rows(&ui);
    inside(&label(&ui, "Copy Details"), 760., 520.);
}
#[test]
fn t5_detail_header_columns_and_footer_share_the_left_edge() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = ready(root.path(), &["one.eml", "two.eml", "three.eml"]);
    let ui = c.window();
    ui.window().set_size(slint::LogicalSize::new(760., 520.));
    select_first(&c);
    mock_elapsed_time(Duration::from_millis(1));
    let x = label(&ui, "3 emails").absolute_position().x;
    let name = ui.get_detail().name;
    for e in [
        label(&ui, &name),
        label(&ui, "Email"),
        label(&ui, "Save copies"),
    ] {
        assert!((e.absolute_position().x - x).abs() <= 1.);
    }
    let y = label(&ui, "Email").absolute_position().y;
    c.start_selection();
    pump(&c, || !c.is_busy());
    assert!((label(&ui, "Email").absolute_position().y - y).abs() <= 1.);
}
#[test]
fn t6_u9_v4_inline_parts_and_notices_keep_their_meaning_and_order() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "inline.eml", INLINE);
    let c = controller(root.path(), eml_core::run);
    c.stage(vec![root.path().to_owned()]).unwrap();
    pump(&c, || !c.is_busy());
    for (index, notice) in [
        eml_core::vocabulary::PREPARED_RELATED_RESOURCE_NOTICE,
        eml_core::vocabulary::RELATED_RESOURCE_NOTICE,
        eml_core::vocabulary::EXISTING_RELATED_RESOURCE_NOTICE,
    ]
    .into_iter()
    .enumerate()
    {
        select_first(&c);
        let detail = c.window().get_detail();
        assert_eq!(
            detail.notice,
            notice,
            "detail={detail:?}; rows={:?}; banner={}",
            c.window().get_rows().iter().collect::<Vec<_>>(),
            c.window().get_view().banner
        );
        assert_eq!(detail.parts.row_data(0).unwrap().kind, "Inline image");
        let pane = label(&c.window(), "Email details");
        let elements = pane.query_descendants().find_all();
        let find = |text: &str| {
            elements
                .iter()
                .find(|e| e.accessible_label().as_deref() == Some(text))
                .unwrap()
        };
        let part = detail.parts.row_data(0).unwrap().name;
        assert!(find(&detail.sentence).absolute_position().y < find(&part).absolute_position().y);
        assert!(find(&part).absolute_position().y < find(notice).absolute_position().y);
        assert!(c.clipboard_text().unwrap().contains(notice));
        if index < 2 {
            if index == 1 {
                c.stage(vec![root.path().to_owned()]).unwrap();
                pump(&c, || !c.is_busy());
            }
            c.start_selection();
            pump(&c, || !c.is_busy());
        }
    }
    let root = tempfile::tempdir().unwrap();
    let c = ready(root.path(), &["report.eml"]);
    select_first(&c);
    assert_eq!(c.window().get_detail().parts.row_data(0).unwrap().kind, "");
    let root = tempfile::tempdir().unwrap();
    let unnamed = String::from_utf8(INLINE.to_vec())
        .unwrap()
        .replace("filename=\"image001.png\"", "")
        .replace("name=\"image001.png\"", "");
    write(root.path(), "inline.eml", unnamed.as_bytes());
    let c = controller(root.path(), eml_core::run);
    c.stage(vec![root.path().to_owned()]).unwrap();
    pump(&c, || !c.is_busy());
    select_first(&c);
    assert_eq!(
        c.window().get_detail().parts.row_data(0).unwrap().name,
        "Inline image"
    );
    assert!(c.clipboard_text().unwrap().contains("image/png"));
}

#[test]
#[ignore = "Manual production-view captures; set GUI_CAPTURE_DIR"]
fn s20_capture_one_list_states() {
    let directory = PathBuf::from(
        std::env::var("GUI_CAPTURE_DIR").expect("Set GUI_CAPTURE_DIR for capture output"),
    );
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            mock_time: true,
            renderer_name: Some("software".into()),
            ..Default::default()
        },
    )))
    .unwrap();
    fs::create_dir_all(&directory).unwrap();
    for (theme, scheme) in [
        ("light", slint::language::ColorScheme::Light),
        ("dark", slint::language::ColorScheme::Dark),
    ] {
        let root = tempfile::tempdir().unwrap();
        let (c, release) = gate(root.path());
        let ui = c.window();
        ui.window().set_size(slint::LogicalSize::new(1080., 700.));
        ui.global::<Palette>().set_color_scheme(scheme);
        capture(&ui, &directory, &format!("removing-{theme}"));
        release.release();
        pump(&c, || !c.is_busy());
        select_first(&c);
        capture(&ui, &directory, &format!("done-{theme}"));
        c.start_over();
        capture(&ui, &directory, &format!("empty-{theme}"));
        fs::remove_file(root.path().join("one.no-attachments.eml")).unwrap();
        fs::remove_file(root.path().join("two.no-attachments.eml")).unwrap();
        c.stage(vec![root.path().to_owned()]).unwrap();
        pump(&c, || !c.is_busy());
        select_first(&c);
        capture(&ui, &directory, &format!("ready-{theme}"));
        if theme == "light" {
            ui.window().set_size(slint::LogicalSize::new(760., 520.));
            capture(&ui, &directory, "ready-light-narrow");
        }
        c.start_over();
        let empty = root.path().join("empty");
        fs::create_dir(&empty).unwrap();
        c.stage(vec![empty]).unwrap();
        pump(&c, || !c.is_busy());
        ui.window().set_size(slint::LogicalSize::new(1080., 700.));
        label(&ui, "Details…").mock_single_click(slint::platform::PointerEventButton::Left);
        capture(&ui, &directory, &format!("processing-details-{theme}"));
    }
}

#[test]
fn u5_chips_size_and_status_share_row_centre() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = ready(root.path(), &["one.eml"]);
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
fn u12_destination_popup_tracks_control_at_all_widths() {
    use i_slint_core::window::{PopupWindowLocation, WindowInner};
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = ready(root.path(), &["one.eml"]);
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
fn s5_missing_and_unprocessed_are_not_conflated() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = ready(root.path(), &["one.eml", "two.eml", "three.eml"]);
    fs::remove_file(root.path().join("two.eml")).unwrap();
    fs::remove_file(root.path().join("three.eml")).unwrap();
    c.start_selection();
    pump(&c, || !c.is_busy());
    let statuses: Vec<_> = c
        .window()
        .get_rows()
        .iter()
        .map(|r| r.status.to_string())
        .collect();
    assert_eq!(
        statuses
            .iter()
            .filter(|s| s.as_str() == "No longer found")
            .count(),
        2
    );
    assert!(statuses.contains(&"Copy saved".into()));
    assert!(c.window().get_view().summary.contains("2 no longer found"));
}
#[test]
fn s8_stopped_totals_include_work_without_preview_rows_and_continue() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "one.eml", ATTACHMENT);
    let directory = root.path().to_owned();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let creates = calls.clone();
    let c = controller(root.path(), move |r, control, sink| {
        let first = r.mode == RunMode::Create
            && creates.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0;
        if first {
            write(&directory, "two.eml", PLAIN);
            write(&directory, "three.eml", PLAIN);
        }
        run_with(r, control, sink, |e, t| {
            let start = matches!(e, Event::Start(_));
            t.emit(e)?;
            if first && start {
                control.request_stop();
            }
            Ok(())
        })
    });
    c.stage(vec![root.path().to_owned()]).unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_rows().row_count(), 1);
    c.start_selection();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_view().state, BatchState::Stopped);
    assert_eq!(c.window().get_view().summary, "3 not processed");
    assert!(c.window().get_view().can_start);
    c.start_selection();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_rows().row_count(), 3);
    assert_eq!(c.window().get_view().title, "3 copies saved");
}
#[test]
fn u10_check_failures_persist_while_create_is_held() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let (c, release) = gate(root.path());
    let bad = c
        .window()
        .get_rows()
        .iter()
        .find(|r| r.name == "three.eml")
        .unwrap();
    assert_eq!(bad.status, "Not a valid email");
    assert_eq!(c.window().get_view().problems, "1 can’t be copied");
    let good = c
        .window()
        .get_rows()
        .iter()
        .find(|r| r.name == "one.eml")
        .unwrap();
    assert_eq!(good.status, "Waiting");
    release.release();
    pump(&c, || !c.is_busy());
    assert_eq!(
        c.window()
            .get_rows()
            .iter()
            .find(|r| r.name == "three.eml")
            .unwrap()
            .status,
        "Not a valid email"
    );
}
#[test]
fn t2_continue_keeps_saved_rows_until_current_core_results_arrive() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    for name in ["one.eml", "two.eml"] {
        write(root.path(), name, ATTACHMENT);
    }
    let creates = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls = creates.clone();
    let (release, held) = latch();
    let c = controller(root.path(), move |r, control, sink| {
        let attempt = if r.mode == RunMode::Create {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        } else {
            usize::MAX
        };
        run_with(r, control, sink, |e, t| {
            let item = matches!(e, Event::Item(_));
            let start = matches!(e, Event::Start(_));
            t.emit(e)?;
            if attempt == 0 && item {
                control.request_stop();
            }
            if attempt == 1 && start {
                held.wait();
            }
            Ok(())
        })
    });
    c.stage(vec![root.path().to_owned()]).unwrap();
    pump(&c, || !c.is_busy());
    c.start_selection();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_view().state, BatchState::Stopped);
    let saved = c
        .window()
        .get_rows()
        .iter()
        .find(|r| r.status == "Copy saved")
        .unwrap()
        .name;
    c.start_selection();
    pump(&c, || {
        c.window()
            .get_view()
            .progress_text
            .starts_with("0 of 2 done")
    });
    assert_eq!(
        c.window()
            .get_rows()
            .iter()
            .find(|r| r.name == saved)
            .unwrap()
            .status,
        "Copy saved"
    );
    release.release();
    pump(&c, || !c.is_busy());
    assert_eq!(
        c.window()
            .get_rows()
            .iter()
            .find(|r| r.name == saved)
            .unwrap()
            .status,
        "Copy already exists"
    );
}
#[test]
fn t3_deep_folder_keeps_its_end_and_accessible_full_label() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let folder = root
        .path()
        .join("a-very-long-branch-name".repeat(5))
        .join("Inbox");
    let input = write(&folder, "one.eml", ATTACHMENT);
    let c = controller(root.path(), eml_core::run);
    c.stage(vec![input]).unwrap();
    pump(&c, || !c.is_busy());
    c.window()
        .window()
        .set_size(slint::LogicalSize::new(760., 520.));
    mock_elapsed_time(Duration::from_millis(1));
    let row = c.window().get_rows().row_data(0).unwrap();
    assert_eq!(row.folder_suffix, "Inbox");
    assert_eq!(
        format!("{}{}", row.folder_prefix, row.folder_suffix),
        row.folder.as_str()
    );
    let suffix = label(&c.window(), "Inbox");
    assert!(suffix.size().width >= 30.);
    assert!(label(&c.window(), &row.folder).size().width > 30.);
}
#[test]
#[cfg(unix)]
fn t9_native_units_controls_and_distinct_paths_never_use_lossy_display_for_actions() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let a = root
        .path()
        .join(OsString::from_vec(b"odd-\xff.eml".to_vec()));
    let b = root
        .path()
        .join(OsString::from_vec(b"odd-\xfe.eml".to_vec()));
    let controls = write(root.path(), "line\n\u{202e}.eml", ATTACHMENT);
    // Missing native operands work on filesystems that reject creating these bytes.
    let c = controller(root.path(), eml_core::run);
    c.stage(vec![a.clone(), b]).unwrap();
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_rows().row_count(), 2);
    let keys: std::collections::HashSet<_> = c.window().get_rows().iter().map(|r| r.key).collect();
    assert_eq!(keys.len(), 2);
    let row = c
        .window()
        .get_rows()
        .iter()
        .find(|r| r.name == "odd-<0xFF>.eml")
        .unwrap();
    c.window().invoke_select_email(row.key);
    assert_eq!(c.selected_original(), Some(a));
    let details = c.clipboard_text().unwrap();
    assert!(details.contains("odd-<0xFF>.eml"));
    assert!(!details.contains('�'));
    c.start_over();
    c.stage(vec![controls]).unwrap();
    pump(&c, || !c.is_busy());
    select_first(&c);
    assert_eq!(
        c.window().get_rows().row_data(0).unwrap().name,
        r"line\n\u{202e}.eml"
    );
    c.start_selection();
    pump(&c, || !c.is_busy());
    assert_eq!(
        fs::read(c.selected_copy().unwrap()).unwrap(),
        include_bytes!("../../../../tests/contract/mime/golden/01-gmail-attachment.eml")
    );
}

#[test]
fn u6_t8_headers_and_first_row_stay_fixed_across_run_and_filter_states() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let (c, release) = gate(root.path());
    let ui = c.window();
    let y = label(&ui, "Email").absolute_position().y;
    let row_y = label(&ui, "one.eml, Waiting").absolute_position().y;
    ui.invoke_toggle_problems();
    assert!((label(&ui, "Email").absolute_position().y - y).abs() <= 1.);
    assert!(
        (label(&ui, "three.eml, Not a valid email")
            .absolute_position()
            .y
            - row_y)
            .abs()
            <= 1.
    );
    ui.invoke_toggle_problems();
    release.release();
    pump(&c, || !c.is_busy());
    assert!((label(&ui, "Email").absolute_position().y - y).abs() <= 1.);
    assert!((label(&ui, "one.eml, Copy saved").absolute_position().y - row_y).abs() <= 1.);
    c.start_over();
    c.stage(vec![root.path().to_owned()]).unwrap();
    pump(&c, || !c.is_busy());
    assert!((label(&ui, "Email").absolute_position().y - y).abs() <= 1.);
    assert!(
        (label(&ui, "one.eml, Copy exists, will verify")
            .absolute_position()
            .y
            - row_y)
            .abs()
            <= 1.
    );
}
#[test]
fn s6_selection_filter_and_escape_keep_or_clear_the_native_identity() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = ready(root.path(), &["one.eml", "two.eml"]);
    write(root.path(), "bad.eml", b"");
    c.stage(vec![root.path().to_owned()]).unwrap();
    pump(&c, || !c.is_busy());
    let good = c
        .window()
        .get_rows()
        .iter()
        .find(|r| r.name == "one.eml")
        .unwrap();
    c.window().invoke_select_email(good.key.clone());
    assert!(c.window().get_detail().visible);
    c.window().invoke_select_email(good.key.clone());
    assert!(!c.window().get_detail().visible);
    c.window().invoke_select_email(good.key);
    c.window().invoke_toggle_problems();
    assert!(c.selected_original().is_none());
    assert_eq!(c.window().get_rows().row_count(), 1);
    select_first(&c);
    assert_eq!(c.selected_original(), Some(root.path().join("bad.eml")));
    c.window().invoke_toggle_problems();
    assert_eq!(c.selected_original(), Some(root.path().join("bad.eml")));
    c.window().invoke_clear_selected();
    assert!(c.selected_original().is_none());
}
