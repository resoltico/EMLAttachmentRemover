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
    controller.session.borrow_mut().export = Some(ExportJob {
        run: id,
        path: root.path().join("report.jsonl"),
        task,
    });
    assert!(!controller.request_quit());
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(5));
    controller
        .window()
        .window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed {
            text: slint::platform::Key::Escape.into(),
        });
    assert_eq!(controller.window().get_confirmation(), Confirmation::None);
    assert!(
        !controller.should_exit(),
        "Escape cancels the decision, not its owned export"
    );
    assert!(controller.is_busy());
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

#[test]
fn native_open_is_nonblocking_scoped_and_clears_a_recovered_error() {
    i_slint_backend_testing::init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = DesktopController::new(
        root.path().join("unused-cli"),
        root.path().into(),
        preferences::Preferences::load(root.path().join("preferences.json")),
    )
    .unwrap();
    let (release, wait) = mpsc::channel();
    c.open_advisory(
        OpenTarget::Licenses,
        "Cannot open notices. Try again.",
        move || {
            wait.recv().unwrap();
            Err("missing fixture".into())
        },
    );
    // The UI thread can process a separate operation before the native call ends.
    c.message("A separate operation completed.");
    assert_eq!(c.licenses.get_notice(), "Sending open request…");
    release.send(()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !c.open_jobs.borrow().is_empty() {
        c.tick();
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(c.licenses.get_notice(), "Cannot open notices. Try again.");
    assert_eq!(c.licenses.get_notice_details(), "missing fixture");
    assert_eq!(
        c.ui.get_operation_feedback(),
        "A separate operation completed."
    );
    c.open_advisory(
        OpenTarget::Licenses,
        "Cannot open notices. Try again.",
        || Ok(()),
    );
    assert_eq!(c.licenses.get_notice_details(), "");
    while !c.open_jobs.borrow().is_empty() {
        c.tick();
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(c.licenses.get_notice(), "Open request sent.");
    assert_eq!(c.licenses.get_notice_details(), "");
    let (release, wait) = mpsc::channel();
    c.open_advisory(OpenTarget::Main, "Cannot reveal.", move || {
        wait.recv().unwrap();
        Err("obsolete native failure".into())
    });
    c.message("Newer operation.");
    release.send(()).unwrap();
    while !c.open_jobs.borrow().is_empty() {
        c.tick();
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(c.ui.get_operation_feedback(), "Newer operation.");
}

#[test]
fn quitting_does_not_wait_for_private_report_cache_io() {
    i_slint_backend_testing::init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = DesktopController::new(
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
    c.session.borrow_mut().report_task = Some(ReportTask::Store(
        id,
        std::thread::spawn(move || {
            wait.recv().unwrap();
            Err(io::Error::other("cancelled cache fixture"))
        }),
    ));
    assert!(c.is_busy());
    assert!(c.request_quit());
    assert!(c.should_exit());
    assert!(
        !c.is_busy(),
        "Private unnamed storage must not become an owned user export"
    );
    release.send(()).unwrap();
}
