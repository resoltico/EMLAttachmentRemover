//! Captured native selection operations. No source data or namespace writes.
use eml_fs::{BoundDirectory, EntryKind, EntryObservation};
use std::{
    borrow::Cow,
    io,
    path::{Path, PathBuf},
};

pub(super) enum SelectionError {
    Io(io::Error),
    #[cfg(windows)]
    Changed,
}
impl From<io::Error> for SelectionError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
pub(super) struct WorkingContext {
    cwd: BoundDirectory,
    #[cfg(windows)]
    paths: eml_fs::PathContext,
}
impl WorkingContext {
    pub(super) fn capture(path: &Path) -> io::Result<Self> {
        #[cfg(unix)]
        {
            Ok(Self {
                cwd: BoundDirectory::open(path)?,
            })
        }
        #[cfg(windows)]
        {
            let paths = eml_fs::PathContext::capture(path)?;
            let cwd = BoundDirectory::open(&paths.resolve(path)?)?;
            Ok(Self { cwd, paths })
        }
    }
    pub(super) fn operand(&self, root: &Path, suffix: &Path) -> io::Result<PathBuf> {
        if root.as_os_str().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Empty selection does not denote a native path",
            ));
        }
        #[cfg(unix)]
        {
            Ok(append(root, suffix))
        }
        #[cfg(windows)]
        {
            let complete = append(&self.paths.resolve(root)?, suffix);
            // The result is already extended: this branch validates/preserves
            // the complete literal operand, without ordinary Win32 renormalization.
            self.paths.resolve(&complete)
        }
    }
    pub(super) fn key<'a>(&self, native: &'a Path) -> Cow<'a, Path> {
        #[cfg(unix)]
        {
            if native.is_absolute() {
                Cow::Borrowed(native)
            } else {
                Cow::Owned(self.cwd.requested_path().join(native))
            }
        }
        #[cfg(windows)]
        {
            Cow::Borrowed(native)
        }
    }
    pub(super) fn same_spelling(&self, left: &Path, right: &Path) -> bool {
        self.key(left).as_os_str() == self.key(right).as_os_str()
    }
    pub(super) fn observe(&self, expression: &Path) -> Result<EntryObservation, SelectionError> {
        #[cfg(unix)]
        {
            Ok(self.cwd.observe_selected(expression)?)
        }
        #[cfg(windows)]
        {
            Ok(eml_fs::observe_selected(&self.paths.resolve(expression)?)?)
        }
    }
    pub(super) fn directory(&self, expression: &Path) -> Result<BoundDirectory, SelectionError> {
        #[cfg(unix)]
        {
            Ok(self.cwd.open_selected(expression)?)
        }
        #[cfg(windows)]
        {
            let directory = BoundDirectory::open(&self.paths.resolve(expression)?)?;
            let resolved = directory.resolved_path()?;
            let confirmed = BoundDirectory::open(&resolved)?;
            if confirmed.identity() != directory.identity() {
                return Err(SelectionError::Changed);
            }
            Ok(directory)
        }
    }
    pub(super) fn parent_identity(
        &self,
        expression: &Path,
    ) -> Result<eml_fs::Identity, SelectionError> {
        let parent = expression
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let observed = self.observe(parent)?;
        if observed.kind != EntryKind::Directory {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                "Selected source parent is not an ordinary directory",
            )
            .into());
        }
        Ok(observed.identity)
    }
    pub(super) fn check_working_address(&self) -> io::Result<bool> {
        #[cfg(unix)]
        {
            Ok(true)
        } // Relative selection itself stays anchored to the held descriptor.
        #[cfg(windows)]
        {
            let current =
                BoundDirectory::open(&self.paths.resolve(self.paths.working_directory())?)?;
            Ok(current.identity() == self.cwd.identity())
        }
    }
}

/// Append discovered native names without changing the admitted root spelling.
pub(super) fn append(root: &Path, suffix: &Path) -> PathBuf {
    if suffix.as_os_str().is_empty() {
        return root.to_owned();
    }
    let bytes = root.as_os_str().as_encoded_bytes();
    let terminal_separator = bytes
        .last()
        .is_some_and(|byte| *byte == b'/' || (cfg!(windows) && *byte == b'\\'));
    let bare_drive =
        cfg!(windows) && bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    let mut joined = root.as_os_str().to_owned();
    if !bytes.is_empty() && !terminal_separator && !bare_drive {
        joined.push(std::path::MAIN_SEPARATOR_STR);
    }
    joined.push(suffix);
    joined.into()
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    #[test]
    fn complete_literal_operand_uses_backend_limit_and_preserves_name_content() {
        let fixture = tempfile::tempdir().unwrap();
        let context = WorkingContext::capture(fixture.path()).unwrap();
        // Pure operational boundary, not an actual filesystem component claim.
        let base = format!(r"\\?\C:\{}", "x".repeat(32_757));
        let exact = context.operand(Path::new(&base), Path::new("x")).unwrap();
        assert_eq!(exact.as_os_str().encode_wide().count(), 32_766);
        assert!(context.operand(Path::new(&base), Path::new("xx")).is_err());
        let literal = context
            .operand(Path::new(r"\\?\C:\owned"), Path::new(r"folder. \a.eml"))
            .unwrap();
        assert_eq!(
            literal.as_os_str(),
            OsStr::new(r"\\?\C:\owned\folder. \a.eml")
        );
        assert!(
            context
                .operand(Path::new(r"\\?\C:\owned"), Path::new(r"sub\..\escape"))
                .is_err()
        );
        assert!(
            context
                .operand(Path::new(r"\\?\C:\owned"), Path::new("NUL.eml"))
                .is_err()
        );
        drop(context);
        fixture.close().unwrap();
    }
}
