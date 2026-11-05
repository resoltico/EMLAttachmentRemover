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
