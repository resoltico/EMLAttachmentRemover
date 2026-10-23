//! Pure admission and readback rules, independent of native command execution.
use super::*;

pub(super) fn hex_id(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
#[cfg(any(target_os = "macos", test))]
pub(super) fn validate_authority(values: &BTreeMap<String, String>) -> Result<()> {
    for (name, expected) in [
        ("GITHUB_ACTIONS", "true"),
        ("RUNNER_ENVIRONMENT", "github-hosted"),
        ("GITHUB_REPOSITORY", "resoltico/EMLAttachmentRemover"),
        ("GITHUB_JOB", "app_picker"),
    ] {
        if values.get(name).map(String::as_str) != Some(expected) {
            return Err(
                format!("Picker host setup requires its exact fresh hosted role: {name}").into(),
            );
        }
    }
    let workflow = values.get("GITHUB_WORKFLOW").map(String::as_str);
    let file = match workflow {
        Some("CI") => "ci.yml",
        Some("Owner release") => "release.yml",
        _ => return Err("Picker host setup requires the CI or Owner release workflow".into()),
    };
    let prefix = format!("resoltico/EMLAttachmentRemover/.github/workflows/{file}@");
    if !values
        .get("GITHUB_WORKFLOW_REF")
        .is_some_and(|v| v.starts_with(&prefix) && v.len() > prefix.len())
        || !values.get("GITHUB_WORKFLOW_SHA").is_some_and(|v| hex_id(v))
    {
        return Err("Picker host workflow file or immutable identity differs".into());
    }
    for name in ["GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT"] {
        if !values.get(name).is_some_and(|v| {
            v.parse::<u64>().is_ok_and(|n| n > 0) && v.bytes().all(|b| b.is_ascii_digit())
        }) {
            return Err("Picker host run/attempt is unavailable".into());
        }
    }
    Ok(())
}
pub(super) fn validate_host(host: &Host, policy: &IncidentHost, target: &str) -> Result<()> {
    if host.target != target
        || host.process_os != "macos"
        || host.process_architecture != "x86_64"
        || host.kernel_machine != "x86_64"
        || host.image_os != policy.image_os
        || host.image_version != policy.image_version
        || host.product_version != policy.product_version
        || host.build_version != policy.build_version
        || !uuid(&host.boot_uuid)
    {
        return Err("Unknown picker host image, OS, native CPU or boot identity; no intervention authorized".into());
    }
    Ok(())
}
fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}
pub(super) fn validate_caller(caller: &Caller) -> Result<()> {
    if caller.uid == 0
        || caller.uid != caller.euid
        || caller.uid != caller.console_uid
        || caller.security_session.status != 0
        || caller.security_session.graphic_access != Some(true)
        || !caller
            .security_session
            .security_session_id
            .is_some_and(|id| id > 0 && id < u32::MAX)
    {
        return Err(
            "Caller is not the observed non-root console GUI user with graphic access".into(),
        );
    }
    Ok(())
}
pub(super) fn target(uid: u32) -> String {
    format!("gui/{uid}/{SERVICE}")
}
pub(super) fn clean(process: &ProcessObservation) -> Result<i64> {
    Ok(i64::from(process.clean()?))
}
pub(super) fn command_is(program: &str, arguments: &[String], expected: &[&str]) -> Result<()> {
    if program != LAUNCHCTL || arguments != expected {
        return Err("Native service command differs from the exact authorized operation".into());
    }
    Ok(())
}
#[cfg(any(target_os = "macos", test))]
pub(super) fn parse_fields(text: &str) -> Result<BTreeMap<String, String>> {
    let mut fields = BTreeMap::new();
    for line in text.lines() {
        let Some(line) = line
            .strip_prefix('\t')
            .filter(|v| !v.starts_with('\t') && !v.starts_with(' '))
        else {
            continue;
        };
        let Some((key, value)) = line.split_once(" = ") else {
            continue;
        };
        if ![
            "path",
            "program",
            "type",
            "state",
            "domain",
            "asid",
            "pid",
            "runs",
            "successive crashes",
            "last terminating signal",
            "last exit code",
        ]
        .contains(&key)
        {
            continue;
        }
        if value.is_empty()
            || value.len() > 1024
            || fields
                .insert(key.replace(' ', "_"), value.to_owned())
                .is_some()
        {
            return Err("Ambiguous or oversized selected launchctl service field".into());
        }
    }
    Ok(fields)
}
#[cfg(any(target_os = "macos", test))]
pub(super) fn parse_disabled(text: &str) -> Result<Option<bool>> {
    if text.len() > 2 * 1024 * 1024 {
        return Err("Disabled-state readback exceeds its bound".into());
    }
    let lines: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines == ["disabled services = {}"] {
        return Ok(None);
    }
    if lines.len() < 2
        || lines.first() != Some(&"disabled services = {")
        || lines.last() != Some(&"}")
    {
        return Err("Disabled-state readback lacks a complete recognized map".into());
    }
    let mut labels = std::collections::BTreeSet::new();
    let mut result = None;
    for line in &lines[1..lines.len() - 1] {
        let (label, state) = line
            .split_once(" => ")
            .ok_or("Unrecognized disabled-state entry")?;
        let label: String = serde_json::from_str(label)?;
        if label.is_empty() || label.len() > 4096 || !labels.insert(label.clone()) {
            return Err("Invalid or duplicate disabled-state label".into());
        }
        let disabled = match state {
            "disabled" => true,
            "enabled" => false,
            _ => return Err("Unknown disabled-state value".into()),
        };
        if label == SERVICE {
            result = Some(disabled);
        }
    }
    Ok(result)
}
pub(super) fn absent_text(uid: u32) -> String {
    format!("Could not find service \"{SERVICE}\" in domain for user gui: {uid}")
}
#[cfg(any(target_os = "macos", test))]
pub(super) fn parse_absence(text: &str, uid: u32) -> Result<Option<String>> {
    let expected = absent_text(uid);
    let found = text
        .lines()
        .map(str::trim)
        .filter(|line| *line == expected)
        .count();
    if found > 1 {
        return Err("Ambiguous exact service-absence diagnostic".into());
    }
    Ok((found == 1).then_some(expected))
}
pub(super) fn loaded(observation: &ServiceObservation, uid: u32) -> Result<()> {
    let exact = target(uid);
    if observation.target != exact {
        return Err("Observed service target differs from caller".into());
    }
    command_is(
        &observation.print.program,
        &observation.print.arguments,
        &["print", &exact],
    )?;
    if clean(&observation.print.process)? != 0 || observation.print.absence_diagnostic.is_some() {
        return Err("The exact Apple GUI launch agent is not observed loaded".into());
    }
    for (key, value) in [
        ("path", APPLE_PLIST),
        ("program", APPLE_PROGRAM),
        ("type", "LaunchAgent"),
    ] {
        if observation
            .print
            .selected_fields
            .get(key)
            .map(String::as_str)
            != Some(value)
        {
            return Err("Loaded service identity is not the fixed Apple launch agent".into());
        }
    }
    let fields = &observation.print.selected_fields;
    let domain = fields.get("domain").ok_or("Missing service domain")?;
    let prefix = format!("gui/{uid} [");
    if !domain
        .strip_prefix(&prefix)
        .and_then(|v| v.strip_suffix(']'))
        .is_some_and(|v| v.parse::<u32>().is_ok_and(|n| n > 0))
        || !fields.get("state").is_some_and(|v| !v.is_empty())
    {
        return Err("Loaded service domain/state is not established".into());
    }
    disabled(&observation.disabled, uid, false)
}
fn disabled(observation: &DisabledObservation, uid: u32, required: bool) -> Result<()> {
    let domain = format!("gui/{uid}");
    command_is(
        &observation.program,
        &observation.arguments,
        &["print-disabled", &domain],
    )?;
    if clean(&observation.process)? != 0 || (required && observation.r#override != Some(true)) {
        return Err("The exact disabled override is unconfirmed".into());
    }
    Ok(())
}
pub(super) fn absent(observation: &PrintObservation, uid: u32) -> Result<()> {
    let exact = target(uid);
    command_is(
        &observation.program,
        &observation.arguments,
        &["print", &exact],
    )?;
    if clean(&observation.process)? != 113
        || observation.absence_diagnostic.as_deref() != Some(absent_text(uid).as_str())
        || !observation.selected_fields.is_empty()
    {
        return Err("Exact service absence is unconfirmed".into());
    }
    Ok(())
}
pub(super) fn provisioned(observation: &ServiceObservation, uid: u32) -> Result<()> {
    if observation.target != target(uid) {
        return Err("Post-state service target differs".into());
    }
    absent(&observation.print, uid)?;
    disabled(&observation.disabled, uid, true)
}
pub(super) fn action(observation: &CommandObservation, operation: &str, uid: u32) -> Result<i64> {
    let exact = target(uid);
    command_is(
        &observation.program,
        &observation.arguments,
        &[operation, &exact],
    )?;
    clean(&observation.process)
}
pub(crate) fn validate_preparation(p: &Preparation) -> Result<()> {
    if p.stage != "picker_host_preparation" || !hex_id(&p.binding.source) {
        return Err("Not a completed picker host preparation".into());
    }
    validate_host(&p.host, &p.picker_host, &p.binding.target)?;
    validate_caller(&p.caller)?;
    loaded(&p.service_before, p.caller.uid)?;
    crashes::require_known(&p.crash_reports, &p.host, p.caller.uid)?;
    let code = action(&p.bootout, "bootout", p.caller.uid)?;
    match (code, &p.bootout_absence) {
        (0, None) => (),
        (n, Some(observation)) if n > 0 => absent(observation, p.caller.uid)?,
        _ => return Err("Bootout lacks its required exact absence evidence".into()),
    }
    if action(&p.disable, "disable", p.caller.uid)? != 0 {
        return Err("Disable did not succeed".into());
    }
    provisioned(&p.service_after, p.caller.uid)
}

pub(crate) fn validate_post(prepared: &Preparation, verified: &Postverification) -> Result<()> {
    validate_preparation(prepared)?;
    if verified.stage != "picker_host_verification"
        || verified.binding != prepared.binding
        || verified.picker_host != prepared.picker_host
        || verified.host != prepared.host
        || verified.caller != prepared.caller
    {
        return Err("Postverification input, policy, host boot or caller context changed".into());
    }
    validate_host(
        &verified.host,
        &verified.picker_host,
        &verified.binding.target,
    )?;
    validate_caller(&verified.caller)?;
    provisioned(&verified.service, verified.caller.uid)
}
