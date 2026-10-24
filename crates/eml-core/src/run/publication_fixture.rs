//! Records for the finite core publication controls, not a product report codec.
use super::*;
use eml_fs::fixture_evidence::{
    cleanup_json, identity_json, inventory_json, native_code_json, signature_json, sync_json,
    write_record,
};
use std::{fs, path::Path};
const INPUT: &[u8] =
    include_bytes!("../../../../tests/contract/mime/inputs/01-gmail-attachment.eml");
const GOLDEN: &[u8] =
    include_bytes!("../../../../tests/contract/mime/golden/01-gmail-attachment.eml");
const FINAL: &str = "a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml";
fn write(name: &'static str, case: &str, origin: &str, fields: &str) {
    if let Some(directory) = std::env::var_os("EML_NATIVE_PUBLICATION_EVIDENCE_DIR") {
        let document = format!(
            "{{\"schema\":1,\"case\":\"{case}\",\"origin\":\"{origin}\",\"process_os\":\"{}\",\"process_architecture\":\"{}\",{fields}}}",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        write_record(Path::new(&directory), name, &document).unwrap();
    }
}
fn signature(root: &Path, name: &str) -> FileSignature {
    BoundDirectory::open(&root.canonicalize().unwrap())
        .unwrap()
        .observe_child(std::ffi::OsStr::new(name))
        .unwrap()
        .file
        .unwrap()
}
pub(super) struct Before {
    bytes: Vec<u8>,
    signature: FileSignature,
    inventory: String,
}
pub(super) fn before(root: &Path) -> Before {
    Before {
        bytes: fs::read(root.join("a.eml")).unwrap(),
        signature: signature(root, "a.eml"),
        inventory: inventory_json(root).unwrap(),
    }
}
fn receipt(result: &ItemResult<'_>) -> String {
    let (classification, code, phase, native, effect, id, address, file, namespace, cleanup) =
        match result {
            ItemResult::Created(copy, _) => {
                let p = copy.publication();
                (
                    "Created",
                    "null".into(),
                    "null".into(),
                    None,
                    "Published",
                    Some(p.identity()),
                    p.address(),
                    p.file_sync(),
                    p.namespace_sync(),
                    p.cleanup(),
                )
            }
            ItemResult::Failed(f) => {
                let (effect, id, address, file, namespace, cleanup) = match f.effect() {
                    FailureEffect::NotPublished {
                        candidate_identity,
                        address,
                        file_sync,
                        namespace_sync,
                        cleanup,
                    } => (
                        "NotPublished",
                        *candidate_identity,
                        *address,
                        *file_sync,
                        *namespace_sync,
                        cleanup,
                    ),
                    FailureEffect::Published(p) => (
                        "Published",
                        Some(p.identity()),
                        p.address(),
                        p.file_sync(),
                        p.namespace_sync(),
                        p.cleanup(),
                    ),
                    FailureEffect::Unknown(p) => (
                        "Unknown",
                        p.candidate_identity(),
                        p.address(),
                        p.file_sync(),
                        p.namespace_sync(),
                        p.cleanup(),
                    ),
                };
                (
                    "Failed",
                    format!("\"{:?}\"", f.error().code()),
                    format!("\"{:?}\"", f.error().phase()),
                    f.error()
                        .native_error()
                        .and_then(std::io::Error::raw_os_error)
                        .map(i64::from),
                    effect,
                    id,
                    address,
                    file,
                    namespace,
                    cleanup,
                )
            }
            _ => panic!("Unexpected publication fixture result"),
        };
    format!(
        "{{\"classification\":\"{classification}\",\"error_code\":{code},\"error_phase\":{phase},\"native_code\":{},\"effect\":\"{effect}\",\"candidate_identity\":{},\"address\":\"{address:?}\",\"file_sync\":{},\"namespace\":{},\"cleanup\":{}}}",
        native_code_json(native),
        id.map_or_else(|| "null".into(), identity_json),
        file.map_or_else(|| "null".into(), sync_json),
        namespace.fixture_json(),
        cleanup_json(cleanup)
    )
}
pub(super) fn stop(
    root: &Path,
    after: bool,
    before: Before,
    phases: &[eml_fs::StoragePhase],
    result: &ItemResult<'_>,
) {
    let final_file = match fs::read(root.join(FINAL)) {
        Ok(bytes) => format!(
            "{{\"signature\":{},\"bytes\":{bytes:?}}}",
            signature_json(signature(root, FINAL))
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "null".into(),
        Err(error) => panic!("Final fixture observation failed: {error}"),
    };
    let phases = phases
        .iter()
        .map(|phase| format!("\"{phase:?}\""))
        .collect::<Vec<_>>()
        .join(",");
    let fields = format!(
        "\"engine_phases\":[{phases}],\"source\":{{\"expected\":{INPUT:?},\"before\":{:?},\"after\":{:?},\"signature_before\":{},\"signature_after\":{}}},\"candidate_expected\":{GOLDEN:?},\"final\":{final_file},\"receipt\":{},\"inventory_before\":{},\"inventory_after\":{}",
        before.bytes,
        fs::read(root.join("a.eml")).unwrap(),
        signature_json(before.signature),
        signature_json(signature(root, "a.eml")),
        receipt(result),
        before.inventory,
        inventory_json(root).unwrap()
    );
    write(
        if after {
            "core-stop-after.json"
        } else {
            "core-stop-before.json"
        },
        if after {
            "core-stop-after"
        } else {
            "core-stop-before"
        },
        "actual_core_with_controlled_stop",
        &fields,
    );
}
pub(super) fn translated(
    stopped: bool,
    error: &OperationFailure,
    file: Option<eml_fs::SyncReceipt>,
    cleanup: &CleanupReceipt,
) -> String {
    format!(
        "{{\"checkpoint_stopped\":{stopped},\"original_error_kind\":\"{:?}\",\"translated_code\":\"{:?}\",\"translated_phase\":\"{:?}\",\"file_sync\":{},\"cleanup\":{}}}",
        error.native_error().unwrap().kind(),
        error.code(),
        error.phase(),
        file.map_or_else(|| "null".into(), sync_json),
        cleanup_json(cleanup)
    )
}
pub(super) fn cleanup_causes(rows: &[String]) {
    write(
        "controlled-cleanup-cause.json",
        "controlled-cleanup-cause",
        "controlled_stage_failure_translation",
        &format!("\"cases\":[{}]", rows.join(",")),
    );
}
