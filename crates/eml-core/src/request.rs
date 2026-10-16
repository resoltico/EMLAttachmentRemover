//! Native request values and resource admission before collection or delivery.
use std::path::{Path, PathBuf};

/// Maximum explicit path arguments, counted before deduplication.
pub const MAX_INPUT_PATHS: usize = 4096;
/// Maximum combined native input bytes; output and working paths are separate.
pub const MAX_INPUT_PATH_BYTES: usize = 4 * 1024 * 1024;
/// Maximum bytes on Unix or UTF-16 code units on Windows in one explicit path.
pub const MAX_PATH_UNITS: usize = 32768;
/// Maximum lexical components in one explicit path, excluding its root/prefix.
pub const MAX_PATH_COMPONENTS: usize = 256;

/// Requested placement; no missing destination is created during admission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputLocation {
    /// Use each selected source's bound parent.
    BesideSource,
    /// Use an existing directory, with walked-source layout decided by core.
    Directory(PathBuf),
}

/// Whether execution may publish and synchronize copies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunMode {
    /// Verify existing copies or publish complete prepared copies.
    Create,
    /// Inspect and plan without creating entries or attempting synchronization.
    DryRun,
}

/// Frontend-independent request; all paths retain their original native units.
///
/// The caller captures the absolute working directory once, without subsequently
/// changing it. Validation does not resolve paths or access the filesystem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunRequest {
    /// Explicit selection order, before deduplication.
    pub paths: Vec<PathBuf>,
    /// Absolute base for relative expressions; never applied through `set_current_dir`.
    pub working_directory: PathBuf,
    /// Requested destination placement.
    pub output: OutputLocation,
    /// Creation or read-only planning.
    pub mode: RunMode,
}

/// Which request expression failed native resource admission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestPath {
    /// Zero-based explicit argument index.
    Input(usize),
    /// Explicit output directory.
    OutputDirectory,
    /// Captured working directory.
    WorkingDirectory,
}

/// A native path violates a resource or representation requirement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathViolation {
    /// Native NUL cannot be passed to filesystem APIs.
    EmbeddedNul,
    /// The native path exceeds the per-expression unit bound.
    TooManyUnits,
    /// The raw expression exceeds the lexical component bound.
    TooManyComponents,
}

/// Pure request-path representation/resource failure, without filesystem I/O.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestPathError {
    /// Argument count exceeds the request bound, even when arguments repeat.
    TooManyInputs,
    /// Combined native input bytes exceed the request bound.
    InputPathsTooLarge,
    /// An explicit path violates its native admission requirements.
    InvalidPath {
        /// Location in the request, without lossy path text.
        path: RequestPath,
        /// Specific rejected property.
        violation: PathViolation,
    },
    /// Relative expressions must have a captured absolute base.
    WorkingDirectoryNotAbsolute,
}
impl std::fmt::Display for RequestPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyInputs => write!(f, "Request exceeds {MAX_INPUT_PATHS} input paths"),
            Self::InputPathsTooLarge => {
                write!(f, "Input paths exceed {MAX_INPUT_PATH_BYTES} native bytes")
            }
            Self::InvalidPath { path, violation } => write!(f, "{path:?}: {violation:?}"),
            Self::WorkingDirectoryNotAbsolute => {
                f.write_str("Captured working directory must be absolute")
            }
        }
    }
}
impl std::error::Error for RequestPathError {}

impl RunRequest {
    /// Check native representation and request resource limits without I/O.
    ///
    /// Equality at each bound is allowed. Empty selections and empty input
    /// expressions are retained for collection/root admission. Output-directory
    /// existence and usable source bindings still need filesystem admission.
    /// Limits apply to raw spelling: dot/dot-dot components count and are not
    /// collapsed. Repeated separators do not create additional components.
    pub fn validate_path_limits(&self) -> Result<(), RequestPathError> {
        if self.paths.len() > MAX_INPUT_PATHS {
            return Err(RequestPathError::TooManyInputs);
        }
        validate_path(&self.working_directory, RequestPath::WorkingDirectory)?;
        if !self.working_directory.is_absolute() {
            return Err(RequestPathError::WorkingDirectoryNotAbsolute);
        }
        if let OutputLocation::Directory(path) = &self.output {
            validate_path(path, RequestPath::OutputDirectory)?;
        }
        let mut total = 0usize;
        for (index, path) in self.paths.iter().enumerate() {
            let bytes = validate_path(path, RequestPath::Input(index))?;
            total = total
                .checked_add(bytes)
                .filter(|bytes| *bytes <= MAX_INPUT_PATH_BYTES)
                .ok_or(RequestPathError::InputPathsTooLarge)?;
        }
        Ok(())
    }
}

fn validate_path(path: &Path, location: RequestPath) -> Result<usize, RequestPathError> {
    path_limits(path).map_err(|violation| RequestPathError::InvalidPath {
        path: location,
        violation,
    })
}
pub(crate) fn native_bytes(path: &Path) -> usize {
    #[cfg(unix)]
    {
        path.as_os_str().as_encoded_bytes().len()
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str().encode_wide().count() * 2
    }
}
pub(crate) fn path_limits(path: &Path) -> Result<usize, PathViolation> {
    #[cfg(unix)]
    let (units, prefix_units, bytes_per_unit) = {
        use std::os::unix::ffi::OsStrExt;
        (
            path.as_os_str().as_bytes().iter().copied().map(u16::from),
            0,
            1,
        )
    };
    #[cfg(windows)]
    let (units, prefix_units, bytes_per_unit) = {
        use std::os::windows::ffi::OsStrExt;
        let prefix = match path.components().next() {
            Some(std::path::Component::Prefix(prefix)) => prefix.as_os_str().encode_wide().count(),
            _ => 0,
        };
        (path.as_os_str().encode_wide(), prefix, 2)
    };
    let mut length = 0usize;
    let mut components = 0usize;
    let mut in_component = false;
    for unit in units {
        length += 1;
        if length > MAX_PATH_UNITS {
            return Err(PathViolation::TooManyUnits);
        }
        if unit == 0 {
            return Err(PathViolation::EmbeddedNul);
        }
        if length <= prefix_units {
            continue;
        }
        let separator = unit == u16::from(b'/') || (cfg!(windows) && unit == u16::from(b'\\'));
        if separator {
            in_component = false;
        } else if !in_component {
            components += 1;
            if components > MAX_PATH_COMPONENTS {
                return Err(PathViolation::TooManyComponents);
            }
            in_component = true;
        }
    }
    // The unit limit above makes this multiplication bounded on every host.
    Ok(length * bytes_per_unit)
}
