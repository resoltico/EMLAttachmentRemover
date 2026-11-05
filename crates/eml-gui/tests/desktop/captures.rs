//! Production toolkit boundary contracts.
use super::*;

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
        ui.window().set_size(slint::LogicalSize::new(760.0, 520.0));
        ui.set_draft_policy_expanded(true);
        mock_elapsed_time(Duration::from_millis(5));
        capture_window(&ui, &directory, &format!("A-policy-narrow-{scheme}"));
        ui.set_draft_policy_expanded(false);
        ui.window().set_size(slint::LogicalSize::new(1080.0, 700.0));
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
        let stream = mixed_create_stream(&sources).replace(
            "\"reason\":\"attachment\"",
            "\"reason\":\"related_resource\"",
        );
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
        ui.invoke_filter_results(0);
        ui.invoke_select_item(0);
        mock_elapsed_time(Duration::from_millis(5));
        capture_window(&ui, &directory, &format!("E-policy-{scheme}"));
        ui.window().set_size(slint::LogicalSize::new(760.0, 520.0));
        mock_elapsed_time(Duration::from_millis(5));
        capture_window(&ui, &directory, &format!("F-policy-narrow-{scheme}"));
        ui.invoke_new_request();
        c.stage(vec![folder]).unwrap();
        ui.set_draft_policy_expanded(true);
        mock_elapsed_time(Duration::from_millis(5));
        capture_window(&ui, &directory, &format!("G-draft-policy-narrow-{scheme}"));
        ui.hide().unwrap();
    }
}

