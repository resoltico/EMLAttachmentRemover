//! Owned private candidates, checked storage, and exact no-replace publication edges.
use crate::{
    BoundDirectory, CleanupReceipt, CleanupState, CreatedDirectory, CreationFailure, Identity,
    unix::{component, identity},
};
use rustix::fs::{self, AtFlags, FileType, Mode, OFlags};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    os::fd::AsFd,
};

/// Exact requested synchronization method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncMethod {
    /// Portable fsync.
    Fsync,
    /// fsync and Darwin F_FULLFSYNC both completed.
    FsyncFullfsync,
    /// fsync completed; only the stronger Darwin request was unsupported.
    FsyncFullfsyncUnsupported,
}
/// Native request result, never inferred from file existence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncState {
    /// The specified method completed.
    Succeeded,
    /// A required request is unsupported.
    Unsupported,
    /// A required request failed.
    Failed,
}
/// Receipt for one exact held-object synchronization request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncReceipt {
    /// Method actually requested.
    pub method: SyncMethod,
    /// Result actually observed.
    pub state: SyncState,
    /// Native error retained for failure or limited-method evidence.
    pub native_code: Option<i64>,
}
fn unsupported_sync(error: rustix::io::Errno) -> bool {
    [
        rustix::io::Errno::INVAL,
        rustix::io::Errno::NOTSUP,
        rustix::io::Errno::OPNOTSUPP,
    ]
    .contains(&error)
}
fn sync_error(method: SyncMethod, error: rustix::io::Errno) -> SyncReceipt {
    SyncReceipt {
        method,
        state: if unsupported_sync(error) {
            SyncState::Unsupported
        } else {
            SyncState::Failed
        },
        native_code: Some(i64::from(error.raw_os_error())),
    }
}
pub(crate) fn portable_sync(fd: impl AsFd) -> SyncReceipt {
    match fs::fsync(fd) {
        Ok(()) => SyncReceipt {
            method: SyncMethod::Fsync,
            state: SyncState::Succeeded,
            native_code: None,
        },
        Err(error) => sync_error(SyncMethod::Fsync, error),
    }
}
pub(crate) fn file_sync(fd: impl AsFd) -> SyncReceipt {
    let portable = portable_sync(fd.as_fd());
    if portable.state != SyncState::Succeeded {
        return portable;
    }
    #[cfg(target_os = "macos")]
    return match fs::fcntl_fullfsync(fd) {
        Ok(()) => SyncReceipt {
            method: SyncMethod::FsyncFullfsync,
            state: SyncState::Succeeded,
            native_code: None,
        },
        Err(error) if unsupported_sync(error) => SyncReceipt {
            method: SyncMethod::FsyncFullfsyncUnsupported,
            state: SyncState::Succeeded,
            native_code: Some(i64::from(error.raw_os_error())),
        },
        Err(error) => sync_error(SyncMethod::FsyncFullfsync, error),
    };
    #[cfg(not(target_os = "macos"))]
    portable
}
/// Native operation in progress when staging failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageOperation {
    /// Create and validate private owned resources before bytes.
    Create,
    /// Write or reread candidate storage.
    Write,
    /// Request candidate file synchronization.
    Sync,
}
/// Staging failed before publication; preserves its cause, flush and owned cleanup.
#[derive(Debug)]
pub struct StageFailure {
    /// Native operation, so callers need not parse error prose.
    pub operation: StageOperation,
    /// Primary native/storage invariant failure.
    pub error: io::Error,
    /// File synchronization, if reached.
    pub file_sync: Option<SyncReceipt>,
    /// Actual attempt to remove this operation's owned resources.
    pub cleanup: CleanupReceipt,
}
impl std::fmt::Display for StageFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}; cleanup {:?}", self.error, self.cleanup.state)
    }
}
impl std::error::Error for StageFailure {}
impl From<CreationFailure> for StageFailure {
    fn from(error: CreationFailure) -> Self {
        Self {
            operation: StageOperation::Create,
            error: error.error,
            file_sync: None,
            cleanup: error.cleanup,
        }
    }
}

