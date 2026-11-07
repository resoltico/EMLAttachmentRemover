//! Retained safety and secondary-window contracts.
use super::*;
use i_slint_backend_testing::ElementHandle;

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
        close_shortcut(about.window());
        assert!(!about.window().is_visible());
        assert!(c.window().window().is_visible());
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
    close_shortcut(licenses.window());
    assert!(!licenses.window().is_visible());
    assert!(about.window().is_visible());
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

fn close_shortcut(window: &slint::Window) {
    use slint::platform::{Key, WindowEvent};
    // Testing dispatch uses toolkit modifiers; winit maps macOS Command to Control.
    let modifier = Key::Control;
    window.dispatch_event(WindowEvent::KeyPressed {
        text: modifier.into(),
    });
    window.dispatch_event(WindowEvent::KeyPressed { text: "w".into() });
    window.dispatch_event(WindowEvent::KeyReleased { text: "w".into() });
    window.dispatch_event(WindowEvent::KeyReleased {
        text: modifier.into(),
    });
    mock_elapsed_time(Duration::from_millis(5));
}

#[test]
fn native_launch_paths_are_absolute_and_ignore_only_the_platform_operand_class() {
    use eml_gui::ingress::launch_paths;
    use std::ffi::OsString;
    let root = tempfile::tempdir().unwrap();
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
    assert!(
        launch_paths([OsString::from("-psn_0_42")], root.path())
            .unwrap()
            .is_empty()
    );
    assert!(launch_paths([OsString::from("one.eml")], Path::new("relative")).is_err());
}
#[test]
fn native_drop_paths_are_deferred_as_one_check_and_cancelled_hover_adds_nothing() {
    use slint::Model;
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let cli = super::acceptance::dual(
        root.path(),
        &super::acceptance::check_stream(root.path(), &["one.eml", "two.eml", "three.eml"]),
        "",
    );
    let c = controller(root.path(), cli);
    for name in ["one.eml", "two.eml", "three.eml"] {
        c.deliver_drop(root.path().join(name));
    }
    assert!(!c.is_busy());
    mock_elapsed_time(Duration::from_millis(1));
    pump(&c, || !c.is_busy());
    assert_eq!(c.window().get_rows().row_count(), 3);
    let seen = super::acceptance::requests(root.path());
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0]["paths"].as_array().unwrap().len(), 3);
    c.deliver_drop(root.path().join("cancelled.eml"));
    c.cancel_drop();
    mock_elapsed_time(Duration::from_millis(1));
    assert_eq!(super::acceptance::requests(root.path()).len(), 1);
}
#[test]
fn secondary_close_shortcuts_leave_the_active_worker_and_native_selection_intact() {
    use slint::Model;
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = super::acceptance::gate(root.path());
    let ui = c.window();
    ui.invoke_select_email(ui.get_rows().row_data(0).unwrap().key);
    let selected = c.selected_original();
    ui.invoke_about();
    c.about_window().invoke_licenses();
    close_shortcut(c.license_window().window());
    assert!(!c.license_window().window().is_visible());
    assert!(c.about_window().window().is_visible());
    close_shortcut(c.about_window().window());
    assert!(!c.about_window().window().is_visible());
    assert!(c.is_busy());
    assert_eq!(c.selected_original(), selected);
    fs::write(root.path().join("release"), b"go").unwrap();
    pump(&c, || !c.is_busy());
}
