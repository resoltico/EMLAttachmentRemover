//! Map accepted protocol facts into structured Slint values using core vocabulary.
use crate::{CountView, DetailView, ItemView, PathView, RemovalView};
use base64::{Engine, engine::general_purpose::STANDARD};
use eml_core::{
    AddressState, CleanupReceipt, CleanupState, NamespaceObservation, PlanAction, SyncMethod,
    SyncReceipt, SyncState,
    vocabulary::{
        self, CountKind, ExistingFacts, MeaningKind, OutcomeFacts, Publication, ResultKind,
    },
};
use eml_protocol::records as wire;
use std::{ffi::OsString, os::unix::ffi::OsStringExt};
fn address(value: wire::Address) -> AddressState {
    match value {
        wire::Address::NotChecked => AddressState::NotChecked,
        wire::Address::Verified => AddressState::Verified,
        wire::Address::Changed => AddressState::Changed,
        wire::Address::Unknown => AddressState::Unknown,
    }
}
fn receipt(
    method: wire::SynchronizationMethod,
    state: wire::Synchronization,
    code: Option<i64>,
) -> Option<SyncReceipt> {
    let method = match method {
        wire::SynchronizationMethod::NotRequested => return None,
        wire::SynchronizationMethod::Fsync => SyncMethod::Fsync,
        wire::SynchronizationMethod::FsyncFullfsync => SyncMethod::FsyncFullfsync,
        wire::SynchronizationMethod::FsyncFullfsyncUnsupported => {
            SyncMethod::FsyncFullfsyncUnsupported
        }
        wire::SynchronizationMethod::FlushFileBuffers => SyncMethod::FlushFileBuffers,
    };
    let state = match state {
        wire::Synchronization::Succeeded => SyncState::Succeeded,
        wire::Synchronization::Unsupported => SyncState::Unsupported,
        wire::Synchronization::Failed => SyncState::Failed,
        wire::Synchronization::NotAttempted => return None,
    };
    Some(SyncReceipt {
        method,
        state,
        native_code: code,
    })
}
fn sync(value: &wire::SyncRecord) -> Option<SyncReceipt> {
    receipt(
        value.method,
        value.state,
        value.native_code.value().copied(),
    )
}
fn namespace(value: &wire::NamespaceRecord) -> Option<NamespaceObservation> {
    Some(NamespaceObservation {
        receipt: receipt(
            value.method,
            value.state,
            value.native_code.value().copied(),
        ),
        required_edges: value.required_edges.try_into().ok()?,
        completed_edges: value.completed_edges.try_into().ok()?,
        failed_edge_index: value
            .failed_edge_index
            .value()
            .map(|value| (*value).try_into())
            .transpose()
            .ok()?,
    })
}
fn cleanup(value: &wire::CleanupRecord) -> Option<CleanupReceipt> {
    let residual_name = if let Some(native) = value.residual_name.value() {
        if native.encoding != wire::PathEncoding::UnixBytes {
            return None;
        }
        Some(OsString::from_vec(STANDARD.decode(&native.base64).ok()?))
    } else {
        None
    };
    Some(CleanupReceipt {
        state: match value.state {
            wire::Cleanup::NotNeeded => CleanupState::NotNeeded,
            wire::Cleanup::Complete => CleanupState::Complete,
            wire::Cleanup::Residual => CleanupState::Residual,
            wire::Cleanup::Unknown => CleanupState::Unknown,
        },
        residual_name,
        native_code: value.native_code.value().copied(),
    })
}
fn facts(item: &wire::ItemRecord) -> Option<OutcomeFacts> {
    let existing = match item.existing_observation.value() {
        Some(value) => Some(ExistingFacts {
            identity_known: value.identity.value().is_some(),
            bytes_equal: value.bytes_equal.value().copied(),
            stable: value.stable,
            address: address(value.address),
            file_sync: sync(&value.file_sync),
            namespace_sync: namespace(&value.namespace_sync)?,
        }),
        None => None,
    };
    Some(OutcomeFacts {
        result: match item.status {
            wire::ItemStatus::Created => ResultKind::Created,
            wire::ItemStatus::UpToDate => ResultKind::Existing,
            wire::ItemStatus::Planned => ResultKind::Planned,
            wire::ItemStatus::Failed => ResultKind::Failed,
        },
        plan: item.plan.value().map(|value| match value {
            wire::Plan::Create => PlanAction::Create,
            wire::Plan::VerifyExisting => PlanAction::VerifyExisting,
        }),
        publication: match item.publication.state {
            wire::Publication::NotPublished => Publication::NotPublished,
            wire::Publication::Published => Publication::Published,
            wire::Publication::Unknown => Publication::Unknown,
        },
        address: address(item.publication.address),
        candidate_identity_known: item.publication.candidate_identity.value().is_some(),
        final_readback: (item.status == wire::ItemStatus::Created).then_some(true),
        file_sync: sync(&item.publication.file_sync),
        namespace_sync: namespace(&item.publication.namespace_sync)?,
        cleanup: cleanup(&item.publication.cleanup)?,
        existing,
        error: item
            .error
            .value()
            .and_then(|error| eml_core::ErrorCode::from_wire_name(&error.code)),
        related_resources: item
            .removed
            .iter()
            .any(|part| part.reason == wire::Removal::RelatedResource),
    })
}
fn meaning(item: &wire::ItemRecord) -> Option<vocabulary::OutcomeMeaning> {
    facts(item).map(|facts| vocabulary::classify(&facts))
}
pub(super) fn item_words(item: &wire::ItemRecord) -> crate::wording::ItemWords {
    crate::wording::outcome(
        meaning(item).map_or(MeaningKind::Unestablished, |meaning| meaning.kind),
        item.error
            .value()
            .and_then(|error| eml_core::ErrorCode::from_wire_name(&error.code)),
        item.removed.len(),
    )
}
/// Concise desktop labels preserve the core qualification and typed failure facts.
pub(crate) fn item(item: &wire::ItemRecord) -> ItemView {
    ItemView {
        folder: "—".into(),
        tone: match category(item) {
            crate::state::Category::Created | crate::state::Category::UpToDate => crate::Tone::Ok,
            crate::state::Category::Checked => crate::Tone::Neutral,
            crate::state::Category::Attention => crate::Tone::Attention,
            crate::state::Category::NotProcessed => crate::Tone::Stopped,
        },
        original_index: 0,
        path: path(&item.source.display),
        status: item_words(item).verdict.into(),
        attention: category(item) == crate::state::Category::Attention,
    }
}
pub(super) fn category(item: &wire::ItemRecord) -> crate::state::Category {
    crate::state::category(
        meaning(item).map(|value| (value.kind, value.severity)),
        facts(item)
            .as_ref()
            .is_some_and(vocabulary::explicitly_interrupted),
    )
}
pub(super) fn policy_notice(item: &wire::ItemRecord) -> &'static str {
    meaning(item)
        .and_then(|meaning| meaning.related_notice)
        .unwrap_or("")
}
pub(super) fn guidance(item: &wire::ItemRecord) -> String {
    let ordinary = item_words(item).guidance;
    let Some(facts) = facts(item).filter(vocabulary::explicitly_interrupted) else {
        return ordinary.into();
    };
    let consequences = vocabulary::classify(&facts).consequences;
    let warnings: Vec<_> = consequences
        .into_iter()
        .filter(|c| *c != vocabulary::Consequence::NoPublication)
        .map(vocabulary::Consequence::text)
        .collect();
    if warnings.is_empty() {
        ordinary.into()
    } else {
        format!("{} {}", warnings.join(" "), ordinary)
    }
}
pub(super) fn report_facts(
    report: &eml_app::RunReport,
) -> (
    Vec<crate::state::Category>,
    crate::state::Tally,
    Option<u32>,
) {
    let total = report.start().map(|start| start.total);
    let categories: Vec<_> = report.items().iter().map(category).collect();
    let mut tally = crate::state::Tally {
        all: total.unwrap_or(categories.len() as u32),
        ..Default::default()
    };
    for category in &categories {
        tally.observe(*category);
    }
    // Only an accepted summary establishes that missing messages were not processed.
    // Missing results after a forced stop or transport failure remain unknown.
    tally.not_processed += report
        .summary()
        .map_or(0, |summary| summary.counts.not_processed);
    (categories, tally, total)
}
pub(crate) fn count(kind: CountKind, value: u32) -> CountView {
    CountView {
        label: kind.label().into(),
        value: value as i32,
        accessible: vocabulary::count_label(kind, value).into(),
        attention: matches!(kind, CountKind::Failed) && value > 0,
    }
}
pub(crate) fn byte_counts(item: &wire::ItemRecord) -> Vec<CountView> {
    [
        (CountKind::SourceBytes, item.source_bytes.value()),
        (CountKind::CopyBytes, item.copy_bytes.value()),
    ]
    .into_iter()
    .filter_map(|(kind, value)| {
        value
            .and_then(|value| u32::try_from(*value).ok())
            .map(|value| count(kind, value))
    })
    .collect()
}
/// Separate consequence, error and metadata elements, never newline-joined prose.
pub(crate) fn details(item: &wire::ItemRecord) -> (Vec<DetailView>, String, String) {
    let mut rows = Vec::new();
    if let Some(error) = item.error.value() {
        rows.push(DetailView {
            text: format!("Code: {}", error.code).into(),
        });
        rows.push(DetailView {
            text: format!("Phase: {:?}", error.phase).into(),
        });
        for line in error.detail.lines() {
            rows.push(DetailView { text: line.into() });
        }
        if let Some(hint) = vocabulary::recovery_hint(&error.code) {
            rows.push(DetailView { text: hint.into() });
        }
    }
    if let Some(value) = meaning(item) {
        rows.push(DetailView {
            text: format!("Engine outcome: {}", value.label()).into(),
        });
        if let Some(residue) =
            cleanup(&item.publication.cleanup).and_then(|cleanup| cleanup.residual_name)
        {
            rows.push(DetailView {
                text: format!(
                    "Temporary file: {}",
                    eml_protocol::TerminalPath::new(std::path::Path::new(&residue))
                )
                .into(),
            });
        }
        rows.extend(value.consequences.iter().map(|consequence| DetailView {
            text: consequence.text().into(),
        }));
        (
            rows,
            match value.removal_mode {
                vocabulary::RemovalMode::Removed => "Removed from copy",
                vocabulary::RemovalMode::Absent => "Already absent from copy",
                vocabulary::RemovalMode::Selected => "Would remove",
            }
            .into(),
            value.removal_mode.fields().into(),
        )
    } else {
        (rows, String::new(), String::new())
    }
}
/// Removal categories remain core-owned; source metadata is shown without rewriting.
pub(crate) fn removals(item: &wire::ItemRecord) -> Vec<RemovalView> {
    item.removed
        .iter()
        .map(|part| RemovalView {
            section: part.section.clone().into(),
            filename: part.filename.value().cloned().unwrap_or_default().into(),
            media: part.content_type.clone().into(),
            bytes: part.bytes as i32,
            byte_label: CountKind::PartBytes.label().into(),
            byte_accessible: vocabulary::count_label(CountKind::PartBytes, part.bytes as u32)
                .into(),
            reason: vocabulary::removal_reason(match part.reason {
                wire::Removal::Attachment => eml_core::RemovalReason::Attachment,
                wire::Removal::RelatedResource => eml_core::RemovalReason::RelatedResource,
                wire::Removal::NonBodyPart => eml_core::RemovalReason::NonBodyPart,
            })
            .into(),
        })
        .collect()
}

