//! GUI-owned wording for typed outcomes. CLI vocabulary is used only in technical details.
use crate::state::Tally;
use eml_core::{
    ErrorCode, RunMode,
    vocabulary::{MeaningKind, RunStatus},
};

#[derive(Clone, Copy)]
pub(super) struct Words {
    pub verdict: &'static str,
    pub headline: &'static str,
    pub guidance: &'static str,
}
const fn words(verdict: &'static str, headline: &'static str, guidance: &'static str) -> Words {
    // Byte length is a conservative compile-time bound on character length.
    assert!(!verdict.is_empty() && verdict.len() <= 28);
    assert!(!headline.is_empty());
    Words {
        verdict,
        headline,
        guidance,
    }
}
pub(super) fn failure(code: ErrorCode) -> Words {
    use ErrorCode::*;
    match code {
        ReadFailed => {
            const {
                words(
                    "Can't read file",
                    "A file or folder could not be read",
                    "Check that it still exists and that you have permission to read it.",
                )
            }
        }
        SourceNotRegular => {
            const {
                words(
                    "Not a file",
                    "This is not a regular file",
                    "Add the email file itself.",
                )
            }
        }
        SourceChanged => {
            const {
                words(
                    "Changed during run",
                    "The source file changed during the run",
                    "Run again when nothing else is writing to it.",
                )
            }
        }
        Malformed => {
            const {
                words(
                    "Damaged message",
                    "The message is not valid email",
                    "Export it again from your mail app.",
                )
            }
        }
        LimitExceeded => {
            const {
                words(
                    "Too large or complex",
                    "The message exceeds the supported size or structure",
                    "Export a smaller or simpler message. Messages above 256 MiB cannot be processed.",
                )
            }
        }
        Protected => {
            const {
                words(
                    "Signed or encrypted",
                    "Not copied — signed or encrypted message",
                    "Export an unsigned, decrypted copy from your mail app, then add it again.",
                )
            }
        }
        UnsupportedMime => {
            const {
                words(
                    "Unsupported format",
                    "This format cannot be safely reduced",
                    "The original is unchanged.",
                )
            }
        }
        NoBody => {
            const {
                words(
                    "No message text",
                    "There is no supported message text to keep",
                    "Add an email containing a plain-text or HTML body.",
                )
            }
        }
        Internal => {
            const {
                words(
                    "Verification failed",
                    "The copy did not pass verification, so it was not kept",
                    "Please report this with the saved report.",
                )
            }
        }
        InputAdmissionFailed => {
            const {
                words(
                    "Can't open",
                    "A selected file or folder could not be opened",
                    "Check that it still exists and that you have permission to read it.",
                )
            }
        }
        InputLimitExceeded => {
            const {
                words(
                    "Too many files",
                    "The search reached the supported limit",
                    "Split the folders into smaller requests, with at most 4,096 messages each.",
                )
            }
        }
        NoInputs => const { words("No emails found", "No .eml files were found", "") },
        OutputOverlap => {
            const {
                words(
                    "Folders overlap",
                    "The source and destination folders overlap",
                    "Choose a separate destination that neither contains nor sits inside a selected folder.",
                )
            }
        }
        DestinationCollision => {
            const {
                words(
                    "Name conflict",
                    "Copies would share the same destination name",
                    "Process the messages into separate destination folders.",
                )
            }
        }
        DestinationExists => {
            const {
                words(
                    "Name conflict",
                    "A different file already uses the copy's name",
                    "Rename or move that file, or choose another destination, then run again.",
                )
            }
        }
        SourceDestinationAlias => {
            const {
                words(
                    "Same file",
                    "The copy would be written over an original",
                    "Choose another destination.",
                )
            }
        }
        DestinationUnsupported => {
            const {
                words(
                    "Can't write there",
                    "The destination does not support safe copies",
                    "Choose another folder.",
                )
            }
        }
        BindFailed => {
            const {
                words(
                    "Folder unavailable",
                    "A required file or folder became unavailable",
                    "Check the source and destination folders, reconnect any removed disk, then run again.",
                )
            }
        }
        StageFailed | WriteFailed => {
            const {
                words(
                    "Couldn't write",
                    "The copy could not be written",
                    "Check free space and permissions, or choose another folder.",
                )
            }
        }
        FileSyncFailed | NamespaceSyncFailed => {
            const {
                words(
                    "Couldn't save to disk",
                    "The copy could not be confirmed on disk",
                    "Check the disk, then run again.",
                )
            }
        }
        AddressChanged => {
            const {
                words(
                    "Folder changed",
                    "The destination changed during the run",
                    "Check the destination, then run again.",
                )
            }
        }
        AddressUnverified => {
            const {
                words(
                    "Folder unverified",
                    "The destination could not be confirmed",
                    "Check the destination, then run again.",
                )
            }
        }
        CleanupFailed => {
            const {
                words(
                    "Leftover temporary file",
                    "A temporary file could not be removed",
                    "After all runs have finished or stopped, inspect the file named in Technical Details before removing it.",
                )
            }
        }
        ExistingSyncFailed => {
            const {
                words(
                    "Existing copy unverified",
                    "An existing copy could not be confirmed on disk",
                    "Check the disk, then run again.",
                )
            }
        }
        Interrupted => {
            const {
                words(
                    "Stopped",
                    "Stopped before this message finished",
                    "Use Continue… to process the remaining messages.",
                )
            }
        }
        OutputFailed => {
            const {
                words(
                    "Report problem",
                    "Some results could not be delivered",
                    "Save Report keeps what was received. Check the destination before running again.",
                )
            }
        }
    }
}
pub(super) fn meaning(kind: MeaningKind) -> Words {
    match kind {
        MeaningKind::Created => const { words("Created", "Copy created", "") },
        MeaningKind::ExistingVerified => {
            const {
                words(
                    "Already up to date",
                    "An identical copy already exists",
                    "Nothing was written.",
                )
            }
        }
        // Matching bytes alone do not establish address/durability qualification.
        MeaningKind::ExistingMatched => {
            const {
                words(
                    "Copy needs verification",
                    "A matching copy could not be fully verified",
                    "Check the destination, then run again to verify the copy. Existing copies are never replaced.",
                )
            }
        }
        MeaningKind::Prepared => {
            const {
                words(
                    "Ready to copy",
                    "Can be copied",
                    "Start Create Copies to write it.",
                )
            }
        }
        MeaningKind::Comparison => {
            const {
                words(
                    "Copy exists — will verify",
                    "A copy with this name already exists",
                    "It will be verified when copies are created.",
                )
            }
        }
        MeaningKind::PublishedIncomplete => {
            const {
                words(
                    "Created — check folder",
                    "Copy created, but final checks did not finish",
                    "The copy is complete. Check that it exists in the destination, then run again to verify it.",
                )
            }
        }
        MeaningKind::PublicationUncertain | MeaningKind::Unestablished => {
            const {
                words(
                    "Unknown result",
                    "Could not confirm whether a copy was written",
                    "Check the destination, then run again. Existing copies are verified, never replaced.",
                )
            }
        }
        MeaningKind::NotPublished => {
            const {
                words(
                    "Not copied",
                    "No copy was written",
                    "See Technical Details and save the report if you need help.",
                )
            }
        }
    }
}
pub(super) struct ItemWords {
    pub verdict: String,
    pub headline: &'static str,
    pub guidance: &'static str,
}
pub(super) fn outcome(kind: MeaningKind, error: Option<ErrorCode>, removed: usize) -> ItemWords {
    let mut value = meaning(kind);
    if kind == MeaningKind::NotPublished
        && let Some(error) = error
    {
        value = failure(error);
    }
    if kind == MeaningKind::PublishedIncomplete
        && let Some(error) = error
    {
        value.guidance = failure(error).guidance;
    }
    if kind == MeaningKind::ExistingMatched && error == Some(ErrorCode::Interrupted) {
        value = failure(ErrorCode::Interrupted);
    }
    let verdict = match kind {
        MeaningKind::Created => format!("Created · {removed} removed"),
        MeaningKind::Prepared => format!("Ready · {removed} to remove"),
        MeaningKind::Comparison
        | MeaningKind::ExistingVerified
        | MeaningKind::ExistingMatched
        | MeaningKind::PublishedIncomplete
        | MeaningKind::PublicationUncertain
        | MeaningKind::NotPublished
        | MeaningKind::Unestablished => value.verdict.into(),
    };
    ItemWords {
        verdict,
        headline: value.headline,
        guidance: value.guidance,
    }
}
pub(super) fn phase(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Queued => "Waiting",
        RunStatus::Checking => "Checking",
        RunStatus::Creating => "Creating copies",
        RunStatus::Completed => "Done",
        RunStatus::CompletedWithFailures => "Finished",
        RunStatus::Stopped => "Stopped",
        RunStatus::StoppedBeforeStart => "Stopped before start",
        RunStatus::ForceStopped => "Force stopped",
        RunStatus::RequestRefused | RunStatus::IdentityFailure | RunStatus::StartFailed => {
            "Couldn’t start"
        }
        RunStatus::Incomplete => "Couldn’t finish",
        RunStatus::StopRequested | RunStatus::ForceStopRequested => "Stopping…",
    }
}
pub(super) fn run_title(
    id: impl std::fmt::Display,
    status: RunStatus,
    mode: RunMode,
    tally: Tally,
    total: Option<u32>,
) -> String {
    let verdict = match status {
        RunStatus::Queued => "Waiting".into(),
        RunStatus::Checking => "Checking…".into(),
        RunStatus::Creating => "Creating copies…".into(),
        RunStatus::Completed if mode == RunMode::DryRun => format!(
            "Done — {} {} checked, no problems",
            tally.checked,
            if tally.checked == 1 {
                "message"
            } else {
                "messages"
            }
        ),
        RunStatus::Completed if tally.created > 0 => format!(
            "Done — {} {} created",
            tally.created,
            if tally.created == 1 { "copy" } else { "copies" }
        ),
        RunStatus::Completed => format!("Done — {} already up to date", tally.up_to_date),
        RunStatus::CompletedWithFailures if tally.attention > 0 => {
            format!("Finished — {}", attention_count(tally.attention))
        }
        RunStatus::CompletedWithFailures => "Couldn’t finish".into(),
        RunStatus::Stopped | RunStatus::StoppedBeforeStart => total.map_or_else(
            || "Stopped before messages were found".into(),
            |total| format!("Stopped — {} of {total} processed", tally.processed),
        ),
        RunStatus::ForceStopped => "Force stopped — report may be incomplete".into(),
        RunStatus::RequestRefused | RunStatus::StartFailed | RunStatus::IdentityFailure => {
            "Couldn’t start".into()
        }
        RunStatus::Incomplete => "Couldn’t finish".into(),
        RunStatus::StopRequested | RunStatus::ForceStopRequested => "Stopping…".into(),
    };
    format!("Run {id} · {verdict}")
}
fn attention_count(count: u32) -> String {
    format!(
        "{count} {} attention",
        if count == 1 { "needs" } else { "need" }
    )
}

