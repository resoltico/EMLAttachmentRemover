//! Toolkit-independent observed child lifecycle facts.
/// A desktop lifecycle/transport failure; Core's per-item outcomes stay separate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerFailure {
    /// The exact bundled executable could not be started or set up.
    Launch,
    /// Exact private worker version/compiled CPU/OS did not match.
    Identity,
    /// A pipe, framing, decoder or owned-thread operation failed.
    Transport,
    /// Native ownership or complete reader/writer settlement is unverified.
    Unverified,
}
impl std::fmt::Display for WorkerFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Desktop worker {self:?}")
    }
}
impl std::error::Error for WorkerFailure {}
/// Actual completed lifecycle observations. A valid Core failure is not a broken
/// transport, and Stop intent never changes a received terminal item/summary.
#[derive(Clone, Debug, serde::Serialize)]
pub struct WorkerCompletion {
    /// Exact operation whose process and transport were observed.
    pub operation_id: eml_protocol::desktop::OperationId,
    /// Child's actual normal exit; absent if no operation child or signalled.
    pub exit_code: Option<i32>,
    /// Child's actual terminating signal.
    pub signal: Option<i32>,
    /// Body feeding was granted; some bytes may have started delivery.
    pub input_may_have_been_sent: bool,
    /// Actual process/reader/writer/control settlement was observed.
    pub settled: bool,
    /// A later queued request can safely be admitted after final UI application.
    pub reusable: bool,
    /// First lifecycle failure, separate from Core's typed item errors.
    pub failure: Option<WorkerFailure>,
    /// Bounded authored/native diagnostics, excluding raw stdout/message bytes.
    pub faults: Vec<String>,
    /// At most 64 KiB of separately drained stderr for explicit failure inspection.
    pub stderr: Vec<u8>,
    /// Retained stderr reached its cap; the reader still drained to EOF.
    pub stderr_truncated: bool,
    /// Actual successful native interrupt calls, not inferred handler delivery.
    pub interrupt_calls_succeeded: u8,
    /// The private first-interrupt observation byte was actually received.
    pub first_interrupt_observed: bool,
}

