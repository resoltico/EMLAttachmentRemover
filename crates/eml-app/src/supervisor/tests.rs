use super::*;
#[test]
fn export_refuses_busy_model_without_waiting_for_inspector() {
    let report = ReportHandle::new("test-version".into());
    let held = report.0.lock().unwrap();
    let captured = report.clone();
    let (send, receive) = std::sync::mpsc::channel();
    let actor = thread::spawn(move || {
        send.send(captured.export_bytes().map_err(|error| error.kind()))
            .unwrap();
    });
    let observed = receive.recv_timeout(Duration::from_secs(1));
    drop(held);
    actor.join().unwrap();
    assert_eq!(observed.unwrap(), Err(io::ErrorKind::WouldBlock));
}
#[test]
fn held_final_report_warns_then_finishes_without_erasing_the_warning() {
    use std::sync::mpsc;
    let report = Arc::new(Mutex::new(RunReport::new("test-version")));
    let health = Arc::new(Mutex::new(None));
    let (locked, wait_locked) = mpsc::sync_channel(1);
    let (release, wait_release) = mpsc::sync_channel(1);
    let held = Arc::clone(&report);
    let holder = thread::spawn(move || {
        let _guard = held.lock().unwrap();
        locked.send(()).unwrap();
        wait_release.recv_timeout(Duration::from_secs(6)).unwrap();
    });
    wait_locked.recv_timeout(Duration::from_secs(1)).unwrap();
    let observed = Arc::clone(&health);
    let (done, wait_done) = mpsc::sync_channel(1);
    let owner = thread::spawn(move || {
        let result = finalize_report(&report, &observed, |_| 17);
        done.send(result).unwrap();
    });
    let deadline = Instant::now() + Duration::from_secs(4);
    while health.lock().unwrap().is_none() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    let warning = health.lock().unwrap().clone();
    let premature = wait_done.try_recv();
    release.send(()).unwrap();
    holder.join().unwrap();
    let result = wait_done.recv_timeout(Duration::from_secs(1)).unwrap();
    owner.join().unwrap();
    assert!(warning.is_some(), "No bounded pending warning");
    assert!(matches!(premature, Err(mpsc::TryRecvError::Empty)));
    assert_eq!(result, Ok(17));
    assert_eq!(*health.lock().unwrap(), warning);
}
#[test]
fn poisoned_final_report_is_not_recovered_as_a_qualified_empty_report() {
    let report = Arc::new(Mutex::new(RunReport::new("test-version")));
    let held = Arc::clone(&report);
    assert!(
        thread::spawn(move || {
            let _guard = held.lock().unwrap();
            panic!("synthetic report failure");
        })
        .join()
        .is_err()
    );
    let health = Arc::new(Mutex::new(None));
    assert_eq!(
        finalize_report(&report, &health, |_| ()),
        Err(ProtocolFailure::MalformedRecord)
    );
    assert!(health.lock().unwrap().is_some());
    assert!(report.is_poisoned());
}
#[test]
fn independent_prelaunch_abort_is_not_hidden_by_either_stop_order() {
    use crate::RunQueue;
    for abort_first in [false, true] {
        let request =
            RequestSnapshot::new(vec![PathBuf::from("/owned-synthetic/message.eml")], None)
                .unwrap();
        let mut queue = RunQueue::default();
        let id = queue.enqueue(request.clone()).unwrap();
        let token = WorkerControl::new(id);
        if abort_first {
            token.abort_transport().unwrap();
        }
        token.request_stop().unwrap();
        if !abort_first {
            token.abort_transport().unwrap();
        }
        let health = Arc::new(Mutex::new(Some("Earlier retained warning".into())));
        let completion = execute(
            PathBuf::from("/owned-synthetic/absent-helper"),
            "test-version".into(),
            request,
            token,
            Arc::new(Mutex::new(RunReport::new("test-version"))),
            health,
        );
        assert!(!completion.reusable);
        assert_eq!(completion.failure, Some(WorkerFailure::Transport));
        assert!(!completion.input_may_have_been_sent);
        assert_eq!(completion.faults, ["Earlier retained warning"]);
    }
}
