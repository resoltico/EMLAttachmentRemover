//! Pure admission of actual source-bound desktop case observations. No native mutation authority.
use crate::Result;
use crate::picker_evidence::{
    LifetimeOwner, NativePreference, OwnershipReceipt, PersistentPreferences, PreferenceComparison,
    ProcessObservation, StringMap, TestSummary,
};
use serde_json::Value;
use xtask::archive::ByteIdentity;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DesktopCase {
    Workflow,
    Picker,
    Pagination,
    Appearance,
    Stop,
    ForceStop,
    ActiveQuit,
    PublishedError,
}
impl DesktopCase {
    pub(crate) fn uses_native_pickers(self) -> bool {
        matches!(self, Self::Workflow | Self::Picker)
    }
    pub(crate) fn is_publication(self) -> bool {
        matches!(
            self,
            Self::Stop | Self::ForceStop | Self::ActiveQuit | Self::PublishedError
        )
    }
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Workflow => "workflow",
            Self::Picker => "picker",
            Self::Pagination => "pagination",
            Self::Appearance => "appearance",
            Self::Stop => "stop",
            Self::ForceStop => "force-stop",
            Self::ActiveQuit => "active-quit",
            Self::PublishedError => "published-error",
        }
    }
    pub(crate) fn method(self) -> &'static str {
        match self {
            Self::Workflow => "testOwnedDesktopWorkflowAndNativeIngress",
            Self::Picker => "testOwnedNativePickerCompatibility",
            Self::Pagination => "testOwnedPagination",
            Self::Appearance => "testOwnedNativeAppearance",
            Self::Stop => "testOwnedCoreStop",
            Self::ForceStop => "testOwnedCoreForceStop",
            Self::ActiveQuit => "testOwnedCoreActiveQuit",
            Self::PublishedError => "testOwnedCorePublishedError",
        }
    }
}

impl DesktopCase {
    pub(crate) fn originals(self) -> u32 {
        if self == Self::Pagination { 69 } else { 4 }
    }
}
fn field<'a>(value: &'a Value, name: &str) -> Result<&'a Value> {
    value
        .get(name)
        .ok_or_else(|| format!("Missing actual desktop observation {name}").into())
}
fn record<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T> {
    if !value.is_object() {
        return Err("Desktop observation must be an object".into());
    }
    Ok(serde_json::from_value(value.clone())?)
}
fn summary(value: &Value) -> Result<TestSummary> {
    Ok(serde_json::from_value(
        serde_json::json!({"total":field(value,"totalTestCount")?,"passed":field(value,"passedTests")?,"failed":field(value,"failedTests")?,"skipped":field(value,"skippedTests")?,"expected_failures":field(value,"expectedFailures")?}),
    )?)
}
/// Project only required facts from the retained complete native record, then qualify them.
pub(crate) fn validate_case(
    value: &Value,
    case: DesktopCase,
    source: &str,
    archive: &ByteIdentity,
    app_root: &str,
) -> Result<()> {
    if field(value, "case")? != case.name()
        || field(value, "test_method")? != case.method()
        || field(value, "source")? != source
        || record::<ByteIdentity>(field(value, "source_archive")?)? != *archive
    {
        return Err("Desktop case/method/source/archive binding differs".into());
    }
    let process: ProcessObservation = record(field(value, "process")?)?;
    let summary = summary(field(value, "summary")?)?;
    if process.clean()? != 0
        || summary.total != 1
        || summary.passed != 1
        || summary.failed != 0
        || summary.skipped != 0
        || summary.expected_failures != 0
    {
        return Err("Desktop case discovery/execution/process settlement failed".into());
    }
    let app = field(value, "app")?
        .as_str()
        .ok_or("Desktop app is not a path string")?;
    let owner: LifetimeOwner =
        crate::picker_evidence::project_lifetime_owner(field(value, "lifetime_owner")?)?;
    if owner.failure.is_some()
        || owner.source_check_failure.is_some()
        || owner.timed_out
        || owner.cancelled
        || !owner.runner_reaped
        || owner.runner_status != Some(0)
        || !owner.owned_app_terminated
        || owner.forced_app_cleanup
        || owner.originals_captured != case.originals()
        || !owner.originals_unchanged
        || !owner.owned_pasteboard_released
        || !owner.unexpected_application_pids.is_empty()
    {
        return Err("Desktop case normal ownership/original settlement failed".into());
    }
    validate_ownership(&owner.ownership_receipt, app, case, app_root)?;
    validate_observations(value, case, &owner)?;
    let preferences = field(value, "persistent_preferences")?;
    if case.uses_native_pickers() {
        let observations: PersistentPreferences = record(preferences)?;
        crate::picker_evidence::validate_preferences(&observations)?;
    } else {
        let before: StringMap<NativePreference> = record(field(preferences, "before")?)?;
        let after: StringMap<NativePreference> = record(field(preferences, "after")?)?;
        let observed: PreferenceComparison = record(field(preferences, "comparison")?)?;
        if before != after
            || !observed.whole_domain_unchanged
            || !observed.application_and_other_preferences_unchanged
            || observed.real_pickers_observed
            || !observed.permitted_system_changes.is_empty()
        {
            return Err(
                "Non-picker desktop case changed native preference domains or comparison facts"
                    .into(),
            );
        }
    }
    Ok(())
}
fn validate_ownership(
    identity: &OwnershipReceipt,
    app: &str,
    case: DesktopCase,
    app_root: &str,
) -> Result<()> {
    let root = if case.is_publication() {
        "Observed Core.app"
    } else {
        app_root
    };
    if identity.pid == 0
        || !identity.launch_date.is_finite()
        || identity.launch_date <= 0.0
        || identity.bundle_path != app
        || !app.starts_with('/')
        || std::path::Path::new(app)
            .file_name()
            .and_then(|s| s.to_str())
            != Some(root)
        || identity.pasteboard.is_empty()
        || identity.pasteboard == "Apple CFPasteboard general"
    {
        return Err("Desktop case actual app ownership identity differs".into());
    }
    Ok(())
}