/// Shared terminal run classification derived from accepted facts and actual ownership.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunMeaning {
    /// No helper operation could be launched.
    LaunchFailed,
    /// Exact product/target/protocol identity did not match.
    IdentityMismatch,
    /// Accepted facts coexist with a later transport failure.
    TransportFailed,
    /// Actual process or endpoint settlement remains unverified.
    OwnershipUnverified,
    /// The operation was stopped before body feeding began.
    NotStarted,
    /// A real interrupted process or summary was observed.
    Stopped,
    /// Complete accepted success facts and matching clean actual completion.
    Completed,
    /// Complete accepted terminal facts contain errors or incomplete collection.
    CompletedWithErrors,
    /// Complete terminal facts retain usable results alongside errors.
    CompletedWithMixedResults,
    /// A complete consistent terminal report cannot be established.
    ResultsIncomplete,
}
impl RunMeaning {
    /// One application-owned explanation for desktop rendering and report inspection.
    pub fn label(self) -> &'static str {
        match self {
            Self::LaunchFailed => {
                "The bundled worker could not start. Reinstall the app and try again."
            }
            Self::IdentityMismatch => {
                "The bundled worker identity differs. Reinstall the app before processing."
            }
            Self::TransportFailed => {
                "Worker results could not be fully verified. Earlier observations are retained."
            }
            Self::OwnershipUnverified => {
                "Processing could not be fully verified. Further processing is closed."
            }
            Self::NotStarted => "Stopped before processing.",
            Self::Stopped => "Stopped. Completed results are retained.",
            Self::Completed => "Finished.",
            Self::CompletedWithErrors | Self::CompletedWithMixedResults => {
                "Finished with errors. Originals are unchanged; inspect the results."
            }
            Self::ResultsIncomplete => {
                "Processing results are incomplete. Earlier observations are retained; complete copies may exist."
            }
        }
    }
}
pub(crate) fn classify_run(
    summary: Option<&eml_protocol::records::SummaryRecord>,
    protocol_failure: Option<crate::ProtocolFailure>,
    completion: &WorkerCompletion,
) -> RunMeaning {
    if !completion.settled {
        return RunMeaning::OwnershipUnverified;
    }
    if let Some(failure) = completion.failure {
        return match failure {
            WorkerFailure::Launch => RunMeaning::LaunchFailed,
            WorkerFailure::Identity => RunMeaning::IdentityMismatch,
            WorkerFailure::Transport => RunMeaning::TransportFailed,
            WorkerFailure::Unverified => RunMeaning::OwnershipUnverified,
        };
    }
    if completion.signal.is_some()
        || completion.exit_code == Some(130)
        || summary.is_some_and(|s| s.interrupted)
    {
        return RunMeaning::Stopped;
    }
    if !completion.input_may_have_been_sent && completion.exit_code.is_none() {
        return RunMeaning::NotStarted;
    }
    if protocol_failure.is_some() {
        return RunMeaning::ResultsIncomplete;
    }
    let Some(summary) = summary else {
        return RunMeaning::ResultsIncomplete;
    };
    if completion.exit_code != Some(summary.exit_code) {
        return RunMeaning::ResultsIncomplete;
    }
    if summary.exit_code == 0
        && summary.counts.failed == 0
        && summary.counts.not_processed == 0
        && summary.run_error.value().is_none()
        && summary.collection_complete
    {
        RunMeaning::Completed
    } else {
        if summary.counts.created > 0 || summary.counts.up_to_date > 0 {
            RunMeaning::CompletedWithMixedResults
        } else {
            RunMeaning::CompletedWithErrors
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn worker() -> WorkerCompletion {
        WorkerCompletion {
            operation_id: eml_protocol::desktop::OperationId::new(1).unwrap(),
            exit_code: Some(0),
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
    fn summary() -> eml_protocol::records::SummaryRecord {
        use eml_protocol::records::{CountRecord, Nullable, SummaryRecord};
        SummaryRecord {
            counts: CountRecord {
                created: 1,
                up_to_date: 0,
                planned: 0,
                failed: 0,
                not_processed: 0,
            },
            interrupted: false,
            collection_complete: true,
            run_error: Nullable::Null(()),
            exit_code: 0,
        }
    }
    #[test]
    fn terminal_matrix_requires_accepted_summary_and_actual_clean_matching_completion() {
        let good = summary();
        let mut c = worker();
        assert_eq!(classify_run(Some(&good), None, &c), RunMeaning::Completed);
        assert_eq!(classify_run(None, None, &c), RunMeaning::ResultsIncomplete);
        assert_eq!(
            classify_run(
                Some(&good),
                Some(crate::ProtocolFailure::MalformedRecord),
                &c
            ),
            RunMeaning::ResultsIncomplete
        );
        c.signal = Some(9);
        c.exit_code = None;
        assert_eq!(classify_run(Some(&good), None, &c), RunMeaning::Stopped);
        c = worker();
        c.exit_code = Some(130);
        assert_eq!(
            classify_run(None, Some(crate::ProtocolFailure::MissingSummary), &c),
            RunMeaning::Stopped
        );
        c = worker();
        c.input_may_have_been_sent = false;
        c.exit_code = None;
        assert_eq!(classify_run(None, None, &c), RunMeaning::NotStarted);
        for (failure, expected) in [
            (WorkerFailure::Launch, RunMeaning::LaunchFailed),
            (WorkerFailure::Identity, RunMeaning::IdentityMismatch),
            (WorkerFailure::Transport, RunMeaning::TransportFailed),
            (WorkerFailure::Unverified, RunMeaning::OwnershipUnverified),
        ] {
            c = worker();
            c.failure = Some(failure);
            assert_eq!(classify_run(Some(&good), None, &c), expected);
        }
        c = worker();
        c.settled = false;
        assert_eq!(
            classify_run(Some(&good), None, &c),
            RunMeaning::OwnershipUnverified
        );
        c = worker();
        c.exit_code = Some(1);
        let mut errors = good;
        errors.exit_code = 1;
        errors.counts.failed = 1;
        assert_eq!(
            classify_run(Some(&errors), None, &c),
            RunMeaning::CompletedWithMixedResults
        );
        errors.counts.created = 0;
        assert_eq!(
            classify_run(Some(&errors), None, &c),
            RunMeaning::CompletedWithErrors
        );
    }
}
