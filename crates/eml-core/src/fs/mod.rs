//! Held filesystem objects and no-replace publication under the documented threat model.
mod observation;
mod receipts;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "Win32 handle APIs require a bounded native boundary"
)]
mod windows;
pub use observation::{BoundFileError, EntryKind, EntryObservation, FileSignature};
pub use receipts::{
    PublicationState, StageFailure, StageOperation, SyncMethod, SyncReceipt, SyncState,
};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
#[cfg(unix)]
use unix as backend;
#[cfg(windows)]
use windows as backend;
#[cfg(windows)]
pub use windows::path::{PathContext, validate_component};

/// Native object identity, independent of its pathname.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Identity {
    /// POSIX device and inode.
    Posix {
        /// Device identifier.
        device: u64,
        /// Inode number.
        inode: u64,
    },
    /// Windows volume and full file identifier.
    Windows {
        /// Volume serial.
        volume: u64,
        /// File identifier.
        file: [u8; 16],
    },
}
/// Result of removing resources created by this operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CleanupReceipt {
    /// Observed cleanup state.
    pub state: CleanupState,
    /// Remaining owned basename, when known.
    pub residual_name: Option<OsString>,
    /// Native cleanup failure.
    pub native_code: Option<i64>,
}
/// Cleanup is explicit; an unknown result is never success.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupState {
    /// No resource needed cleanup.
    NotNeeded,
    /// Owned resource was removed.
    Complete,
    /// A known owned resource remains.
    Residual,
    /// Cleanup could not be confirmed.
    Unknown,
}
fn cleanup_state(state: CleanupState) -> CleanupReceipt {
    CleanupReceipt {
        state,
        residual_name: None,
        native_code: None,
    }
}
/// Failed creation with an independently observed cleanup result.
#[derive(Debug)]
pub struct CreationFailure {
    /// Primary failure.
    pub error: io::Error,
    /// Owned-resource cleanup.
    pub cleanup: CleanupReceipt,
}
impl CreationFailure {
    /// A definite occupied name without an unresolved owned resource.
    pub fn is_name_collision(&self) -> bool {
        self.error.kind() == io::ErrorKind::AlreadyExists
            && matches!(
                self.cleanup.state,
                CleanupState::NotNeeded | CleanupState::Complete
            )
    }
}
impl std::fmt::Display for CreationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}; cleanup {:?}", self.error, self.cleanup.state)
    }
}
impl std::error::Error for CreationFailure {}

