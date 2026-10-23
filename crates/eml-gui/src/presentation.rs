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
fn item_meaning(item: &ItemRecord) -> Option<eml_core::meaning::OutcomeMeaning> {
    eml_protocol::meaning::outcome_facts(item).map(|facts| eml_core::meaning::classify(&facts))
}
fn item_label(item: &ItemRecord) -> String {
    item_meaning(item)
        .map_or("Outcome cannot be established", |meaning| meaning.label())
        .into()
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
    if let Some(hint) = eml_core::meaning::recovery_hint(&error.code) {
        text.push('\n');
        text.push_str(hint);
    }
    Some(text)
}

fn report_artwork(
    facts: &eml_app::RunReport,
    completion: Option<&eml_app::WorkerCompletion>,
) -> crate::view::ArtworkState {
    use crate::view::ArtworkState;
    let Some(completion) = completion else {
        return ArtworkState::Processing;
    };
    match facts.terminal_meaning(completion) {
        eml_app::RunMeaning::Completed => ArtworkState::Success,
        eml_app::RunMeaning::CompletedWithMixedResults | eml_app::RunMeaning::ResultsIncomplete => {
            ArtworkState::Attention
        }
        eml_app::RunMeaning::Stopped | eml_app::RunMeaning::NotStarted => ArtworkState::Stopped,
        eml_app::RunMeaning::LaunchFailed
        | eml_app::RunMeaning::IdentityMismatch
        | eml_app::RunMeaning::TransportFailed
        | eml_app::RunMeaning::OwnershipUnverified
        | eml_app::RunMeaning::CompletedWithErrors => ArtworkState::Failure,
    }
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
            "Collected: {} · Results received: {}",
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
        if facts.start().is_some_and(|start| start.dry_run) {
            text.push_str(&format!(
                "\n{} planned · {}",
                c.planned,
                eml_core::RunMode::DryRun.label()
            ));
        }
    }
    if let Some(problem) = run_problem(facts.summary()) {
        text.push_str(&format!("\n{problem}"));
    }
    if let Some(missing) = facts.unreported_item_count().filter(|n| *n > 0) {
        text.push_str(&format!("\n{missing} {} no reported result. Complete copies may exist; inspect the destination.",
            if missing == 1 { "item has" } else { "items have" }));
    }
    if let Some(completion) = completion {
        text.push_str(&format!("\n{}", facts.terminal_meaning(completion).label()));
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
    let meaning = item_meaning(item);
    if let Some(error) = item.error.value() {
        text.push_str(&format!("{}\n", DisplayText::new(&error.detail)));
        if let Some(hint) = eml_core::meaning::recovery_hint(&error.code) {
            text.push_str(&format!("{hint}\n"));
        }
        text.push_str("Original message unchanged.\n");
    }
    if let Some(meaning) = &meaning {
        for consequence in &meaning.consequences {
            text.push_str(consequence.text());
            text.push('\n');
        }
    } else {
        text.push_str("Outcome cannot be established.\n");
    }
    if !item.removed.is_empty() {
        let heading = meaning
            .as_ref()
            .map_or("Parts selected for removal", |m| m.removal_mode.parts());
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
                eml_protocol::meaning::removal_reason(part.reason).label(),
                DisplayText::new(&part.section)
            ));
        }
    }
    if let Some(notice) = meaning.as_ref().and_then(|m| m.related_notice) {
        text.push_str(notice);
        text.push('\n');
    }
    if !item.stripped_fields.is_empty() {
        text.push_str(&format!(
            "{}: {}\n",
            meaning
                .as_ref()
                .map_or("Fields selected for removal", |m| m.removal_mode.fields()),
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
    let status = if completion.is_some_and(|completion| completion.settled) {
        "Processing child settled; inspect the retained report."
    } else {
        "Waiting for owned processing to stop."
    };
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
    let state = completion.map_or("Running", |completion| {
        facts.terminal_meaning(completion).label()
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
            let mut text = format!(
                "{} created · {} up to date\n{} failed · {} not processed",
                c.created, c.up_to_date, c.failed, c.not_processed
            );
            if facts.start().is_some_and(|start| start.dry_run) {
                text.push_str(&format!("\n{} planned", c.planned));
            }
            text
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
        "Run {id}: Not started\nMode: {}\nSelected paths: {}\nSave copies: {destination}",
        request.mode().label(),
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
pub(crate) fn export_notice(completion: &eml_app::ExportCompletion) -> UserNotice {
    let details = format!(
        "Report export operation {} for run {}\nDestination: {}\nAccepted receipt: {:?}\nWorker lifecycle: {:?}",
        completion.operation_id.get(),
        completion.run_id,
        DisplayText::new(&completion.destination.to_string_lossy()),
        completion.receipt,
        completion.worker
    );
    UserNotice {
        message: format!(
            "Run {} report: {}",
            completion.run_id,
            completion.outcome_message()
        ),
        details,
    }
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
            operation_id: eml_protocol::desktop::OperationId::new(1).unwrap(),
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
    fn check_renders_the_received_exact_destination_and_planned_count() {
        let wire = include_str!("../../../tests/contract/protocol/planned-create-unix.jsonl")
            .replace("@VERSION@", "test-version");
        let mut facts = RunReport::for_native_mode("test-version", eml_core::RunMode::DryRun);
        for line in wire.lines() {
            facts.accept_frame(line.as_bytes()).unwrap();
        }
        facts.finish(Some(0), false).unwrap();
        let done = completion(0);
        let page = report_page(
            run_id(),
            &facts,
            Some(&done),
            ReportPosition::default(),
            None,
        );
        assert!(page.summary.contains("1 planned · Check only"));
        assert!(page.summary.contains("0 created"));
        assert!(
            page.detail
                .contains("Copy: a.ulaixrqur2pkcd3knlgxwbxf6i.no-attachments.eml")
        );
        assert!(page.detail.starts_with("Prepared only"));
        assert!(!page.detail.contains("Saved and read back"));
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
    #[test]
    fn missing_export_receipt_never_claims_absence_or_success() {
        let completion = eml_app::ExportCompletion {
            operation_id: eml_protocol::desktop::OperationId::new(1).unwrap(),
            run_id: run_id(),
            destination: std::env::temp_dir().join("report.json"),
            receipt: None,
            report_too_large: false,
            worker: completion(1),
        };
        let notice = export_notice(&completion);
        assert!(notice.message.contains("unknown"));
        assert!(notice.message.contains("may exist"));
        assert!(!notice.message.contains("success"));
        assert!(notice.details.contains("Accepted receipt: None"));
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
        assert!(text.contains(eml_core::meaning::PREPARED_RELATED_RESOURCE_NOTICE));
        item.publication.state = eml_protocol::records::Publication::Published;
        let text = item_detail(&item, 0, Some(165));
        assert!(text.contains("Removed parts"));
        assert!(text.contains("Removed stale fields: Content-MD5"));
        assert!(text.contains(eml_core::meaning::RELATED_RESOURCE_NOTICE));
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
            assert_eq!(
                item_meaning(&item).unwrap().removal_mode,
                eml_core::meaning::RemovalMode::Selected
            );
            assert!(
                size_comparison(&item, 165)
                    .unwrap()
                    .starts_with("Prepared copy: ")
            );
        }
        item.publication.state = Publication::Published;
        assert_eq!(
            item_meaning(&item).unwrap().removal_mode,
            eml_core::meaning::RemovalMode::Removed
        );
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
        assert!(eml_core::meaning::recovery_hint("FUTURE_CODE").is_none());
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
                "No new copy published",
            ),
            (
                include_bytes!("../../../tests/contract/protocol/published-with-error.jsonl")
                    .as_slice(),
                "Complete copy published; qualification incomplete",
            ),
            (
                include_bytes!("../../../tests/contract/protocol/unknown-publication.jsonl")
                    .as_slice(),
                "Publication uncertain",
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
