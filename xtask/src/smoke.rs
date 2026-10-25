//! Real extracted-CLI behavior against independent synthetic message oracles.
use crate::{Result, controller};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fmt, fs, io::Read, path::Path, process::Command, time::Duration};

const INPUT: &[u8] = include_bytes!("../../tests/contract/mime/inputs/01-gmail-attachment.eml");
const GOLDEN: &[u8] = include_bytes!("../../tests/contract/mime/golden/01-gmail-attachment.eml");
const PROTECTED: &[u8] = include_bytes!("../../tests/contract/mime/inputs/06-smime-signed.eml");
const MAX_DOCUMENT: u64 = 1024 * 1024;

fn extracted_command(executable: &Path, directory: &Path) -> Command {
    let mut command = Command::new(executable);
    command.current_dir(directory);
    // Explicit removals survive Controller serialization; env_clear does not.
    for (name, _) in std::env::vars_os() {
        if name.as_encoded_bytes().starts_with(b"LD_")
            || name.as_encoded_bytes().starts_with(b"DYLD_")
        {
            command.env_remove(name);
        }
    }
    for name in [
        "LD_LIBRARY_PATH",
        "LD_PRELOAD",
        "LD_AUDIT",
        "DYLD_LIBRARY_PATH",
        "DYLD_FALLBACK_LIBRARY_PATH",
        "DYLD_FRAMEWORK_PATH",
        "DYLD_FALLBACK_FRAMEWORK_PATH",
        "DYLD_ROOT_PATH",
        "DYLD_INSERT_LIBRARIES",
        "DYLD_IMAGE_SUFFIX",
    ] {
        command.env_remove(name);
    }
    command
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FailureKind {
    Framing,
    Contract,
    GoldenCopy,
    SourceChanged,
    Inventory,
    Process,
}
#[derive(Debug)]
pub(crate) struct SmokeFailure {
    pub kind: FailureKind,
    detail: String,
}
impl fmt::Display for SmokeFailure {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(output, "CLI smoke {:?}: {}", self.kind, self.detail)
    }
}
impl std::error::Error for SmokeFailure {}
fn failure(kind: FailureKind, detail: impl Into<String>) -> Box<dyn std::error::Error> {
    Box::new(SmokeFailure {
        kind,
        detail: detail.into(),
    })
}

#[derive(Clone, Copy)]
enum Case {
    Created,
    UpToDate,
    DryRun,
    Conflict,
    Protected,
}
impl Case {
    fn name(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::UpToDate => "up_to_date",
            Self::DryRun => "dry_run",
            Self::Conflict => "conflict",
            Self::Protected => "protected",
        }
    }
    fn exit(self) -> i32 {
        if matches!(self, Self::Conflict | Self::Protected) {
            1
        } else {
            0
        }
    }
    fn status(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::UpToDate => "up_to_date",
            Self::DryRun => "planned",
            Self::Conflict | Self::Protected => "failed",
        }
    }
}

