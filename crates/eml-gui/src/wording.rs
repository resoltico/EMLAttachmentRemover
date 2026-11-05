//! GUI-owned run verdicts; CLI vocabulary remains independent.
use crate::state::Tally;
use eml_core::{RunMode, vocabulary::RunStatus};

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
        RunStatus::Completed if mode == RunMode::DryRun => {
            format!("Done — {} messages checked, no problems", tally.checked)
        }
        RunStatus::Completed if tally.created > 0 => {
            format!("Done — {} copies created", tally.created)
        }
        RunStatus::Completed => format!("Done — {} already up to date", tally.up_to_date),
        RunStatus::CompletedWithFailures if tally.attention > 0 => {
            format!("Finished — {} need attention", tally.attention)
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
pub(super) fn estimate(seconds: Option<u64>) -> String {
    match seconds {
        None => "Estimating time…".into(),
        Some(0..=59) => "Less than a minute".into(),
        Some(seconds) => format!("About {} minutes", seconds.div_ceil(60)),
    }
}
