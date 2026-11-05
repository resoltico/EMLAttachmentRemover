//! Owned-task settlement across confirmed native Quit decisions.
use super::*;
use eml_app::RunQueue;
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

#[test]
fn s14_theme_text_and_status_contrast_in_both_schemes() {
    use std::collections::BTreeMap;
    fn luminance(rgb: u32) -> f64 {
        let channels = [rgb >> 16, rgb >> 8, rgb];
        channels
            .into_iter()
            .zip([0.2126, 0.7152, 0.0722])
            .map(|(v, weight)| {
                let c = f64::from(v & 255) / 255.0;
                weight
                    * if c <= 0.04045 {
                        c / 12.92
                    } else {
                        ((c + 0.055) / 1.055).powf(2.4)
                    }
            })
            .sum()
    }
    fn ratio(a: u32, b: u32) -> f64 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
    assert_eq!(ratio(0, 0xffffff), 21.0);
    assert_eq!(ratio(0x777777, 0x777777), 1.0);
    let tokens: BTreeMap<_, Vec<_>> = include_str!("../ui/theme.slint")
        .lines()
        .filter(|line| line.contains("<color>"))
        .map(|line| {
            let name = line
                .split('>')
                .nth(1)
                .unwrap()
                .split(':')
                .next()
                .unwrap()
                .trim();
            let colors = line
                .split('#')
                .skip(1)
                .map(|s| u32::from_str_radix(&s[..6], 16).unwrap())
                .collect();
            (name, colors)
        })
        .collect();
    for background in ["sidebar", "surface", "raised", "accent-soft"] {
        for foreground in ["text", "muted", "accent", "ok", "warn", "bad", "idle"] {
            assert_eq!(tokens[foreground].len(), 2);
            assert_eq!(tokens[background].len(), 2);
            for (scheme, (&front, &back)) in tokens[foreground]
                .iter()
                .zip(&tokens[background])
                .enumerate()
            {
                let contrast = ratio(front, back);
                assert!(
                    contrast >= 4.5,
                    "{foreground} on {background}, scheme {scheme}: {contrast}"
                );
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn storage_failure_preserves_bytes_and_three_memory_reports_block_admission_until_closed() {
    i_slint_backend_testing::init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = DesktopController::new(
        root.path().join("unused-cli"),
        root.path().into(),
        preferences::Preferences::load(root.path().join("preferences.json")),
    )
    .unwrap();
    let mut seed = None;
    for index in 0..3 {
        let request = RequestSnapshot::new(
            vec![root.path().join(format!("{index}.eml"))],
            None,
            RunMode::Create,
        )
        .unwrap();
        let mut s = c.session.borrow_mut();
        let id = s.state.queue.enqueue(request.clone()).unwrap();
        s.state.queue.start_next().unwrap();
        s.state.queue.complete(id).unwrap();
        let mut worker =
            WorkerRun::start(Path::new("/usr/bin/false"), &request, root.path()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut completion = loop {
            if let Some(completion) = worker.poll().unwrap() {
                break completion;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        };
        if seed.is_none() {
            seed = Some(eml_app::StoredReport::write(&completion, RunMode::Create).unwrap());
        }
        // Simulate a reader failure that retained observations but lost the
        // corresponding stream: storage must reject replay and keep the original.
        completion.jsonl = b"exact captured partial stream".to_vec().into();
        let storage_error = eml_app::StoredReport::write(&completion, RunMode::Create)
            .err()
            .expect("non-replayable report")
            .to_string();
        s.entries.push(Entry {
            id,
            request,
            report: Some(completion.report.clone()),
            completion: Some(completion),
            launch_error: None,
            worker_error: None,
            presentation: state::RunPresentation::default(),
            storage_error: Some(storage_error),
            load_error: None,
            stored: None,
        });
    }
    c.stage(vec![root.path().join("next.eml")]).unwrap();
    assert!(!c.window().get_can_start());
    assert_eq!(
        c.window().get_start_label(),
        "Waiting for space — close a finished run"
    );
    assert!(matches!(
        c.start_selection(),
        Err(QueueError::MemoryReportsFull)
    ));
    c.maintain_reports();
    assert!(
        c.session.borrow().report_task.is_none(),
        "Failed writes must not retry silently"
    );
    for entry in &c.session.borrow().entries {
        assert_eq!(
            &*entry.completion.as_ref().unwrap().jsonl,
            b"exact captured partial stream"
        );
    }
    // An older backed report must not become a fourth resident report.
    let request =
        RequestSnapshot::new(vec![root.path().join("backed.eml")], None, RunMode::Create).unwrap();
    let token = {
        let mut s = c.session.borrow_mut();
        let id = s.state.queue.enqueue(request.clone()).unwrap();
        s.state.queue.start_next().unwrap();
        s.state.queue.complete(id).unwrap();
        s.entries.push(Entry {
            id,
            request,
            report: None,
            completion: None,
            launch_error: None,
            worker_error: None,
            presentation: state::RunPresentation::default(),
            storage_error: None,
            load_error: None,
            stored: seed,
        });
        id.to_string()
    };
    c.window().invoke_select_session(token.into());
    assert!(
        c.session
            .borrow()
            .selected_entry()
            .unwrap()
            .report
            .is_none()
    );
    assert!(c.session.borrow().report_task.is_none());
    assert!(c.window().get_run_notice().contains("Close a finished run"));
    let first = c.window().get_runs().row_data(0).unwrap().token;
    c.window().invoke_close_session(first);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while c.is_busy() {
        c.tick();
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(
        c.session
            .borrow()
            .selected_entry()
            .unwrap()
            .report
            .is_some()
    );
    assert!(
        c.session.borrow().memory_full(),
        "Loaded backed reports count toward residency too"
    );
    c.window().invoke_new_request();
    assert!(
        c.window().get_can_start(),
        "The hidden backed report is evicted before draft admission"
    );
    assert_eq!(c.session.borrow().entries.len(), 3);
}

#[test]
fn s2_session_and_stored_byte_caps_disable_start_until_a_result_is_closed() {
    i_slint_backend_testing::init_no_event_loop();
    for bytes_full in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let c = DesktopController::new(
            root.path().join("unused-cli"),
            root.path().into(),
            preferences::Preferences::load(root.path().join("preferences.json")),
        )
        .unwrap();
        let count = if bytes_full {
            11
        } else {
            eml_app::MAX_SESSION_REQUESTS
        };
        let mut last = None;
        // Seed settled reservations at the pure queue boundary. No multi-GiB
        // allocation is needed to exercise the GUI's admission and close actions.
        for index in 0..count {
            let request = RequestSnapshot::new(
                vec![root.path().join(format!("{index}.eml"))],
                None,
                RunMode::Create,
            )
            .unwrap();
            let mut s = c.session.borrow_mut();
            let id = s.state.queue.enqueue(request.clone()).unwrap();
            s.state.queue.start_next().unwrap();
            s.state.queue.complete(id).unwrap();
            if bytes_full {
                let base = eml_app::MAX_STORED_REPORT_BYTES / count as u64;
                let bytes = if index + 1 == count {
                    eml_app::MAX_STORED_REPORT_BYTES - base * (count as u64 - 1)
                } else {
                    base
                };
                s.state.queue.store_report(id, bytes).unwrap();
            }
            s.entries.push(Entry {
                id,
                request,
                report: None,
                completion: None,
                launch_error: Some("Owned settled fixture".into()),
                worker_error: None,
                presentation: state::RunPresentation::default(),
                storage_error: None,
                load_error: None,
                stored: None,
            });
            last = Some(id);
        }
        c.stage(vec![root.path().join("next.eml")]).unwrap();
        assert!(!c.window().get_can_start());
        assert_eq!(
            c.window().get_start_label(),
            "Close a finished run to add more"
        );
        assert!(matches!(
            c.start_selection(),
            Err(QueueError::SessionFull | QueueError::StoredBytesFull)
        ));
        c.window()
            .invoke_close_session(last.unwrap().to_string().into());
        assert!(c.window().get_can_start());
        assert_eq!(c.window().get_selection().row_count(), 1);
    }
}
