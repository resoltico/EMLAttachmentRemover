//! Authored workflow diagnostics retaining the exact native/validation cause.
use crate::{ErrorCode, ErrorPhase, Refusal};
use std::path::PathBuf;
use std::{io, sync::Arc};

/// Numeric native context for bounded frontend diagnostics; never a semantic code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeDiagnostic {
    /// Raw host OS error number.
    OsCode(i32),
    /// Exact primary NTSTATUS and secondary Win32 translation.
    #[cfg(windows)]
    NtStatus {
        /// Signed completed native status.
        status: i32,
        /// Secondary translation, never the primary status.
        win32: u32,
    },
}
impl std::fmt::Display for NativeDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OsCode(code) => write!(f, "OS error {code}"),
            #[cfg(windows)]
            Self::NtStatus { status, win32 } => {
                write!(f, "NTSTATUS 0x{:08X}; Win32 {win32}", *status as u32)
            }
        }
    }
}

#[derive(Clone, Debug)]
enum FailureContext {
    Operation,
    SelectionLimit,
    Message(Refusal),
    SaveFolder(PathBuf),
    WorkingFolder(PathBuf),
}

/// Typed operation failure, independent of output, publication or cleanup facts.
/// Construction stays private to business operations; frontends only observe it.
#[derive(Clone, Debug)]
pub struct OperationFailure {
    code: ErrorCode,
    phase: ErrorPhase,
    detail: &'static str,
    context: FailureContext,
    cause: Option<Arc<io::Error>>,
}
impl OperationFailure {
    /// A frontend event/report adapter failed. This grants no publication facts
    /// and cannot erase receipts already observed before delivery ended.
    pub fn delivery(error: impl Into<Arc<io::Error>>) -> Self {
        Self {
            code: ErrorCode::OutputFailed,
            phase: ErrorPhase::Delivery,
            detail: "Result delivery did not complete.",
            context: FailureContext::Operation,
            cause: Some(error.into()),
        }
    }
    /// Plain core-owned guidance, preserving the facts responsible for failure.
    pub fn explanation(&self) -> String {
        use crate::vocabulary::{RunFailure, output_folder_unavailable};
        match &self.context {
            FailureContext::SaveFolder(path) if self.code == ErrorCode::OutputOverlap => format!(
                "The save folder is inside a folder you added: {}. Choose a different save folder.",
                crate::display::DisplayPath::new(path)
            ),
            FailureContext::SaveFolder(path)
                if self
                    .native_error()
                    .is_some_and(|e| e.kind() == io::ErrorKind::NotADirectory) =>
            {
                format!(
                    "The save folder is a file, not a folder: {}. Choose a folder.",
                    crate::display::DisplayPath::new(path)
                )
            }
            FailureContext::SaveFolder(path) => output_folder_unavailable(path),
            FailureContext::WorkingFolder(path) => format!(
                "The working folder isn't available: {}. Choose an existing folder and try again.",
                crate::display::DisplayPath::new(path)
            ),
            FailureContext::Message(Refusal::NotEmail) => {
                "This file isn't an email. It may be empty or a different kind of file.".into()
            }
            FailureContext::Message(Refusal::Malformed) => {
                "This email is damaged or incomplete. Export it again from your mail app.".into()
            }
            FailureContext::Message(Refusal::MessageTooLarge) => {
                "This email exceeds the size limit (256 MiB maximum).".into()
            }
            FailureContext::Message(Refusal::LimitExceeded) => {
                "This email is too deeply nested or has too many parts.".into()
            }
            FailureContext::SelectionLimit => format!(
                "Select at most {} emails at a time; split large folders.",
                crate::vocabulary::format_count(crate::MAX_COLLECTED_ITEMS)
            ),
            _ => {
                if (matches!(self.code, ErrorCode::WriteFailed | ErrorCode::StageFailed)
                    || self.phase == ErrorPhase::Stage)
                    && let Some(sentence) =
                        self.native_error().and_then(crate::vocabulary::write_cause)
                {
                    return sentence.into();
                }
                if let Some(error) = self
                    .native_error()
                    .and_then(|e| e.get_ref())
                    .and_then(|e| e.downcast_ref::<crate::RequestPathError>())
                {
                    return error.to_string();
                }
                RunFailure(self.code).explanation().into()
            }
        }
    }
    /// Short human label selected from the same typed context as the explanation.
    pub fn label(&self) -> &'static str {
        match self.context {
            FailureContext::Message(Refusal::NotEmail) => "Not an email",
            FailureContext::Message(Refusal::Malformed) => "Damaged email",
            _ => crate::vocabulary::Outcome::Failed(self.code).label(),
        }
    }
    /// Whether an explicit work-item count (rather than a path/depth bound) was exceeded.
    pub fn is_selection_limit(&self) -> bool {
        matches!(self.context, FailureContext::SelectionLimit)
    }
    pub(super) fn mark_selection_limit(&mut self) {
        self.context = FailureContext::SelectionLimit;
    }
    pub(super) fn for_message(mut self, refusal: Refusal) -> Self {
        self.context = FailureContext::Message(refusal);
        self
    }
    pub(super) fn for_save_folder(mut self, path: PathBuf) -> Self {
        self.context = FailureContext::SaveFolder(path);
        self
    }
    pub(super) fn for_working_folder(mut self, path: PathBuf) -> Self {
        self.context = FailureContext::WorkingFolder(path);
        self
    }
    /// The save-folder admission fact, not an inference from a generic binding code.
    pub fn is_save_folder_unavailable(&self) -> bool {
        matches!(self.context, FailureContext::SaveFolder(_)) && self.code == ErrorCode::BindFailed
    }
    /// A refusal verdict, rather than a misleading partial inventory count.
    pub fn refusal_headline(&self) -> Option<String> {
        (self.code == ErrorCode::InputLimitExceeded).then(|| {
            if self.is_selection_limit() {
                crate::vocabulary::too_many_emails()
            } else {
                "Selection too large".into()
            }
        })
    }
    /// Core diagnostic fields for pre-stream refusals; native errors remain numeric.
    pub fn diagnostic(&self) -> String {
        let native = self
            .native_diagnostic()
            .map_or_else(String::new, |n| format!(" ({n})"));
        format!(
            "{}\n[{}: {} · {}: {} · {}: {}{native}]",
            self.explanation(),
            crate::vocabulary::fields::CODE,
            self.code,
            crate::vocabulary::fields::PHASE,
            self.phase,
            crate::vocabulary::fields::DETAIL,
            self.detail
        )
    }
    /// Exact numeric cause without unbounded/localized io::Error text or payload.
    pub fn native_diagnostic(&self) -> Option<NativeDiagnostic> {
        let error = self.native_error()?;
        error.raw_os_error().map(NativeDiagnostic::OsCode)
    }
    pub(super) fn with_context(
        mut self,
        code: ErrorCode,
        phase: ErrorPhase,
        detail: &'static str,
    ) -> Self {
        self.code = code;
        self.phase = phase;
        self.detail = detail;
        self
    }
    /// Stable semantic code, never inferred from native error prose.
    pub fn code(&self) -> ErrorCode {
        self.code
    }
    /// Operation that actually failed.
    pub fn phase(&self) -> ErrorPhase {
        self.phase
    }
    /// Authored context without payload or a lossy path authority.
    pub fn detail(&self) -> &'static str {
        self.detail
    }
    /// Exact native/validation error, including structured native status payloads.
    pub fn native_error(&self) -> Option<&io::Error> {
        self.cause.as_deref()
    }
    pub(super) fn new(code: ErrorCode, phase: ErrorPhase, detail: &'static str) -> Self {
        Self {
            code,
            phase,
            detail,
            context: FailureContext::Operation,
            cause: None,
        }
    }
    pub(super) fn io(
        code: ErrorCode,
        phase: ErrorPhase,
        detail: &'static str,
        error: io::Error,
    ) -> Self {
        Self {
            code,
            phase,
            detail,
            context: FailureContext::Operation,
            cause: Some(Arc::new(error)),
        }
    }
}
impl std::fmt::Display for OperationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.detail)?;
        if let Some(error) = &self.cause {
            write!(f, ": {error}")?;
        }
        Ok(())
    }
}
impl std::error::Error for OperationFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause.as_deref().map(|error| error as _)
    }
}

