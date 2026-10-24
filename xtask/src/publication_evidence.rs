//! Source-bound actual native publication records, separate from fixture-controlled inputs.
mod record;
use crate::{
    Result,
    distribution::BuildPlan,
    picker_evidence::{ProcessObservation, observation_document},
};
use record::{FILES, required_tests, validate_record};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, io::Read, path::Path};
use xtask::archive::ByteIdentity;
const LIMIT: usize = 256 * 1024;
fn require(ok: bool, message: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(message.into()) }
}
fn fields(value: &Value, names: &[&str]) -> Result<()> {
    let object = value
        .as_object()
        .ok_or("Publication evidence requires a named object")?;
    require(
        object.keys().map(String::as_str).collect::<BTreeSet<_>>()
            == names.iter().copied().collect(),
        "Publication evidence fields differ",
    )
}
fn identity(value: &Value) -> Result<ByteIdentity> {
    fields(value, &["byte_length", "sha256"])?;
    let id: ByteIdentity = serde_json::from_value(value.clone())?;
    require(
        id.byte_length > 0
            && id.sha256.len() == 64
            && id
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "Invalid publication evidence byte identity",
    )?;
    Ok(id)
}
fn validate_context(
    value: &Value,
    source: &str,
    epoch: u64,
    target: Option<&str>,
    os: &str,
    architecture: &str,
    profile: &str,
) -> Result<()> {
    fields(
        value,
        &[
            "schema",
            "schema_version",
            "source",
            "source_epoch",
            "source_provenance",
            "target",
            "profile",
            "host",
            "test_execution",
            "images",
            "records",
        ],
    )?;
    require(
        matches!(os, "linux" | "macos" | "windows") && matches!(architecture, "x86_64" | "aarch64"),
        "Unsupported native publication observation OS/CPU",
    )?;
    require(
        value["schema"] == "eml-native-publication-evidence"
            && value["schema_version"] == 1
            && value["source"] == source
            && value["source_epoch"] == epoch
            && value["target"] == json!(target)
            && value["profile"] == profile
            && (value["source_provenance"] == "git_commit"
                || (target.is_none() && value["source_provenance"] == "declared_archive")),
        "Publication evidence source/epoch/target/profile differs",
    )?;
    let host = &value["host"];
    require(
        host["process_os"] == os
            && host["process_architecture"] == architecture
            && target.is_none_or(|target| host["target"] == target),
        "Publication evidence actual host differs",
    )?;
    let actual_target = host["target"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 128)
        .ok_or("Missing actual publication-test compiler host")?;
    require(
        host["compiler"].as_str().is_some_and(|text| {
            text.lines()
                .any(|line| line == format!("host: {actual_target}"))
        }) && matches!(profile, "test" | "release"),
        "Missing actual compiler/profile observation",
    )?;
    if os == "windows" {
        require(
            host["native_machine"]
                == if architecture == "x86_64" {
                    0x8664
                } else {
                    0xaa64
                }
                && host["emulated_process_machine"] == 0
                && host["actual_kernel"]["major"]
                    .as_u64()
                    .is_some_and(|v| v >= 10)
                && host["actual_kernel"]["minor"].as_u64().is_some()
                && host["actual_kernel"]["build"].as_u64().is_some(),
            "Missing native Windows publication-test kernel/CPU observations",
        )?;
    } else {
        let kernel = if os == "macos" && architecture == "aarch64" {
            "arm64"
        } else {
            architecture
        };
        require(
            host["kernel_machine"] == kernel
                && host["actual_os"].as_str().is_some_and(|s| !s.is_empty()),
            "Missing actual Unix publication-test kernel/OS observations",
        )?;
    }
    let execution = &value["test_execution"];
    fields(
        execution,
        &[
            "process",
            "discovery_identity",
            "execution_identity",
            "required_tests",
        ],
    )?;
    let process: ProcessObservation = serde_json::from_value(execution["process"].clone())?;
    require(
        process.clean()? == 0,
        "Publication tests did not finish with verified process cleanup",
    )?;
    identity(&execution["discovery_identity"])?;
    identity(&execution["execution_identity"])?;
    let expected: Vec<_> = required_tests(os)
        .iter()
        .map(|name| json!({"name":name,"discovered":1,"executed":1}))
        .collect();
    require(
        execution["required_tests"] == json!(expected),
        "Native publication controls were not discovered/executed exactly once",
    )?;
    let images = value["images"]
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= 1024)
        .ok_or("No actual launched test-image observations")?;
    let mut paths = BTreeSet::new();
    for image in images {
        fields(image, &["path", "identity"])?;
        let path = image["path"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 131072 && !s.contains('\0'))
            .ok_or("Invalid actual test-image path")?;
        require(
            paths.insert(path),
            "Duplicate launched test-image observation",
        )?;
        identity(&image["identity"])?;
    }
    let records = value["records"]
        .as_array()
        .filter(|v| v.len() == FILES.len())
        .ok_or("Incomplete native publication record set")?;
    for (item, name) in records.iter().zip(FILES) {
        fields(item, &["name", "identity", "document"])?;
        require(
            item["name"] == name,
            "Native publication record order/name differs",
        )?;
        let text = item["document"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= LIMIT)
            .ok_or("Native publication record exceeds its bound")?;
        require(
            identity(&item["identity"])? == ByteIdentity::of(text.as_bytes()),
            "Native publication record bytes changed",
        )?;
        validate_record(
            name,
            &observation_document(text.as_bytes())?,
            os,
            architecture,
        )?;
    }
    Ok(())
}
/// Admission always requires an explicit shipping target and Git source proof.
pub(crate) fn validate_summary(
    value: &Value,
    source: &str,
    epoch: u64,
    target: &str,
    os: &str,
    architecture: &str,
    profile: &str,
) -> Result<()> {
    validate_context(
        value,
        source,
        epoch,
        Some(target),
        os,
        architecture,
        profile,
    )
}
/// Retain the exact records from one already-settled test selection.
pub(crate) fn collect(
    plan: &BuildPlan,
    target: Option<&str>,
    release: bool,
    out: &Path,
    selection: &Value,
    cleanup: &Value,
    host: &Value,
) -> Result<Value> {
    let os = host["process_os"]
        .as_str()
        .ok_or("Missing actual test OS")?;
    let architecture = host["process_architecture"]
        .as_str()
        .ok_or("Missing actual test CPU")?;
    let directory = out.join("native-publication-evidence");
    let found: BTreeSet<_> = fs::read_dir(&directory)?
        .map(|entry| -> Result<_> {
            let entry = entry?;
            require(
                entry.file_type()?.is_file(),
                "Publication record is not an ordinary file",
            )?;
            Ok(entry
                .file_name()
                .into_string()
                .map_err(|_| "Non-ASCII publication case filename")?)
        })
        .collect::<Result<_>>()?;
    require(
        found == FILES.iter().map(|s| s.to_string()).collect(),
        "Native publication directory has missing/extra cases",
    )?;
    let mut records = Vec::new();
    for name in FILES {
        let file = fs::File::open(directory.join(name))?;
        require(
            file.metadata()?.is_file(),
            "Opened publication record is not ordinary",
        )?;
        let mut bytes = Vec::new();
        file.take(LIMIT as u64 + 1).read_to_end(&mut bytes)?;
        require(
            bytes.len() <= LIMIT,
            "Native publication record exceeds its read bound",
        )?;
        validate_record(name, &observation_document(&bytes)?, os, architecture)?;
        records.push(json!({"name":name,"identity":ByteIdentity::of(&bytes),"document":String::from_utf8(bytes)?}));
    }
    let discovery = fs::read(out.join("native-test-discovery.log"))?;
    let execution = fs::read(out.join("native-tests.log"))?;
    let names = crate::qualify::test_names(std::str::from_utf8(&discovery)?, true);
    let passed = crate::qualify::test_names(std::str::from_utf8(&execution)?, false);
    let cases:Vec<_>=required_tests(os).iter().map(|name|json!({"name":name,"discovered":names.get(*name).copied().unwrap_or(0),"executed":passed.get(*name).copied().unwrap_or(0)})).collect();
    require(
        cleanup["processes_settled"] == true,
        "Test-image lifetime remains uncertain",
    )?;
    let summary = json!({"schema":"eml-native-publication-evidence","schema_version":1,"source":plan.source_commit(),"source_epoch":plan.source_epoch(),"source_provenance":plan.source_provenance(),"target":target,"profile":if release{"release"}else{"test"},"host":host,"test_execution":{"process":selection.get("execution_process").ok_or("Missing actual native test process observation")?,"discovery_identity":ByteIdentity::of(&discovery),"execution_identity":ByteIdentity::of(&execution),"required_tests":cases},"images":cleanup.get("executable_identities").ok_or("Missing actual launched test image identities")?,"records":records});
    validate_context(
        &summary,
        plan.source_commit(),
        u64::from(plan.source_epoch().seconds()),
        target,
        os,
        architecture,
        if release { "release" } else { "test" },
    )?;
    fs::write(
        out.join("native-publication-evidence.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;
    Ok(summary)
}
#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::fixture as test_fixture;
