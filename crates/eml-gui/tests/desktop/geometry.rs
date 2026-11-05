//! Regression checks for hit targets, rendered alignment and popup context.
use super::*;

fn init_rendered() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            mock_time: true,
            renderer_name: Some("software".into()),
            ..Default::default()
        },
    )))
    .unwrap();
}

fn element(ui: &MainWindow, label: &str, role: AccessibleRole) -> ElementHandle {
    ElementHandle::find_by_accessible_label(ui, label)
        .find(|element| element.accessible_role() == Some(role))
        .unwrap_or_else(|| panic!("Missing {label}"))
}

fn pointer(ui: &MainWindow, x: f32, y: f32) {
    use slint::platform::{PointerEventButton, WindowEvent};
    let position = slint::LogicalPosition::new(x, y);
    ui.window()
        .dispatch_event(WindowEvent::PointerMoved { position });
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
    mock_elapsed_time(Duration::from_millis(5));
}

// Compare painted glyph origins, not only identically positioned wrapper items.
fn ink(ui: &MainWindow, item: &ElementHandle) -> (f32, f32) {
    let image = ui.window().take_snapshot().unwrap();
    let p = item.absolute_position();
    let s = item.size();
    let mut left = image.width();
    let mut right = 0;
    for y in (p.y + 3.0).ceil() as u32..(p.y + s.height - 3.0).floor() as u32 {
        for x in (p.x + 3.0).ceil() as u32..(p.x + s.width - 3.0).floor() as u32 {
            let pixel = image.as_slice()[(y * image.width() + x) as usize];
            if pixel.r < 120 && pixel.g < 120 && pixel.b < 120 {
                left = left.min(x);
                right = right.max(x);
            }
        }
    }
    assert!(left <= right, "No glyphs in {p:?} {s:?}");
    (left as f32, right as f32)
}

#[test]
fn input_remove_is_square_and_blank_row_space_preserves_inputs() {
    init_rendered();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "created"));
    c.stage(vec![
        root.path().join("first.eml"),
        root.path().join("second.eml"),
    ])
    .unwrap();
    let ui = c.window();
    mock_elapsed_time(Duration::from_millis(5));
    let remove = element(&ui, "Remove first.eml", AccessibleRole::Button);
    let p = remove.absolute_position();
    let s = remove.size();
    assert_eq!(s.width, 32.0, "The icon must not absorb spare row width");
    assert!(s.height >= 32.0);
    pointer(&ui, p.x - 100.0, p.y + s.height / 2.0);
    assert_eq!(
        ui.get_selection().row_count(),
        2,
        "Blank row space is not Remove"
    );
    pointer(&ui, p.x + s.width / 2.0, p.y + s.height / 2.0);
    assert_eq!(ui.get_selection().row_count(), 1);
    assert!(
        ui.get_selection()
            .row_data(0)
            .unwrap()
            .path
            .full
            .ends_with("second.eml")
    );
}

#[test]
fn paired_fields_align_and_segment_captions_are_centered() {
    init_rendered();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "created"));
    let ui = c.window();
    for size in [13.0, 20.0] {
        ui.set_ui_size(size);
        mock_elapsed_time(Duration::from_millis(5));
        let segment = element(&ui, "Action", AccessibleRole::TabList);
        let destination = element(
            &ui,
            "Copy destination: Next to originals",
            AccessibleRole::Button,
        );
        assert!((segment.size().height - destination.size().height).abs() < 0.1);
        assert!((segment.absolute_position().y - destination.absolute_position().y).abs() < 0.1);
        for label in ["Create copies", "Check only"] {
            let tab = element(&ui, label, AccessibleRole::Tab);
            let (left, right) = ink(&ui, &tab);
            let center = tab.absolute_position().x + tab.size().width / 2.0;
            assert!(
                ((left + right) / 2.0 - center).abs() < 2.0,
                "{label}: {left}..{right}, center {center}"
            );
        }
    }
}

fn popup_changes(
    before: &slint::SharedPixelBuffer<slint::Rgba8Pixel>,
    ui: &MainWindow,
    exclude: &ElementHandle,
) -> (f32, f32, f32, f32) {
    let after = ui.window().take_snapshot().unwrap();
    let p = exclude.absolute_position();
    let s = exclude.size();
    let mut bounds = (after.width(), after.height(), 0, 0);
    for y in 0..after.height() {
        for x in 0..after.width() {
            if (x as f32) >= p.x - 2.0
                && (x as f32) <= p.x + s.width + 2.0
                && (y as f32) >= p.y - 2.0
                && (y as f32) <= p.y + s.height + 2.0
            {
                continue;
            }
            let index = (y * after.width() + x) as usize;
            if before.as_slice()[index] != after.as_slice()[index] {
                bounds.0 = bounds.0.min(x);
                bounds.1 = bounds.1.min(y);
                bounds.2 = bounds.2.max(x);
                bounds.3 = bounds.3.max(y);
            }
        }
    }
    assert!(bounds.0 < bounds.2 && bounds.1 < bounds.3);
    (
        bounds.0 as f32,
        bounds.1 as f32,
        bounds.2 as f32,
        bounds.3 as f32,
    )
}

