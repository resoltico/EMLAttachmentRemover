//! Fixed existing-sync fixture records; never an operation authority or shipping log.
use super::{BoundDirectory, BoundFile, path::PathContext};
use crate::fixture_evidence::{identity_json, signature_json};
use std::{
    io,
    os::windows::{ffi::OsStrExt, io::AsRawHandle},
    path::Path,
    ptr,
};
use windows_sys::Win32::Storage::FileSystem::GetVolumeInformationByHandleW;

pub(super) fn held_file(file: &BoundFile, readonly: bool) -> io::Result<String> {
    let signature = file.current_signature().map_err(io::Error::other)?;
    if signature.size > 1024 * 1024 {
        return Err(io::Error::other("Fixture readback exceeds 1 MiB"));
    }
    let mut bytes = vec![0; signature.size as usize];
    let mut offset = 0;
    while offset < bytes.len() {
        let count = file.read_at(&mut bytes[offset..], offset as u64)?;
        if count == 0 {
            return Err(io::Error::other("Fixture readback ended early"));
        }
        offset += count;
    }
    let mut extra = [0];
    if file.read_at(&mut extra, signature.size)? != 0
        || file.current_signature().map_err(io::Error::other)? != signature
    {
        return Err(io::Error::other("Fixture file changed during readback"));
    }
    Ok(format!(
        "{{\"signature\":{},\"parent\":{},\"bytes\":{bytes:?},\"readonly\":{readonly}}}",
        signature_json(signature),
        identity_json(file.parent_identity())
    ))
}
/// Independently observe a fixture file, then checked-close that temporary owner.
pub fn file_observation(path: &Path) -> io::Result<String> {
    let context = PathContext::capture(
        path.parent()
            .ok_or_else(|| io::Error::other("Fixture file has no parent"))?,
    )?;
    let file = BoundFile::open(&context.resolve(path)?, false).map_err(io::Error::other)?;
    let observed = file.fixture_file_observation();
    let closed = file.close();
    let value = observed?;
    closed?;
    Ok(value)
}
/// Independently observe the current full file/parent binding and checked-close it.
pub fn file_binding(path: &Path) -> io::Result<(crate::FileSignature, crate::Identity)> {
    let context = PathContext::capture(
        path.parent()
            .ok_or_else(|| io::Error::other("Fixture file has no parent"))?,
    )?;
    let file = BoundFile::open(&context.resolve(path)?, false).map_err(io::Error::other)?;
    let observation = (file.initial_signature(), file.parent_identity());
    file.close()?;
    Ok(observation)
}
/// One finite observation after the actual operation named by the caller returned.
#[derive(Clone, Debug)]
pub struct Step {
    /// Closed operation vocabulary for the qualification reader.
    pub operation: &'static str,
    /// Observed operation result, not an expected-case verdict.
    pub result: &'static str,
    /// Actual signature observation, or explicit unattempted value.
    pub signature: Option<crate::FileSignature>,
    /// Actual parent identity, or explicit unattempted value.
    pub parent: Option<crate::Identity>,
    /// Same-held-source observation, when present for this case.
    pub source: Option<String>,
    /// Actual comparison result, when reached.
    pub bytes_equal: Option<bool>,
    /// Immediate actual native error, when supplied.
    pub native_code: Option<i64>,
    /// Actual native file flush receipt, when reached.
    pub sync: Option<crate::SyncReceipt>,
    /// Actual core namespace receipt JSON, when reached.
    pub namespace: Option<String>,
}
impl Step {
    /// Start one fixed observation with all unattempted fields explicitly absent.
    pub fn new(operation: &'static str, result: &'static str) -> Self {
        Self {
            operation,
            result,
            signature: None,
            parent: None,
            source: None,
            bytes_equal: None,
            native_code: None,
            sync: None,
            namespace: None,
        }
    }
    /// Encode all fields, including required explicit nulls.
    pub fn json(&self) -> String {
        format!(
            "{{\"operation\":\"{}\",\"result\":\"{}\",\"signature\":{},\"parent\":{},\"source\":{},\"bytes_equal\":{},\"native_code\":{},\"sync\":{},\"namespace\":{}}}",
            self.operation,
            self.result,
            self.signature.map_or_else(|| "null".into(), signature_json),
            self.parent.map_or_else(|| "null".into(), identity_json),
            self.source.as_deref().unwrap_or("null"),
            self.bytes_equal
                .map_or_else(|| "null".into(), |v| v.to_string()),
            crate::fixture_evidence::native_code_json(self.native_code),
            self.sync
                .map_or_else(|| "null".into(), crate::fixture_evidence::sync_json),
            self.namespace.as_deref().unwrap_or("null")
        )
    }
}
/// Retain a fixed case in the explicitly configured native test output directory.
/// Ordinary developer tests keep all assertions but need not configure retention.
#[allow(
    clippy::too_many_arguments,
    reason = "This test-only record mirrors the closed native evidence schema; a general recorder would hide the actual observations."
)]
pub fn write_case(
    case: &'static str,
    root: &Path,
    parent: &Path,
    before_source: Option<&str>,
    before_copy: &str,
    steps: &[Step],
    classification: &str,
    native_code: Option<i64>,
    after_source: Option<&str>,
    after_copy: &str,
) -> io::Result<()> {
    let Some(directory) = std::env::var_os("EML_WINDOWS_EVIDENCE_DIR") else {
        return Ok(());
    };
    let name = match case {
        "adapter-handoff-success" => "existing-sync-adapter-handoff-success.json",
        "adapter-readonly-sync-denied" => "existing-sync-adapter-readonly-sync-denied.json",
        "adapter-close-required" => "existing-sync-adapter-close-required.json",
        "adapter-parent-retarget" => "existing-sync-adapter-parent-retarget.json",
        "core-readonly-equal" => "existing-sync-core-readonly-equal.json",
        "core-readonly-unequal" => "existing-sync-core-readonly-unequal.json",
        "core-sync-success" => "existing-sync-core-sync-success.json",
        "core-gap-same-id" => "existing-sync-core-gap-same-id.json",
        "core-gap-replacement" => "existing-sync-core-gap-replacement.json",
        "core-source-alias" => "existing-sync-core-source-alias.json",
        _ => return Err(io::Error::other("Unknown existing-sync evidence case")),
    };
    let context = PathContext::capture(root)?;
    let root_path = context.resolve(root)?;
    let held = BoundDirectory::open(&root_path)?;
    let mut filesystem = [0u16; 32];
    // SAFETY: the existing metadata-only held directory remains live, with a
    // fixed writable filesystem-name buffer; all unused outputs are null.
    if unsafe {
        GetVolumeInformationByHandleW(
            held.file.as_raw_handle(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            filesystem.as_mut_ptr(),
            filesystem.len() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let count = filesystem
        .iter()
        .position(|u| *u == 0)
        .ok_or_else(|| io::Error::other("Unterminated fixture filesystem name"))?;
    if filesystem[..count] != [78, 84, 70, 83] {
        return Err(io::Error::other(
            "Existing-sync qualification fixture is not observed NTFS",
        ));
    }
    let root_units: Vec<u16> = root_path.as_os_str().encode_wide().collect();
    let parent_units: Vec<u16> = context.resolve(parent)?.as_os_str().encode_wide().collect();
    let exe: Vec<u16> = std::fs::canonicalize(std::env::current_exe()?)?
        .as_os_str()
        .encode_wide()
        .collect();
    let steps = steps.iter().map(Step::json).collect::<Vec<_>>().join(",");
    let contents = format!(
        "{{\"schema\":\"eml-windows-existing-sync-evidence\",\"schema_version\":1,\"case\":\"{case}\",\"architecture\":\"{}\",\"process_id\":{},\"current_exe_utf16\":{exe:?},\"fixture\":{{\"root_utf16\":{root_units:?},\"identity\":{},\"filesystem\":\"NTFS\"}},\"requested_parent_utf16\":{parent_units:?},\"before\":{{\"source\":{},\"copy\":{before_copy}}},\"steps\":[{steps}],\"outcome\":{{\"classification\":\"{classification}\",\"native_code\":{}}},\"after\":{{\"source\":{},\"copy\":{after_copy}}}}}",
        std::env::consts::ARCH,
        std::process::id(),
        identity_json(held.identity()),
        before_source.unwrap_or("null"),
        crate::fixture_evidence::native_code_json(native_code),
        after_source.unwrap_or("null")
    );
    crate::fixture_evidence::write_record(Path::new(&directory), name, &contents)
}