fn document(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > MAX_DOCUMENT {
        return Err(failure(
            FailureKind::Framing,
            "output is not a bounded ordinary document",
        ));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_DOCUMENT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_DOCUMENT {
        return Err(failure(FailureKind::Framing, "output exceeds its bound"));
    }
    Ok(bytes)
}
fn frames(bytes: &[u8]) -> Result<Vec<Value>> {
    if bytes.last() != Some(&b'\n') {
        return Err(failure(FailureKind::Framing, "missing final LF"));
    }
    let lines: Vec<_> = bytes[..bytes.len() - 1]
        .split(|byte| *byte == b'\n')
        .collect();
    if lines.len() != 3 || lines.iter().any(|line| line.is_empty()) {
        return Err(failure(
            FailureKind::Framing,
            "expected exactly start/item/summary, without blank or extra frames",
        ));
    }
    lines
        .into_iter()
        .map(|line| {
            serde_json::from_slice(line)
                .map_err(|error| failure(FailureKind::Framing, error.to_string()))
        })
        .collect()
}
fn field<'a>(value: &'a Value, name: &str) -> Result<&'a Value> {
    value
        .as_object()
        .and_then(|object| object.get(name))
        .ok_or_else(|| {
            failure(
                FailureKind::Contract,
                format!("missing required field {name}"),
            )
        })
}
// Match required schema/value facts while allowing additive event fields.
// In particular an absent nullable member never compares equal to JSON null.
fn required(actual: &Value, expected: &Value) -> Result<()> {
    match expected {
        Value::Object(object) => {
            if !actual.is_object() {
                return Err(failure(
                    FailureKind::Contract,
                    "required object has wrong type",
                ));
            }
            for (name, value) in object {
                required(field(actual, name)?, value)?;
            }
        }
        Value::Array(array) => {
            let actual = actual
                .as_array()
                .ok_or_else(|| failure(FailureKind::Contract, "required array has wrong type"))?;
            if actual.len() != array.len() {
                return Err(failure(FailureKind::Contract, "array length differs"));
            }
            for (actual, expected) in actual.iter().zip(array) {
                required(actual, expected)?;
            }
        }
        _ if actual != expected => {
            return Err(failure(
                FailureKind::Contract,
                "required scalar/type differs",
            ));
        }
        _ => {}
    }
    Ok(())
}
fn unattempted_sync() -> Value {
    json!({"method":"not_requested","state":"not_attempted","native_code":null})
}
fn unattempted_namespace() -> Value {
    json!({"method":"not_requested","state":"not_attempted","native_code":null,"required_edges":0,"completed_edges":0,"failed_edge_index":null})
}
fn unpublished() -> Value {
    json!({"state":"not_published","candidate_identity":null,"address":"not_checked","file_sync":unattempted_sync(),"namespace_sync":unattempted_namespace(),"cleanup":{"state":"not_needed","residual_name":null,"native_code":null}})
}
fn native_path(directory: &Path, name: &str) -> Result<Value> {
    #[cfg(unix)]
    let native = {
        let _ = directory;
        json!({"encoding":"unix-bytes","base64":STANDARD.encode(name.as_bytes())})
    };
    #[cfg(windows)]
    let native = {
        use std::os::windows::ffi::OsStrExt;
        let path = directory.canonicalize()?.join(name);
        let bytes: Vec<_> = path
            .as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect();
        json!({"encoding":"windows-utf16le","base64":STANDARD.encode(bytes)})
    };
    Ok(json!({"display":name,"native":native}))
}

struct ObservedFile {
    signature: eml_fs::FileSignature,
    identity: Value,
    bytes: Vec<u8>,
}
fn observe(path: &Path) -> Result<ObservedFile> {
    #[cfg(unix)]
    let bound = {
        let parent =
            eml_fs::BoundDirectory::open(path.parent().ok_or("Missing smoke file parent")?)?;
        parent.open_file(path.file_name().ok_or("Missing smoke basename")?, false)?
    };
    #[cfg(windows)]
    let bound = eml_fs::BoundFile::open(
        &path
            .parent()
            .ok_or("Missing smoke file parent")?
            .canonicalize()?
            .join(path.file_name().ok_or("Missing smoke basename")?),
        false,
    )?;
    let signature = bound.initial_signature();
    if signature.size > MAX_DOCUMENT {
        return Err(failure(
            FailureKind::SourceChanged,
            "fixture file exceeds its known bound",
        ));
    }
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let capacity = buffer
            .len()
            .min((MAX_DOCUMENT + 1 - bytes.len() as u64) as usize);
        let count = match bound.read_at(&mut buffer[..capacity], bytes.len() as u64) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            value => value?,
        };
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.len() as u64 > MAX_DOCUMENT {
            return Err(failure(
                FailureKind::SourceChanged,
                "fixture grew past its bound",
            ));
        }
    }
    bound.check_binding()?;
    let identity = match signature.identity {
        eml_fs::Identity::Posix { device, inode } => {
            json!({"kind":"posix","value":format!("{device}:{inode}")})
        }
        eml_fs::Identity::Windows { volume, file } => {
            let suffix: String = file.iter().map(|byte| format!("{byte:02X}")).collect();
            json!({"kind":"windows","value":format!("{volume:016X}:{suffix}")})
        }
    };
    #[cfg(windows)]
    bound.close()?;
    Ok(ObservedFile {
        signature,
        identity,
        bytes,
    })
}
fn sync(value: &Value, namespace: bool) -> Result<()> {
    required(value, &json!({"state":"succeeded"}))?;
    let method = field(value, "method")?
        .as_str()
        .ok_or_else(|| failure(FailureKind::Contract, "sync method type"))?;
    let code = field(value, "native_code")?;
    #[cfg(target_os = "macos")]
    let valid = if namespace {
        method == "fsync" && code.is_null()
    } else {
        (method == "fsync_fullfsync" && code.is_null())
            || (method == "fsync_fullfsync_unsupported"
                && matches!(code.as_i64(), Some(22 | 45 | 102)))
    };
    #[cfg(target_os = "linux")]
    let valid = {
        let _ = namespace;
        method == "fsync" && code.is_null()
    };
    #[cfg(windows)]
    let valid = {
        let _ = namespace;
        method == "flush_file_buffers" && code.is_null()
    };
    if !valid {
        return Err(failure(
            FailureKind::Contract,
            "native synchronization receipt does not qualify",
        ));
    }
    if namespace {
        required(
            value,
            &json!({"required_edges":1,"completed_edges":1,"failed_edge_index":null}),
        )?;
    }
    Ok(())
}

