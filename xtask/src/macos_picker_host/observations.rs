//! Managed native observations; environment-bearing launchctl output stays private.
use super::*;
use crate::{controller, macos::MacBuild};
use std::{fs, io::Read, path::Path, time::Duration};

pub(super) fn read(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err("Host evidence input is not a bounded ordinary file".into());
    }
    let file = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::CLOEXEC | rustix::fs::OFlags::NOFOLLOW,
        rustix::fs::Mode::empty(),
    )?;
    let file = fs::File::from(file);
    if !file.metadata()?.is_file() {
        return Err("Host evidence handle is not an ordinary file".into());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err("Host evidence input grew past its bound".into());
    }
    Ok(bytes)
}
pub(super) struct Observer<'a> {
    pub(super) root: &'a Path,
    pub(super) build: &'a MacBuild,
    pub(super) out: &'a Path,
}
impl Observer<'_> {
    fn checked(&self, name: &str, program: &str, args: &[&str]) -> Result<String> {
        let mut command = self.build.command(self.root, program);
        command.args(args);
        let log = self.out.join(format!("{name}.log"));
        let process = controller()?.run_separate_output(command, &log, Duration::from_secs(20))?;
        process.require_success(&log)?;
        Ok(std::str::from_utf8(&read(&log, 1024 * 1024)?)?
            .trim()
            .to_owned())
    }
    pub(super) fn host(&self, target: &str) -> Result<Host> {
        let observed =
            crate::native_host::inspect(self.root, target, "macos", &self.build.architecture)?;
        let field = |name: &str| -> Result<String> {
            observed[name]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| "Native host observation field is unavailable".into())
        };
        Ok(Host {
            target: field("target")?,
            process_os: field("process_os")?,
            process_architecture: field("process_architecture")?,
            kernel_machine: field("kernel_machine")?,
            image_os: std::env::var("ImageOS")?,
            image_version: std::env::var("ImageVersion")?,
            product_version: self.checked(
                "product-version",
                "/usr/bin/sw_vers",
                &["-productVersion"],
            )?,
            build_version: self.checked("build-version", "/usr/bin/sw_vers", &["-buildVersion"])?,
            boot_uuid: self
                .checked(
                    "boot-uuid",
                    "/usr/sbin/sysctl",
                    &["-n", "kern.bootsessionuuid"],
                )?
                .to_ascii_lowercase(),
        })
    }
    pub(super) fn binding(&self, archive: &Path, reference: &Path) -> Result<Binding> {
        if !crate::git(
            self.root,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        )?
        .is_empty()
        {
            return Err("Picker host evidence requires the exact clean source checkout".into());
        }
        let source = crate::git(self.root, &["rev-parse", "HEAD"])?;
        // MacBuild::load already selects this native host from the distribution table.
        let target = self.build.host.clone();
        if self.build.architecture != "x86_64" {
            return Err(
                "Picker host build is not the existing Intel macOS distribution target".into(),
            );
        }
        let reference_bytes = read(reference, 4 * 1024 * 1024)?;
        let declared: Value = serde_json::from_slice(&reference_bytes)?;
        if declared["source"] != source
            || declared["target"] != target
            || declared["architecture"] != "x86_64"
        {
            return Err(
                "Picker archive reference differs from current source/native target".into(),
            );
        }
        if declared["schema"] != 2
            || declared["workflow"]
                != serde_json::json!({"run":std::env::var("GITHUB_RUN_ID")?,"attempt":std::env::var("GITHUB_RUN_ATTEMPT")?,"workflow_ref":std::env::var("GITHUB_WORKFLOW_REF")?,"workflow_sha":std::env::var("GITHUB_WORKFLOW_SHA")?})
        {
            return Err("Frozen picker reference lacks exact current-run workflow identity".into());
        }
        Ok(Binding {
            source,
            target,
            run: std::env::var("GITHUB_RUN_ID")?,
            attempt: std::env::var("GITHUB_RUN_ATTEMPT")?,
            workflow_ref: std::env::var("GITHUB_WORKFLOW_REF")?,
            workflow_sha: std::env::var("GITHUB_WORKFLOW_SHA")?,
            archive: xtask::archive::ArchiveSnapshot::read(archive)?.identity(),
            reference: ByteIdentity::of(&reference_bytes),
        })
    }
    pub(super) fn caller(&self) -> Result<Caller> {
        let executable = self.out.join("CallerSessionObservation");
        let mut compile = self.build.command(self.root, &self.build.swiftc);
        compile
            .args([
                "-swift-version",
                "6",
                "-warnings-as-errors",
                "-target",
                &self.build.swift_target(),
                "-sdk",
            ])
            .arg(&self.build.sdk)
            .args(["-framework", "Security"])
            .arg(self.root.join("tests/macos/CallerSessionObservation.swift"))
            .arg("-o")
            .arg(&executable);
        crate::macos::checked_run(compile, &self.out.join("caller-build.log"), 120)?;
        crate::macos::fixture_identity(
            &read(&executable, 16 * 1024 * 1024)?,
            &self.build.architecture,
            self.build.minimum,
        )?;
        let command = self.build.command(self.root, &executable);
        let log = self.out.join("caller-session.log");
        let process = controller()?.run_separate_output(command, &log, Duration::from_secs(20))?;
        if evidence::clean(&ProcessObservation::from_process(&process)?)? != 0 {
            return Err("Caller security-session observer did not exit successfully".into());
        }
        let (uid, euid, console_uid) = unix_identity()?;
        Ok(Caller {
            uid,
            euid,
            console_uid,
            security_session: serde_json::from_slice(&read(&log, 16 * 1024)?)?,
        })
    }
    pub(super) fn print(&self, uid: u32) -> Result<PrintObservation> {
        let exact = evidence::target(uid);
        private_observation(self, "print", &exact, |process, stdout, stderr| {
            Ok(PrintObservation {
                program: LAUNCHCTL.into(),
                arguments: vec!["print".into(), exact.clone()],
                process,
                selected_fields: evidence::parse_fields(stdout)?.into(),
                absence_diagnostic: evidence::parse_absence(stderr, uid)?,
            })
        })
    }
    pub(super) fn disabled(&self, uid: u32) -> Result<DisabledObservation> {
        let domain = format!("gui/{uid}");
        private_observation(self, "print-disabled", &domain, |process, stdout, _| {
            Ok(DisabledObservation {
                program: LAUNCHCTL.into(),
                arguments: vec!["print-disabled".into(), domain.clone()],
                process,
                r#override: evidence::parse_disabled(stdout)?,
            })
        })
    }
    pub(super) fn service(&self, uid: u32) -> Result<ServiceObservation> {
        Ok(ServiceObservation {
            target: evidence::target(uid),
            print: self.print(uid)?,
            disabled: self.disabled(uid)?,
        })
    }
    pub(super) fn action(
        &self,
        operation: &str,
        uid: u32,
        guard: impl FnOnce() -> Result<crate::picker_evidence::ActionGuardObservation>,
    ) -> Result<CommandObservation> {
        check_unix_identity(uid)?;
        let exact = evidence::target(uid);
        let mut command = self.build.command(self.root, LAUNCHCTL);
        command.args([operation, &exact]);
        let log = self.out.join(format!("{operation}.log"));
        guarded_service_action(guard, |guard| {
            let process =
                controller()?.run_separate_output(command, &log, Duration::from_secs(20))?;
            // Bound readback without forwarding potentially unexpected native output.
            read(&log, 1024 * 1024)?;
            read(&log.with_extension("stderr"), 1024 * 1024)?;
            Ok(CommandObservation {
                guard,
                program: LAUNCHCTL.into(),
                arguments: vec![operation.into(), exact],
                process: ProcessObservation::from_process(&process)?,
                stdout_path: log.to_str().ok_or("Non-UTF8 host evidence path")?.into(),
                stderr_path: log
                    .with_extension("stderr")
                    .to_str()
                    .ok_or("Non-UTF8 host evidence path")?
                    .into(),
            })
        })
    }
}
// This is the actual service-spawn edge. A failed fresh admission guard must
// prevent execution, not merely change the receipt after a command ran.
pub(super) fn guarded_service_action(
    guard: impl FnOnce() -> Result<crate::picker_evidence::ActionGuardObservation>,
    execute: impl FnOnce(crate::picker_evidence::ActionGuardObservation) -> Result<CommandObservation>,
) -> Result<CommandObservation> {
    let observation = guard()?;
    execute(observation)
}
fn private_observation<T>(
    observer: &Observer<'_>,
    operation: &str,
    target: &str,
    parse: impl FnOnce(ProcessObservation, &str, &str) -> Result<T>,
) -> Result<T> {
    let private = tempfile::Builder::new()
        .prefix("eml-picker-private-")
        .tempdir()?;
    let log = private.path().join("launchctl.log");
    let observed = (|| {
        let mut command = observer.build.command(observer.root, LAUNCHCTL);
        command.args([operation, target]);
        let process = controller()?.run_separate_output(command, &log, Duration::from_secs(20));
        retain_private_status(observer.out, operation, target, &process)?;
        let process = process?;
        parse(
            ProcessObservation::from_process(&process)?,
            std::str::from_utf8(&read(&log, 1024 * 1024)?)?,
            std::str::from_utf8(&read(&log.with_extension("stderr"), 1024 * 1024)?)?,
        )
    })();
    if let Err(error) = &observed {
        // Raw output may contain inherited environment values. Never copy it to
        // the uploaded tree, including after unknown syntax or controller failure.
        let detail = error.to_string();
        let _private_path = private.keep();
        return Err(format!("Targeted service observation failed: {detail}; raw evidence remains private outside uploaded tree").into());
    }
    observed
}
fn retain_private_status(
    out: &Path,
    operation: &str,
    target: &str,
    result: &Result<xtask::process::ProcessResult>,
) -> Result<()> {
    use std::io::Write;
    let observation = match result {
        Ok(process) => {
            serde_json::json!({"operation":operation,"target":target,"process":process.evidence()})
        }
        Err(error) => {
            serde_json::json!({"operation":operation,"target":target,"error":error.to_string()})
        }
    };
    let mut file = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(out.join("service-command-observations.jsonl"))?;
    file.write_all(&serde_json::to_vec(&observation)?)?;
    file.write_all(b"\n")?;
    Ok(())
}
pub(super) fn unix_identity() -> Result<(u32, u32, u32)> {
    use std::os::unix::fs::MetadataExt;
    Ok((
        rustix::process::getuid().as_raw(),
        rustix::process::geteuid().as_raw(),
        fs::metadata("/dev/console")?.uid(),
    ))
}
pub(super) fn check_unix_identity(expected: u32) -> Result<()> {
    let (uid, euid, console_uid) = unix_identity()?;
    if uid == 0 || uid != expected || euid != expected || console_uid != expected {
        return Err("Console/real/effective user changed before host action".into());
    }
    Ok(())
}
