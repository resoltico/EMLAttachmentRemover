//! Bounded, read-only presentation of accepted application and Core facts.
use crate::view::ArtworkState;
use eml_app::{RunId, RunReport, WorkerCompletion};
use eml_protocol::{DisplayText, records::ItemRecord};
pub(crate) const PAGE_SIZE: usize = 64;

pub(crate) fn display_lines(text: &str) -> String {
    text.split('\n')
        .map(|line| DisplayText::new(line).to_string())
        .collect::<Vec<_>>()
        .join("\n")
}
pub(crate) fn terminal_label(completion: &eml_app::WorkerCompletion) -> &'static str {
    use eml_app::WorkerFailure;
    match completion.failure {
        Some(WorkerFailure::Request) => {
            "Selection not started. Choose an existing copy destination or Beside originals. Originals are unchanged."
        }
        Some(WorkerFailure::Launch) => {
            "The bundled worker could not start. Reinstall the app and try again."
        }
        Some(WorkerFailure::Identity) => {
            "The bundled worker identity differs. Reinstall the app before processing."
        }
        Some(WorkerFailure::Transport) => {
            "Worker results could not be fully verified. Earlier observations are retained."
        }
        Some(WorkerFailure::Unverified) => {
            "Processing could not be fully verified. Further processing is closed."
        }
        None if completion.signal.is_some() || completion.exit_code == Some(130) => {
            "Stopped. Completed results are retained."
        }
        None if completion.exit_code == Some(0) => "Finished.",
        None if completion.exit_code.is_some() => {
            "Finished with errors. Originals are unchanged; inspect the results."
        }
        None => "Stopped before processing.",
    }
}
fn item_label(item: &eml_protocol::records::ItemRecord) -> String {
    use eml_protocol::records::{ItemStatus, Publication};
    match item.status {
        ItemStatus::Created => format!(
            "Created copy · Removed {} {}",
            item.removed.len(),
            if item.removed.len() == 1 {
                "part"
            } else {
                "parts"
            }
        ),
        ItemStatus::UpToDate => "Already up to date".into(),
        ItemStatus::Planned => "Planned".into(),
        ItemStatus::Failed => match item.publication.state {
            Publication::Published => "Copy published with an error".into(),
            Publication::Unknown => "Publication unconfirmed".into(),
            Publication::NotPublished if item.existing_observation.value().is_some() => {
                "Existing copy could not be verified".into()
            }
            Publication::NotPublished => "Could not create copy".into(),
        },
    }
}
fn copy_was_materialized(item: &eml_protocol::records::ItemRecord) -> bool {
    item.publication.state == eml_protocol::records::Publication::Published
        || item.status == eml_protocol::records::ItemStatus::UpToDate
}
fn size_comparison(item: &eml_protocol::records::ItemRecord, smaller_by: u64) -> Option<String> {
    let bytes = item.copy_bytes.value()?;
    Some(format!(
        "Prepared copy: {bytes} bytes; {smaller_by} bytes smaller than the original"
    ))
}
fn run_problem(summary: Option<&eml_protocol::records::SummaryRecord>) -> Option<String> {
    let error = summary?.run_error.value()?;
    let mut text = DisplayText::new(&error.detail).to_string();
    if let Some(hint) = eml_protocol::user_messages::recovery_hint(&error.code) {
        text.push('\n');
        text.push_str(hint);
    }
    Some(text)
}

