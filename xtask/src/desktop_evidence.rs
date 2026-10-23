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
    BlockedProcessingPreflight,
    BlockedProcessingReadback,
    BlockedProcessingStageWrite,
    BlockedProcessingFileSync,
    BlockedProcessingPublication,
    BlockedProcessingNamespaceSync,
    BlockedExportPreflight,
    BlockedExportReadback,
    BlockedExportStageWrite,
    BlockedExportFileSync,
    BlockedExportPublication,
    BlockedExportNamespaceSync,
    ExportPublication,
    ExportWriterBlock,
}
impl DesktopCase {
    pub(crate) fn uses_native_pickers(self) -> bool {
        matches!(self, Self::Workflow | Self::Picker)
    }
    pub(crate) fn uses_panel_path_clipboard(self) -> bool {
        self.uses_native_pickers() || self.uses_export_picker()
    }
    #[cfg(target_os = "macos")]
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
            Self::BlockedProcessingPreflight => "blocked-processing-preflight",
            Self::BlockedProcessingReadback => "blocked-processing-readback",
            Self::BlockedProcessingStageWrite => "blocked-processing-stage-write",
            Self::BlockedProcessingFileSync => "blocked-processing-file-sync",
            Self::BlockedProcessingPublication => "blocked-processing-publication",
            Self::BlockedProcessingNamespaceSync => "blocked-processing-namespace-sync",
            Self::BlockedExportPreflight => "blocked-export-preflight",
            Self::BlockedExportReadback => "blocked-export-readback",
            Self::BlockedExportStageWrite => "blocked-export-stage-write",
            Self::BlockedExportFileSync => "blocked-export-file-sync",
            Self::BlockedExportPublication => "blocked-export-publication",
            Self::BlockedExportNamespaceSync => "blocked-export-namespace-sync",
            Self::ExportPublication => "export-publication",
            Self::ExportWriterBlock => "export-writer-block",
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
            Self::BlockedProcessingPreflight => "testOwnedBlockedProcessingPreflight",
            Self::BlockedProcessingReadback => "testOwnedBlockedProcessingReadback",
            Self::BlockedProcessingStageWrite => "testOwnedBlockedProcessingStageWrite",
            Self::BlockedProcessingFileSync => "testOwnedBlockedProcessingFileSync",
            Self::BlockedProcessingPublication => "testOwnedBlockedProcessingPublication",
            Self::BlockedProcessingNamespaceSync => "testOwnedBlockedProcessingNamespaceSync",
            Self::BlockedExportPreflight => "testOwnedBlockedExportPreflight",
            Self::BlockedExportReadback => "testOwnedBlockedExportReadback",
            Self::BlockedExportStageWrite => "testOwnedBlockedExportStageWrite",
            Self::BlockedExportFileSync => "testOwnedBlockedExportFileSync",
            Self::BlockedExportPublication => "testOwnedBlockedExportPublication",
            Self::BlockedExportNamespaceSync => "testOwnedBlockedExportNamespaceSync",
            Self::ExportPublication => "testOwnedExportPublication",
            Self::ExportWriterBlock => "testOwnedExportWriterBlock",
        }
    }
}