fn verify(
    directory: &Path,
    program: &str,
    version: &str,
    case: Case,
    output: &[u8],
    source_before: &ObservedFile,
    copy_before: Option<&ObservedFile>,
) -> Result<Value> {
    let events = frames(output)?;
    required(
        &events[0],
        &json!({"type":"start","protocol":1,"program":program,"version":version,
        "dry_run":matches!(case,Case::DryRun),"total":1,"collection_complete":true}),
    )?;
    let status = case.status();
    let counts = json!({"created":usize::from(matches!(case,Case::Created)),"up_to_date":usize::from(matches!(case,Case::UpToDate)),
        "planned":usize::from(matches!(case,Case::DryRun)),"failed":usize::from(matches!(case,Case::Conflict|Case::Protected)),"not_processed":0});
    required(
        &events[2],
        &json!({"type":"summary","counts":counts,"interrupted":false,"collection_complete":true,"run_error":null,"exit_code":case.exit()}),
    )?;
    let item = &events[1];
    required(
        item,
        &json!({"type":"item","index":0,"status":status,"source":native_path(directory,"a.eml")?,
        "destination":native_path(directory,"a.no-attachments.eml")?,"stripped_fields":[]}),
    )?;
    let source_after = observe(&directory.join("a.eml"))?;
    if source_after.signature != source_before.signature
        || source_after.bytes != source_before.bytes
    {
        return Err(failure(
            FailureKind::SourceChanged,
            "source identity/signature/bytes changed",
        ));
    }
    let prepared = !matches!(case, Case::Protected);
    let removals = if prepared {
        json!([{"section":"2","content_type":"application/pdf","filename":"report.pdf","bytes":507,"reason":"attachment"}])
    } else {
        json!([])
    };
    required(
        item,
        &json!({"source_bytes":if prepared { json!(1028) } else { Value::Null },
        "copy_bytes":if prepared { json!(513) } else { Value::Null },"removed":removals,
        "plan":if matches!(case,Case::DryRun) { json!("create") } else { Value::Null }}),
    )?;
    let copy = verify_copy_files(directory, case, copy_before)?;
    verify_item_effect(item, case, copy.as_ref())?;
    Ok(
        json!({"case":case.name(),"actual_exit":case.exit(),"frames":3,"source_identity":source_after.identity,
        "copy_identity":copy.as_ref().map(|copy|&copy.identity),"independent_bytes_checked":true,"exact_directory_inventory":true}),
    )
}