fn removal_label(reason: eml_protocol::records::Removal) -> &'static str {
    use eml_protocol::records::Removal;
    match reason {
        Removal::Attachment => "Attachment",
        Removal::RelatedResource => "Related image or resource",
        Removal::NonBodyPart => "Other non-body content",
    }
}
fn report_artwork(
    facts: &eml_app::RunReport,
    completion: Option<&eml_app::WorkerCompletion>,
) -> crate::view::ArtworkState {
    use crate::view::ArtworkState;
    let Some(completion) = completion else {
        return ArtworkState::Processing;
    };
    if facts.failure().is_some() || completion.failure.is_some() {
        return ArtworkState::Failure;
    }
    if facts.summary().is_some_and(|summary| summary.interrupted)
        || completion.signal.is_some()
        || completion.exit_code == Some(130)
        || completion.exit_code.is_none()
    {
        return ArtworkState::Stopped;
    }
    if completion.exit_code != Some(0)
        || facts.summary().is_some_and(|summary| {
            summary.counts.failed > 0
                || summary.run_error.value().is_some()
                || !summary.collection_complete
        })
    {
        return if facts
            .summary()
            .is_some_and(|summary| summary.counts.created > 0 || summary.counts.up_to_date > 0)
        {
            ArtworkState::Attention
        } else {
            ArtworkState::Failure
        };
    }
    ArtworkState::Success
}