impl DesktopCase {
    pub(crate) fn controlled_cases() -> &'static [Self] {
        &[
            Self::Stop,
            Self::ForceStop,
            Self::ActiveQuit,
            Self::PublishedError,
            Self::BlockedProcessingPreflight,
            Self::BlockedProcessingReadback,
            Self::BlockedProcessingStageWrite,
            Self::BlockedProcessingFileSync,
            Self::BlockedProcessingPublication,
            Self::BlockedProcessingNamespaceSync,
            Self::BlockedExportPreflight,
            Self::BlockedExportReadback,
            Self::BlockedExportStageWrite,
            Self::BlockedExportFileSync,
            Self::BlockedExportPublication,
            Self::BlockedExportNamespaceSync,
            Self::ExportPublication,
            Self::ExportWriterBlock,
        ]
    }
    pub(crate) fn is_controlled(self) -> bool {
        Self::controlled_cases().contains(&self)
    }
    pub(crate) fn uses_export_picker(self) -> bool {
        self.blocked()
            .is_some_and(|(operation, _)| operation == "export")
            || matches!(self, Self::ExportPublication | Self::ExportWriterBlock)
    }
    pub(crate) fn blocked(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::BlockedProcessingPreflight => Some(("processing", "preflight")),
            Self::BlockedProcessingReadback => Some(("processing", "readback")),
            Self::BlockedProcessingStageWrite => Some(("processing", "stage-write")),
            Self::BlockedProcessingFileSync => Some(("processing", "file-sync")),
            Self::BlockedProcessingPublication => Some(("processing", "publication")),
            Self::BlockedProcessingNamespaceSync => Some(("processing", "namespace-sync")),
            Self::BlockedExportPreflight => Some(("export", "preflight")),
            Self::BlockedExportReadback => Some(("export", "readback")),
            Self::BlockedExportStageWrite => Some(("export", "stage-write")),
            Self::BlockedExportFileSync => Some(("export", "file-sync")),
            Self::BlockedExportPublication => Some(("export", "publication")),
            Self::BlockedExportNamespaceSync => Some(("export", "namespace-sync")),
            _ => None,
        }
    }
    pub(crate) fn originals(self) -> u32 {
        match self {
            Self::Pagination => 69,
            Self::ExportWriterBlock => 1028,
            _ => 4,
        }
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
    owner.fixture_filesystem.check()?;
    owner
        .panel_path_clipboard
        .check(case.uses_panel_path_clipboard(), "requested")?;
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
        let expected = crate::desktop_preferences::compare(
            &serde_json::to_value(&before)?,
            &serde_json::to_value(&after)?,
            case.uses_export_picker(),
        )?;
        if serde_json::to_value(&observed)? != expected {
            return Err(
                "Non-picker desktop case changed native preference domains or comparison facts"
                    .into(),
            );
        }
    }
    Ok(())
}
fn validate_child_actor(actor: &Value, case: DesktopCase) -> Result<()> {
    let operation = case.blocked().map_or("export", |(operation, _)| operation);
    let phase = case.blocked().map_or(
        if case == DesktopCase::ExportWriterBlock {
            "request-body"
        } else {
            "after-publication"
        },
        |(_, phase)| phase,
    );
    let id = field(actor, "operation_id")?
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or("Missing exact operation identity")?;
    let receipt = field(actor, "ui_receipt")?;
    let contents = field(receipt, "contents")?
        .as_str()
        .ok_or("Missing raw native UI observations")?;
    let identity: ByteIdentity = record(field(receipt, "identity")?)?;
    if contents.len() > 1024 * 1024 || ByteIdentity::of(contents.as_bytes()) != identity {
        return Err("Raw native UI observations changed identity".into());
    }
    let ui = crate::picker_evidence::observation_document(contents.as_bytes())?;
    if actor["case"] != case.name()
        || actor["operation"] != operation
        || actor["phase"] != phase
        || actor["worker_absent"] != true
        || actor["worker_pid"].as_i64().is_none_or(|pid| pid <= 0)
        || ui["case"] != case.name()
        || ui["operation"] != operation
        || ui["phase"] != phase
        || ui["operation_id"] != id
        || ui["run_id"] != if operation == "processing" { 2 } else { 1 }
        || ui["worker_pid"] != actor["worker_pid"]
        || actor["originals"]
            .as_array()
            .is_none_or(|rows| rows.len() != 4)
    {
        return Err("Controlled native UI lane/phase/run/owner facts are incomplete".into());
    }
    let mut names = std::collections::BTreeSet::new();
    for original in actor["originals"]
        .as_array()
        .ok_or("Missing original identities")?
    {
        let name = field(original, "name")?
            .as_str()
            .ok_or("Missing original name")?;
        let identity: ByteIdentity = record(field(original, "identity")?)?;
        crate::picker_evidence::byte_identity(&identity)?;
        if !names.insert(name) {
            return Err("Original source identity is duplicated".into());
        }
    }
    if names
        != [
            "one.eml",
            "two.eml",
            "folder-input/folder.eml",
            "service-input/日本語-مرحبا-\u{202e}report.eml",
        ]
        .into_iter()
        .collect()
    {
        return Err("Original source identity set differs".into());
    }
    let target = field(actor, "target")?;
    if !target["path"]
        .as_str()
        .is_some_and(|path| path.starts_with('/'))
    {
        return Err("Controlled target is not an owned absolute address".into());
    }
    if !field(target, "identity")?.is_null() {
        crate::picker_evidence::byte_identity(&record::<ByteIdentity>(field(
            target, "identity",
        )?)?)?;
    }
    if case == DesktopCase::ExportWriterBlock {
        crate::picker_evidence::byte_identity(&record::<ByteIdentity>(field(
            actor,
            "writer_document",
        )?)?)?;
    }
    if case == DesktopCase::ExportPublication {
        for key in ["expected_document", "winner"] {
            crate::picker_evidence::byte_identity(&record::<ByteIdentity>(field(
                field(actor, "matrix")?,
                key,
            )?)?)?;
        }
    }
    if let Some((operation, phase)) = case.blocked() {
        if matches!(phase, "readback" | "namespace-sync") {
            let identity: ByteIdentity = record(field(target, "identity")?)?;
            if operation == "processing"
                && identity
                    != ByteIdentity::of(include_bytes!(
                        "../../tests/contract/mime/golden/01-gmail-attachment.eml"
                    ))
            {
                return Err("Controlled copy differs from independent golden".into());
            }
        } else if !field(target, "identity")?.is_null() {
            return Err("Pre-edge control unexpectedly published a target".into());
        }
    }
    if case == DesktopCase::ExportWriterBlock && !field(target, "identity")?.is_null() {
        return Err("Body-blocked report export performed target I/O".into());
    }
    let first = field(actor, "first_interrupt_micros")?
        .as_u64()
        .ok_or("Missing actual first interrupt")?;
    if case != DesktopCase::ExportWriterBlock
        && field(actor, "second_interrupt_micros")?
            .as_u64()
            .is_none_or(|second| second < first)
    {
        return Err("Missing actual second interrupt".into());
    }
    if let Some((operation, _)) = case.blocked() {
        if ui["ui_responsive"] != true
            || ui["other_report_viewed"] != true
            || ui["first_stop_observed"] != true
            || ui["force_stop_observed"] != true
            || ui["source_reserved"] != (operation == "export")
            || ui["queued_then_processing"] != (operation == "export")
            || ui["ui_response_seconds"]
                .as_f64()
                .is_none_or(|seconds| !seconds.is_finite() || !(0.0..120.0).contains(&seconds))
        {
            return Err(
                "Blocked native UI response, reservation or actual settlement facts are missing"
                    .into(),
            );
        }
        if case != DesktopCase::BlockedExportPreflight {
            let text = ui["settled_text"]
                .as_str()
                .ok_or("Missing settled child observations")?;
            let expected = if operation == "export" {
                format!("Report export operation {id} for run 1")
            } else {
                format!("Processing operation: {id}")
            };
            if !text.contains(&expected)
                || !text.contains(if operation == "export" {
                    "settled: true"
                } else {
                    "settled true"
                })
            {
                return Err("Settled UI observations belong to another operation".into());
            }
        }
        if case == DesktopCase::BlockedExportPreflight {
            let other = field(actor, "additional_worker")?;
            let first_other = field(other, "first_interrupt_micros")?
                .as_u64()
                .ok_or("Missing second lane first interrupt")?;
            let second_other = field(other, "second_interrupt_micros")?
                .as_u64()
                .ok_or("Missing second lane second interrupt")?;
            let second = field(actor, "second_interrupt_micros")?
                .as_u64()
                .ok_or("Missing export force interrupt")?;
            if ui["dual_quit"] != true
                || ui["native_panel_seen"] != true
                || ui["native_panel_cancelled"] != true
                || ui["pending_requests"] != 8
                || ui["cosmetic_notice"]
                    != "8 requests are already waiting. Remove a waiting run before adding another."
                || ui["quit_cancel_preserved_children"] != true
                || ui["app_quit_completed"] != true
                || other["worker_absent"] != true
                || !field(field(other, "target")?, "identity")?.is_null()
                || other["operation_id"] != ui["processing_operation_id"]
                || other["operation_id"] == id
                || ui["processing_run_id"] != 3
                || other["run_id"] != ui["processing_run_id"]
                || other["worker_pid"].as_i64().is_none_or(|pid| pid <= 0)
                || other["worker_pid"] == actor["worker_pid"]
                || other["worker_pid"] != ui["processing_worker_pid"]
                || second
                    .checked_sub(first)
                    .is_none_or(|elapsed| !(9_800_000..=12_500_000).contains(&elapsed))
                || second_other
                    .checked_sub(first_other)
                    .is_none_or(|elapsed| !(9_800_000..=12_500_000).contains(&elapsed))
            {
                return Err("Actual two-lane Quit, pending panel/batch or immutable shutdown deadline evidence is incomplete".into());
            }
        }
    } else if case == DesktopCase::ExportWriterBlock {
        if ui["writer_busy_observed"] != true
            || ui["ui_responsive"] != true
            || ui["other_report_viewed"] != true
            || ui["source_reserved"] != true
            || ui["first_stop_observed"] != true
            || ui["document_bytes"]
                .as_u64()
                .is_none_or(|bytes| bytes <= 1024 * 1024 || bytes > 256 * 1024 * 1024)
            || !actor["second_interrupt_micros"].is_null()
            || actor["writer_document"]["byte_length"] != ui["document_bytes"]
            || actor["writer_document"]["sha256"] != ui["document_sha256"]
        {
            return Err("Actual blocked report writer/pipe facts are incomplete".into());
        }
    } else if ui["cases"]
        != serde_json::json!([
            "ordinary",
            "occupied",
            "directory",
            "symlink",
            "racing-winner",
            "pre-edge-stop",
            "post-edge-stop",
            "post-edge-loss"
        ])
        || actor["matrix"]["expected_document"]["byte_length"] != ui["expected_bytes"]
        || actor["matrix"]["expected_document"]["sha256"] != ui["expected_sha256"]
        || actor["target"]["identity"] != actor["matrix"]["expected_document"]
        || actor["matrix"]["winner"]["sha256"] != ui["winner_sha256"]
        || actor["matrix"]["directory_unchanged"] != true
        || actor["matrix"]["symlink_unchanged"] != true
        || actor["matrix"]["pre_edge_absent"] != true
        || ui["no_replace_observed"] != true
        || ui["post_edge_bytes_preserved"] != true
        || ui["expected_bytes"]
            .as_u64()
            .is_none_or(|bytes| bytes == 0 || bytes > 256 * 1024 * 1024)
    {
        return Err("Native report no-replace/interruption matrix is incomplete".into());
    }
    if case == DesktopCase::ExportPublication {
        let controls = actor["matrix"]["controls"]
            .as_array()
            .ok_or("Missing native matrix child controls")?;
        let mut seen = std::collections::BTreeSet::new();
        let mut operations = std::collections::BTreeSet::from([id]);
        if controls.len() != 3 {
            return Err("Native matrix child controls are incomplete".into());
        }
        for control in controls {
            let name = control["case"]
                .as_str()
                .ok_or("Missing matrix control case")?;
            let (phase, stopped) = match name {
                "racing-winner" => ("publication", false),
                "pre-edge-stop" => ("publication", true),
                "post-edge-stop" => ("namespace-sync", true),
                _ => return Err("Unexpected matrix control".into()),
            };
            let operation = control["operation_id"]
                .as_u64()
                .filter(|id| *id > 0)
                .ok_or("Missing matrix operation identity")?;
            if !seen.insert(name)
                || !operations.insert(operation)
                || control["phase"] != phase
                || control["worker_pid"].as_i64().is_none_or(|pid| pid <= 0)
                || control["worker_absent"] != true
                || control["first_interrupt_micros"].as_u64().is_some() != stopped
                || !control["second_interrupt_micros"].is_null()
            {
                return Err(
                    "Native matrix operation or actual interrupt/settlement differs".into(),
                );
            }
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
    let root = if case.is_controlled() {
        "Observed Operations.app"
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
        DesktopCase::BlockedProcessingPreflight
        | DesktopCase::BlockedProcessingReadback
        | DesktopCase::BlockedProcessingStageWrite
        | DesktopCase::BlockedProcessingFileSync
        | DesktopCase::BlockedProcessingPublication
        | DesktopCase::BlockedProcessingNamespaceSync
        | DesktopCase::BlockedExportPreflight
        | DesktopCase::BlockedExportReadback
        | DesktopCase::BlockedExportStageWrite
        | DesktopCase::BlockedExportFileSync
        | DesktopCase::BlockedExportPublication
        | DesktopCase::BlockedExportNamespaceSync
        | DesktopCase::ExportPublication
        | DesktopCase::ExportWriterBlock => {
            validate_child_actor(field(value, "actor")?, case)?;
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

#[cfg(test)]
mod child_control_tests {
    use super::*;
    use serde_json::json;
    fn actor() -> Value {
        // Independent literal data receipt: this tests consumer refusal, never
        // supplies evidence of actual GUI or child execution.
        let contents = r#"{"case":"blocked-processing-preflight","operation":"processing","phase":"preflight","operation_id":9,"worker_pid":27,"run_id":2,"ui_response_seconds":0.25,"ui_responsive":true,"other_report_viewed":true,"source_reserved":false,"queued_then_processing":false,"first_stop_observed":true,"force_stop_observed":true,"settled_text":"Processing operation: 9; Worker exit None; settled true; reusable true"}"#;
        let original = ByteIdentity::of(b"independently observed original");
        json!({"case":"blocked-processing-preflight","operation":"processing","phase":"preflight","operation_id":9,"worker_pid":27,"worker_absent":true,"first_interrupt_micros":10,"second_interrupt_micros":20,"additional_worker":null,"matrix":null,"writer_document":null,"ui_receipt":{"identity":ByteIdentity::of(contents.as_bytes()),"contents":contents},"originals":[{"name":"one.eml","identity":original},{"name":"two.eml","identity":original},{"name":"folder-input/folder.eml","identity":original},{"name":"service-input/日本語-مرحبا-\u{202e}report.eml","identity":original}],"target":{"path":"/owned/copy.eml","identity":null}})
    }
    #[test]
    fn child_control_receipts_reject_coherent_wrong_lane_identity_and_missing_facts() {
        let good = actor();
        validate_child_actor(&good, DesktopCase::BlockedProcessingPreflight).unwrap();
        for (key, value) in [
            ("phase", json!("publication")),
            ("operation", json!("export")),
            ("operation_id", json!(10)),
            ("worker_pid", json!(28)),
            ("run_id", json!(1)),
            ("ui_responsive", json!(false)),
            ("settled_text", json!("Stop requested")),
        ] {
            let mut changed = good.clone();
            let mut ui = crate::picker_evidence::observation_document(
                changed["ui_receipt"]["contents"]
                    .as_str()
                    .unwrap()
                    .as_bytes(),
            )
            .unwrap();
            ui[key] = value;
            let contents = serde_json::to_string(&ui).unwrap();
            changed["ui_receipt"] =
                json!({"identity":ByteIdentity::of(contents.as_bytes()),"contents":contents});
            assert!(
                validate_child_actor(&changed, DesktopCase::BlockedProcessingPreflight).is_err(),
                "{key}"
            );
        }
        let mut missing = good.clone();
        missing["originals"][1] = missing["originals"][0].clone();
        assert!(validate_child_actor(&missing, DesktopCase::BlockedProcessingPreflight).is_err());
        let mut stale = good;
        stale["ui_receipt"]["contents"] = json!("{}");
        assert!(validate_child_actor(&stale, DesktopCase::BlockedProcessingPreflight).is_err());
    }
    #[test]
    fn child_ui_receipt_duplicate_fields_are_rejected_even_with_matching_hash() {
        let mut changed = actor();
        let contents =
            r#"{"case":"blocked-processing-preflight","case":"blocked-processing-preflight"}"#;
        changed["ui_receipt"] =
            json!({"identity":ByteIdentity::of(contents.as_bytes()),"contents":contents});
        assert!(validate_child_actor(&changed, DesktopCase::BlockedProcessingPreflight).is_err());
    }
}
