//! Pure exact-address report drafts; no filesystem observation occurs here.
use std::path::{Path, PathBuf};
/// Shared no-replace explanation rendered by desktop views.
pub const NO_REPLACE_EXPLANATION: &str = eml_core::meaning::NO_REPLACEMENT;
/// Shared explanation for disabled report dismissal.
pub const EXPORT_RESERVATION_EXPLANATION: &str =
    "This report is reserved until its export child and transport have settled.";
#[derive(Clone, Debug)]
/// Immutable lexical folder and filename selection.
pub struct ExportDraft {
    folder: PathBuf,
    name: String,
    destination: PathBuf,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// The pure report address contract refused a draft.
pub struct InvalidExportDraft;
impl std::fmt::Display for InvalidExportDraft {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Choose an absolute folder and one filename ending in .json, without separators, NUL, dot or dot-dot")
    }
}
impl std::error::Error for InvalidExportDraft {}
impl ExportDraft {
    /// Validate and capture one exact address without touching its filesystem.
    pub fn new(folder: PathBuf, name: String) -> Result<Self, InvalidExportDraft> {
        if name.is_empty()
            || name.contains(['/', '\\', '\0'])
            || name == "."
            || name == ".."
            || !name.ends_with(".json")
        {
            return Err(InvalidExportDraft);
        }
        let destination = folder.join(&name);
        validate_destination(&destination)?;
        Ok(Self {
            folder,
            name,
            destination,
        })
    }
    /// Captured absolute export folder.
    pub fn folder(&self) -> &Path {
        &self.folder
    }
    /// Captured single-component report name.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Exact requested address, unchanged by occupancy or publication.
    pub fn destination(&self) -> &Path {
        &self.destination
    }
}
/// Validate the same pure native address contract in parent and child.
pub fn validate_destination(destination: &Path) -> Result<(), InvalidExportDraft> {
    if !destination.is_absolute()
        || destination.file_name().is_none()
        || destination.extension() != Some(std::ffi::OsStr::new("json"))
    {
        return Err(InvalidExportDraft);
    }
    #[cfg(unix)]
    {
        let name = destination.file_name().ok_or(InvalidExportDraft)?;
        use std::os::unix::ffi::OsStrExt;
        if name.as_bytes().contains(&b'\\') {
            return Err(InvalidExportDraft);
        }
    }
    let request = eml_core::RunRequest {
        paths: vec![destination.to_owned()],
        working_directory: destination
            .ancestors()
            .last()
            .ok_or(InvalidExportDraft)?
            .to_owned(),
        output: eml_core::OutputLocation::BesideSource,
        mode: eml_core::RunMode::DryRun,
    };
    request
        .validate_path_limits()
        .map_err(|_| InvalidExportDraft)
}