#[derive(Clone, Copy, Default)]
pub(crate) struct ReportPosition {
    pub(crate) page: usize,
    pub(crate) item: usize,
    pub(crate) part_page: usize,
}
impl ReportPosition {
    pub(crate) fn clamped(self, facts: &RunReport) -> Self {
        let item = self.item.min(facts.items().len().saturating_sub(1));
        let pages = facts.items().len().div_ceil(PAGE_SIZE).max(1);
        let part_pages = facts
            .items()
            .get(item)
            .map_or(1, |i| i.removed.len().div_ceil(PAGE_SIZE).max(1));
        Self {
            page: self.page.min(pages - 1),
            item,
            part_page: self.part_page.min(part_pages - 1),
        }
    }
}
pub(crate) struct Navigation {
    pub(crate) previous_items: bool,
    pub(crate) next_items: bool,
    pub(crate) previous_parts: bool,
    pub(crate) next_parts: bool,
}
pub(crate) struct ReportPage {
    pub(crate) summary: String,
    pub(crate) rows: Vec<String>,
    pub(crate) detail: String,
    pub(crate) technical: String,
    pub(crate) artwork: ArtworkState,
    pub(crate) navigation: Navigation,
}
pub(crate) fn view_key(
    id: RunId,
    facts: &RunReport,
    completed: bool,
    position: ReportPosition,
    appearance: Option<(bool, bool)>,
) -> String {
    format!(
        "{id}:{}:{}:{}:{}:{}:{}:{:?}:{:?}:{completed}:{appearance:?}",
        position.page,
        position.item,
        position.part_page,
        facts.items().len(),
        facts.start().is_some(),
        facts.summary().is_some(),
        facts.failure(),
        facts.unreported_item_count()
    )
}
pub(crate) fn report_page(
    id: RunId,
    facts: &RunReport,
    completion: Option<&WorkerCompletion>,
    position: ReportPosition,
    appearance: Option<(bool, bool)>,
) -> ReportPage {
    let pages = facts.items().len().div_ceil(PAGE_SIZE).max(1);
    let selected = facts.items().get(position.item);
    let part_pages = selected.map_or(1, |item| item.removed.len().div_ceil(PAGE_SIZE).max(1));
    ReportPage {
        summary: report_summary(id, facts, completion, position.page, pages),
        rows: facts
            .items()
            .iter()
            .skip(position.page * PAGE_SIZE)
            .take(PAGE_SIZE)
            .map(|item| {
                let basename = item.source.display.rsplit('/').next().unwrap_or("Message");
                let name: String = DisplayText::new(basename)
                    .to_string()
                    .chars()
                    .take(180)
                    .collect();
                format!("{name} · {}", item_label(item))
            })
            .collect(),
        detail: selected.map_or_else(String::new, |item| {
            item_detail(
                item,
                position.part_page,
                facts.copy_size_reduction(position.item),
            )
        }),
        technical: technical_detail(facts, completion, selected, appearance),
        artwork: report_artwork(facts, completion),
        navigation: Navigation {
            previous_items: position.page > 0,
            next_items: position.page + 1 < pages,
            previous_parts: position.part_page > 0,
            next_parts: position.part_page + 1 < part_pages,
        },
    }
}
fn report_summary(
    id: RunId,
    facts: &RunReport,
    completion: Option<&WorkerCompletion>,
    page: usize,
    pages: usize,
) -> String {
    let mut text = format!("Run {id} · ");
    match facts.start() {
        Some(start) => text.push_str(&format!(
            "{} collected · {} results received",
            start.total,
            facts.items().len()
        )),
        None if completion.is_some() => text.push_str("Processing could not be confirmed."),
        None => text.push_str("Scanning files…"),
    }
    if let Some(summary) = facts.summary() {
        let c = &summary.counts;
        text.push_str(&format!(
            "\n{} created · {} up to date · {} failed · {} not processed",
            c.created, c.up_to_date, c.failed, c.not_processed
        ));
    }
    if let Some(problem) = run_problem(facts.summary()) {
        text.push_str(&format!("\n{problem}"));
    }
    if let Some(missing) = facts.unreported_item_count().filter(|n| *n > 0) {
        text.push_str(&format!("\n{missing} {} no reported result. Complete copies may exist; inspect the destination.",
            if missing == 1 { "item has" } else { "items have" }));
    }
    if let Some(completion) = completion {
        text.push_str(&format!("\n{}", terminal_label(completion)));
    }
    if !facts.items().is_empty() {
        text.push_str(&format!("\nItems page {} of {pages}", page + 1));
    }
    text
}
fn item_detail(item: &ItemRecord, part_page: usize, smaller_by: Option<u64>) -> String {
    let mut text = format!(
        "{}\nSource: {}\nCopy: {}\n",
        item_label(item),
        DisplayText::new(&item.source.display),
        item.destination
            .value()
            .map_or_else(|| "—".into(), |p| DisplayText::new(&p.display).to_string())
    );
    if let Some(comparison) = smaller_by.and_then(|delta| size_comparison(item, delta)) {
        text.push_str(&format!("{comparison}\n"));
    }
    if let Some(error) = item.error.value() {
        text.push_str(&format!("{}\n", DisplayText::new(&error.detail)));
        if let Some(hint) = eml_protocol::user_messages::recovery_hint(&error.code) {
            text.push_str(&format!("{hint}\n"));
        }
        text.push_str("Original message unchanged.\n");
        use eml_protocol::records::Publication;
        text.push_str(match item.publication.state {
            Publication::Published => {
                "A complete copy was published. Inspect the destination before retrying.\n"
            }
            Publication::Unknown => {
                "A complete copy may exist. Inspect the destination before retrying.\n"
            }
            Publication::NotPublished => "No new copy was published.\n",
        });
    }
    if !item.removed.is_empty() {
        let heading = if copy_was_materialized(item) {
            "Removed parts"
        } else {
            "Parts selected for removal"
        };
        let pages = item.removed.len().div_ceil(PAGE_SIZE).max(1);
        text.push_str(&format!("{heading} · page {} of {pages}\n", part_page + 1));
        for part in item
            .removed
            .iter()
            .skip(part_page * PAGE_SIZE)
            .take(PAGE_SIZE)
        {
            text.push_str(&format!(
                "{} · {} · {} bytes · {} · section {}\n",
                DisplayText::new(part.filename.value().map_or("Unnamed part", String::as_str)),
                DisplayText::new(&part.content_type),
                part.bytes,
                removal_label(part.reason),
                DisplayText::new(&part.section)
            ));
        }
    }
    if item
        .removed
        .iter()
        .any(|p| p.reason == eml_protocol::records::Removal::RelatedResource)
    {
        text.push_str(if copy_was_materialized(item) {
            eml_protocol::user_messages::RELATED_RESOURCE_NOTICE
        } else {
            eml_protocol::user_messages::PREPARED_RELATED_RESOURCE_NOTICE
        });
        text.push('\n');
    }
    if !item.stripped_fields.is_empty() {
        text.push_str(&format!(
            "{}: {}\n",
            if copy_was_materialized(item) {
                "Removed stale fields"
            } else {
                "Fields selected for removal"
            },
            DisplayText::new(&item.stripped_fields.join(", "))
        ));
    }
    text
}
fn technical_detail(
    facts: &RunReport,
    completion: Option<&WorkerCompletion>,
    item: Option<&ItemRecord>,
    appearance: Option<(bool, bool)>,
) -> String {
    let mut text = format!("Protocol failure: {:?}\n", facts.failure());
    text.push_str(&lifecycle_detail(completion, appearance));
    if let Some(item) = item {
        text.push_str(&format!(
            "Publication: {:#?}\nExisting copy: {:#?}\nItem error: {:#?}\n",
            item.publication,
            item.existing_observation.value(),
            item.error.value()
        ));
    }
    text
}
fn lifecycle_detail(
    completion: Option<&WorkerCompletion>,
    appearance: Option<(bool, bool)>,
) -> String {
    let mut text = String::new();
    match appearance {
        Some((dark, contrast)) => text.push_str(&format!(
            "Interface native appearance: {}; increased contrast: {contrast}\n",
            if dark { "dark" } else { "light" }
        )),
        None => text.push_str("Interface native appearance: unavailable\n"),
    }
    if let Some(c) = completion {
        text.push_str(&format!("Worker exit {:?}; signal {:?}; failure {:?}; settled {}; reusable {}\nInterrupt calls {}; first interrupt observed {}\n{}\nStderr{}:\n{}\n",
            c.exit_code, c.signal, c.failure, c.settled, c.reusable, c.interrupt_calls_succeeded, c.first_interrupt_observed,
            DisplayText::new(&c.faults.join("; ")), if c.stderr_truncated { " (truncated)" } else { "" }, display_lines(&String::from_utf8_lossy(&c.stderr))));
    }
    text
}
/// Show only independently available lifecycle facts when the report lock failed.
/// No accepted model data is recovered from a poisoned lock.
pub(crate) fn unavailable_report_page(
    id: RunId,
    completion: Option<&WorkerCompletion>,
    failure: eml_app::WorkerFailure,
    appearance: Option<(bool, bool)>,
) -> ReportPage {
    let status = completion.map_or("Waiting for owned processing to stop.", terminal_label);
    ReportPage {
        summary: format!("Run {id} · Report unavailable.\n{status}"),
        rows: Vec::new(),
        detail: "The report could not be safely inspected. Processing outcomes cannot be confirmed from this report; complete copies may exist. After processing stops, inspect destinations before retrying.".into(),
        technical: format!("Report inspection: {failure:?}\n{}", lifecycle_detail(completion, appearance)),
        artwork: ArtworkState::Failure,
        navigation: Navigation {previous_items:false,next_items:false,previous_parts:false,next_parts:false},
    }
}
pub(crate) fn run_row(
    id: RunId,
    facts: &RunReport,
    completion: Option<&WorkerCompletion>,
) -> String {
    let state = completion.map_or("Running", |c| {
        if c.failure.is_some() {
            "Needs attention"
        } else if c.signal.is_some() || c.exit_code == Some(130) {
            "Stopped"
        } else if c.exit_code == Some(0) {
            "Finished"
        } else {
            "Finished with errors"
        }
    });
    let counts = facts.summary().map_or_else(
        || {
            let mut text = format!("{} received", facts.items().len());
            if let Some(missing) = facts.unreported_item_count().filter(|n| *n > 0) {
                text.push_str(&format!("\n{missing} without reported result"));
            }
            text
        },
        |summary| {
            let c = &summary.counts;
            format!(
                "{} created · {} up to date\n{} failed · {} not processed",
                c.created, c.up_to_date, c.failed, c.not_processed
            )
        },
    );
    format!("Run {id}\n{state}\n{counts}")
}
pub(crate) fn waiting_row(id: RunId, request: &eml_app::RequestSnapshot) -> String {
    let destination = match &request.request().output {
        eml_core::OutputLocation::BesideSource => "Beside originals".into(),
        eml_core::OutputLocation::Directory(path) => {
            DisplayText::new(&path.to_string_lossy()).to_string()
        }
    };
    format!(
        "Run {id}: Not started\n{} selections\nSave copies: {destination}",
        request.request().paths.len()
    )
}

