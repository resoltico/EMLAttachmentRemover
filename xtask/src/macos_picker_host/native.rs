//! Independent native picker qualification and consumed incident intervention.
use super::{
    observations::{self, Observer},
    *,
};
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

// Neither this owner nor its monotonic deadline can be deserialized or cloned.
// Its constructor verifies current native proof; consuming it admits one flow.
struct IncidentAdmission {
    binding: Binding,
    host: Host,
    caller: Caller,
    service_before: ServiceObservation,
    crash_reports: Vec<CrashReport>,
    identity: crate::host_incidents::IncidentIdentity,
    policy: crate::host_incidents::IncidentRecord,
    deadline: crate::host_incidents::ActionDeadline,
    admitted_at: u64,
    admitted_subsec_ns: u32,
    action_deadline_wall: u64,
}
fn clock_seconds(time: std::time::SystemTime) -> Result<u64> {
    Ok(time.duration_since(std::time::UNIX_EPOCH)?.as_secs())
}
fn host_policy(host: &Host) -> crate::host_incidents::IncidentHost {
    crate::host_incidents::IncidentHost {
        image_os: host.image_os.clone(),
        image_version: host.image_version.clone(),
        product_version: host.product_version.clone(),
        build_version: host.build_version.clone(),
    }
}
fn snapshot(
    observer: &Observer<'_>,
    plan: &crate::distribution::BuildPlan,
    binding: &Binding,
) -> Result<crate::picker_evidence::NormalHostSnapshot> {
    let host = observer.host(&binding.target)?;
    let caller = observer.caller()?;
    let disabled_override = observer.disabled(caller.uid)?;
    let value = crate::picker_evidence::NormalHostSnapshot {
        host,
        caller,
        disabled_override,
    };
    crate::picker_evidence::validate_snapshot(plan, binding, &value)?;
    Ok(value)
}
fn admit_incident(
    observer: &Observer<'_>,
    archive: &Path,
    reference: &Path,
    before: &crate::picker_evidence::NormalHostSnapshot,
    panel: &crate::picker_evidence::PanelReceipt,
    attempt: &mut Attempt,
) -> Result<IncidentAdmission> {
    crate::picker_evidence::validate_panel(panel, false)?;
    require_context(observer.root)?;
    let binding = observer.binding(archive, reference)?;
    let host = observer.host(&binding.target)?;
    let caller = observer.caller()?;
    evidence::validate_caller(&caller)?;
    if host != before.host || caller != before.caller {
        return Err("Native host or caller changed before incident admission".into());
    }
    // This is the first policy, loaded-service or crash access in the flow.
    let registry = crate::host_incidents::Registry::load(observer.root)?;
    let (identity, policy) = registry.select(
        &host_policy(&host),
        &host.process_architecture,
        &binding.source,
        std::time::SystemTime::now(),
    )?;
    evidence::validate_host(&host, &policy.host, &binding.target)?;
    attempt.record("incident_identity", &identity)?;
    let service_before = observer.service(caller.uid)?;
    evidence::loaded(&service_before, caller.uid)?;
    if service_before.disabled.r#override == Some(true) {
        return Err("Pre-disabled host cannot admit incident intervention".into());
    }
    attempt.record("service_before", &service_before)?;
    let crash_reports = capture_crashes(observer.out, attempt)?;
    crashes::require_known(&crash_reports, &host, caller.uid)?;
    attempt.record("crash_reports", &crash_reports)?;
    let monotonic = std::time::Instant::now();
    let wall = std::time::SystemTime::now();
    let admitted_subsec_ns = wall.duration_since(std::time::UNIX_EPOCH)?.subsec_nanos();
    let admitted_at = clock_seconds(wall)?;
    let deadline = crate::host_incidents::ActionDeadline::admit(&policy, wall, monotonic)?;
    let action_deadline_wall = admitted_at
        .checked_add(
            policy
                .expires_at
                .checked_sub(admitted_at)
                .ok_or("Incident lifetime elapsed")?
                .min(600),
        )
        .ok_or("Incident wall deadline overflow")?;
    Ok(IncidentAdmission {
        binding,
        host,
        caller,
        service_before,
        crash_reports,
        identity,
        policy,
        deadline,
        admitted_at,
        admitted_subsec_ns,
        action_deadline_wall,
    })
}
impl IncidentAdmission {
    fn check_clock_policy(
        &self,
        root: &Path,
    ) -> Result<crate::picker_evidence::ActionGuardObservation> {
        let wall = std::time::SystemTime::now();
        self.deadline.check(wall, std::time::Instant::now())?;
        crate::host_incidents::Registry::load(root)?.recheck(&self.identity, &self.policy, wall)?;
        let wall = std::time::SystemTime::now();
        let monotonic = std::time::Instant::now();
        self.deadline.check(wall, monotonic)?;
        Ok(crate::picker_evidence::ActionGuardObservation {
            wall_at: clock_seconds(wall)?,
            monotonic_elapsed_ns: self.deadline.elapsed_ns(monotonic)?,
        })
    }
    fn revalidate(
        &self,
        observer: &Observer<'_>,
        archive: &Path,
        reference: &Path,
        out: &Path,
    ) -> Result<()> {
        // Each pre-action observation is fresh evidence. The process owner
        // deliberately refuses to overwrite earlier exclusive output files.
        fs::create_dir(out)?;
        let observer = Observer {
            root: observer.root,
            build: observer.build,
            out,
        };
        require_context(observer.root)?;
        if observer.binding(archive, reference)? != self.binding
            || observer.host(&self.binding.target)? != self.host
            || observer.caller()? != self.caller
        {
            return Err("Incident host/caller/boot/source/workflow/run/archive/reference binding changed before action".into());
        }
        evidence::validate_caller(&self.caller)?;
        self.check_clock_policy(observer.root).map(|_| ())
    }
}
fn prepare_owned(
    observer: &Observer<'_>,
    archive: &Path,
    reference: &Path,
    admission: IncidentAdmission,
    attempt: &mut Attempt,
) -> Result<(
    Preparation,
    crate::picker_evidence::IncidentAdmissionReceipt,
)> {
    attempt.phase("bootout")?;
    admission.revalidate(
        observer,
        archive,
        reference,
        &observer.out.join("before-bootout"),
    )?;
    let bootout = observer.action("bootout", admission.caller.uid, || {
        admission.check_clock_policy(observer.root)
    })?;
    attempt.record("bootout", &bootout)?;
    let code = evidence::action(&bootout, "bootout", admission.caller.uid)?;
    let bootout_absence = if code > 0 {
        let observation = observer.print(admission.caller.uid)?;
        attempt.record("bootout_absence", &observation)?;
        evidence::absent(&observation, admission.caller.uid)?;
        Some(observation)
    } else {
        None
    };
    attempt.phase("disable")?;
    admission.revalidate(
        observer,
        archive,
        reference,
        &observer.out.join("before-disable"),
    )?;
    let disable = observer.action("disable", admission.caller.uid, || {
        admission.check_clock_policy(observer.root)
    })?;
    attempt.record("disable", &disable)?;
    if evidence::action(&disable, "disable", admission.caller.uid)? != 0 {
        return Err("Exact incident disable action failed".into());
    }
    let service_after = observer.service(admission.caller.uid)?;
    attempt.record("service_after", &service_after)?;
    evidence::provisioned(&service_after, admission.caller.uid)?;
    let receipt = crate::picker_evidence::IncidentAdmissionReceipt {
        identity: admission.identity,
        policy: admission.policy.clone(),
        admitted_at: admission.admitted_at,
        admitted_subsec_ns: admission.admitted_subsec_ns,
        action_deadline_wall: admission.action_deadline_wall,
    };
    let prepared = Preparation {
        stage: "picker_host_preparation".into(),
        binding: admission.binding,
        picker_host: admission.policy.host,
        host: admission.host,
        caller: admission.caller,
        service_before: admission.service_before,
        crash_reports: admission.crash_reports,
        bootout,
        bootout_absence,
        disable,
        service_after,
    };
    evidence::validate_preparation(&prepared)?;
    write_final(&observer.out.join("preparation.json"), &prepared)?;
    Ok((prepared, receipt))
}
fn verify_after(
    observer: &Observer<'_>,
    archive: &Path,
    reference: &Path,
    prepared: &Preparation,
    admission: &crate::picker_evidence::IncidentAdmissionReceipt,
) -> Result<Postverification> {
    require_context(observer.root)?;
    crate::host_incidents::Registry::load(observer.root)?.recheck(
        &admission.identity,
        &admission.policy,
        std::time::SystemTime::now(),
    )?;
    let binding = observer.binding(archive, reference)?;
    let host = observer.host(&binding.target)?;
    let caller = observer.caller()?;
    evidence::validate_caller(&caller)?;
    let service = observer.service(caller.uid)?;
    let verified = Postverification {
        stage: "picker_host_verification".into(),
        binding,
        picker_host: admission.policy.host.clone(),
        host,
        caller,
        service,
    };
    evidence::validate_post(prepared, &verified)?;
    write_final(&observer.out.join("verification.json"), &verified)?;
    Ok(verified)
}
fn probe_identity(value: &Value) -> Result<crate::picker_evidence::ProbeIdentity> {
    use crate::picker_evidence::{ProbeFiles, ProbeIdentity};
    let text = |name: &str| -> Result<String> {
        value[name]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("Probe identity field is absent: {name}").into())
    };
    let files = &value["files"];
    Ok(ProbeIdentity {
        path: text("path")?,
        files: ProbeFiles {
            info: serde_json::from_value(files["Contents/Info.plist"].clone())?,
            executable: serde_json::from_value(
                files["Contents/MacOS/NativePanelDiagnostic"].clone(),
            )?,
            resources: serde_json::from_value(
                files["Contents/_CodeSignature/CodeResources"].clone(),
            )?,
        },
        signature_display: text("signature_display")?,
        scope: text("scope")?,
    })
}
fn panel(value: Value) -> Result<crate::picker_evidence::PanelReceipt> {
    use crate::picker_evidence::{PanelObservation, PanelReceipt, ProcessObservation};
    let observation = match value["observation"].as_str() {
        Some("healthy") => PanelObservation::Healthy,
        Some("known_panel_failure") => PanelObservation::KnownFailure,
        _ => return Err("Unclassified native probe cannot admit qualification".into()),
    };
    let receipt = PanelReceipt {
        identity: probe_identity(&value["identity"])?,
        identity_after: probe_identity(&value["identity_after"])?,
        process: serde_json::from_value::<ProcessObservation>(value["process"].clone())?,
        phases: serde_json::from_value(value["phases"].clone())?,
        observation,
    };
    crate::picker_evidence::validate_panel(&receipt, observation == PanelObservation::Healthy)?;
    Ok(receipt)
}
fn qualified_output_path(root: &Path, out: &Path) -> Result<PathBuf> {
    if !root.is_absolute() {
        return Err("Picker qualification needs an absolute repository root".into());
    }
    Ok(root.join(out))
}
pub(crate) fn qualify(
    root: &Path,
    plan: &crate::distribution::BuildPlan,
    archive: &Path,
    reference: &Path,
    out: &Path,
) -> Result<()> {
    require_context(root)?;
    let output = qualified_output_path(root, out)?;
    let out = output.as_path();
    let build = crate::macos::load(root, plan)?;
    let mut attempt = Attempt::new(out)?;
    let result = (|| -> Result<()> {
        let observer = Observer {
            root,
            build: &build,
            out,
        };
        attempt.phase("binding")?;
        let binding = observer.binding(archive, reference)?;
        attempt.record("binding", &binding)?;
        let before = snapshot(&observer, plan, &binding)?;
        attempt.record("before", &before)?;
        let probe = crate::macos_panel::build_probe(root, &build, &out.join("probe"))?;
        attempt.phase("independent_probe")?;
        let first_panel = panel(crate::macos_panel::run_probe(
            root,
            &build,
            &probe,
            &out.join("panel-before"),
        )?)?;
        attempt.record("panel_before", &first_panel)?;
        let intervention =
            if first_panel.observation == crate::picker_evidence::PanelObservation::KnownFailure {
                let incident_out = out.join("incident");
                let mut incident_attempt = Attempt::new(&incident_out)?;
                let incident_observer = Observer {
                    root,
                    build: &build,
                    out: &incident_out,
                };
                let prepared = (|| -> Result<_> {
                    let admission = admit_incident(
                        &incident_observer,
                        archive,
                        reference,
                        &before,
                        &first_panel,
                        &mut incident_attempt,
                    )?;
                    prepare_owned(
                        &incident_observer,
                        archive,
                        reference,
                        admission,
                        &mut incident_attempt,
                    )
                })();
                let (prepared, admission) = incident_attempt.finish(prepared)?;
                let recovered = panel(crate::macos_panel::run_probe(
                    root,
                    &build,
                    &probe,
                    &out.join("panel-recovered"),
                )?)?;
                crate::picker_evidence::validate_panel(&recovered, true)?;
                attempt.record("recovered_panel", &recovered)?;
                Some((prepared, admission, recovered))
            } else {
                None
            };
        // Exactly one actual product call. Every failure is terminal in either route.
        attempt.phase("actual_frozen_product")?;
        if observer.binding(archive, reference)? != binding {
            return Err("Frozen input changed before product execution".into());
        }
        let product_path = crate::macos_archive::picker_receipt(root, plan, archive, reference)?;
        let product_bytes = observations::read(&product_path, DOCUMENT_LIMIT)?;
        let product =
            crate::picker_evidence::project_product(&product_bytes, &binding, &before.host)?;
        let mut product_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(out.join("product-result.json"))?;
        product_file.write_all(&product_bytes)?;
        product_file.sync_all()?;
        if observations::read(&out.join("product-result.json"), DOCUMENT_LIMIT)? != product_bytes {
            return Err("Preserved product evidence differs from observed native bytes".into());
        }
        attempt.record("product_identity", &product.raw_product_identity)?;
        attempt.phase("postverification")?;
        let qualification = if let Some((prepared, admission, recovered_panel)) = intervention {
            let verified_out = out.join("postverification");
            fs::create_dir(&verified_out)?;
            let verified_observer = Observer {
                root,
                build: &build,
                out: &verified_out,
            };
            let postverification = verify_after(
                &verified_observer,
                archive,
                reference,
                &prepared,
                &admission,
            )?;
            crate::picker_evidence::PickerQualification::IncidentModifiedHost(Box::new(
                crate::picker_evidence::IncidentModifiedHostReceipt {
                    binding: binding.clone(),
                    failed_panel: first_panel,
                    admission,
                    preparation: prepared,
                    recovered_panel,
                    product,
                    postverification,
                },
            ))
        } else {
            let after = snapshot(&observer, plan, &binding)?;
            attempt.record("after", &after)?;
            crate::picker_evidence::PickerQualification::UnmodifiedHost(Box::new(
                crate::picker_evidence::UnmodifiedHostReceipt {
                    binding: binding.clone(),
                    before,
                    panel: first_panel,
                    product,
                    after,
                    actions: Vec::new(),
                },
            ))
        };
        if observer.binding(archive, reference)? != binding {
            return Err("Frozen input/source changed after product execution".into());
        }
        let bytes = serde_json::to_vec_pretty(&qualification)?;
        crate::picker_evidence::verify_receipt(
            root,
            plan,
            &bytes,
            &binding,
            std::time::SystemTime::now(),
        )?;
        write_final(&out.join("result.json"), &qualification)?;
        attempt.phase("qualified")?;
        Ok(())
    })();
    attempt.finish(result)
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

#[cfg(test)]
mod tests;