pub(crate) fn run(
    root: &Path,
    executable: &Path,
    program: &str,
    version: &str,
    private_enclosure: &Path,
    logs: &Path,
) -> Result<Value> {
    fs::create_dir(logs)?;
    let cases = private_enclosure.join("behavior-smoke");
    fs::create_dir(&cases)?;
    fs::write(
        logs.join("environment-policy.json"),
        serde_json::to_vec_pretty(
            &json!({"removed_inherited_selector_prefixes":["LD_","DYLD_"],"secret_values_recorded":false,
        "scope":"extracted Unix CLI independent of Cargo build/sysroot loader search; Windows execution remains unavailable"}),
        )?,
    )?;
    verify_version(executable, &cases, logs, program, version)?;
    let mut results = Vec::new();
    let ordinary = cases.join("ordinary");
    for case in [
        Case::Created,
        Case::UpToDate,
        Case::DryRun,
        Case::Conflict,
        Case::Protected,
    ] {
        let directory = if matches!(case, Case::Created | Case::UpToDate) {
            ordinary.clone()
        } else {
            cases.join(case.name())
        };
        if !matches!(case, Case::UpToDate) {
            fs::create_dir(&directory)?;
            fs::write(
                directory.join("a.eml"),
                if matches!(case, Case::Protected) {
                    PROTECTED
                } else {
                    INPUT
                },
            )?;
            if matches!(case, Case::Conflict) {
                fs::write(
                    directory.join("a.no-attachments.eml"),
                    b"owned existing copy must remain unchanged\n",
                )?;
            }
        }
        let source = observe(&directory.join("a.eml"))?;
        let before = if matches!(case, Case::UpToDate | Case::Conflict) {
            Some(observe(&directory.join("a.no-attachments.eml"))?)
        } else {
            None
        };
        let mut command = extracted_command(executable, &directory);
        command.args(["--format", "json"]);
        if matches!(case, Case::DryRun) {
            command.arg("--dry-run");
        }
        command.args(["--", "a.eml"]);
        let log = logs.join(format!("{}.log", case.name()));
        let result = controller()?.run_separate_output(command, &log, Duration::from_secs(30))?;
        if result.status.code() != Some(case.exit())
            || result.timed_out
            || result.cancelled
            || result.unexpected_survivors != 0
            || !result.cleanup_complete
        {
            return Err(failure(
                FailureKind::Process,
                format!("{} actual exit or lifetime evidence differs", case.name()),
            ));
        }
        if !document(&log.with_extension("stderr"))?.is_empty() {
            return Err(failure(
                FailureKind::Contract,
                "unexpected pre-stream/side-channel diagnostic",
            ));
        }
        let observed = verify(
            &directory,
            program,
            version,
            case,
            &document(&log)?,
            &source,
            before.as_ref(),
        )?;
        fs::write(
            logs.join(format!("{}.json", case.name())),
            serde_json::to_vec_pretty(&observed)?,
        )?;
        results.push(observed);
    }
    let controls = counterfeit_controls(root, program, version, &cases, logs)?;
    Ok(
        json!({"scope":"actual supplied native CLI fixture behavior only; archive identity and other qualification gates are separate","cases":results,"version_exit":0,"negative_controls":controls}),
    )
}

fn counterfeit_controls(
    root: &Path,
    program: &str,
    version: &str,
    cases: &Path,
    logs: &Path,
) -> Result<Value> {
    let mut build = Command::new("cargo");
    build.current_dir(root).args([
        "build",
        "--locked",
        "-p",
        "xtask",
        "--bin",
        "archive-counterfeit",
        "--message-format=json",
    ]);
    let output = crate::capture(&mut build)?;
    fs::write(logs.join("counterfeit-build.jsonl"), &output)?;
    let executable = crate::cli_build::named_artifact(
        root,
        &output,
        "xtask/Cargo.toml",
        "xtask/tests/support/archive_counterfeit.rs",
        "archive-counterfeit",
    )?;
    let parent = cases.join("counterfeit-controls");
    fs::create_dir(&parent)?;
    fs::write(
        parent.join("owned-counterfeit-marker"),
        b"EML owned synthetic counterfeit cases\n",
    )?;
    let trace = document(&logs.join("created.log"))?;
    let mut results = Vec::new();
    for (mode, expected) in [
        ("identity-copy", FailureKind::GoldenCopy),
        ("summary-only", FailureKind::Framing),
    ] {
        let directory = parent.join(mode);
        fs::create_dir(&directory)?;
        fs::write(directory.join("a.eml"), INPUT)?;
        let trace_path = parent.join(format!("counterfeit-{mode}-trace.jsonl"));
        use std::io::Write;
        let mut owned_trace = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(trace_path)?;
        owned_trace.write_all(&counterfeit_trace(&trace, &directory)?)?;
        drop(owned_trace);
        let source = observe(&directory.join("a.eml"))?;
        let mut version_command = extracted_command(&executable, &directory);
        version_command.arg("--version");
        let log = logs.join(format!("counterfeit-{mode}-version.log"));
        let process =
            controller()?.run_separate_output(version_command, &log, Duration::from_secs(30))?;
        if !process.success()
            || document(&log)?
                != format!(
                    "{program} {version} (Build {})\n",
                    eml_core::product::BUILD_NUMBER
                )
                .as_bytes()
            || !document(&log.with_extension("stderr"))?.is_empty()
        {
            return Err(failure(
                FailureKind::Process,
                "counterfeit version did not reach the intended control",
            ));
        }
        let mut command = extracted_command(&executable, &directory);
        command
            .args(["--format", "json", "--", "a.eml"])
            .env("EML_ARCHIVE_COUNTERFEIT_MODE", mode);
        let log = logs.join(format!("counterfeit-{mode}.log"));
        let process = controller()?.run_separate_output(command, &log, Duration::from_secs(30))?;
        if !process.success() || !document(&log.with_extension("stderr"))?.is_empty() {
            return Err(failure(
                FailureKind::Process,
                "counterfeit execution failed before its intended oracle",
            ));
        }
        let result = verify(
            &directory,
            program,
            version,
            Case::Created,
            &document(&log)?,
            &source,
            None,
        );
        let error = result
            .err()
            .ok_or("Counterfeit executable incorrectly passed actual behavior checks")?;
        let error = error.downcast::<SmokeFailure>()?;
        if error.kind != expected {
            return Err(format!(
                "Counterfeit {mode} rejected at {:?}, expected {expected:?}",
                error.kind
            )
            .into());
        }
        results.push(json!({"mode":mode,"actual_exit":0,"version_accepted":true,"rejected_at":format!("{expected:?}")}));
    }
    Ok(json!(results))
}

