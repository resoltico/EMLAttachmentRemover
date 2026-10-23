//! Independent data oracles; actual hosted native product integration is separate.
use super::*;
use serde_json::json;

fn policy() -> IncidentHost {
    IncidentHost {
        image_os: "macos26".into(),
        image_version: "20260824.0517.1".into(),
        product_version: "26.6.1".into(),
        build_version: "25G76".into(),
    }
}
pub(super) fn host() -> Host {
    Host {
        target: "x86_64-apple-darwin".into(),
        process_os: "macos".into(),
        process_architecture: "x86_64".into(),
        kernel_machine: "x86_64".into(),
        image_os: "macos26".into(),
        image_version: "20260824.0517.1".into(),
        product_version: "26.6.1".into(),
        build_version: "25G76".into(),
        boot_uuid: "12345678-1234-1234-1234-123456789abc".into(),
    }
}
fn caller() -> Caller {
    Caller {
        uid: 501,
        euid: 501,
        console_uid: 501,
        security_session: SecuritySession {
            status: 0,
            security_session_id: Some(9),
            graphic_access: Some(true),
        },
    }
}
fn process(code: i64) -> ProcessObservation {
    serde_json::from_value(json!({"exit_code":code,"signal":null,"timed_out":false,"cancelled":false,"unexpected_survivors":0,"cleanup_complete":true,"elapsed_seconds":0.1,"cleanup_seconds":0.0})).unwrap()
}
fn command(operation: &str) -> CommandObservation {
    CommandObservation {
        program: LAUNCHCTL.into(),
        arguments: vec![operation.into(), format!("gui/501/{SERVICE}")],
        process: process(0),
        stdout_path: "owned/command.log".into(),
        stderr_path: "owned/command.stderr".into(),
        guard: crate::picker_evidence::ActionGuardObservation {
            wall_at: 1792713600,
            monotonic_elapsed_ns: 1,
        },
    }
}
fn service(present: bool) -> ServiceObservation {
    let fields = if present {
        [
            ("path", APPLE_PLIST),
            ("program", APPLE_PROGRAM),
            ("type", "LaunchAgent"),
            ("state", "spawn scheduled"),
            ("domain", "gui/501 [100001]"),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect()
    } else {
        BTreeMap::new()
    };
    ServiceObservation {
        target: format!("gui/501/{SERVICE}"),
        print: PrintObservation {
            program: LAUNCHCTL.into(),
            arguments: vec!["print".into(), format!("gui/501/{SERVICE}")],
            process: process(if present { 0 } else { 113 }),
            selected_fields: fields.into(),
            absence_diagnostic: (!present).then(|| {
                format!("Could not find service \"{SERVICE}\" in domain for user gui: 501")
            }),
        },
        disabled: DisabledObservation {
            program: LAUNCHCTL.into(),
            arguments: vec!["print-disabled".into(), "gui/501".into()],
            process: process(0),
            r#override: (!present).then_some(true),
        },
    }
}
fn crash() -> CrashReport {
    CrashReport {
        file_name: "iconservicesagent-fixture.ips".into(),
        identity: ByteIdentity {
            sha256: "a".repeat(64),
            byte_length: 100,
        },
        pid: 42,
        user_id: 501,
        boot_uuid: host().boot_uuid,
        os_build: "25G76".into(),
        capture_time: "2026-10-21 00:00:00.0000 +0000".into(),
        proc_path: APPLE_PROGRAM.into(),
        exception_signal: "SIGABRT".into(),
        termination: Termination {
            namespace: "SIGNAL".into(),
            code: 6,
        },
        faulting_thread: FaultThread {
            index: 1,
            triggered: true,
            frames: vec![
                FaultFrame {
                    symbol: Some(
                        "+[MTLLoader sliceIDForDevice:legacyDriverVersion:airntDriverVersion:]"
                            .into(),
                    ),
                    image_name: "Metal".into(),
                    image_path: METAL.into(),
                },
                FaultFrame {
                    symbol: Some("RB::FunctionLibrary::FunctionLibrary".into()),
                    image_name: "RenderBox".into(),
                    image_path: RENDERBOX.into(),
                },
            ],
        },
    }
}
fn preparation() -> Preparation {
    Preparation {
        stage: "picker_host_preparation".into(),
        binding: Binding {
            source: "a".repeat(40),
            target: "x86_64-apple-darwin".into(),
            run: "12".into(),
            attempt: "1".into(),
            workflow_ref: "resoltico/EMLAttachmentRemover/.github/workflows/ci.yml@refs/heads/main"
                .into(),
            workflow_sha: "b".repeat(40),
            archive: ByteIdentity {
                sha256: "c".repeat(64),
                byte_length: 100,
            },
            reference: ByteIdentity {
                sha256: "d".repeat(64),
                byte_length: 50,
            },
        },
        picker_host: policy(),
        host: host(),
        caller: caller(),
        service_before: service(true),
        crash_reports: vec![crash()],
        bootout: command("bootout"),
        bootout_absence: None,
        disable: command("disable"),
        service_after: service(false),
    }
}
fn post(prepared: &Preparation) -> Postverification {
    Postverification {
        stage: "picker_host_verification".into(),
        binding: prepared.binding.clone(),
        picker_host: policy(),
        host: host(),
        caller: caller(),
        service: service(false),
    }
}
#[test]
fn exact_policy_and_caller_are_required_without_conflating_session_namespaces() {
    assert!(evidence::validate_preparation(&preparation()).is_ok());
    for field in [
        "image_os",
        "image_version",
        "product_version",
        "build_version",
        "kernel_machine",
        "process_architecture",
        "process_os",
        "boot_uuid",
    ] {
        let mut raw = serde_json::to_value(host()).unwrap();
        raw[field] = json!("unknown");
        let wrong: Host = serde_json::from_value(raw).unwrap();
        assert!(
            evidence::validate_host(&wrong, &policy(), "x86_64-apple-darwin").is_err(),
            "{field}"
        );
    }
    for field in ["uid", "euid", "console_uid"] {
        let mut raw = serde_json::to_value(caller()).unwrap();
        raw[field] = json!(0);
        assert!(evidence::validate_caller(&serde_json::from_value(raw).unwrap()).is_err());
    }
    for (field, value) in [
        ("status", json!(1)),
        ("security_session_id", json!(0)),
        ("security_session_id", json!(u32::MAX)),
        ("graphic_access", json!(false)),
    ] {
        let mut raw = serde_json::to_value(caller()).unwrap();
        raw["security_session"][field] = value;
        assert!(evidence::validate_caller(&serde_json::from_value(raw).unwrap()).is_err());
    }
}
#[test]
fn service_parsing_excludes_environment_and_rejects_duplicate_or_unknown_fields() {
    let text = format!(
        "gui/501/{SERVICE} = {{\n\tpath = {APPLE_PLIST}\n\tprogram = {APPLE_PROGRAM}\n\tenvironment = {{\n\t\tpath = SECRET\n\t\tTOKEN = SECRET\n\t}}\n}}\n"
    );
    let fields = evidence::parse_fields(&text).unwrap();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields["path"], APPLE_PLIST);
    assert!(!serde_json::to_string(&fields).unwrap().contains("SECRET"));
    assert!(evidence::parse_fields(&format!("{text}\tpath = other\n")).is_err());
    for (native, expected) in [("disabled", true), ("enabled", false)] {
        assert_eq!(
            evidence::parse_disabled(&format!(
                "disabled services = {{\n\t\t\"{SERVICE}\" => {native}\n}}\n"
            ))
            .unwrap(),
            Some(expected)
        );
    }
    assert_eq!(
        evidence::parse_disabled("disabled services = {}\n").unwrap(),
        None
    );
    for text in [
        String::new(),
        "unrecognized output".into(),
        "disabled services = {".into(),
        format!("\"{SERVICE}\" => disabled\n"),
        format!("disabled services = {{\n\"{SERVICE}\" => disabled\n\"{SERVICE}\" => enabled\n}}"),
        format!("\"{SERVICE}\" => true\n"),
        format!("\"{SERVICE}\" => disabled\n\"{SERVICE}\" => enabled\n"),
        format!("\"{SERVICE}\" ? disabled\n"),
    ] {
        assert!(evidence::parse_disabled(&text).is_err());
    }
}
#[test]
fn absence_parser_requires_the_exact_service_and_user_diagnostic() {
    let exact = "Could not find service \"com.apple.iconservices.iconservicesagent\" in domain for user gui: 501";
    assert_eq!(
        evidence::parse_absence(&format!("  {exact}\n"), 501)
            .unwrap()
            .as_deref(),
        Some(exact)
    );
    assert_eq!(evidence::parse_absence(exact, 502).unwrap(), None);
    for unknown in ["", "Could not find service", "permission denied"] {
        assert_eq!(evidence::parse_absence(unknown, 501).unwrap(), None);
    }
    assert!(evidence::parse_absence(&format!("{exact}\n{exact}\n"), 501).is_err());
}
#[test]
fn clean_commands_and_exact_post_state_are_required_even_after_exit_zero() {
    let valid = preparation();
    for field in [
        "timed_out",
        "cancelled",
        "cleanup_complete",
        "unexpected_survivors",
        "signal",
    ] {
        let mut wrong = preparation();
        match field {
            "cleanup_complete" => wrong.disable.process.cleanup_complete = false,
            "unexpected_survivors" => wrong.disable.process.unexpected_survivors = 1,
            "signal" => wrong.disable.process.signal = Some(9),
            "timed_out" => wrong.disable.process.timed_out = true,
            _ => wrong.disable.process.cancelled = true,
        }
        assert!(evidence::validate_preparation(&wrong).is_err(), "{field}");
    }
    for observed in [None, Some(false)] {
        let mut wrong = preparation();
        wrong.service_after.disabled.r#override = observed;
        assert!(evidence::validate_preparation(&wrong).is_err());
    }
    let mut wrong = preparation();
    wrong.service_after.print.process = process(0);
    assert!(evidence::validate_preparation(&wrong).is_err());
    let mut wrong = preparation();
    wrong.disable.arguments[1] = "gui/502/other".into();
    assert!(evidence::validate_preparation(&wrong).is_err());
    for observed in [None, Some(false), Some(true)] {
        let mut before = preparation();
        before.service_before.disabled.r#override = observed;
        assert!(evidence::validate_preparation(&before).is_ok());
    }
    assert!(evidence::validate_post(&valid, &post(&valid)).is_ok());
}
#[test]
fn nonzero_bootout_needs_immediate_known_absence_but_disable_needs_zero() {
    for code in [1, 3, 113] {
        let mut prepared = preparation();
        prepared.bootout.process = process(code);
        assert!(evidence::validate_preparation(&prepared).is_err());
        prepared.bootout_absence = Some(service(false).print);
        assert!(evidence::validate_preparation(&prepared).is_ok());
        prepared.disable.process = process(1);
        assert!(evidence::validate_preparation(&prepared).is_err());
    }
    let mut prepared = preparation();
    prepared.bootout_absence = Some(service(false).print);
    assert!(evidence::validate_preparation(&prepared).is_err());
}
#[test]
fn postverification_rejects_changed_binding_policy_boot_and_opaque_caller_context() {
    let prepared = preparation();
    for path in [
        "/binding/source",
        "/binding/run",
        "/binding/attempt",
        "/binding/workflow_sha",
        "/binding/archive/sha256",
        "/binding/reference/sha256",
        "/picker_host/image_version",
        "/host/boot_uuid",
        "/caller/security_session/security_session_id",
    ] {
        let mut raw = serde_json::to_value(post(&prepared)).unwrap();
        *raw.pointer_mut(path).unwrap() = if path.ends_with("security_session_id") {
            json!(10)
        } else {
            json!("changed")
        };
        assert!(
            evidence::validate_post(&prepared, &serde_json::from_value(raw).unwrap()).is_err(),
            "{path}"
        );
    }
}
#[test]
fn partial_or_failed_records_never_promote_even_with_other_valid_facts() {
    let bytes = serde_json::to_vec(&preparation()).unwrap();
    assert!(
        serde_json::from_slice::<Preparation>(&bytes)
            .and_then(|p| evidence::validate_preparation(&p).map_err(serde::de::Error::custom))
            .is_ok()
    );
    for field in ["failure", "error"] {
        let mut raw = serde_json::to_value(preparation()).unwrap();
        raw[field] = json!(null);
        assert!(serde_json::from_value::<Preparation>(raw.clone()).is_err());
    }
    let mut raw = serde_json::to_value(preparation()).unwrap();
    raw["stage"] = json!("attempt");
    assert!(
        evidence::validate_preparation(&serde_json::from_value::<Preparation>(raw).unwrap())
            .is_err()
    );
}
#[test]
fn authorization_rejects_owner_desktop_other_jobs_and_unknown_workflow_files() {
    let base: BTreeMap<String, String> = [
        ("GITHUB_ACTIONS", "true"),
        ("RUNNER_ENVIRONMENT", "github-hosted"),
        ("GITHUB_REPOSITORY", "resoltico/EMLAttachmentRemover"),
        ("GITHUB_JOB", "app_picker"),
        ("GITHUB_WORKFLOW", "CI"),
        (
            "GITHUB_WORKFLOW_REF",
            "resoltico/EMLAttachmentRemover/.github/workflows/ci.yml@refs/pull/1/merge",
        ),
        ("GITHUB_RUN_ID", "12"),
        ("GITHUB_RUN_ATTEMPT", "1"),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v.into()))
    .chain([("GITHUB_WORKFLOW_SHA".into(), "a".repeat(40))])
    .collect();
    assert!(evidence::validate_authority(&base).is_ok());
    for (key, value) in [
        ("GITHUB_ACTIONS", "false"),
        ("RUNNER_ENVIRONMENT", "self-hosted"),
        ("GITHUB_JOB", "app_picker_followup"),
        ("GITHUB_WORKFLOW", "other"),
        ("GITHUB_REPOSITORY", "other/repo"),
        (
            "GITHUB_WORKFLOW_REF",
            "resoltico/EMLAttachmentRemover/.github/workflows/other.yml@main",
        ),
        ("GITHUB_RUN_ATTEMPT", "0"),
    ] {
        let mut wrong = base.clone();
        wrong.insert(key.into(), value.into());
        assert!(evidence::validate_authority(&wrong).is_err(), "{key}");
    }
}