/// A fully written, reread and synchronized held candidate. Its file is never
/// exposed for mutation, and publication accepts this object rather than a path.
pub struct ReadyCandidate {
    owned: CreatedDirectory,
    destination: BoundDirectory,
    file: File,
    identity: Identity,
    length: u64,
    sync: SyncReceipt,
    state: PublicationState,
}
/// Observation of this candidate's no-replace publication attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicationState {
    /// No publication edge succeeded.
    NotPublished,
    /// The native no-replace edge succeeded or its identity was reconciled.
    Published,
    /// An ambiguous native error could not be reconciled.
    Unknown,
}
/// Retains both the candidate and edge observation, including on errors. A caller
/// must finish reconciliation/sync/cleanup after Published or Unknown.
pub struct PublishAttempt {
    /// Held file/directory owners remain alive.
    pub candidate: PublicationCandidate,
    /// Whether the native edge published this candidate.
    pub state: PublicationState,
    /// Native error, even if later identity reconciliation established publication.
    pub error: Option<io::Error>,
}
/// Held candidate after its single publication attempt. It can be reconciled or
/// cleaned but cannot be published again under another final name.
pub struct PublicationCandidate(ReadyCandidate);
impl PublicationCandidate {
    /// Exact object identity retained across the edge.
    pub fn identity(&self) -> Identity {
        self.0.identity()
    }
    /// File synchronization completed before the edge.
    pub fn file_sync(&self) -> SyncReceipt {
        self.0.file_sync()
    }
    /// Compare a final entry with the held candidate.
    pub fn check_final_identity(&self, name: &OsStr) -> io::Result<bool> {
        self.0.check_final_identity(name)
    }
    /// Remove only the owned staging resources; never roll back a final copy.
    pub fn cleanup(self) -> CleanupReceipt {
        self.0.cleanup()
    }
}
fn random_name() -> io::Result<OsString> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(io::Error::other)?;
    let mut name = String::from(".eml-attachment-remover-");
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut name, "{byte:02x}").expect("writing String cannot fail");
    }
    Ok(name.into())
}
fn validate_file(
    file: &File,
    directory: &BoundDirectory,
    expected: Identity,
    length: u64,
    initial: bool,
) -> io::Result<()> {
    directory.check_private()?;
    let stat = fs::fstat(file)?;
    let entry = fs::statat(directory, "candidate", AtFlags::SYMLINK_NOFOLLOW)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
        || identity(&stat)? != expected
        || identity(&entry)? != expected
        || stat.st_uid != directory.owner_uid()
        || stat.st_mode & 0o777 != 0o600
        || stat.st_size < 0
        || stat.st_size as u64 != length
        || (initial && stat.st_nlink != 1)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Owned candidate identity, mode, type, length or link count changed",
        ));
    }
    Ok(())
}
fn cleanup(owned: CreatedDirectory, expected: Option<Identity>) -> CleanupReceipt {
    // Even if the outer staging name moved, its held private directory permits
    // deleting only the known owned candidate inside it. Never recursively sweep.
    if let Some(expected) = expected {
        let directory = owned.directory();
        let removed = (|| -> io::Result<()> {
            directory.check_private()?;
            let entry = fs::statat(directory, "candidate", AtFlags::SYMLINK_NOFOLLOW)?;
            if identity(&entry)? != expected {
                return Err(io::Error::other("Candidate entry was replaced"));
            }
            fs::unlinkat(directory, "candidate", AtFlags::empty())?;
            Ok(())
        })();
        if let Err(error) = removed {
            // A macOS successful rename already removed this owned stage entry.
            if error.raw_os_error() != Some(libc::ENOENT) {
                return CleanupReceipt {
                    state: CleanupState::Residual,
                    residual_name: None,
                    native_code: error.raw_os_error().map(i64::from),
                };
            }
        }
    }
    owned.remove_empty()
}
impl BoundDirectory {
    /// Stage supplied verified bytes privately, with bounded storage checks and
    /// caller cancellation checkpoints. This function never publishes a final name.
    pub fn stage_bytes(
        &self,
        bytes: &[u8],
        mut checkpoint: impl FnMut() -> io::Result<()>,
    ) -> Result<ReadyCandidate, StageFailure> {
        let plain_failure = |error| StageFailure {
            operation: StageOperation::Create,
            error,
            file_sync: None,
            cleanup: CleanupReceipt {
                state: CleanupState::NotNeeded,
                residual_name: None,
                native_code: None,
            },
        };
        checkpoint().map_err(plain_failure)?;
        let mut owned = None;
        for _ in 0..16 {
            let name = random_name().map_err(plain_failure)?;
            match self.create_private_child(&name) {
                Ok(created) => {
                    owned = Some(created);
                    break;
                }
                Err(error) if error.error.raw_os_error() == Some(libc::EEXIST) => {}
                Err(error) => return Err(error.into()),
            }
        }
        let owned = owned.ok_or_else(|| {
            plain_failure(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Exclusive staging names exhausted",
            ))
        })?;
        let mut observed = None;
        let mut sync = None;
        let mut operation = StageOperation::Create;
        let result = (|| -> io::Result<File> {
            let directory = owned.directory();
            directory.check_private()?;
            let fd = fs::openat(
                directory,
                "candidate",
                OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
            )?;
            let mut file = File::from(fd);
            let stat = fs::fstat(&file)?;
            // Do not authorize cleanup of an object whose creation identity/type
            // has not been established through the exclusive descriptor.
            if stat.st_ino == 0
                || FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
                || stat.st_uid != directory.owner_uid()
            {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "Candidate ownership cannot be established",
                ));
            }
            observed = Some(identity(&stat)?);
            fs::fchmod(&file, Mode::from_raw_mode(0o600))?;
            #[cfg(target_os = "macos")]
            crate::darwin_acl::clear_owned(&file)?;
            validate_file(&file, directory, observed.unwrap(), 0, true)?;
            operation = StageOperation::Write;
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
                        "Candidate storage reread differs from verified bytes",
                    ));
                }
            }
            if file.read(&mut buffer[..1])? != 0 {
                return Err(io::Error::other(
                    "Candidate storage has unexpected trailing bytes",
                ));
            }
            validate_file(
                &file,
                directory,
                observed.unwrap(),
                bytes.len() as u64,
                true,
            )?;
            operation = StageOperation::Sync;
            checkpoint()?;
            let receipt = file_sync(&file);
            sync = Some(receipt);
            if receipt.state != SyncState::Succeeded {
                return Err(io::Error::other(
                    "Candidate file synchronization did not succeed",
                ));
            }
            Ok(file)
        })();
        match result {
            Ok(file) => Ok(ReadyCandidate {
                owned,
                destination: self.clone(),
                file,
                identity: observed.unwrap(),
                length: bytes.len() as u64,
                sync: sync.unwrap(),
                state: PublicationState::NotPublished,
            }),
            Err(error) => Err(StageFailure {
                operation,
                error,
                file_sync: sync,
                cleanup: cleanup(owned, observed),
            }),
        }
    }
}
impl ReadyCandidate {
    /// Identity of the exact object written and synchronized.
    pub fn identity(&self) -> Identity {
        self.identity
    }
    /// Completed file synchronization receipt.
    pub fn file_sync(&self) -> SyncReceipt {
        self.sync
    }
    /// Verify held staging entry and private file immediately before the edge.
    pub fn check_binding(&self) -> io::Result<()> {
        validate_file(
            &self.file,
            self.owned.directory(),
            self.identity,
            self.length,
            self.state == PublicationState::NotPublished,
        )
    }
    /// Attempt exactly one native no-replace edge. Pre-edge business/source checks
    /// belong to the core; no fallback replaces or copies to the final pathname.
    pub fn publish(mut self, name: &OsStr) -> PublishAttempt {
        let mut attempted_edge = false;
        let result = (|| -> io::Result<()> {
            component(name)?;
            self.destination.admit_namespace()?;
            self.check_binding()?;
            attempted_edge = true;
            #[cfg(target_os = "macos")]
            fs::renameat_with(
                self.owned.directory(),
                "candidate",
                &self.destination,
                name,
                fs::RenameFlags::NOREPLACE,
            )?;
            #[cfg(target_os = "linux")]
            fs::linkat(
                self.owned.directory(),
                "candidate",
                &self.destination,
                name,
                AtFlags::empty(),
            )?;
            self.state = PublicationState::Published;
            Ok(())
        })();
        if let Err(error) = &result
            && attempted_edge
            && !matches!(
                error.raw_os_error(),
                Some(
                    libc::EEXIST
                        | libc::EACCES
                        | libc::EPERM
                        | libc::EROFS
                        | libc::ENOENT
                        | libc::ENOTDIR
                        | libc::EINVAL
                        | libc::ENAMETOOLONG
                        | libc::EXDEV
                        | libc::ENOTSUP
                )
            )
        {
            self.state = PublicationState::Unknown;
            if let Ok(entry) = fs::statat(&self.destination, name, AtFlags::SYMLINK_NOFOLLOW)
                && identity(&entry).ok() == Some(self.identity)
            {
                self.state = PublicationState::Published;
            }
        }
        PublishAttempt {
            state: self.state,
            candidate: PublicationCandidate(self),
            error: result.err(),
        }
    }
    /// Reconcile a final entry with the held candidate. This observation does not
    /// qualify requested address or namespace synchronization by itself.
    pub fn check_final_identity(&self, name: &OsStr) -> io::Result<bool> {
        component(name)?;
        Ok(identity(&fs::statat(
            &self.destination,
            name,
            AtFlags::SYMLINK_NOFOLLOW,
        )?)? == self.identity)
    }
    /// Remove only this owned private staging resource, retaining the final copy
    /// after publication and never rolling back a published/unknown final name.
    pub fn cleanup(self) -> CleanupReceipt {
        cleanup(self.owned, Some(self.identity))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    #[test]
    fn only_documented_unsupported_sync_errors_allow_the_limited_method() {
        for error in [
            rustix::io::Errno::INVAL,
            rustix::io::Errno::NOTSUP,
            rustix::io::Errno::OPNOTSUPP,
        ] {
            assert!(unsupported_sync(error));
            assert_eq!(
                sync_error(SyncMethod::Fsync, error).state,
                SyncState::Unsupported
            );
        }
        for error in [
            rustix::io::Errno::IO,
            rustix::io::Errno::ACCESS,
            rustix::io::Errno::NOSPC,
        ] {
            assert!(!unsupported_sync(error));
            assert_eq!(
                sync_error(SyncMethod::FsyncFullfsync, error).state,
                SyncState::Failed
            );
        }
        #[cfg(target_os = "macos")]
        {
            assert_eq!(rustix::io::Errno::NOTSUP.raw_os_error(), 45);
            assert_eq!(rustix::io::Errno::OPNOTSUPP.raw_os_error(), 102);
        }
    }
    fn root() -> tempfile::TempDir {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fs-tests");
        std::fs::create_dir_all(&base).unwrap();
        tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in(base)
            .unwrap()
    }
    fn ready(parent: &BoundDirectory, bytes: &[u8]) -> ReadyCandidate {
        parent.stage_bytes(bytes, || Ok(())).unwrap()
    }
    #[test]
    fn actual_storage_private_modes_sync_and_no_replace_preserve_winner() {
        let root = root();
        let parent = BoundDirectory::open(root.path()).unwrap();
        let bytes = b"Subject: synthetic\r\n\r\ncomplete verified bytes\r\n";
        let first = ready(&parent, bytes);
        assert_eq!(first.file_sync().state, SyncState::Succeeded);
        assert_eq!(fs::fstat(&first.file).unwrap().st_mode & 0o777, 0o600);
        assert_eq!(
            fs::fstat(first.owned.directory()).unwrap().st_mode & 0o777,
            0o700
        );
        let attempt = first.publish(OsStr::new("copy.eml"));
        assert!(attempt.error.is_none());
        assert_eq!(attempt.state, PublicationState::Published);
        assert!(
            attempt
                .candidate
                .check_final_identity(OsStr::new("copy.eml"))
                .unwrap()
        );
        assert!(parent.check_requested_address().unwrap());
        assert_eq!(parent.sync_namespace().state, SyncState::Succeeded);
        assert_eq!(attempt.candidate.cleanup().state, CleanupState::Complete);
        let second = ready(&parent, b"different bytes").publish(OsStr::new("copy.eml"));
        assert_eq!(second.state, PublicationState::NotPublished);
        assert_eq!(second.error.unwrap().raw_os_error(), Some(libc::EEXIST));
        assert_eq!(second.candidate.cleanup().state, CleanupState::Complete);
        assert_eq!(std::fs::read(root.path().join("copy.eml")).unwrap(), bytes);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }
    #[test]
    fn simultaneous_publishers_have_one_complete_winner() {
        let root = root();
        let parent = BoundDirectory::open(root.path()).unwrap();
        let candidates = [
            ready(&parent, b"first complete"),
            ready(&parent, b"second complete"),
        ];
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = candidates
            .into_iter()
            .map(|candidate| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let attempt = candidate.publish(OsStr::new("copy"));
                    let state = attempt.state;
                    let error = attempt.error.map(|e| e.raw_os_error());
                    assert_eq!(attempt.candidate.cleanup().state, CleanupState::Complete);
                    (state, error)
                })
            })
            .collect();
        let outcomes: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(
            outcomes
                .iter()
                .filter(|(state, _)| *state == PublicationState::Published)
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|(state, error)| *state == PublicationState::NotPublished
                    && *error == Some(Some(libc::EEXIST)))
                .count(),
            1
        );
        let bytes = std::fs::read(root.path().join("copy")).unwrap();
        assert!(bytes == b"first complete" || bytes == b"second complete");
    }
    #[test]
    fn descriptor_publication_survives_destination_and_stage_name_substitution() {
        let root = root();
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(root.path().join("destination"))
            .unwrap();
        let parent = BoundDirectory::open(&root.path().join("destination")).unwrap();
        let candidate = ready(&parent, b"held verified bytes");
        let stage_name = candidate
            .owned
            .directory()
            .requested_path()
            .file_name()
            .unwrap()
            .to_owned();
        std::fs::rename(root.path().join("destination"), root.path().join("moved")).unwrap();
        std::fs::create_dir(root.path().join("destination")).unwrap();
        std::fs::rename(
            root.path().join("moved").join(&stage_name),
            root.path().join("moved/held-stage"),
        )
        .unwrap();
        std::fs::create_dir(root.path().join("moved").join(&stage_name)).unwrap();
        std::fs::write(
            root.path()
                .join("moved")
                .join(&stage_name)
                .join("candidate"),
            b"fake bytes",
        )
        .unwrap();
        let attempt = candidate.publish(OsStr::new("copy"));
        assert_eq!(attempt.state, PublicationState::Published);
        assert!(attempt.error.is_none());
        assert!(!parent.check_requested_address().unwrap());
        assert_eq!(
            std::fs::read(root.path().join("moved/copy")).unwrap(),
            b"held verified bytes"
        );
        assert!(!root.path().join("destination/copy").exists());
        assert_eq!(attempt.candidate.cleanup().state, CleanupState::Residual);
        assert_eq!(
            std::fs::read(root.path().join("moved").join(stage_name).join("candidate")).unwrap(),
            b"fake bytes"
        );
        assert_eq!(
            std::fs::read_dir(root.path().join("moved/held-stage"))
                .unwrap()
                .count(),
            0
        );
    }
    #[test]
    fn candidate_entry_substitution_prevents_publication_and_retains_unknown_resource() {
        let root = root();
        let parent = BoundDirectory::open(root.path()).unwrap();
        let candidate = ready(&parent, b"held verified bytes");
        let stage = candidate.owned.directory();
        fs::renameat(stage, "candidate", stage, "moved").unwrap();
        std::fs::write(stage.requested_path().join("candidate"), b"fake bytes").unwrap();
        let attempt = candidate.publish(OsStr::new("copy"));
        assert_eq!(attempt.state, PublicationState::NotPublished);
        assert!(attempt.error.is_some());
        assert!(!root.path().join("copy").exists());
        assert_eq!(attempt.candidate.cleanup().state, CleanupState::Residual);
    }
    #[test]
    fn cancellation_before_bytes_and_during_write_cleans_only_owned_stage() {
        let root = root();
        let parent = BoundDirectory::open(root.path()).unwrap();
        let bytes = vec![b'x'; 200_000];
        for stop_at in [1, 2, 3, 5, 10] {
            let mut calls = 0;
            let failure = parent
                .stage_bytes(&bytes, || {
                    calls += 1;
                    if calls == stop_at {
                        Err(io::Error::new(
                            io::ErrorKind::Interrupted,
                            "owned test stop",
                        ))
                    } else {
                        Ok(())
                    }
                })
                .err()
                .unwrap();
            assert_eq!(failure.error.kind(), io::ErrorKind::Interrupted);
            assert_eq!(
                failure.operation,
                match stop_at {
                    1 => StageOperation::Create,
                    10 => StageOperation::Sync,
                    _ => StageOperation::Write,
                }
            );
            assert!(matches!(
                failure.cleanup.state,
                CleanupState::NotNeeded | CleanupState::Complete
            ));
            assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        }
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn read_only_inherited_acl_is_cleared_on_stage_and_candidate() {
        let root = root();
        assert!(std::process::Command::new("/bin/chmod").args(["+a", "everyone allow readattr,readsecurity,list,search,directory_inherit,file_inherit"]).arg(root.path()).status().unwrap().success());
        let parent = BoundDirectory::open(root.path()).unwrap();
        let acl_entries = |path: &std::path::Path| {
            let listing = std::process::Command::new("/bin/ls")
                .args(["-den"])
                .arg(path)
                .output()
                .unwrap();
            assert!(listing.status.success());
            String::from_utf8(listing.stdout)
                .unwrap()
                .lines()
                .skip(1)
                .count()
        };
        // Independent OS listing must detect a real inherited ACL even with
        // restrictive mode. This control would reject omitting ACL clearing.
        let control = root.path().join("inheritance-control");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&control)
            .unwrap();
        assert!(acl_entries(&control) > 0);
        let candidate = ready(&parent, b"synthetic bytes");
        assert_eq!(acl_entries(candidate.owned.directory().requested_path()), 0);
        assert_eq!(
            acl_entries(
                &candidate
                    .owned
                    .directory()
                    .requested_path()
                    .join("candidate")
            ),
            0
        );
        assert_eq!(candidate.file_sync().state, SyncState::Succeeded);
        let attempt = candidate.publish(OsStr::new("copy"));
        assert!(attempt.error.is_none());
        assert_eq!(attempt.candidate.cleanup().state, CleanupState::Complete);
        assert_eq!(
            std::fs::metadata(root.path().join("copy"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(acl_entries(&root.path().join("copy")), 0);
    }
}
