//! Owned-task settlement across confirmed native Quit decisions.
use super::*;
use std::sync::mpsc;

#[test]
fn confirmed_force_quit_waits_for_an_owned_report_export_to_settle() {
    i_slint_backend_testing::init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let controller = DesktopController::new(
        root.path().join("unused-cli"),
        root.path().into(),
        preferences::Preferences::load(root.path().join("preferences.json")),
    )
    .unwrap();
    let mut queue = RunQueue::default();
    let id = queue
        .enqueue(
            RequestSnapshot::new(vec![root.path().join("message.eml")], None, RunMode::Create)
                .unwrap(),
        )
        .unwrap();
    let (release, wait) = mpsc::channel();
    let task = std::thread::spawn(move || {
        wait.recv().unwrap();
        Ok(())
    });
    controller.session.borrow_mut().export = Some(ExportJob { run: id, task });
    assert!(!controller.request_quit());
    controller.window().invoke_force_and_quit();
    let waited_for_export = !controller.should_exit();
    let ignored_repeated_quit =
        !controller.request_quit() && controller.window().get_confirmation() == Confirmation::None;
    release.send(()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while controller.is_busy() {
        controller.tick();
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(
        waited_for_export,
        "Quit must not abandon an owned export thread"
    );
    assert!(
        ignored_repeated_quit,
        "A committed shutdown must not reopen a contradictory Quit decision"
    );
    assert!(controller.should_exit());
}
