//! Event-specific CI admission. Missing results and unexpected skips fail closed.
use crate::Result;
use serde_json::Value;
use std::{collections::BTreeSet, fs, path::Path};

pub(crate) fn validate(event: &str, native: bool, needs: &Value) -> Result<()> {
    let results = needs
        .as_object()
        .ok_or("CI needs must be a result object")?;
    let expected = if native {
        if event != "workflow_call" {
            return Err("Native CI admission requires the reusable-workflow event policy".into());
        }
        vec![
            ("plan", "success"),
            ("lint", "success"),
            ("legal", "success"),
            ("test", "success"),
            ("app", "success"),
            ("app_smoke", "success"),
            ("app_picker", "success"),
        ]
    } else {
        let pr = match event {
            "pull_request" => true,
            "push" | "merge_group" | "workflow_dispatch" => false,
            _ => return Err("Unsupported CI event has no admission policy".into()),
        };
        vec![
            ("identity", "success"),
            ("native", "success"),
            ("integration", if pr { "success" } else { "skipped" }),
            ("freshness", if pr { "success" } else { "skipped" }),
        ]
    };
    if results.keys().map(String::as_str).collect::<BTreeSet<_>>()
        != expected.iter().map(|(name, _)| *name).collect()
    {
        return Err("CI results omit or add a job outside the event policy".into());
    }
    for (job, required) in expected {
        if results[job]["result"] != required {
            return Err(format!(
                "CI job {job} requires {required}; observed {}",
                results[job]["result"]
            )
            .into());
        }
    }
    Ok(())
}

pub(crate) fn run(event: &str, native: bool, path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return Err("CI results must be an ordinary bounded document".into());
    }
    let bytes = fs::read(path)?;
    if bytes.len() > 1024 * 1024 {
        return Err("CI results grew beyond their document bound".into());
    }
    validate(event, native, &serde_json::from_slice(&bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn required_failure_cancellation_skip_and_missing_result_cannot_admit_ci() {
        for (native, event, good) in [
            (
                false,
                "pull_request",
                json!({"identity":{"result":"success"},"native":{"result":"success"},"integration":{"result":"success"},"freshness":{"result":"success"}}),
            ),
            (
                true,
                "workflow_call",
                json!({"plan":{"result":"success"},"lint":{"result":"success"},"legal":{"result":"success"},"test":{"result":"success"},"app":{"result":"success"},"app_smoke":{"result":"success"},"app_picker":{"result":"success"}}),
            ),
        ] {
            validate(event, native, &good).unwrap();
            for job in good.as_object().unwrap().keys() {
                for result in [
                    "failure",
                    "cancelled",
                    "skipped",
                    "timed_out",
                    "",
                    "neutral",
                ] {
                    if good[job]["result"] == result {
                        continue;
                    }
                    let mut changed = good.clone();
                    changed[job]["result"] = result.into();
                    assert!(validate(event, native, &changed).is_err(), "{job} {result}");
                }
                let mut changed = good.clone();
                changed.as_object_mut().unwrap().remove(job);
                assert!(validate(event, native, &changed).is_err());
            }
        }
    }
    #[test]
    fn native_picker_requires_one_registered_actual_product_job() {
        let mut needs = json!({"plan":{"result":"success"},"lint":{"result":"success"},"legal":{"result":"success"},"test":{"result":"success"},"app":{"result":"success"},"app_smoke":{"result":"success"},"app_picker":{"result":"success"}});
        validate("workflow_call", true, &needs).unwrap();
        for obsolete in ["app_picker_normal", "app_picker_followup"] {
            needs[obsolete] = json!({"result":"success"});
            assert!(validate("workflow_call", true, &needs).is_err());
            needs.as_object_mut().unwrap().remove(obsolete);
        }
        validate("workflow_call", true, &needs).unwrap();
    }
    #[test]
    fn only_non_pr_events_allow_their_inapplicable_merge_roles_to_be_absent() {
        let needs = json!({"identity":{"result":"success"},"native":{"result":"success"},"integration":{"result":"skipped"},"freshness":{"result":"skipped"}});
        for event in ["push", "merge_group", "workflow_dispatch"] {
            validate(event, false, &needs).unwrap();
        }
        assert!(validate("pull_request", false, &needs).is_err());
        assert!(validate("unknown", false, &needs).is_err());
        assert!(validate("unknown", true, &needs).is_err());
        let mut changed = needs;
        changed["integration"]["result"] = "failure".into();
        assert!(validate("push", false, &changed).is_err());
    }
}
