//! Immutable request boundary checks.
use eml_app::{RequestError, RequestSnapshot};
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
        Err(RequestError::InvalidRequest(_))
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