/// One held directory and the requested address used to reach it.
#[derive(Clone)]
pub struct BoundDirectory {
    inner: Arc<backend::Directory>,
    path: PathBuf,
    identity: Identity,
}
impl BoundDirectory {
    fn bind(path: PathBuf, parent: Option<&Self>, follow: bool) -> io::Result<Self> {
        let inner = backend::open_directory(parent.map(|p| &*p.inner), &path, follow)?;
        let identity = backend::directory_identity(&inner)?;
        let path = parent
            .filter(|_| !path.is_absolute())
            .map_or_else(|| path.clone(), |p| p.path.join(&path));
        Ok(Self {
            inner: Arc::new(inner),
            path,
            identity,
        })
    }
    /// Open an explicitly selected absolute directory.
    pub fn open(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Directory binding requires an absolute path",
            ));
        }
        Self::bind(path.to_owned(), None, true)
    }
    /// Resolve an explicit expression through this captured directory.
    pub fn open_selected(&self, expression: &Path) -> io::Result<Self> {
        Self::bind(expression.to_owned(), Some(self), true)
    }
    /// Open a selected destination without following its final directory link.
    pub fn open_destination(&self, expression: &Path) -> io::Result<Self> {
        // Preserve native components while removing a terminal separator that
        // could otherwise make O_NOFOLLOW apply beyond the selected link.
        let operation: PathBuf = expression.components().collect();
        Self::bind(operation, Some(self), false)
    }
    /// Open an ordinary child directory without following its final link.
    pub fn open_child(&self, name: &OsStr) -> io::Result<Self> {
        component(name)?;
        Self::bind(name.into(), Some(self), false)
    }
    /// Identity of the held directory.
    pub fn identity(&self) -> Identity {
        self.identity
    }
    /// Original requested address, without a current-address guarantee.
    pub fn requested_path(&self) -> &Path {
        &self.path
    }
    /// Check the held object's type and identity before mutation.
    pub fn check_binding(&self) -> io::Result<()> {
        if backend::directory_identity(&self.inner)? == self.identity {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Held directory identity changed",
            ))
        }
    }
    /// Compare the requested address with this held object.
    pub fn check_requested_address(&self) -> io::Result<bool> {
        Ok(Self::open(&self.path)?.identity == self.identity)
    }
    /// Filesystem component limit, in native units.
    pub fn name_limit(&self) -> io::Result<usize> {
        backend::name_limit(&self.inner)
    }
    /// Synchronize this directory's namespace.
    pub fn sync_namespace(&self) -> SyncReceipt {
        sync_receipt(&backend::sync_directory(&self.inner))
    }
    /// Stream native directory names; collection owns the entry bound.
    pub fn read_entries(&self) -> io::Result<DirectoryEntries> {
        Ok(DirectoryEntries(backend::entries(&self.inner)?))
    }
    /// Observe an explicitly selected entry, following its final link.
    pub fn observe_selected(&self, expression: &Path) -> io::Result<EntryObservation> {
        backend::observe(&self.inner, expression, true)
    }
    /// Observe one child without following links.
    pub fn observe_child(&self, name: &OsStr) -> io::Result<EntryObservation> {
        component(name)?;
        backend::observe(&self.inner, Path::new(name), false)
    }
    /// Only a missing entry becomes `None`; other failures retain their cause.
    pub fn observe_child_if_present(&self, name: &OsStr) -> io::Result<Option<EntryObservation>> {
        match self.observe_child(name) {
            Ok(value) => Ok(Some(value)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
    /// Open one read-only regular file; only explicit sources may follow links.
    pub fn open_file(&self, name: &OsStr, follow_final: bool) -> Result<BoundFile, BoundFileError> {
        component(name)?;
        let file = backend::open_file(&self.inner, name, follow_final)?;
        let initial = backend::signature(&file)?;
        let bound = BoundFile {
            file,
            parent: self.clone(),
            name: name.to_owned(),
            follow_final,
            initial,
        };
        bound.check_binding()?;
        Ok(bound)
    }
    /// Create one mirrored directory exclusively, retaining ownership for cleanup.
    pub fn create_child(&self, name: &OsStr) -> Result<CreatedDirectory, CreationFailure> {
        let uncreated = |error| CreationFailure {
            error,
            cleanup: cleanup_state(CleanupState::NotNeeded),
        };
        component(name).map_err(uncreated)?;
        backend::create_directory(&self.inner, name).map_err(uncreated)?;
        match self.open_child(name) {
            Ok(directory) => Ok(CreatedDirectory {
                directory,
                parent: self.clone(),
                name: name.into(),
            }),
            Err(error) => Err(CreationFailure {
                error,
                cleanup: CleanupReceipt {
                    state: CleanupState::Residual,
                    residual_name: Some(name.into()),
                    native_code: None,
                },
            }),
        }
    }
    /// Write and synchronize an exclusive same-directory staging file.
    pub fn stage_bytes(
        &self,
        bytes: &[u8],
        mut checkpoint: impl FnMut() -> io::Result<()>,
    ) -> Result<ReadyCandidate, StageFailure> {
        let uncreated = |error| StageFailure {
            operation: StageOperation::Create,
            error,
            file_sync: None,
            cleanup: cleanup_state(CleanupState::NotNeeded),
        };
        checkpoint().map_err(uncreated)?;
        self.check_binding().map_err(uncreated)?;
        let mut created = None;
        for _ in 0..16 {
            let name = random_name().map_err(uncreated)?;
            match backend::create_file(&self.inner, &name) {
                Ok(file) => {
                    created = Some((name, file));
                    break;
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(uncreated(e)),
            }
        }
        let (name, mut file) = created.ok_or_else(|| {
            uncreated(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Exclusive staging names exhausted",
            ))
        })?;
        let mut operation = StageOperation::Write;
        let mut sync = None;
        let result = (|| {
            for chunk in bytes.chunks(64 * 1024) {
                checkpoint()?;
                file.write_all(chunk)?;
            }
            file.seek(SeekFrom::Start(0))?;
            let mut buffer = [0; 64 * 1024];
            for expected in bytes.chunks(buffer.len()) {
                checkpoint()?;
                file.read_exact(&mut buffer[..expected.len()])?;
                if &buffer[..expected.len()] != expected {
                    return Err(io::Error::other(
                        "Staging readback differs from verified bytes",
                    ));
                }
            }
            if file.read(&mut buffer[..1])? != 0 {
                return Err(io::Error::other("Unexpected staging bytes"));
            }
            operation = StageOperation::Sync;
            checkpoint()?;
            let result = file.sync_all();
            sync = Some(sync_receipt(&result));
            result?;
            Ok(())
        })();
        if let Err(error) = result {
            return Err(StageFailure {
                operation,
                error,
                file_sync: sync,
                cleanup: remove_owned(
                    self,
                    &name,
                    backend::signature(&file).ok().map(|s| s.identity),
                    false,
                ),
            });
        }
        let initial = backend::signature(&file).map_err(|error| StageFailure {
            operation: StageOperation::Write,
            error: error.into_io_error(),
            file_sync: sync,
            cleanup: remove_owned(self, &name, None, false),
        })?;
        Ok(ReadyCandidate {
            directory: self.clone(),
            file,
            name,
            initial,
            sync: sync.expect("File was synchronized"),
        })
    }
}
/// Streaming native directory names.
pub struct DirectoryEntries(backend::Entries);
impl Iterator for DirectoryEntries {
    type Item = io::Result<OsString>;
    fn next(&mut self) -> Option<Self::Item> {
        backend::next_entry(&mut self.0)
    }
}
/// A read-only file with a captured signature and parent/name binding.
pub struct BoundFile {
    file: File,
    parent: BoundDirectory,
    name: OsString,
    follow_final: bool,
    initial: FileSignature,
}
impl BoundFile {
    /// Open an absolute explicitly supplied file.
    pub fn open(path: &Path, follow_final: bool) -> Result<Self, BoundFileError> {
        let parent =
            BoundDirectory::open(path.parent().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "File has no parent")
            })?)?;
        parent.open_file(
            path.file_name().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "File has no basename")
            })?,
            follow_final,
        )
    }
    /// Captured source/existing-file signature.
    pub fn initial_signature(&self) -> FileSignature {
        self.initial
    }
    /// Current held-file signature.
    pub fn current_signature(&self) -> Result<FileSignature, BoundFileError> {
        backend::signature(&self.file).map_err(BoundFileError::against_regular_baseline)
    }
    /// Parent identity captured with this file.
    pub fn parent_identity(&self) -> Identity {
        self.parent.identity
    }
    /// Read without changing a process-global or shared stream position.
    pub fn read_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
        backend::read_at(&self.file, buffer, offset)
    }
    /// Detect source changes through both the file handle and its admitted name.
    pub fn check_binding(&self) -> Result<(), BoundFileError> {
        let observed =
            backend::observe(&self.parent.inner, Path::new(&self.name), self.follow_final)?;
        if observed.file != Some(self.initial) || self.current_signature()? != self.initial {
            return Err(BoundFileError::Changed);
        }
        Ok(())
    }
    /// Synchronize an existing destination without writing its bytes.
    pub fn sync_file(&self) -> SyncReceipt {
        sync_receipt(&backend::sync_existing(
            &self.file,
            &self.parent.inner,
            &self.name,
            self.initial.identity,
        ))
    }
}
/// A directory created by this operation, never recursively removed.
pub struct CreatedDirectory {
    directory: BoundDirectory,
    parent: BoundDirectory,
    name: OsString,
}
impl CreatedDirectory {
    /// Borrow the held child directory.
    pub fn directory(&self) -> &BoundDirectory {
        &self.directory
    }
    /// Keep an intended output directory.
    pub fn persist(self) -> BoundDirectory {
        self.directory
    }
    /// Remove only the known owned empty entry.
    pub fn remove_empty(self) -> CleanupReceipt {
        remove_owned(
            &self.parent,
            &self.name,
            Some(self.directory.identity),
            true,
        )
    }
}
fn remove_owned(
    parent: &BoundDirectory,
    name: &OsStr,
    identity: Option<Identity>,
    directory: bool,
) -> CleanupReceipt {
    let Some(expected) = identity else {
        return cleanup_state(CleanupState::Unknown);
    };
    match parent.observe_child_if_present(name) {
        Ok(None) => return cleanup_state(CleanupState::Complete),
        Ok(Some(entry)) if entry.identity == expected => {}
        Ok(Some(_)) => return cleanup_state(CleanupState::Unknown),
        Err(error) => {
            return CleanupReceipt {
                state: CleanupState::Unknown,
                residual_name: None,
                native_code: error.raw_os_error().map(i64::from),
            };
        }
    }
    match backend::remove(&parent.inner, name, directory) {
        Ok(()) => cleanup_state(CleanupState::Complete),
        Err(error) => CleanupReceipt {
            state: CleanupState::Residual,
            residual_name: Some(name.into()),
            native_code: error.raw_os_error().map(i64::from),
        },
    }
}
/// Synchronized staging bytes, not yet published.
pub struct ReadyCandidate {
    directory: BoundDirectory,
    file: File,
    name: OsString,
    initial: FileSignature,
    sync: SyncReceipt,
}
impl ReadyCandidate {
    /// Exact staged object identity.
    pub fn identity(&self) -> Identity {
        self.initial.identity
    }
    /// File synchronization receipt from staging.
    pub fn file_sync(&self) -> SyncReceipt {
        self.sync
    }
    /// Revalidate the staged object before its publication edge.
    pub fn check_binding(&self) -> io::Result<()> {
        if backend::signature(&self.file).map_err(BoundFileError::into_io_error)? != self.initial
            || self.directory.observe_child(&self.name)?.file != Some(self.initial)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Staging file changed",
            ));
        }
        Ok(())
    }
    /// Attempt atomic publication exactly once, without replacing an existing name.
    pub fn publish(self, name: &OsStr) -> PublishAttempt {
        let result = component(name)
            .and_then(|()| self.check_binding())
            .and_then(|()| backend::publish(&self.directory.inner, &self.name, name));
        let error = result.err();
        let state = if error.is_none() || self.check_final_identity(name).unwrap_or(false) {
            PublicationState::Published
        } else if error
            .as_ref()
            .is_some_and(|e| e.kind() == io::ErrorKind::AlreadyExists)
        {
            PublicationState::NotPublished
        } else {
            PublicationState::Unknown
        };
        PublishAttempt {
            candidate: PublicationCandidate(self),
            state,
            error,
        }
    }
    /// Compare a final entry with the exact staged identity, without following links.
    pub fn check_final_identity(&self, name: &OsStr) -> io::Result<bool> {
        Ok(self
            .directory
            .observe_child_if_present(name)?
            .is_some_and(|e| e.kind == EntryKind::File && e.identity == self.identity()))
    }
    /// Remove only the still-owned staging name; a published final name is preserved.
    pub fn cleanup(self) -> CleanupReceipt {
        remove_owned(
            &self.directory,
            &self.name,
            Some(self.initial.identity),
            false,
        )
    }
}
/// Edge observation together with the still-held candidate.
pub struct PublishAttempt {
    /// Candidate retained for reconciliation and cleanup.
    pub candidate: PublicationCandidate,
    /// Observed publication state.
    pub state: PublicationState,
    /// Original native failure.
    pub error: Option<io::Error>,
}
impl PublishAttempt {
    /// A definite occupied destination name.
    pub fn is_name_collision(&self) -> bool {
        self.error
            .as_ref()
            .is_some_and(|e| e.kind() == io::ErrorKind::AlreadyExists)
    }
    /// The filesystem does not implement a required publication primitive.
    pub fn is_unsupported(&self) -> bool {
        self.error
            .as_ref()
            .is_some_and(|e| e.kind() == io::ErrorKind::Unsupported)
    }
}
/// A candidate after its single publication attempt.
pub struct PublicationCandidate(ReadyCandidate);
impl PublicationCandidate {
    /// Exact staged object identity.
    pub fn identity(&self) -> Identity {
        self.0.identity()
    }
    /// Pre-edge synchronization receipt.
    pub fn file_sync(&self) -> SyncReceipt {
        self.0.file_sync()
    }
    /// Compare the requested final entry with the held file.
    pub fn check_final_identity(&self, name: &OsStr) -> io::Result<bool> {
        self.0.check_final_identity(name)
    }
    /// Remove the remaining owned staging name only.
    pub fn cleanup(self) -> CleanupReceipt {
        self.0.cleanup()
    }
}
fn component(name: &OsStr) -> io::Result<()> {
    #[cfg(unix)]
    if name.as_encoded_bytes().contains(&b'/') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Filename component contains a separator",
        ));
    }
    #[cfg(windows)]
    validate_component(name, 32_768)?;

    if name.is_empty()
        || Path::new(name).components().count() != 1
        || !matches!(
            Path::new(name).components().next(),
            Some(std::path::Component::Normal(_))
        )
        || name.as_encoded_bytes().contains(&0)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Expected one native filename component",
        ));
    }
    Ok(())
}
fn random_name() -> io::Result<OsString> {
    use std::fmt::Write;
    let mut random = [0; 16];
    getrandom::fill(&mut random).map_err(io::Error::other)?;
    let mut name = String::from(".emlattachmentremover-");
    for byte in random {
        write!(name, "{byte:02x}").expect("String write");
    }
    name.push_str(".tmp");
    Ok(name.into())
}
fn sync_receipt(result: &io::Result<()>) -> SyncReceipt {
    #[cfg(unix)]
    let method = SyncMethod::Fsync;
    #[cfg(windows)]
    let method = SyncMethod::FlushFileBuffers;
    match result {
        Ok(()) => SyncReceipt {
            method,
            state: SyncState::Succeeded,
            native_code: None,
        },
        Err(error) => SyncReceipt {
            method,
            state: if error.kind() == io::ErrorKind::Unsupported {
                SyncState::Unsupported
            } else {
                SyncState::Failed
            },
            native_code: error.raw_os_error().map(i64::from),
        },
    }
}