#[test]
fn destination_keeps_its_name_and_popup_stays_at_the_trigger() {
    init_rendered();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "created"));
    let destination = root
        .path()
        .join("A lengthy destination whose identifying ending is November 2026");
    fs::create_dir(&destination).unwrap();
    c.set_destination(Some(destination));
    let ui = c.window();
    let path = ui.get_destination();
    assert_eq!(
        format!("{}{}", path.prefix, path.suffix),
        "A lengthy destination whose identifying ending is November 2026"
    );
    assert_eq!(path.full, ui.get_destination_label());
    mock_elapsed_time(Duration::from_millis(5));
    let trigger = element(
        &ui,
        &format!("Copy destination: {}", path.full),
        AccessibleRole::Button,
    );
    let before = ui.window().take_snapshot().unwrap();
    trigger.mock_single_click(slint::platform::PointerEventButton::Left);
    mock_elapsed_time(Duration::from_millis(5));
    let (left, top, right, bottom) = popup_changes(&before, &ui, &trigger);
    let p = trigger.absolute_position();
    let s = trigger.size();
    assert!(
        left <= p.x + s.width && right >= p.x,
        "Menu detached horizontally"
    );
    assert!(
        (bottom - (p.y - 4.0)).abs() < 3.0 || (top - (p.y + s.height + 4.0)).abs() < 3.0,
        "Menu {left},{top}..{right},{bottom}; trigger {p:?} {s:?}"
    );
    press_key(&ui, slint::platform::Key::Escape);
    assert_eq!(ui.get_destination(), path);
}

#[test]
fn header_ink_aligns_with_values_and_failed_runs_offer_review() {
    init_rendered();
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("first.eml");
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
    let output_label = ui.get_reveal_output_label();
    ui.set_can_reveal_output(false);
    mock_elapsed_time(Duration::from_millis(5));
    assert!(
        has_label(&ui, output_label.as_str()),
        "Temporary disabled state must not retarget the primary action"
    );
    let more = element(&ui, "More actions", AccessibleRole::Button);
    let before = ui.window().take_snapshot().unwrap();
    more.mock_single_click(slint::platform::PointerEventButton::Left);
    mock_elapsed_time(Duration::from_millis(5));
    let (_, top, right, _) = popup_changes(&before, &ui, &more);
    assert!((top - more.absolute_position().y - more.size().height - 4.0).abs() < 3.0);
    assert!((right + 1.0 - more.absolute_position().x - more.size().width).abs() < 3.0);
    press_key(&ui, slint::platform::Key::Escape);
    let header = element(&ui, "Outcome", AccessibleRole::Button);
    let value = element(&ui, "Created · 0 removed", AccessibleRole::Text);
    let (header_left, _) = ink(&ui, &header);
    let (value_left, _) = ink(&ui, &value);
    assert!(
        (header_left - value_left).abs() <= 3.0,
        "Painted headers must share the value grid"
    );
    script(
        root.path(),
        &("cat >/dev/null\n".to_owned() + &print(&fixture("prepublication-failure")) + "exit 1\n"),
    );
    c.submit(vec![root.path().join("failed.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    assert!(ui.get_run_title().contains("1 needs attention"));
    assert_eq!(
        ui.get_runs().row_data(1).unwrap().status,
        "1 needs attention"
    );
    assert!(!has_label(&ui, output_label.as_str()));
    assert!(has_label(&ui, "Review Request…"));
    click(&ui, "Review Request…");
    assert!(ui.get_reviewing());
    assert!(!c.is_busy(), "Reviewing must not start another worker");
}

#[test]
fn keyboard_context_menu_uses_the_selected_session_row() {
    init_rendered();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "created"));
    for name in ["first.eml", "second.eml"] {
        c.submit(vec![root.path().join(name)]).unwrap();
        pump(&c, || !c.is_busy());
    }
    let ui = c.window();
    let caption = result_caption(&ui, 1);
    click_role(&ui, &caption, AccessibleRole::ListItem);
    let row = element(&ui, &caption, AccessibleRole::ListItem);
    let before = ui.window().take_snapshot().unwrap();
    chord(&ui, slint::platform::Key::Shift, slint::platform::Key::F10);
    assert!(has_label(&ui, "Close Result"));
    let (_, top, _, _) = popup_changes(&before, &ui, &row);
    assert!(top >= row.absolute_position().y + row.size().height - 1.0);
    click(&ui, "Close Result");
    assert_eq!(ui.get_runs().row_count(), 1);
    assert!(ui.get_runs().row_data(0).unwrap().label.contains("Run 1"));
}

#[test]
fn disclosure_glyph_is_an_operable_part_of_the_policy_control() {
    init_rendered();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "created"));
    let ui = c.window();
    mock_elapsed_time(Duration::from_millis(5));
    let disclosure = element(&ui, "What copies keep", AccessibleRole::Button);
    let p = disclosure.absolute_position();
    let s = disclosure.size();
    assert!(!ui.get_draft_policy_expanded());
    pointer(&ui, p.x + 8.0, p.y + s.height / 2.0);
    assert!(
        ui.get_draft_policy_expanded(),
        "The visible chevron must not be a dead hit area"
    );
    pointer(
        &ui,
        p.x + 8.0,
        disclosure.absolute_position().y + s.height / 2.0,
    );
    assert!(!ui.get_draft_policy_expanded());
    assert!(!c.is_busy());
}
