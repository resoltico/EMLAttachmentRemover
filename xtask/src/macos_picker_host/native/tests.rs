//! Counter-only executor probes at the real guarded service-spawn boundary.
use super::*;
use crate::{
    host_incidents::{ActionDeadline, IncidentHost, IncidentRecord},
    picker_evidence::ActionGuardObservation,
};
use std::{
    cell::Cell,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

fn policy() -> IncidentRecord {
    IncidentRecord {
        id: "intel-iconservices-panel-2026-10".into(),
        host: IncidentHost {
            image_os: "macos26".into(),
            image_version: "20260824.0517.1".into(),
            product_version: "26.6.1".into(),
            build_version: "25G76".into(),
        },
        architecture: "x86_64".into(),
        classifier: "iconservices-metal-renderbox-panel".into(),
        service: SERVICE.into(),
        upstream_issue: "https://github.com/actions/runner-images/issues/14751".into(),
        reviewed_at: 100,
        expires_at: 1000,
    }
}
fn guard(
    deadline: &ActionDeadline,
    wall: SystemTime,
    monotonic: Instant,
) -> Result<ActionGuardObservation> {
    deadline.check(wall, monotonic)?;
    Ok(ActionGuardObservation {
        wall_at: clock_seconds(wall)?,
        monotonic_elapsed_ns: deadline.elapsed_ns(monotonic)?,
    })
}
fn observation(operation: &str, guard: ActionGuardObservation) -> CommandObservation {
    CommandObservation {
        program: LAUNCHCTL.into(),
        arguments: vec![operation.into(), format!("gui/501/{SERVICE}")],
        guard,
        process: ProcessObservation {
            exit_code: Some(0),
            signal: None,
            timed_out: false,
            cancelled: false,
            unexpected_survivors: 0,
            cleanup_complete: true,
            elapsed_seconds: 0.001,
            cleanup_seconds: 0.0,
        },
        stdout_path: format!("owned/{operation}.stdout"),
        stderr_path: format!("owned/{operation}.stderr"),
    }
}

#[test]
fn expiry_between_service_actions_retains_bootout_and_never_executes_disable() {
    for wall_expired in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut attempt = Attempt::new(&root.path().join("incident")).unwrap();
        let start = Instant::now();
        let admitted = UNIX_EPOCH + Duration::from_millis(950_900);
        let deadline = ActionDeadline::admit(&policy(), admitted, start).unwrap();
        let bootout_calls = Cell::new(0);
        let disable_calls = Cell::new(0);
        let result = (|| -> Result<()> {
            attempt.phase("bootout")?;
            let bootout = observations::guarded_service_action(
                || {
                    guard(
                        &deadline,
                        UNIX_EPOCH + Duration::from_secs(951),
                        start + Duration::from_millis(100),
                    )
                },
                |proof| {
                    bootout_calls.set(bootout_calls.get() + 1);
                    Ok(observation("bootout", proof))
                },
            )?;
            attempt.record("bootout", &bootout)?;
            attempt.phase("disable")?;
            let (wall, monotonic) = if wall_expired {
                (
                    UNIX_EPOCH + Duration::from_secs(1000),
                    start + Duration::from_secs(1),
                )
            } else {
                // Wall rollback remains inside the review interval; the exact
                // original49.100s lifetime must still end monotonically.
                (
                    UNIX_EPOCH + Duration::from_secs(950),
                    start + Duration::from_millis(49_100),
                )
            };
            let disable = observations::guarded_service_action(
                || guard(&deadline, wall, monotonic),
                |proof| {
                    disable_calls.set(disable_calls.get() + 1);
                    Ok(observation("disable", proof))
                },
            )?;
            attempt.record("disable", &disable)?;
            Ok(())
        })();
        assert!(attempt.finish(result).is_err());
        assert_eq!(bootout_calls.get(), 1);
        assert_eq!(
            disable_calls.get(),
            0,
            "An expired guard must prevent the executor at the actual spawn edge"
        );
        let retained: Value =
            serde_json::from_slice(&fs::read(attempt.out.join("attempt.json")).unwrap()).unwrap();
        assert_eq!(retained["phase"], "disable");
        assert_eq!(
            retained["observations"]["bootout"]["arguments"],
            json!(["bootout", format!("gui/501/{SERVICE}")])
        );
        assert_eq!(
            retained["observations"]["bootout"]["process"]["exit_code"],
            0
        );
        assert!(retained["observations"].get("disable").is_none());
        assert!(retained["failure"].as_str().unwrap().contains("expired"));
        assert!(!attempt.out.join("preparation.json").exists());
        assert!(!attempt.out.join("result.json").exists());
    }
}
#[test]
fn relative_workflow_output_produces_an_absolute_probe_identity() {
    let root = Path::new("/owned/repository");
    let output = qualified_output_path(root, Path::new("target/picker-qualification")).unwrap();
    assert_eq!(output, root.join("target/picker-qualification"));
    assert!(output.join("probe/NativePanelDiagnostic.app").is_absolute());
    let absolute = Path::new("/owned/other/qualification");
    assert_eq!(qualified_output_path(root, absolute).unwrap(), absolute);
    assert!(qualified_output_path(Path::new("relative-root"), absolute).is_err());
}