#[derive(Debug)]
struct ReportCleanupFailure {
    primary: io::Error,
    path: PathBuf,
    cleanup: io::Error,
}
impl std::fmt::Display for ReportCleanupFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} The private report staging file could not be removed: {}.",
            crate::vocabulary::interface_io_error(
                crate::vocabulary::InterfaceIo::SaveReport,
                &self.primary
            ),
            crate::display::DisplayPath::new(&self.path)
        )?;
        if let Some(code) = self.cleanup.raw_os_error() {
            write!(f, " [Cleanup: {}]", NativeDiagnostic::OsCode(code))?;
        }
        Ok(())
    }
}
impl std::error::Error for ReportCleanupFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.primary)
    }
}
/// Preserve primary/cleanup native errors while core owns the diagnostic wording.
pub fn report_cleanup_failure(primary: io::Error, path: PathBuf, cleanup: io::Error) -> io::Error {
    io::Error::new(
        primary.kind(),
        ReportCleanupFailure {
            primary,
            path,
            cleanup,
        },
    )
}
pub(super) fn report_cleanup_details(error: &io::Error) -> Option<String> {
    error
        .get_ref()?
        .downcast_ref::<ReportCleanupFailure>()
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn y6_write_explanations_follow_typed_native_causes() {
        #[cfg(unix)]
        let cases = [
            (
                rustix::io::Errno::NOSPC.raw_os_error(),
                "The save folder's disk is full.",
            ),
            (
                rustix::io::Errno::ROFS.raw_os_error(),
                "The save folder is on a read-only disk.",
            ),
            (
                rustix::io::Errno::ACCESS.raw_os_error(),
                "You don't have permission to save in this folder.",
            ),
            (
                rustix::io::Errno::DQUOT.raw_os_error(),
                "The save folder's disk is full.",
            ),
        ];
        #[cfg(windows)]
        let cases = [
            (112, "The save folder's disk is full."),
            (19, "The save folder is on a read-only disk."),
            (5, "You don't have permission to save in this folder."),
            (39, "The save folder's disk is full."),
        ];
        for (code, expected) in cases {
            let failure = OperationFailure::io(
                ErrorCode::WriteFailed,
                ErrorPhase::Write,
                "The copy could not be written",
                io::Error::from_raw_os_error(code),
            );
            assert_eq!(failure.explanation(), expected);
            assert_eq!(
                failure.native_diagnostic(),
                Some(NativeDiagnostic::OsCode(code))
            );
        }
    }
}
