use super::*;

#[test]
fn t1_settlement_without_busy_work_is_rendered_on_next_tick() {
    use std::time::{Duration, Instant};
    i_slint_backend_testing::init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("mail.eml"),
        b"Content-Type: text/plain\r\n\r\nbody\r\n",
    )
    .unwrap();
    let c = DesktopController::new(
        eml_core::run,
        root.path().into(),
        preferences::Preferences::load(root.path().join("settings")),
    )
    .unwrap();
    c.stage(vec![root.path().join("mail.eml")]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut r = c.runtime.borrow_mut();
        let a = r.active.as_mut().unwrap();
        if let Some(completion) = a.worker.poll() {
            let (kind, generation) = (a.kind, a.generation);
            let before = r.batch.revision();
            r.batch.settle(kind, generation, completion, false);
            assert!(r.batch.revision() > before);
            r.active = None;
            break;
        }
        assert!(Instant::now() < deadline);
        drop(r);
        std::thread::yield_now();
    }
    assert!(!c.is_busy());
    c.tick();
    assert_eq!(c.window().get_view().title, "1 email");
    assert_eq!(
        c.window().get_rows().row_data(0).unwrap().status,
        "No attachments"
    );
}

#[test]
fn t1_batch_transitions_advance_revision() {
    let mut b = Batch::default();
    let mut before = b.revision();
    b.add(vec!["/mail.eml".into()]);
    assert!(b.revision() > before);
    before = b.revision();
    b.start();
    assert!(b.revision() > before);
    before = b.revision();
    b.reset();
    assert!(b.revision() > before);
}

#[test]
fn t4_states_map_to_exported_enums_without_integer_contracts() {
    for (state, expected) in [
        (State::Empty, BatchState::Empty),
        (State::Checking, BatchState::Checking),
        (State::Ready, BatchState::Ready),
        (State::Removing, BatchState::Removing),
        (State::Stopped, BatchState::Stopped),
        (State::Done, BatchState::Done),
    ] {
        assert_eq!(BatchState::from(state), expected);
    }
    let models = include_str!("../../ui/models.slint");
    assert!(!models.contains("state: int"));
    assert!(!models.contains("tone: int"));
}