/// Run-level facts remain visible even when the stream contains no item records.
pub(crate) fn report_diagnostics(report: &eml_app::RunReport) -> Vec<DetailView> {
    let mut rows = Vec::new();
    if let Some(error) = report.summary().and_then(|s| s.run_error.value()) {
        rows.push(DetailView {
            text: format!("Run error: {}", error.code).into(),
        });
        rows.extend(
            error
                .detail
                .lines()
                .map(|text| DetailView { text: text.into() }),
        );
        if let Some(hint) = vocabulary::recovery_hint(&error.code) {
            rows.push(DetailView { text: hint.into() });
        }
    }
    let complete = report
        .summary()
        .map(|s| s.collection_complete)
        .or_else(|| report.start().map(|s| s.collection_complete));
    if complete == Some(false) {
        rows.push(DetailView {
            text: vocabulary::COLLECTION_INCOMPLETE_NOTICE.into(),
        });
    }
    rows
}
/// Retained process/transport diagnostics belong to the completed report.
pub(crate) fn completion_diagnostics(completion: &eml_app::WorkerCompletion) -> Vec<DetailView> {
    let mut rows = Vec::new();
    if completion.jsonl.is_empty() {
        rows.push(DetailView {
            text: "No protocol report was received, so there is no JSONL report to save.".into(),
        });
    }
    if completion.state == eml_app::WorkerState::RequestRefused {
        rows.push(DetailView {
            text: "The request could not start. Check that the selected inputs still exist and choose an existing, writable destination. Use Run Again… to correct the files, folders or destination.".into(),
        });
    }
    if completion.failure == Some(eml_app::WorkerFailure::Identity) {
        rows.push(DetailView {
            text: vocabulary::WORKER_IDENTITY_HINT.into(),
        });
    }
    if matches!(
        completion.state,
        eml_app::WorkerState::Incomplete | eml_app::WorkerState::ForceStopped
    ) {
        rows.push(DetailView {
            text: vocabulary::INCOMPLETE_REPORT_NOTICE.into(),
        });
    }
    if completion.state == eml_app::WorkerState::ForceStopped {
        rows.push(DetailView {
            text: vocabulary::FORCE_STOP_NOTICE.into(),
        });
    }
    if completion.stderr_truncated {
        rows.push(DetailView {
            text: vocabulary::STDERR_TRUNCATED_NOTICE.into(),
        });
    }
    rows
}

/// Presentation only: retain the full path while giving filename suffixes priority.
pub(crate) fn path(display: &str) -> PathView {
    let path = std::path::Path::new(display);
    let filename = path
        .file_name()
        .map(|s| s.to_string_lossy())
        .unwrap_or_else(|| display.into());
    let split = filename
        .char_indices()
        .rev()
        .nth(19)
        .map_or(0, |(index, _)| index);
    PathView {
        prefix: eml_protocol::DisplayText::new(&filename[..split])
            .to_string()
            .into(),
        suffix: eml_protocol::DisplayText::new(&filename[split..])
            .to_string()
            .into(),
        directory: path
            .parent()
            .map(|p| eml_protocol::TerminalPath::new(p).to_string())
            .unwrap_or_default()
            .into(),
        full: eml_protocol::DisplayText::new(display).to_string().into(),
    }
}
