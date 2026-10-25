//! Authored workflow diagnostics retaining the exact native/validation cause.
use crate::{ErrorCode, ErrorPhase};
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

/// Typed operation failure, independent of output, publication or cleanup facts.
/// Construction stays private to business operations; frontends only observe it.
#[derive(Clone, Debug)]
pub struct OperationFailure {
    code: ErrorCode,
    phase: ErrorPhase,
    detail: &'static str,
    cause: Option<Arc<io::Error>>,
}
impl OperationFailure {
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