#[test]
#[ignore = "Manual policy layout and reachability capture; set GUI_CAPTURE_DIR"]
fn capture_policy_layout_at_supported_sizes_and_scales() {
    use i_slint_backend_testing::ElementRoot;
    use slint::{language::ColorScheme, platform::WindowEvent};
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        i_slint_backend_testing::TestingBackendOptions {
            mock_time: true,
            renderer_name: Some("software".into()),
            ..Default::default()
        },
    )))
    .unwrap();
    let directory =
        PathBuf::from(std::env::var_os("GUI_CAPTURE_DIR").expect("set GUI_CAPTURE_DIR"));
    for (palette, color) in [("light", ColorScheme::Light), ("dark", ColorScheme::Dark)] {
        for scale in [1.0, 2.0] {
            for font in [14.0, 20.0] {
                for (width, height) in [(760.0, 520.0), (1080.0, 700.0)] {
                    for sidebar in [false, true] {
                        let root = tempfile::tempdir().unwrap();
                        let c = controller(root.path(), completed(root.path(), "created"));
                        let ui = c.window();
                        if sidebar {
                            c.submit(vec![root.path().join("previous.eml")]).unwrap();
                            pump(&c, || !c.is_busy());
                            ui.invoke_new_request();
                        }
                        ui.global::<eml_gui::Palette>().set_color_scheme(color);
                        ui.window().dispatch_event(WindowEvent::ScaleFactorChanged {
                            scale_factor: scale,
                        });
                        // TestingWindow::set_size assumes 1x. Supply its physical
                        // backing size and a separate logical resize event, as a
                        // native backend does, before taking a 2x snapshot.
                        let logical = slint::LogicalSize::new(width, height);
                        let physical = logical.to_physical(scale);
                        ui.window().set_size(physical);
                        ui.window()
                            .dispatch_event(WindowEvent::Resized { size: logical });
                        assert_eq!(ui.window().size(), physical);
                        ui.set_ui_size(font);
                        ui.set_draft_policy_expanded(true);
                        mock_elapsed_time(Duration::from_millis(5));
                        let label =
                            format!("{palette}-s{scale}-f{font}-{width}x{height}-sidebar{sidebar}");
                        let draft = ElementHandle::find_by_element_type_name(&ui, "DraftPane")
                            .next()
                            .unwrap();
                        draft.scroll(0.0, -height * 4.0);
                        mock_elapsed_time(Duration::from_millis(180));
                        let policy = ui
                            .root_element()
                            .query_descendants()
                            .find_all()
                            .into_iter()
                            .find(|e| {
                                e.accessible_label()
                                    .is_some_and(|s| s.starts_with("Copies retain supported"))
                            })
                            .unwrap();
                        policy.scroll(0.0, -height * 4.0);
                        mock_elapsed_time(Duration::from_millis(180));
                        let right = draft.absolute_position().x + draft.size().width;
                        let bottom = draft.absolute_position().y + draft.size().height;
                        assert_reachable_bottom(&policy, right, bottom);
                        capture_window(&ui, &directory, &format!("{label}-empty-bottom"));
                        let source = root.path().join(format!(
                            "Ābols-日本語-العربية-{}-message.eml",
                            "descriptive-".repeat(8)
                        ));
                        fs::write(&source, b"Content-Type: text/plain\r\n\r\nbody\r\n").unwrap();
                        let stream = |name: &str| {
                            let mut records: Vec<serde_json::Value> = fixture(name)
                                .lines()
                                .map(|s| serde_json::from_str(s).unwrap())
                                .collect();
                            records[1]["source"] = serde_json::json!({"display":source.to_str().unwrap(),"native":eml_protocol::NativePathValue::new(&source)});
                            records[1]["removed"] = serde_json::json!([{"section":"2","content_type":"image/png","filename":"inline-image.png","bytes":52,"reason":"related_resource"}]);
                            records.iter().map(|r| format!("{r}\n")).collect::<String>()
                        };
                        let check = font == 20.0;
                        script(
                            root.path(),
                            &("cat >/dev/null\n".to_owned()
                                + &print(&stream(if check {
                                    "planned-create-unix"
                                } else {
                                    "created"
                                }))),
                        );
                        choose_operation(&ui, if check { "Check only" } else { "Create copies" });
                        c.stage(vec![source.clone()]).unwrap();
                        draft.scroll(0.0, -height * 4.0);
                        mock_elapsed_time(Duration::from_millis(180));
                        let action = ElementHandle::find_by_accessible_label(
                            &ui,
                            ui.get_start_label().as_str(),
                        )
                        .find(|e| e.accessible_role() == Some(AccessibleRole::Button))
                        .unwrap();
                        action.scroll(0.0, -height * 4.0);
                        mock_elapsed_time(Duration::from_millis(180));
                        assert_reachable_bottom(&action, right, bottom);
                        let explanation = ElementHandle::find_by_accessible_label(&ui, if check {
                            "Nothing is written. Each message and the destination are checked."
                        } else {
                            "Copies are named ‹name›.no-attachments.eml. Existing files are never overwritten."
                        }).next().unwrap();
                        println!(
                            "FOOTER {label}: {:?} {:?}; viewport bottom {bottom}",
                            explanation.absolute_position(),
                            explanation.size()
                        );
                        assert_reachable_bottom(&explanation, right, bottom);
                        capture_window(&ui, &directory, &format!("{label}-draft-action"));
                        action.mock_single_click(slint::platform::PointerEventButton::Left);
                        pump(&c, || {
                            !c.is_busy() && ui.get_has_reports() && !ui.get_reviewing()
                        });
                        let notice = if check {
                            vocabulary::PREPARED_RELATED_RESOURCE_NOTICE
                        } else {
                            vocabulary::RELATED_RESOURCE_NOTICE
                        };
                        assert_eq!(ui.get_item_policy_notice().as_str(), notice);
                        assert_inspector_text_reachable(&ui, notice);
                        assert_inspector_text_reachable(
                            &ui,
                            "inline-image.png · image/png · 52 bytes",
                        );
                        capture_window(&ui, &directory, &format!("{label}-inspector"));
                        assert_inspector_text_reachable(&ui, "Copy Path");
                        assert_inspector_text_reachable(
                            &ui,
                            if ui.get_can_reveal() {
                                "Show Copy"
                            } else {
                                "Show Original"
                            },
                        );
                        capture_window(&ui, &directory, &format!("{label}-inspector-actions"));
                        script(
                            root.path(),
                            &("cat >/dev/null\n".to_owned()
                                + &print(&stream("published-with-error"))
                                + "exit 1\n"),
                        );
                        ui.invoke_new_request();
                        choose_operation(&ui, "Create copies");
                        c.submit(vec![source]).unwrap();
                        pump(&c, || !c.is_busy());
                        assert_inspector_text_reachable(&ui, ui.get_item_headline().as_str());
                        assert_inspector_text_reachable(&ui, ui.get_item_guidance().as_str());
                        assert_inspector_text_reachable(&ui, vocabulary::RELATED_RESOURCE_NOTICE);
                        capture_window(&ui, &directory, &format!("{label}-failure-inspector"));
                        assert_inspector_text_reachable(&ui, "Copy Path");
                        assert_inspector_text_reachable(&ui, "Show Original");
                        capture_window(&ui, &directory, &format!("{label}-failure-actions"));
                        println!(
                            "PASS {label}: empty policy bottom, populated primary pointer activation, related and failure inspector text reachable"
                        );
                        ui.hide().unwrap();
                    }
                }
            }
        }
    }
}

fn assert_reachable_bottom(element: &ElementHandle, width: f32, height: f32) {
    let p = element.absolute_position();
    let s = element.size();
    assert!(
        p.x >= 0.0 && p.x + s.width <= width + 0.1,
        "horizontal clipping: {p:?} {s:?}"
    );
    assert!(
        p.y + s.height > 0.0 && p.y + s.height <= height + 0.1,
        "bottom unreachable: {p:?} {s:?}"
    );
}
