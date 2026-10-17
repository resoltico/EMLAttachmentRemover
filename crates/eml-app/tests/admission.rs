//! Queue reservations and immutable native request integration.
use eml_app::{QueueError, RequestSnapshot, RunQueue};
use eml_core::OutputLocation;
use std::path::PathBuf;

fn path(name: &str) -> PathBuf {
    #[cfg(unix)]
    {
        PathBuf::from("/owned-synthetic").join(name)
    }
    #[cfg(windows)]
    {
        PathBuf::from(r"C:\owned-synthetic").join(name)
    }
}
fn request() -> RequestSnapshot {
    RequestSnapshot::new(vec![path("message.eml")], None).unwrap()
}
#[test]
fn two_reports_pause_without_eviction_and_explicit_dismissal_resumes_in_order() {
    let mut queue = RunQueue::default();
    let first = queue.enqueue(request()).unwrap();
    let second = queue.enqueue(request()).unwrap();
    let third = queue.enqueue(request()).unwrap();
    assert_eq!(queue.start_next().unwrap().unwrap().0, first);
    assert!(queue.start_next().unwrap().is_none());
    assert!(!queue.dismiss(first));
    assert!(matches!(
        queue.complete(second),
        Err(QueueError::InactiveRun)
    ));
    queue.complete(first).unwrap();
    assert_eq!(queue.start_next().unwrap().unwrap().0, second);
    queue.complete(second).unwrap();
    assert!(matches!(queue.start_next(), Err(QueueError::ReportSpace)));
    assert_eq!(queue.reports(), &[first, second]);
    assert_eq!(queue.waiting(), 1);
    assert!(queue.dismiss(first));
    assert_eq!(queue.start_next().unwrap().unwrap().0, third);
}
#[test]
fn waiting_capacity_cancellation_and_removal_do_not_discard_active_results() {
    let mut queue = RunQueue::default();
    let active = queue.enqueue(request()).unwrap();
    queue.start_next().unwrap().unwrap();
    let mut waiting = Vec::new();
    for _ in 0..8 {
        waiting.push(queue.enqueue(request()).unwrap());
    }
    assert!(matches!(queue.enqueue(request()), Err(QueueError::Full)));
    queue.remove_waiting(waiting[2]);
    assert_eq!(queue.waiting(), 7);
    queue.enqueue(request()).unwrap();
    queue.cancel_waiting();
    assert_eq!(queue.waiting(), 0);
    assert_eq!(queue.reports(), &[active]);
    assert!(!queue.dismiss(active));
    queue.complete(active).unwrap();
    assert!(queue.dismiss(active));
}
#[test]
fn destination_is_a_snapshot_and_wire_contains_required_null() {
    let destination = path("copies");
    let selected =
        RequestSnapshot::new(vec![path("message.eml")], Some(destination.clone())).unwrap();
    assert_eq!(
        selected.request().output,
        OutputLocation::Directory(destination)
    );
    let beside = request();
    let wire: serde_json::Value = serde_json::from_slice(beside.encoded()).unwrap();
    assert!(wire.as_object().unwrap().contains_key("output_directory"));
    assert!(wire["output_directory"].is_null());
    assert_eq!(wire["protocol"], 1);
    assert_eq!(wire["dry_run"], false);
    let decoded = eml_protocol::decode_request(
        selected.encoded(),
        selected.request().working_directory.clone(),
    )
    .unwrap();
    assert_eq!(&decoded, selected.request());
}
#[test]
fn app_rejects_empty_relative_and_core_oversized_requests_before_queueing() {
    assert!(RequestSnapshot::new(vec![], None).is_err());
    assert!(RequestSnapshot::new(vec![PathBuf::from("relative.eml")], None).is_err());
    assert!(
        RequestSnapshot::new(vec![path("message.eml")], Some(PathBuf::from("relative"))).is_err()
    );
    assert!(matches!(
        RequestSnapshot::new(vec![path("message.eml"); 4097], None),
        Err(QueueError::InvalidRequest(_))
    ));
}
#[cfg(unix)]
#[test]
fn request_preserves_non_unicode_paths_without_using_display_text() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let native = PathBuf::from(OsString::from_vec(b"/owned-synthetic/\xff.eml".to_vec()));
    let request = RequestSnapshot::new(vec![native.clone()], None).unwrap();
    let value: serde_json::Value = serde_json::from_slice(request.encoded()).unwrap();
    assert_eq!(
        value["paths"][0]["base64"],
        "L293bmVkLXN5bnRoZXRpYy//LmVtbA=="
    );
    assert_eq!(
        eml_protocol::decode_request(request.encoded(), PathBuf::from("/"))
            .unwrap()
            .paths,
        [native]
    );
}
