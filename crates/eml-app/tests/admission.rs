//! Queue reservations and immutable native request integration.
use eml_app::{QueueError, RequestSnapshot, RunQueue};
use eml_core::{OutputLocation, RunMode};
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
    RequestSnapshot::new(vec![path("message.eml")], None, RunMode::Create).unwrap()
}
#[test]
fn s1_twenty_retained_runs_start_without_consent_and_session_cap_rejects() {
    let mut queue = RunQueue::default();
    for _ in 0..20 {
        let id = queue.enqueue(request()).unwrap();
        assert_eq!(queue.start_next().unwrap().unwrap().0, id);
        assert!(queue.start_next().unwrap().is_none());
        assert!(!queue.dismiss(id));
        queue.complete(id).unwrap();
    }
    assert!(matches!(
        queue.enqueue(request()),
        Err(QueueError::SessionFull)
    ));
    let first = queue.reports()[0];
    assert!(queue.dismiss(first));
    assert!(queue.enqueue(request()).is_ok());
}
#[test]
fn s2_stored_byte_cap_rejects_without_overcharging_and_close_releases_bytes() {
    let mut queue = RunQueue::default();
    let first = queue.enqueue(request()).unwrap();
    queue.start_next().unwrap();
    queue.complete(first).unwrap();
    let second = queue.enqueue(request()).unwrap();
    queue.start_next().unwrap();
    queue.complete(second).unwrap();
    let cap = eml_app::MAX_STORED_REPORT_BYTES;
    assert!(matches!(
        queue.store_report(first, cap + 1),
        Err(QueueError::StoredBytesFull)
    ));
    assert_eq!(queue.stored_bytes(), 0);
    queue.store_report(first, cap).unwrap();
    assert!(matches!(
        queue.store_report(second, 1),
        Err(QueueError::StoredBytesFull)
    ));
    assert_eq!(queue.stored_bytes(), cap);
    assert!(matches!(
        queue.enqueue(request()),
        Err(QueueError::StoredBytesFull)
    ));
    assert!(queue.dismiss(first));
    assert_eq!(queue.stored_bytes(), 0);
    assert!(queue.enqueue(request()).is_ok());
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
    assert_eq!(
        queue
            .waiting_requests()
            .map(|(id, _)| id)
            .collect::<Vec<_>>(),
        waiting
    );
    assert!(queue.remove_waiting(waiting[2]));
    assert!(!queue.remove_waiting(waiting[2]));
    assert!(!queue.remove_waiting(active));
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
    let selected = RequestSnapshot::new(
        vec![path("message.eml")],
        Some(destination.clone()),
        RunMode::Create,
    )
    .unwrap();
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
    assert!(RequestSnapshot::new(vec![], None, RunMode::Create).is_err());
    assert!(
        RequestSnapshot::new(vec![PathBuf::from("relative.eml")], None, RunMode::Create).is_err()
    );
    assert!(
        RequestSnapshot::new(
            vec![path("message.eml")],
            Some(PathBuf::from("relative")),
            RunMode::Create
        )
        .is_err()
    );
    assert!(matches!(
        RequestSnapshot::new(vec![path("message.eml"); 4097], None, RunMode::Create),
        Err(QueueError::InvalidRequest(_))
    ));
}
#[cfg(unix)]
#[test]
fn request_preserves_non_unicode_paths_without_using_display_text() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let native = PathBuf::from(OsString::from_vec(b"/owned-synthetic/\xff.eml".to_vec()));
    let request = RequestSnapshot::new(vec![native.clone()], None, RunMode::Create).unwrap();
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
