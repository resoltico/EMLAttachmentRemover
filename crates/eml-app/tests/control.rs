//! Admission/control races are exercised with real threads and immutable IDs.
use eml_app::{RequestSnapshot, RunQueue, WorkerControl};
use std::{
    path::PathBuf,
    sync::{Arc, Barrier},
    thread,
    time::{Duration, Instant},
};
fn control() -> WorkerControl {
    #[cfg(unix)]
    let path = PathBuf::from("/owned-synthetic/message.eml");
    #[cfg(windows)]
    let path = PathBuf::from(r"C:\owned-synthetic\message.eml");
    let mut queue = RunQueue::default();
    let id = queue
        .enqueue(RequestSnapshot::new(vec![path], None).unwrap())
        .unwrap();
    WorkerControl::new(id)
}
#[test]
fn stop_before_late_launch_handoff_withholds_input_and_counts_only_two_requests() {
    let control = control();
    let alias = control.clone();
    assert_eq!(control.id(), alias.id());
    assert!(control.request_stop().unwrap());
    assert!(!alias.begin_feeding().unwrap());
    assert!(alias.request_stop().unwrap());
    assert!(!control.request_stop().unwrap());
    let intent = control.intent(Instant::now()).unwrap();
    assert_eq!(intent.interrupts, 2);
    assert!(!intent.input_may_have_been_sent);
}
#[test]
fn feeding_grant_and_stop_have_one_serialized_boundary_under_real_thread_races() {
    for _ in 0..64 {
        let control = control();
        let worker = control.clone();
        let barrier = Arc::new(Barrier::new(2));
        let other = Arc::clone(&barrier);
        let feed = thread::spawn(move || {
            other.wait();
            worker.begin_feeding().unwrap()
        });
        barrier.wait();
        assert!(control.request_stop().unwrap());
        let granted = feed.join().unwrap();
        let intent = control.intent(Instant::now()).unwrap();
        assert_eq!(intent.input_may_have_been_sent, granted);
        assert_eq!(intent.interrupts, 1);
        assert!(!control.begin_feeding().unwrap());
    }
}
#[test]
fn confirmed_quit_uses_fixed_ten_and_two_second_allowances_without_deadline_extension() {
    let control = control();
    let start = Instant::now();
    control.confirm_shutdown(start).unwrap();
    assert!(!control.begin_feeding().unwrap());
    assert_eq!(
        control
            .intent(start + Duration::from_secs(9))
            .unwrap()
            .interrupts,
        1
    );
    assert_eq!(
        control
            .intent(start + Duration::from_secs(10))
            .unwrap()
            .interrupts,
        2
    );
    assert!(
        !control
            .intent(start + Duration::from_millis(11_999))
            .unwrap()
            .kill_due
    );
    control
        .confirm_shutdown(start + Duration::from_secs(11))
        .unwrap();
    assert!(
        control
            .intent(start + Duration::from_secs(12))
            .unwrap()
            .kill_due
    );
    control.process_reaped().unwrap();
    assert!(!control.request_stop().unwrap());
    assert_eq!(
        control
            .intent(start + Duration::from_secs(100))
            .unwrap()
            .interrupts,
        2
    );
    assert!(
        !control
            .intent(start + Duration::from_secs(100))
            .unwrap()
            .kill_due
    );
}
#[test]
fn late_stop_does_not_change_a_reaped_success_and_input_is_never_granted_twice() {
    let control = control();
    assert!(control.begin_feeding().unwrap());
    assert!(!control.begin_feeding().unwrap());
    control.process_reaped().unwrap();
    assert!(!control.request_stop().unwrap());
    let intent = control.intent(Instant::now()).unwrap();
    assert_eq!(intent.interrupts, 0);
    assert!(intent.reaped);
    assert!(intent.input_may_have_been_sent);
}
#[test]
fn granted_input_is_not_reclassified_as_withheld_by_a_later_stop() {
    let control = control();
    assert!(control.begin_feeding().unwrap());
    assert!(control.request_stop().unwrap());
    let intent = control.intent(Instant::now()).unwrap();
    assert!(intent.input_may_have_been_sent);
    assert_eq!(intent.interrupts, 1);
    assert!(!control.begin_feeding().unwrap());
}
