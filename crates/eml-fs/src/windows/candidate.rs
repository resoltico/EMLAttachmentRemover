//! Owned file storage only. No final-name publication or address qualification.
use super::writable::{Creation, WritableDirectory, flush};
use crate::{
    CleanupReceipt, CleanupState, Identity, NtStatusError, StageFailure, StageOperation,
    SyncReceipt, SyncState,
    staging::{NAME_ATTEMPTS, random_name},
};
use std::{
    ffi::OsString,
    io::{self, Read, Seek, SeekFrom, Write},
};
use windows_sys::Win32::Foundation::STATUS_OBJECT_NAME_COLLISION;

/// Complete reread and synchronized storage of supplied verified bytes.
/// Construction/handles stay private; this capability has no publish operation.
pub struct ReadyCandidate {
    owned: Creation,
    identity: Identity,
    length: u64,
    sync: SyncReceipt,
}
impl ReadyCandidate {
    /// Exact full identity of the owned object written and synchronized.
    pub fn identity(&self) -> Identity {
        self.identity
    }
    /// Expected byte length actually reread before this value was returned.
    pub fn length(&self) -> u64 {
        self.length
    }
    /// Actual completed file synchronization receipt.
    pub fn file_sync(&self) -> SyncReceipt {
        self.sync
    }
    /// Remove only this owned staged file, retaining actual cleanup truth.
    pub fn cleanup(self) -> CleanupReceipt {
        self.owned.cleanup(Some(self.identity))
    }
}
fn before_creation(error: io::Error) -> StageFailure {
    StageFailure {
        operation: StageOperation::Create,
        error,
        file_sync: None,
        cleanup: CleanupReceipt {
            state: CleanupState::NotNeeded,
            residual_name: None,
            native_code: None,
        },
    }
}
fn exact_collision(error: &crate::CreationFailure) -> bool {
    error.cleanup.state == CleanupState::NotNeeded
        && error
            .error
            .get_ref()
            .and_then(|cause| cause.downcast_ref::<NtStatusError>())
            .is_some_and(|cause| cause.nt_status() == STATUS_OBJECT_NAME_COLLISION)
}
impl WritableDirectory {
    /// Stage bytes with bounded same-handle write/reread/EOF and file flush checks.
    /// Native inherited ACL remains unchanged; share-zero excludes normal data
    /// opens temporarily, without promising owner-only or metadata immutability.
    pub fn stage_bytes(
        &self,
        bytes: &[u8],
        checkpoint: impl FnMut() -> io::Result<()>,
    ) -> Result<ReadyCandidate, StageFailure> {
        self.stage_with_names(bytes, checkpoint, random_name)
    }
    fn stage_with_names(
        &self,
        bytes: &[u8],
        mut checkpoint: impl FnMut() -> io::Result<()>,
        mut next_name: impl FnMut() -> io::Result<OsString>,
    ) -> Result<ReadyCandidate, StageFailure> {
        let mut last_collision = None;
        let mut admitted = None;
        for _ in 0..NAME_ATTEMPTS {
            checkpoint().map_err(before_creation)?;
            let name = next_name().map_err(before_creation)?;
            match self.create_file(&name) {
                Ok(value) => {
                    admitted = Some(value);
                    break;
                }
                Err(error) if exact_collision(&error) => last_collision = Some(error),
                Err(error) => return Err(error.into()),
            }
        }
        let (mut owned, identity) = match admitted {
            Some(value) => value,
            None => {
                return Err(last_collision
                    .expect("finite positive attempts record their collision")
                    .into());
            }
        };
        let mut operation = StageOperation::Write;
        let mut sync = None;
        let result = (|| -> io::Result<()> {
            for chunk in bytes.chunks(64 * 1024) {
                let mut remaining = chunk;
                while !remaining.is_empty() {
                    checkpoint()?;
                    match owned.file.write(remaining) {
                        Ok(0) => {
                            return Err(io::Error::new(
                                io::ErrorKind::WriteZero,
                                "Candidate write made no progress",
                            ));
                        }
                        Ok(count) => remaining = &remaining[count..],
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                        Err(error) => return Err(error),
                    }
                }
            }
            loop {
                checkpoint()?;
                match owned.file.seek(SeekFrom::Start(0)) {
                    Ok(0) => break,
                    Ok(_) => {
                        return Err(io::Error::other(
                            "Candidate rewind returned another position",
                        ));
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(error),
                }
            }
            let mut buffer = [0; 64 * 1024];
            for expected in bytes.chunks(buffer.len()) {
                let mut read = 0;
                while read < expected.len() {
                    checkpoint()?;
                    match owned.file.read(&mut buffer[read..expected.len()]) {
                        Ok(0) => {
                            return Err(io::Error::new(
                                io::ErrorKind::UnexpectedEof,
                                "Candidate reread ended early",
                            ));
                        }
                        Ok(count) => read += count,
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                        Err(error) => return Err(error),
                    }
                }
                if &buffer[..expected.len()] != expected {
                    return Err(io::Error::other(
                        "Candidate reread differs from verified bytes",
                    ));
                }
            }
            loop {
                checkpoint()?;
                match owned.file.read(&mut buffer[..1]) {
                    Ok(0) => break,
                    Ok(_) => return Err(io::Error::other("Candidate storage has trailing bytes")),
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(error),
                }
            }
            owned.validate_file(identity, bytes.len() as u64)?;
            operation = StageOperation::Sync;
            checkpoint()?;
            let receipt = flush(&owned.file);
            sync = Some(receipt);
            if receipt.state != SyncState::Succeeded {
                return Err(io::Error::other(
                    "Candidate file synchronization did not succeed",
                ));
            }
            Ok(())
        })();
        match result {
            Ok(()) => finish_after_flush(
                owned,
                identity,
                bytes.len() as u64,
                sync.expect("successful staging requested file sync"),
                &mut checkpoint,
            ),
            Err(error) => Err(StageFailure {
                operation,
                error,
                file_sync: sync,
                cleanup: owned.cleanup(Some(identity)),
            }),
        }
    }
}
fn finish_after_flush(
    owned: Creation,
    identity: Identity,
    length: u64,
    sync: SyncReceipt,
    checkpoint: &mut impl FnMut() -> io::Result<()>,
) -> Result<ReadyCandidate, StageFailure> {
    let result = (|| {
        checkpoint()?;
        owned.validate_file(identity, length)?;
        checkpoint()?;
        Ok(())
    })();
    match result {
        Ok(()) => Ok(ReadyCandidate {
            owned,
            identity,
            length,
            sync,
        }),
        Err(error) => Err(StageFailure {
            operation: StageOperation::Write,
            error,
            file_sync: Some(sync),
            cleanup: owned.cleanup(Some(identity)),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        test_actor::RestrictedToken,
        test_fixture::{directory, operation},
    };
    use super::*;
    use crate::{BoundDirectory, SyncMethod};
    use std::{cell::Cell, os::windows::fs::FileExt};

    fn writer(root: &std::path::Path) -> (BoundDirectory, WritableDirectory) {
        let read = BoundDirectory::open(&operation(root)).unwrap();
        let write = WritableDirectory::bind_resolved(&operation(root), read.identity()).unwrap();
        (read, write)
    }
    fn missing(error: &io::Error) -> bool {
        error
            .get_ref()
            .and_then(|value| value.downcast_ref::<NtStatusError>())
            .is_some_and(|value| {
                value.nt_status() == windows_sys::Win32::Foundation::STATUS_OBJECT_NAME_NOT_FOUND
            })
    }
    fn interrupted() -> io::Error {
        io::Error::new(io::ErrorKind::Interrupted, "Synthetic cooperative stop")
    }

    #[test]
    fn native_same_handle_stage_verifies_nonempty_and_zero_bytes_sync_exclusion_and_cleanup() {
        let fixture = directory();
        let mut payload = b"Subject: synthetic\r\n\r\n".to_vec();
        payload.extend(vec![b'x'; 200_003]);
        let actor = RestrictedToken::enter().unwrap();
        let (read, write) = writer(fixture.path());
        for bytes in [&payload[..], &[][..]] {
            let ready = write.stage_bytes(bytes, || Ok(())).unwrap();
            assert_eq!(ready.length(), bytes.len() as u64);
            assert_eq!(ready.file_sync().method, SyncMethod::FlushFileBuffers);
            assert_eq!(ready.file_sync().state, SyncState::Succeeded);
            let name = ready.owned.name().to_owned();
            assert!(
                name.to_string_lossy()
                    .starts_with(".eml-attachment-remover-")
            );
            let observed = read.observe_child(&name).unwrap().file.unwrap();
            assert_eq!(observed.identity, ready.identity());
            assert_eq!(observed.size, bytes.len() as u64);
            // Independent positioned read through the actual owned handle.
            let mut copied = vec![0; bytes.len() + 1];
            let mut count = 0;
            while count < copied.len() {
                let got = ready
                    .owned
                    .file
                    .seek_read(&mut copied[count..], count as u64)
                    .unwrap();
                if got == 0 {
                    break;
                }
                count += got;
            }
            assert_eq!(&copied[..count], bytes);
            let address = operation(&fixture.path().join(&name));
            let denied_read = std::fs::File::open(&address).unwrap_err();
            assert_eq!(denied_read.raw_os_error(), Some(32));
            let denied_write = std::fs::OpenOptions::new()
                .write(true)
                .open(&address)
                .unwrap_err();
            assert_eq!(denied_write.raw_os_error(), Some(32));
            let cleanup = ready.cleanup();
            assert_eq!(cleanup.state, CleanupState::Complete, "{cleanup:?}");
            assert!(missing(&read.observe_child(&name).unwrap_err()));
        }
        assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 0);
        drop(write);
        drop(read);
        drop(actor);
        fixture.close().unwrap();
    }

    #[test]
    fn native_staging_stop_before_during_and_after_flush_retains_independent_receipts() {
        let fixture = directory();
        let actor = RestrictedToken::enter().unwrap();
        let (read, write) = writer(fixture.path());
        let failure = write
            .stage_bytes(b"owned synthetic", || Err(interrupted()))
            .err()
            .unwrap();
        assert_eq!(failure.operation, StageOperation::Create);
        assert_eq!(failure.cleanup.state, CleanupState::NotNeeded);
        assert!(failure.file_sync.is_none());

        let partial = Cell::new(false);
        let name = OsString::from(".owned-cancelled-storage");
        let bytes = vec![b'p'; 3 * 64 * 1024];
        let failure = write
            .stage_with_names(
                &bytes,
                || match read.observe_child(&name) {
                    Ok(entry) if entry.file.unwrap().size > 0 => {
                        assert!(entry.file.unwrap().size < bytes.len() as u64);
                        partial.set(true);
                        Err(interrupted())
                    }
                    Ok(_) => Ok(()),
                    Err(error) if missing(&error) => Ok(()),
                    Err(error) => Err(error),
                },
                || Ok(name.clone()),
            )
            .err()
            .unwrap();
        assert!(
            partial.get(),
            "stop must follow an actual observed partial write"
        );
        assert_eq!(failure.operation, StageOperation::Write);
        assert_eq!(failure.error.kind(), io::ErrorKind::Interrupted);
        assert!(failure.file_sync.is_none());
        assert_eq!(failure.cleanup.state, CleanupState::Complete);
        assert!(missing(&read.observe_child(&name).unwrap_err()));

        // The real public stage first obtains a real successful file flush.
        // Reenter its private post-flush boundary with that SAME owner/receipt;
        // this is failure mapping with real storage, not a fabricated OS flush.
        let ready = write
            .stage_bytes(b"Subject: synthetic\r\n\r\ncomplete body\r\n", || Ok(()))
            .unwrap();
        let name = ready.owned.name().to_owned();
        let expected_sync = ready.file_sync();
        let failure = finish_after_flush(
            ready.owned,
            ready.identity,
            ready.length,
            ready.sync,
            &mut || Err(interrupted()),
        )
        .err()
        .unwrap();
        assert_eq!(failure.operation, StageOperation::Write);
        assert_eq!(failure.error.kind(), io::ErrorKind::Interrupted);
        assert_eq!(failure.file_sync, Some(expected_sync));
        assert_eq!(expected_sync.state, SyncState::Succeeded);
        assert_eq!(failure.cleanup.state, CleanupState::Complete);
        assert!(missing(&read.observe_child(&name).unwrap_err()));
        drop(write);
        drop(read);
        drop(actor);
        fixture.close().unwrap();
    }

    #[test]
    fn native_staging_retries_only_exact_collisions_and_preserves_last_or_generator_failure() {
        let fixture = directory();
        let marker = OsString::from(".owned-existing-staging-control");
        std::fs::write(fixture.path().join(&marker), b"synthetic unchanged").unwrap();
        let actor = RestrictedToken::enter().unwrap();
        let (read, write) = writer(fixture.path());
        let attempts = Cell::new(0);
        let failure = write
            .stage_with_names(
                b"replacement must not be written",
                || Ok(()),
                || {
                    attempts.set(attempts.get() + 1);
                    Ok(marker.clone())
                },
            )
            .err()
            .unwrap();
        assert_eq!(attempts.get(), 16);
        assert_eq!(failure.operation, StageOperation::Create);
        assert_eq!(failure.cleanup.state, CleanupState::NotNeeded);
        assert_eq!(
            failure
                .error
                .get_ref()
                .unwrap()
                .downcast_ref::<NtStatusError>()
                .unwrap()
                .nt_status(),
            STATUS_OBJECT_NAME_COLLISION
        );
        assert!(
            failure.error.to_string().contains("NTSTATUS"),
            "retain the actual native collision"
        );
        assert_eq!(
            std::fs::read(fixture.path().join(&marker)).unwrap(),
            b"synthetic unchanged"
        );

        let attempts = Cell::new(0);
        let failure = write
            .stage_with_names(
                b"owned",
                || Ok(()),
                || {
                    attempts.set(attempts.get() + 1);
                    if attempts.get() == 1 {
                        Ok(marker.clone())
                    } else {
                        Err(io::Error::other("Synthetic random-source refusal"))
                    }
                },
            )
            .err()
            .unwrap();
        assert_eq!(attempts.get(), 2);
        assert_eq!(failure.error.to_string(), "Synthetic random-source refusal");
        assert_eq!(failure.cleanup.state, CleanupState::NotNeeded);
        let attempts = Cell::new(0);
        let failure = write
            .stage_with_names(
                b"owned",
                || Ok(()),
                || {
                    attempts.set(attempts.get() + 1);
                    Ok(OsString::from("NUL"))
                },
            )
            .err()
            .unwrap();
        assert_eq!(
            attempts.get(),
            1,
            "non-collision validation failure must not retry"
        );
        assert_eq!(failure.error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(failure.cleanup.state, CleanupState::NotNeeded);
        assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 1);
        drop(write);
        drop(read);
        drop(actor);
        fixture.close().unwrap();
    }
}