/// One coherent, replaceable observation; later failures cannot hide behind an old notice.
pub(crate) struct UserNotice {
    pub(crate) message: String,
    pub(crate) details: String,
}
impl UserNotice {
    pub(crate) fn message(message: &str) -> Self {
        Self {
            message: message.chars().take(1000).collect(),
            details: String::new(),
        }
    }
}
pub(crate) fn export_notice(id: RunId, outcome: &eml_app::ReportExportOutcome) -> UserNotice {
    use eml_app::PublicationState;
    let mut message = if outcome.usable() {
        "Saved and read back successfully.".to_owned()
    } else {
        match outcome.publication {
        PublicationState::Published => "A complete report was published, but export did not finish cleanly. Inspect the destination before retrying.".to_owned(),
        PublicationState::Unknown => "A complete report may exist. Inspect the destination before retrying.".to_owned(),
        PublicationState::NotPublished => export_recovery(outcome),
    }
    };
    if matches!(
        outcome.cleanup.state,
        eml_core::CleanupState::Residual | eml_core::CleanupState::Unknown
    ) {
        message.push_str(" Temporary files may remain; see Technical details.");
    }
    let mut details = format!(
        "Report export for run {id}\nPublication: {:?}\nAddress: {:?}\nFinal verification: {:?}\nFile sync: {:?}\nNamespace sync: {:?}\nCleanup: {:?}\n",
        outcome.publication,
        outcome.address_verified,
        outcome.final_file_verified,
        outcome.file_sync,
        outcome.namespace_sync,
        outcome.cleanup
    );
    for failure in &outcome.failures {
        details.push_str(&format!(
            "{:?}: kind {:?}; native {:?}; {}\n",
            failure.stage,
            failure.kind,
            failure.native_code,
            DisplayText::new(&failure.detail)
        ));
    }
    UserNotice {
        message: format!("Run {id} report: {message}"),
        details,
    }
}
fn export_recovery(outcome: &eml_app::ReportExportOutcome) -> String {
    use std::io::ErrorKind;
    let failure = outcome.failures.first();
    match failure.and_then(|f| f.kind) {
        Some(ErrorKind::AlreadyExists) => "No new report was published. That name is already in use; choose an unused filename.",
        Some(ErrorKind::PermissionDenied) => "No new report was published. Choose a folder you can access and write to.",
        Some(ErrorKind::Interrupted) => "Report export was cancelled. No new report was published.",
        Some(ErrorKind::WouldBlock) => "No new report was published. The report is busy; wait for the current operation to finish and try exporting again.",
        _ if failure.is_some_and(|f| f.stage == eml_app::ReportExportStage::Serialization) => "Report data could not be read safely. No new report was published; see Technical details.",
        _ => "No new report was published. Check the destination and choose an unused filename in a writable folder; see Technical details.",
    }.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run_id() -> RunId {
        let request =
            eml_app::RequestSnapshot::new(vec![std::env::temp_dir().join("message.eml")], None)
                .unwrap();
        eml_app::RunQueue::default().enqueue(request).unwrap()
    }
    fn completion(exit: i32) -> WorkerCompletion {
        WorkerCompletion {
            exit_code: Some(exit),
            signal: None,
            input_may_have_been_sent: true,
            settled: true,
            reusable: true,
            failure: None,
            faults: Vec::new(),
            stderr: Vec::new(),
            stderr_truncated: false,
            interrupt_calls_succeeded: 0,
            first_interrupt_observed: false,
        }
    }
    #[test]
    fn unavailable_report_clears_item_navigation_and_preserves_independent_settlement() {
        let id = run_id();
        let mut done = completion(1);
        done.failure = Some(eml_app::WorkerFailure::Unverified);
        done.reusable = false;
        done.faults.push("Report model lock poisoned".into());
        for terminal in [None, Some(&done)] {
            let page = unavailable_report_page(
                id,
                terminal,
                eml_app::WorkerFailure::Unverified,
                Some((true, true)),
            );
            assert!(page.rows.is_empty());
            assert!(!page.navigation.previous_items && !page.navigation.next_items);
            assert!(!page.navigation.previous_parts && !page.navigation.next_parts);
            assert!(page.summary.contains("Report unavailable"));
            assert!(page.detail.contains("complete copies may exist"));
            assert!(!page.detail.contains("Original message unchanged"));
            assert!(page.technical.contains("dark; increased contrast: true"));
            if terminal.is_some() {
                assert!(page.technical.contains("settled true; reusable false"));
                assert!(page.technical.contains("Report model lock poisoned"));
                assert!(!page.summary.contains("resource settlement is unverified"));
            } else {
                assert!(
                    page.summary
                        .contains("Waiting for owned processing to stop")
                );
            }
        }
    }
    #[test]
    fn stream_finalization_invalidates_cached_missing_result_knowledge() {
        let wire = include_str!("../../../tests/contract/protocol/created-attachment.jsonl")
            .replace("@VERSION@", "test-version");
        let mut facts = RunReport::new("test-version");
        facts
            .accept_frame(wire.lines().next().unwrap().as_bytes())
            .unwrap();
        assert!(facts.accept_frame(b"{").is_err());
        let failure = facts.failure();
        let id = run_id();
        let before = view_key(id, &facts, false, ReportPosition::default(), None);
        assert!(facts.unreported_item_count().is_none());
        assert!(facts.finish(Some(1), false).is_err());
        assert_eq!(facts.failure(), failure);
        assert_eq!(facts.unreported_item_count(), Some(1));
        assert_ne!(
            before,
            view_key(id, &facts, false, ReportPosition::default(), None)
        );
    }
    #[test]
    fn serialization_failures_do_not_suggest_changing_a_healthy_destination() {
        let mut outcome = export_outcome(eml_app::PublicationState::NotPublished);
        outcome.failures[0].stage = eml_app::ReportExportStage::Serialization;
        outcome.failures[0].kind = Some(std::io::ErrorKind::Other);
        let notice = export_notice(run_id(), &outcome);
        assert!(
            notice
                .message
                .contains("Report data could not be read safely")
        );
        assert!(!notice.message.contains("choose an unused filename"));
        outcome.failures[0].kind = Some(std::io::ErrorKind::WouldBlock);
        assert!(
            export_notice(run_id(), &outcome)
                .message
                .contains("report is busy")
        );
    }
    fn fixture_item(bytes: &[u8]) -> Box<eml_protocol::records::ItemRecord> {
        bytes
            .split(|b| *b == b'\n')
            .filter(|l| !l.is_empty())
            .find_map(
                |line| match eml_protocol::records::decode_record(line).unwrap() {
                    eml_protocol::records::EventRecord::Item(item) => Some(item),
                    _ => None,
                },
            )
            .unwrap()
    }
    fn export_outcome(publication: eml_app::PublicationState) -> eml_app::ReportExportOutcome {
        eml_app::ReportExportOutcome {
            publication,
            file_sync: None,
            namespace_sync: None,
            address_verified: None,
            candidate_identity: None,
            final_file_verified: None,
            cleanup: eml_core::CleanupReceipt {
                state: eml_core::CleanupState::NotNeeded,
                residual_name: None,
                native_code: None,
            },
            failures: vec![eml_app::ReportExportFailure {
                stage: eml_app::ReportExportStage::Publication,
                kind: Some(std::io::ErrorKind::AlreadyExists),
                native_code: Some(17),
                detail: "Exact OS cause\nnot a forged line".into(),
            }],
        }
    }
    #[test]
    fn export_notices_keep_publication_cleanup_and_exact_failure_distinct() {
        use eml_app::PublicationState;
        let request =
            eml_app::RequestSnapshot::new(vec![std::env::temp_dir().join("message.eml")], None)
                .unwrap();
        let id = eml_app::RunQueue::default().enqueue(request).unwrap();
        for (publication, expected) in [
            (
                PublicationState::NotPublished,
                "No new report was published.",
            ),
            (
                PublicationState::Published,
                "A complete report was published",
            ),
            (PublicationState::Unknown, "A complete report may exist."),
        ] {
            let mut outcome = export_outcome(publication);
            let notice = export_notice(id, &outcome);
            assert!(notice.message.contains(expected), "{}", notice.message);
            assert!(!notice.message.contains("AlreadyExists"));
            assert!(
                notice
                    .details
                    .contains("Exact OS cause\\nnot a forged line")
            );
            assert!(notice.details.contains("native Some(17)"));
            for cleanup in [
                eml_core::CleanupState::Residual,
                eml_core::CleanupState::Unknown,
            ] {
                outcome.cleanup.state = cleanup;
                let notice = export_notice(id, &outcome);
                assert!(notice.message.contains("Temporary files may remain"));
                assert!(notice.details.contains(&format!("{cleanup:?}")));
            }
        }
    }
    #[test]
    fn prepared_parts_and_related_resource_notices_follow_publication() {
        let mut item = fixture_item(include_bytes!(
            "../../../tests/contract/protocol/created-attachment.jsonl"
        ));
        item.status = eml_protocol::records::ItemStatus::Failed;
        item.publication.state = eml_protocol::records::Publication::NotPublished;
        item.stripped_fields = vec!["Content-MD5".into()];
        // The same prepared bytes can be known after a refused publication.
        let text = item_detail(&item, 0, Some(165));
        assert!(text.contains("Prepared copy:"));
        assert!(text.contains("Parts selected for removal"));
        assert!(!text.contains("Removed parts"));
        assert!(text.contains("Fields selected for removal: Content-MD5"));
        item.removed[0].reason = eml_protocol::records::Removal::RelatedResource;
        let text = item_detail(&item, 0, Some(165));
        assert!(text.contains("Related image or resource"));
        assert!(text.contains(eml_protocol::user_messages::PREPARED_RELATED_RESOURCE_NOTICE));
        item.publication.state = eml_protocol::records::Publication::Published;
        let text = item_detail(&item, 0, Some(165));
        assert!(text.contains("Removed parts"));
        assert!(text.contains("Removed stale fields: Content-MD5"));
        assert!(text.contains(eml_protocol::user_messages::RELATED_RESOURCE_NOTICE));
    }
    #[test]
    fn unpublished_and_unknown_copies_do_not_claim_realized_savings() {
        use eml_protocol::records::{ItemStatus, Publication};
        let mut item = fixture_item(include_bytes!(
            "../../../tests/contract/protocol/created-attachment.jsonl"
        ));
        assert!(
            size_comparison(&item, 165)
                .unwrap()
                .starts_with("Prepared copy: ")
        );
        item.status = ItemStatus::Failed;
        for state in [Publication::NotPublished, Publication::Unknown] {
            item.publication.state = state;
            assert!(!copy_was_materialized(&item));
            assert!(
                size_comparison(&item, 165)
                    .unwrap()
                    .starts_with("Prepared copy: ")
            );
        }
        item.publication.state = Publication::Published;
        assert!(copy_was_materialized(&item));
        assert!(
            size_comparison(&item, 165)
                .unwrap()
                .starts_with("Prepared copy: ")
        );
        item.copy_bytes = eml_protocol::records::Nullable::Null(());
        assert!(size_comparison(&item, 165).is_none());
    }
    #[test]
    fn no_item_collection_failures_explain_the_problem_and_action() {
        use eml_protocol::records::{EventRecord, decode_record};
        for bytes in [
            include_bytes!("../../../tests/contract/protocol/no-inputs.jsonl").as_slice(),
            include_bytes!("../../../tests/contract/protocol/collection-limit.jsonl").as_slice(),
        ] {
            let summary = bytes
                .split(|b| *b == b'\n')
                .filter(|l| !l.is_empty())
                .find_map(|line| match decode_record(line).unwrap() {
                    EventRecord::Summary(summary) => Some(summary),
                    _ => None,
                })
                .unwrap();
            assert_eq!(summary.counts.failed, 0);
            let text =
                run_problem(Some(&summary)).expect("Run error must be visible without an item");
            assert!(text.contains("Choose") || text.contains("Select"), "{text}");
            let detail = &summary.run_error.value().unwrap().detail;
            assert!(text.starts_with(&DisplayText::new(detail).to_string()));
            let mut unknown = summary.clone();
            if let eml_protocol::records::Nullable::Value(error) = &mut unknown.run_error {
                error.code = "FUTURE_CODE".into();
                error.detail = "Exact future cause\nnot a forged line".into();
            }
            assert_eq!(
                run_problem(Some(&unknown)).unwrap(),
                "Exact future cause\\nnot a forged line"
            );
        }
        assert!(run_problem(None).is_none());
        assert!(eml_protocol::user_messages::recovery_hint("FUTURE_CODE").is_none());
    }
    #[test]
    fn run_errors_without_failed_items_are_not_presented_as_success() {
        use crate::view::ArtworkState;
        for (fixture, exit, expected) in [
            ("created", 0, ArtworkState::Success),
            ("no-inputs", 1, ArtworkState::Failure),
            ("collection-limit", 1, ArtworkState::Failure),
            ("started-preedge-cancellation", 130, ArtworkState::Stopped),
        ] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../tests/contract/protocol/{fixture}.jsonl"));
            let bytes = std::fs::read_to_string(path)
                .unwrap()
                .replace("@VERSION@", "test-version")
                .into_bytes();
            let mut facts = eml_app::RunReport::new("test-version");
            let mut splitter = eml_app::FrameSplitter::default();
            splitter
                .append(&bytes, |frame| facts.accept_frame(frame))
                .unwrap();
            facts.finish(Some(exit), splitter.finish()).unwrap();
            let completion = completion(exit);
            assert_eq!(
                report_artwork(&facts, Some(&completion)),
                expected,
                "{fixture}"
            );
        }
    }
    #[test]
    fn published_failed_and_unknown_present_distinct_copy_knowledge() {
        use eml_protocol::records::{EventRecord, decode_record};
        for (fixture, expected) in [
            (
                include_bytes!("../../../tests/contract/protocol/prepublication-failure.jsonl")
                    .as_slice(),
                "Could not create copy",
            ),
            (
                include_bytes!("../../../tests/contract/protocol/published-with-error.jsonl")
                    .as_slice(),
                "Copy published with an error",
            ),
            (
                include_bytes!("../../../tests/contract/protocol/unknown-publication.jsonl")
                    .as_slice(),
                "Publication unconfirmed",
            ),
        ] {
            let item = fixture
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
                .find_map(|line| match decode_record(line).unwrap() {
                    EventRecord::Item(item) => Some(item),
                    _ => None,
                })
                .unwrap();
            assert_eq!(item_label(&item), expected);
        }
    }
}