pub(super) fn sidebar(
    status: RunStatus,
    mode: RunMode,
    tally: Tally,
    total: Option<u32>,
) -> String {
    match status {
        RunStatus::Queued => "Waiting".into(),
        RunStatus::Checking | RunStatus::Creating => total.map_or_else(
            || "Finding messages…".into(),
            |total| format!("{} of {total}", tally.processed),
        ),
        RunStatus::StopRequested | RunStatus::ForceStopRequested => "Stopping…".into(),
        RunStatus::Stopped | RunStatus::StoppedBeforeStart => total.map_or_else(
            || "Stopped before processing".into(),
            |total| format!("Stopped · {} of {total} processed", tally.processed),
        ),
        RunStatus::ForceStopped => "Force stopped · may be incomplete".into(),
        RunStatus::RequestRefused | RunStatus::IdentityFailure | RunStatus::StartFailed => {
            "Couldn’t start".into()
        }
        RunStatus::Incomplete => "Couldn’t finish".into(),
        RunStatus::Completed | RunStatus::CompletedWithFailures => {
            let good = if mode == RunMode::DryRun {
                format!("{} checked", tally.checked)
            } else if tally.created > 0 {
                format!("{} created", tally.created)
            } else {
                format!("{} up to date", tally.up_to_date)
            };
            if tally.attention > 0 {
                if tally.created + tally.up_to_date + tally.checked == 0 {
                    attention_count(tally.attention)
                } else {
                    format!("{good} · {}", attention_count(tally.attention))
                }
            } else {
                good
            }
        }
    }
}
pub(super) fn estimate(seconds: Option<u64>) -> String {
    match seconds {
        None => "Estimating time…".into(),
        Some(0..=59) => "Less than a minute".into(),
        Some(seconds) => format!(
            "About {} {}",
            seconds.div_ceil(60),
            if seconds.div_ceil(60) == 1 {
                "minute"
            } else {
                "minutes"
            }
        ),
    }
}

