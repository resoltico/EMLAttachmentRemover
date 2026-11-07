//! Desktop contracts through the real Slint testing backend and stub CLI processes.
use eml_gui::{DesktopController, preferences::Preferences};
use i_slint_backend_testing::{init_no_event_loop, mock_elapsed_time};
use slint::ComponentHandle;
use std::{
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    thread,
    time::{Duration, Instant},
};
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
            "Desktop fixture did not reach its expected state: state={} title={} banner={} busy={}",
            controller.window().get_view().state,
            controller.window().get_view().title,
            controller.window().get_view().banner,
            controller.is_busy()
        );
        thread::sleep(Duration::from_millis(2));
    }
}

#[path = "desktop/one_list.rs"]
mod one_list;

#[path = "desktop/acceptance.rs"]
mod acceptance;