fn validate_observations(value: &Value, case: DesktopCase, owner: &LifetimeOwner) -> Result<()> {
    match case {
        DesktopCase::Workflow | DesktopCase::Picker => {
            let picker = field(value, "picker_observation")?;
            let receipt: OwnershipReceipt = record(field(value, "ownership_receipt")?)?;
            if receipt != owner.ownership_receipt
                || picker["schema"] != 1
                || picker["scenario"] != case.name()
                || picker["owned_pid"] != owner.ownership_receipt.pid
                || !["message", "folder"]
                    .iter()
                    .all(|key| picker[*key].as_str().is_some_and(|p| p.starts_with('/')))
            {
                return Err("Desktop native picker observation belongs to another operation or is incomplete".into());
            }
            if case == DesktopCase::Workflow {
                let missing = field(field(value, "lifetime_owner")?, "missing_copy_observation")?;
                if missing["held"] != true
                    || missing["restored_by_request"] != true
                    || missing["copy_left_held"] != false
                    || missing.get("restoration_failure") != Some(&Value::Null)
                {
                    return Err(
                        "Desktop advisory-navigation fixture restoration is incomplete".into(),
                    );
                }
            }
        }
        DesktopCase::Pagination => {
            let p = field(value, "observation")?;
            if p["owned_pid"] != owner.ownership_receipt.pid
                || p["items"] != 65
                || p["removed_parts_in_first_item"] != 65
                || p["page_transitions"]
                    != serde_json::json!([
                        "parts-next",
                        "parts-previous",
                        "items-next",
                        "items-previous"
                    ])
                || p["outputs_match_independent_goldens"] != true
            {
                return Err("Desktop actual pagination observations are incomplete".into());
            }
        }
        DesktopCase::Appearance => {
            let a = field(value, "appearance")?;
            let raw_owner = field(value, "lifetime_owner")?;
            let settings = field(field(raw_owner, "ownership_receipt")?, "settings")?;
            if a["schema"] != 1
                || a["restored"] != true
                || a["owned_pid"] != owner.ownership_receipt.pid
                || a["settings_pid"] != settings["pid"]
                || !settings["pid"].as_u64().is_some_and(|pid| pid > 0)
                || !settings["launch_date"]
                    .as_f64()
                    .is_some_and(|date| date.is_finite() && date > 0.0)
                || settings["bundle_path"] != "/System/Applications/System Settings.app"
                || a["conditions"] != serde_json::json!(["light", "dark", "increased-contrast"])
                || a["native_product_observations"]
                    != serde_json::json!([
                        "Interface native appearance: light; increased contrast: false",
                        "Interface native appearance: dark; increased contrast: false",
                        "Interface native appearance: dark; increased contrast: true"
                    ])
                || !matches!(
                    a["original_selector"].as_str(),
                    Some("Light" | "Dark" | "Auto")
                )
                || !native_display_settings_match(a)
                || raw_owner["owned_settings_terminated"] != true
                || raw_owner["unexpected_settings_application_pids"] != serde_json::json!([])
            {
                return Err(
                    "Desktop actual appearance restoration/Settings ownership is incomplete".into(),
                );
            }
        }
        DesktopCase::Stop
        | DesktopCase::ForceStop
        | DesktopCase::ActiveQuit
        | DesktopCase::PublishedError => {
            let actor = field(value, "actor")?;
            if actor["case"] != case.name()
                || !actor["worker_pid"].as_i64().is_some_and(|pid| pid > 0)
                || actor["worker_absent"] != true
                || actor["independent_copy_golden"] != true
            {
                return Err(
                    "Desktop publication actor/worker/golden observation is incomplete".into(),
                );
            }
            let first = field(actor, "first_interrupt_micros")?;
            let second = field(actor, "second_interrupt_micros")?;
            let valid = match case {
                DesktopCase::Stop => first.as_u64().is_some() && second.is_null(),
                DesktopCase::ForceStop => first
                    .as_u64()
                    .zip(second.as_u64())
                    .is_some_and(|(a, b)| b >= a),
                DesktopCase::ActiveQuit => first
                    .as_u64()
                    .zip(second.as_u64())
                    .and_then(|(a, b)| b.checked_sub(a))
                    .is_some_and(|d| (9_800_000..=12_500_000).contains(&d)),
                DesktopCase::PublishedError => first.is_null() && second.is_null(),
                _ => false,
            };
            if !valid {
                return Err(
                    "Desktop publication interrupt observations differ from the exact scenario"
                        .into(),
                );
            }
        }
    }
    Ok(())
}
pub(crate) fn native_display_settings_match(observation: &Value) -> bool {
    let states = &observation["native_settings_observations"];
    observation["original_contrast"].is_boolean()
        && observation["original_reduce_transparency"].is_boolean()
        && states["before"]["increase_contrast"] == observation["original_contrast"]
        && states["before"]["reduce_transparency"] == observation["original_reduce_transparency"]
        && states["increased_contrast"]["increase_contrast"] == true
        && states["increased_contrast"]["reduce_transparency"].is_boolean()
        && states["restored"] == states["before"]
}