pub(super) fn queue_error(error: &eml_app::QueueError) -> String {
    use eml_app::QueueError;
    match error {
        QueueError::AbsoluteSelectionRequired => "Add one or more email files or folders.".into(),
        QueueError::InvalidRequest(error) => {
            format!("Couldn’t add these files or folders: {error}")
        }
        QueueError::RequestEncoding => {
            "Too many or overly long paths. Add fewer files or folders.".into()
        }
        QueueError::Full => format!(
            "{} runs are already waiting. Remove one from the queue to add another.",
            eml_app::MAX_WAITING_REQUESTS
        ),
        QueueError::StoredBytesFull | QueueError::SessionFull => {
            "Close a finished run to add more".into()
        }
        QueueError::MemoryReportsFull => "Waiting for space — close a finished run".into(),
        QueueError::InactiveRun => "This run is no longer active.".into(),
        QueueError::IdentityLimit => {
            "Save the reports you need and restart the app to add more runs.".into()
        }
        QueueError::ShuttingDown => "The app is closing.".into(),
        QueueError::DecisionPending => "Finish or cancel the current decision first.".into(),
        QueueError::DestinationUnavailable => "Choose an existing folder to save copies".into(),
    }
}

pub(super) fn force_notice(id: impl std::fmt::Display) -> String {
    format!(
        "Run {id}\n\nStops this run immediately. Waiting runs continue normally. Completed copies stay in place, but some results may be unknown. Check the destination before continuing.\n\nA temporary file may remain. Inspect any such file only after all runs have finished or stopped."
    )
}

