//! Known-image host actions and read-only bracketing verification.
use super::{
    observations::{self, Observer},
    *,
};
use crate::macos::MacBuild;
use serde_json::json;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub(crate) fn require_context(_root: &Path) -> Result<()> {
    let values = [
        "GITHUB_ACTIONS",
        "RUNNER_ENVIRONMENT",
        "GITHUB_REPOSITORY",
        "GITHUB_JOB",
        "GITHUB_WORKFLOW",
        "GITHUB_WORKFLOW_REF",
        "GITHUB_WORKFLOW_SHA",
        "GITHUB_RUN_ID",
        "GITHUB_RUN_ATTEMPT",
    ]
    .into_iter()
    .map(|key| (key.to_owned(), std::env::var(key).unwrap_or_default()))
    .collect();
    evidence::validate_authority(&values)
}
struct Attempt {
    out: PathBuf,
    phase: &'static str,
    facts: BTreeMap<String, Value>,
}
impl Attempt {
    fn new(out: &Path) -> Result<Self> {
        fs::create_dir_all(out.parent().ok_or("Host output needs an owned parent")?)?;
        fs::create_dir(out)?;
        let attempt = Self {
            out: out.to_owned(),
            phase: "starting",
            facts: BTreeMap::new(),
        };
        attempt.save(None)?;
        Ok(attempt)
    }
    fn save(&self, failure: Option<&str>) -> Result<()> {
        let value = json!({"phase":self.phase,"observations":self.facts,"failure":failure,
            "scope":"Partial native-host observation only; never a successful preparation, worker result or cleanup claim"});
        let temporary = self.out.join("attempt.next.json");
        fs::write(&temporary, serde_json::to_vec_pretty(&value)?)?;
        fs::rename(temporary, self.out.join("attempt.json"))?;
        Ok(())
    }
    fn phase(&mut self, phase: &'static str) -> Result<()> {
        self.phase = phase;
        self.save(None)
    }
    fn record(&mut self, name: &str, value: &impl Serialize) -> Result<()> {
        self.facts
            .insert(name.to_owned(), serde_json::to_value(value)?);
        self.save(None)
    }
    fn finish<T>(&self, result: Result<T>) -> Result<T> {
        if let Err(error) = &result {
            self.save(Some(&error.to_string()))?;
        }
        result
    }
}
fn write_final(path: &Path, record: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(record)?;
    if bytes.len() as u64 > DOCUMENT_LIMIT {
        return Err("Host receipt exceeds its bound".into());
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}
pub(crate) fn prepare(
    root: &Path,
    build: &MacBuild,
    archive: &Path,
    reference: &Path,
    out: &Path,
) -> Result<Preparation> {
    require_context(root)?;
    let mut attempt = Attempt::new(out)?;
    let result = prepare_observed(root, build, archive, reference, &mut attempt);
    attempt.finish(result)
}
fn prepare_observed(
    root: &Path,
    build: &MacBuild,
    archive: &Path,
    reference: &Path,
    attempt: &mut Attempt,
) -> Result<Preparation> {
    let out = attempt.out.clone();
    let observer = Observer {
        root,
        build,
        out: &out,
    };
    attempt.phase("binding")?;
    let binding = observer.binding(archive, reference)?;
    attempt.record("binding", &binding)?;
    let picker_host = crate::distribution::picker_host_policy(root)?;
    attempt.record("picker_host", &picker_host)?;
    attempt.phase("host")?;
    let host = observer.host(&binding.target)?;
    attempt.record("host", &host)?;
    evidence::validate_host(&host, &picker_host, &binding.target)?;
    attempt.phase("caller")?;
    let caller = observer.caller()?;
    attempt.record("caller", &caller)?;
    evidence::validate_caller(&caller)?;
    attempt.phase("service_before")?;
    let service_before = observer.service(caller.uid)?;
    attempt.record("service_before", &service_before)?;
    evidence::loaded(&service_before, caller.uid)?;
    attempt.phase("crash_reports")?;
    let crash_reports = capture_crashes(&out, attempt)?;
    attempt.record("crash_reports", &crash_reports)?;
    crashes::require_known(&crash_reports, &host, caller.uid)?;
    let setup = provision_service(&observer, attempt, caller.uid)?;
    if observer.binding(archive, reference)? != binding {
        return Err("Picker input/source binding changed during preparation".into());
    }
    let prepared = Preparation {
        stage: "picker_host_preparation".into(),
        binding,
        picker_host,
        host,
        caller,
        service_before,
        crash_reports,
        bootout: setup.bootout,
        bootout_absence: setup.bootout_absence,
        disable: setup.disable,
        service_after: setup.service_after,
    };
    evidence::validate_preparation(&prepared)?;
    write_final(&out.join("preparation.json"), &prepared)?;
    Ok(prepared)
}
struct ServiceSetup {
    bootout: CommandObservation,
    bootout_absence: Option<PrintObservation>,
    disable: CommandObservation,
    service_after: ServiceObservation,
}
fn provision_service(
    observer: &Observer<'_>,
    attempt: &mut Attempt,
    uid: u32,
) -> Result<ServiceSetup> {
    attempt.phase("bootout")?;
    let bootout = observer.action("bootout", uid)?;
    attempt.record("bootout", &bootout)?;
    let code = evidence::action(&bootout, "bootout", uid)?;
    let bootout_absence = if code > 0 {
        attempt.phase("bootout_absence")?;
        let observation = observer.print(uid)?;
        attempt.record("bootout_absence", &observation)?;
        evidence::absent(&observation, uid)?;
        Some(observation)
    } else {
        None
    };
    attempt.phase("disable")?;
    let disable = observer.action("disable", uid)?;
    attempt.record("disable", &disable)?;
    if evidence::action(&disable, "disable", uid)? != 0 {
        return Err("Exact disable operation failed".into());
    }
    attempt.phase("service_after")?;
    let service_after = observer.service(uid)?;
    attempt.record("service_after", &service_after)?;
    evidence::provisioned(&service_after, uid)?;
    Ok(ServiceSetup {
        bootout,
        bootout_absence,
        disable,
        service_after,
    })
}
pub(crate) fn verify(
    root: &Path,
    build: &MacBuild,
    archive: &Path,
    reference: &Path,
    preparation: &Path,
    out: &Path,
) -> Result<Postverification> {
    require_context(root)?;
    let mut attempt = Attempt::new(out)?;
    let result = verify_observed(root, build, archive, reference, preparation, &mut attempt);
    attempt.finish(result)
}
fn verify_observed(
    root: &Path,
    build: &MacBuild,
    archive: &Path,
    reference: &Path,
    preparation: &Path,
    attempt: &mut Attempt,
) -> Result<Postverification> {
    let out = attempt.out.clone();
    let observer = Observer {
        root,
        build,
        out: &out,
    };
    attempt.phase("preparation_readback")?;
    let bytes = observations::read(preparation, DOCUMENT_LIMIT)?;
    let prepared = evidence::parse_preparation(&bytes)?;
    let binding = observer.binding(archive, reference)?;
    let picker_host = crate::distribution::picker_host_policy(root)?;
    attempt.record("binding", &binding)?;
    attempt.record("picker_host", &picker_host)?;
    if binding != prepared.binding || picker_host != prepared.picker_host {
        return Err(
            "Postverification inputs, source, workflow or policy differ from preparation".into(),
        );
    }
    attempt.phase("host_readback")?;
    let host = observer.host(&binding.target)?;
    attempt.record("host", &host)?;
    evidence::validate_host(&host, &picker_host, &binding.target)?;
    attempt.phase("caller_readback")?;
    let caller = observer.caller()?;
    attempt.record("caller", &caller)?;
    evidence::validate_caller(&caller)?;
    if host != prepared.host || caller != prepared.caller {
        return Err("Host boot/image/native identity or caller security context changed across product execution".into());
    }
    attempt.phase("service_readback")?;
    let service = observer.service(caller.uid)?;
    attempt.record("service", &service)?;
    evidence::provisioned(&service, caller.uid)?;
    if observer.binding(archive, reference)? != binding {
        return Err("Postverification input binding changed during readback".into());
    }
    let verified = Postverification {
        stage: "picker_host_verification".into(),
        binding,
        picker_host,
        host,
        caller,
        service,
    };
    evidence::validate_post(&prepared, &verified)?;
    write_final(&out.join("verification.json"), &verified)?;
    Ok(verified)
}
fn capture_crashes(out: &Path, attempt: &mut Attempt) -> Result<Vec<CrashReport>> {
    let directory = PathBuf::from(std::env::var_os("HOME").ok_or("Caller home is unavailable")?)
        .join("Library/Logs/DiagnosticReports");
    let mut selected = Vec::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or("Native crash filename is unrepresentable")?;
        if name.starts_with("iconservicesagent-") && name.ends_with(".ips") {
            selected.push((fs::symlink_metadata(&path)?.modified()?, path));
        }
    }
    selected.sort_by(|a, b| b.cmp(a));
    selected.truncate(crashes::MAX_REPORTS);
    let destination = out.join("crash-reports");
    fs::create_dir(&destination)?;
    let mut reports = Vec::new();
    let mut manifest = Vec::new();
    for (_, path) in selected {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("Invalid native crash name")?;
        let result = capture_report(&path, name, &destination);
        match result {
            Ok(report) => {
                manifest.push(json!({"file_name":name,"identity":report.identity}));
                reports.push(report);
            }
            Err(error) => {
                manifest.push(json!({"file_name":name,"error":error.to_string()}));
                attempt.record("crash_capture", &manifest)?;
                return Err(error);
            }
        }
        attempt.record("crash_capture", &manifest)?;
    }
    Ok(reports)
}
fn capture_report(path: &Path, name: &str, destination: &Path) -> Result<CrashReport> {
    let bytes = observations::read(path, crashes::REPORT_LIMIT)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination.join(name))?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    crashes::parse(name, &bytes)
}