fn counterfeit_trace(bytes: &[u8], directory: &Path) -> Result<Vec<u8>> {
    let mut events = frames(bytes)?;
    // Windows transport uses absolute native units. Preserve every claimed
    // outcome/receipt while binding only the two path fields to this separate
    // owned case, so the identity-copy control reaches the independent byte oracle.
    events[1]["source"] = native_path(directory, "a.eml")?;
    events[1]["destination"] = native_path(directory, "a.no-attachments.eml")?;
    let mut result = Vec::new();
    for event in events {
        serde_json::to_writer(&mut result, &event)?;
        result.push(b'\n');
    }
    Ok(result)
}

fn verify_copy_files(
    directory: &Path,
    case: Case,
    copy_before: Option<&ObservedFile>,
) -> Result<Option<ObservedFile>> {
    let expected_names: BTreeSet<_> =
        if matches!(case, Case::Created | Case::UpToDate | Case::Conflict) {
            ["a.eml", "a.no-attachments.eml"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        } else {
            BTreeSet::from(["a.eml".to_owned()])
        };
    let actual_names: BTreeSet<_> = fs::read_dir(directory)?
        .map(|entry| {
            entry.and_then(|entry| {
                entry
                    .file_name()
                    .into_string()
                    .map_err(|_| std::io::Error::other("Non-UTF-8 unexpected fixture entry"))
            })
        })
        .collect::<std::io::Result<_>>()?;
    if actual_names != expected_names {
        return Err(failure(
            FailureKind::Inventory,
            "extra/missing output or staging resource",
        ));
    }
    let copy = if matches!(case, Case::Created | Case::UpToDate | Case::Conflict) {
        Some(observe(&directory.join("a.no-attachments.eml"))?)
    } else {
        None
    };
    if let Some(copy) = &copy {
        let expected = if matches!(case, Case::Conflict) {
            &copy_before.ok_or("Missing conflict pre-observation")?.bytes[..]
        } else {
            GOLDEN
        };
        if copy.bytes != expected {
            return Err(failure(
                FailureKind::GoldenCopy,
                "actual copy differs from the independent golden/unchanged conflict bytes",
            ));
        }
        if let Some(before) = copy_before
            && copy.signature != before.signature
        {
            return Err(failure(
                FailureKind::SourceChanged,
                "preexisting output identity/signature changed",
            ));
        }
    }
    Ok(copy)
}

fn verify_item_effect(item: &Value, case: Case, copy: Option<&ObservedFile>) -> Result<()> {
    let publication = field(item, "publication")?;
    if matches!(case, Case::Created) {
        let copy = copy.ok_or("Missing created observation")?;
        required(
            publication,
            &json!({"state":"published","candidate_identity":copy.identity,"address":"verified",
            "cleanup":{"residual_name":null,"native_code":null}}),
        )?;
        let cleanup = field(field(publication, "cleanup")?, "state")?
            .as_str()
            .ok_or("Cleanup state type")?;
        if !matches!(cleanup, "complete" | "not_needed") {
            return Err(failure(
                FailureKind::Contract,
                "created cleanup is not clean",
            ));
        }
        sync(field(publication, "file_sync")?, false)?;
        sync(field(publication, "namespace_sync")?, true)?;
        required(item, &json!({"existing_observation":null,"error":null}))?;
    } else {
        required(publication, &unpublished())?;
        if matches!(case, Case::UpToDate | Case::Conflict) {
            let copy = copy.ok_or("Missing existing observation")?;
            let existing = field(item, "existing_observation")?;
            required(
                existing,
                &json!({"identity":copy.identity,"bytes_equal":matches!(case,Case::UpToDate),"stable":true,
                "address":if matches!(case,Case::UpToDate) { "verified" } else { "not_checked" }}),
            )?;
            if matches!(case, Case::UpToDate) {
                sync(field(existing, "file_sync")?, false)?;
                sync(field(existing, "namespace_sync")?, true)?;
            } else {
                required(
                    existing,
                    &json!({"file_sync":unattempted_sync(),"namespace_sync":unattempted_namespace()}),
                )?;
            }
        } else {
            required(item, &json!({"existing_observation":null}))?;
        }
        if matches!(case, Case::Conflict | Case::Protected) {
            let error = field(item, "error")?;
            required(
                error,
                &json!({"code":if matches!(case,Case::Conflict) { "DESTINATION_EXISTS" } else { "PROTECTED" },
                "phase":if matches!(case,Case::Conflict) { "existing" } else { "prepare" },"section":null}),
            )?;
            let detail = field(error, "detail")?
                .as_str()
                .ok_or("Error detail type")?;
            if detail.is_empty() || detail.chars().count() > 1000 {
                return Err(failure(FailureKind::Contract, "error detail bounds"));
            }
        } else {
            required(item, &json!({"error":null}))?;
        }
    }
    Ok(())
}

fn verify_version(
    executable: &Path,
    cases: &Path,
    logs: &Path,
    program: &str,
    version: &str,
) -> Result<()> {
    let mut version_command = extracted_command(executable, cases);
    version_command.arg("--version");
    let version_log = logs.join("version.log");
    let result = controller()?.run_separate_output(
        version_command,
        &version_log,
        Duration::from_secs(30),
    )?;
    if !result.success()
        || document(&version_log)?
            != format!(
                "{program} {version} (Build {})\n",
                eml_core::product::BUILD_NUMBER
            )
            .as_bytes()
        || !document(&version_log.with_extension("stderr"))?.is_empty()
    {
        return Err(failure(
            FailureKind::Process,
            "extracted executable version/exit/lifetime differs",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn encode(events: &[Value]) -> Vec<u8> {
        events
            .iter()
            .flat_map(|event| {
                let mut bytes = serde_json::to_vec(event).unwrap();
                bytes.push(b'\n');
                bytes
            })
            .collect()
    }
    fn created(directory: &Path) -> (ObservedFile, Vec<Value>) {
        fs::write(directory.join("a.eml"), INPUT).unwrap();
        fs::write(directory.join("a.no-attachments.eml"), GOLDEN).unwrap();
        let source = observe(&directory.join("a.eml")).unwrap();
        let copy = observe(&directory.join("a.no-attachments.eml")).unwrap();
        let mut events = frames(
            include_str!("../../tests/contract/protocol/created-attachment.jsonl")
                .replace("@VERSION@", "6.0.0")
                .as_bytes(),
        )
        .unwrap();
        events[1]["source"] = native_path(directory, "a.eml").unwrap();
        events[1]["destination"] = native_path(directory, "a.no-attachments.eml").unwrap();
        events[1]["publication"]["candidate_identity"] = copy.identity;
        #[cfg(target_os = "macos")]
        {
            events[1]["publication"]["file_sync"]["method"] = json!("fsync_fullfsync");
        }
        #[cfg(windows)]
        {
            for field in ["file_sync", "namespace_sync"] {
                events[1]["publication"][field]["method"] = json!("flush_file_buffers");
            }
        }
        (source, events)
    }
    fn kind(result: Result<Value>) -> FailureKind {
        result.unwrap_err().downcast::<SmokeFailure>().unwrap().kind
    }

    #[test]
    fn required_nullable_members_are_present_and_additive_fields_are_ignored() {
        let expected = json!({"outer":{"nullable":null,"number":1}});
        assert!(
            required(
                &json!({"outer":{"nullable":null,"number":1,"future":true}}),
                &expected
            )
            .is_ok()
        );
        for wrong in [
            json!({"outer":{"number":1}}),
            json!({"outer":{"nullable":null,"number":"1"}}),
            json!({"outer":null}),
        ] {
            assert!(required(&wrong, &expected).is_err());
        }
    }

    #[test]
    fn framing_rejects_missing_item_blank_extra_text_and_missing_final_lf() {
        let valid = include_bytes!("../../tests/contract/protocol/created-attachment.jsonl");
        let events = frames(valid).unwrap();
        for wrong in [
            encode(&events[2..]),
            valid[..valid.len() - 1].to_vec(),
            [valid.as_slice(), b"\n"].concat(),
            [valid.as_slice(), b"diagnostic\n"].concat(),
            b"{}\n\n{}\n".to_vec(),
        ] {
            assert!(matches!(
                frames(&wrong)
                    .unwrap_err()
                    .downcast::<SmokeFailure>()
                    .unwrap()
                    .kind,
                FailureKind::Framing
            ));
        }
    }

    #[test]
    fn independent_golden_guard_rejects_identity_copy_after_valid_earlier_facts() {
        let owned = tempfile::tempdir().unwrap();
        let (source, events) = created(owned.path());
        assert!(
            verify(
                owned.path(),
                "emlattachmentremover",
                "6.0.0",
                Case::Created,
                &encode(&events),
                &source,
                None
            )
            .is_ok()
        );
        fs::write(owned.path().join("a.no-attachments.eml"), INPUT).unwrap();
        assert_eq!(
            kind(verify(
                owned.path(),
                "emlattachmentremover",
                "6.0.0",
                Case::Created,
                &encode(&events),
                &source,
                None
            )),
            FailureKind::GoldenCopy
        );
    }

    #[test]
    fn independent_counterfeit_case_rebinds_native_paths_before_the_byte_oracle() {
        let producer = tempfile::tempdir().unwrap();
        let (_, events) = created(producer.path());
        let counterfeit = tempfile::tempdir().unwrap();
        fs::write(counterfeit.path().join("a.eml"), INPUT).unwrap();
        fs::write(counterfeit.path().join("a.no-attachments.eml"), INPUT).unwrap();
        let source = observe(&counterfeit.path().join("a.eml")).unwrap();
        let trace = counterfeit_trace(&encode(&events), counterfeit.path()).unwrap();
        assert_eq!(
            kind(verify(
                counterfeit.path(),
                "emlattachmentremover",
                "6.0.0",
                Case::Created,
                &trace,
                &source,
                None
            )),
            FailureKind::GoldenCopy
        );
        let rebound = frames(&trace).unwrap();
        assert_eq!(rebound[1]["removed"], events[1]["removed"]);
        assert_eq!(rebound[1]["publication"], events[1]["publication"]);
    }

    #[test]
    fn missing_nullable_receipt_and_wrong_counts_or_order_cannot_pass() {
        let owned = tempfile::tempdir().unwrap();
        let (source, events) = created(owned.path());
        for mutation in ["nullable", "counts", "order"] {
            let mut changed = events.clone();
            match mutation {
                "nullable" => {
                    changed[1].as_object_mut().unwrap().remove("plan");
                }
                "counts" => {
                    changed[2]["counts"]["created"] = json!(0);
                }
                "order" => changed.swap(0, 1),
                _ => unreachable!(),
            }
            assert_eq!(
                kind(verify(
                    owned.path(),
                    "emlattachmentremover",
                    "6.0.0",
                    Case::Created,
                    &encode(&changed),
                    &source,
                    None
                )),
                FailureKind::Contract
            );
        }
    }
}