#[cfg(test)]
mod contracts {
    use super::*;
    #[test]
    fn s12_every_known_code_kind_and_status_has_complete_bounded_gui_wording() {
        // Necessary displayed limit copies remain checked against their owners.
        assert_eq!(eml_core::MAX_MESSAGE_BYTES, 256 * 1024 * 1024);
        assert_eq!(eml_core::MAX_COLLECTED_ITEMS, 4096);
        for code in ErrorCode::ALL {
            let words = failure(*code);
            assert!(
                !words.verdict.is_empty() && words.verdict.chars().count() <= 28,
                "{code:?}"
            );
            assert!(!words.headline.is_empty());
            assert!(!words.guidance.is_empty() || *code == ErrorCode::NoInputs);
        }
        let kinds = [
            MeaningKind::Created,
            MeaningKind::ExistingVerified,
            MeaningKind::ExistingMatched,
            MeaningKind::Prepared,
            MeaningKind::Comparison,
            MeaningKind::PublishedIncomplete,
            MeaningKind::PublicationUncertain,
            MeaningKind::NotPublished,
            MeaningKind::Unestablished,
        ];
        // A count of parts cannot exceed the bounded stream bytes that encoded them.
        for kind in kinds {
            for count in [
                0,
                1,
                eml_core::MAX_MESSAGE_PARTS,
                eml_protocol::MAX_RUN_BYTES,
            ] {
                let value = outcome(kind, None, count);
                assert!(
                    !value.verdict.is_empty() && value.verdict.chars().count() <= 28,
                    "{kind:?}, {count}: {}",
                    value.verdict
                );
                assert!(!value.headline.is_empty());
                assert!(!value.guidance.is_empty() || kind == MeaningKind::Created);
            }
        }
        let statuses = [
            RunStatus::Queued,
            RunStatus::Checking,
            RunStatus::Creating,
            RunStatus::Completed,
            RunStatus::CompletedWithFailures,
            RunStatus::Stopped,
            RunStatus::StoppedBeforeStart,
            RunStatus::ForceStopped,
            RunStatus::RequestRefused,
            RunStatus::Incomplete,
            RunStatus::IdentityFailure,
            RunStatus::StartFailed,
            RunStatus::StopRequested,
            RunStatus::ForceStopRequested,
        ];
        for status in statuses {
            assert!(!phase(status).is_empty() && phase(status).chars().count() <= 28);
            assert!(
                run_title(3, status, RunMode::Create, Tally::default(), None)
                    .starts_with("Run 3 · ")
            );
            assert!(!sidebar(status, RunMode::Create, Tally::default(), None).is_empty());
        }
    }
    #[test]
    fn matching_or_uncertain_bytes_never_claim_qualified_success_or_no_publication() {
        assert_ne!(
            outcome(
                MeaningKind::ExistingMatched,
                Some(ErrorCode::ExistingSyncFailed),
                0
            )
            .verdict,
            "Already up to date"
        );
        for kind in [
            MeaningKind::PublicationUncertain,
            MeaningKind::Unestablished,
        ] {
            assert_eq!(
                outcome(kind, Some(ErrorCode::WriteFailed), 0).verdict,
                "Unknown result"
            );
        }
        assert!(
            outcome(
                MeaningKind::PublishedIncomplete,
                Some(ErrorCode::NamespaceSyncFailed),
                1
            )
            .headline
            .starts_with("Copy created")
        );
        assert_eq!(
            outcome(MeaningKind::NotPublished, Some(ErrorCode::Interrupted), 0).verdict,
            "Stopped"
        );
    }
}
