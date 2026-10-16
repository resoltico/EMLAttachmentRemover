//! One physical no-replace edge; final-copy/address qualification is separate.
use super::{path::validate_component, status, writable::Creation};
use crate::{CleanupReceipt, CleanupState, Identity, PublicationState, SyncReceipt};
use std::{
    ffi::{OsStr, OsString},
    io,
    mem::{offset_of, size_of},
    os::windows::{ffi::OsStrExt, io::AsRawHandle},
    ptr,
};
use windows_sys::{
    Wdk::Storage::FileSystem::{
        FILE_RENAME_INFORMATION, FileRenameInformation, NtSetInformationFile,
    },
    Win32::{Foundation::STATUS_OBJECT_NAME_COLLISION, System::IO::IO_STATUS_BLOCK},
};

/// Consumed staging owner after one publication attempt. It cannot publish again.
/// Only its private NotPublished state retains staging-deletion authority.
pub struct PublicationCandidate {
    owned: Creation,
    identity: Identity,
    length: u64,
    sync: SyncReceipt,
    name: OsString,
    state: PublicationState,
}
/// Physical edge result and original error. This is not a Created outcome.
pub struct PublishAttempt {
    candidate: PublicationCandidate,
    error: Option<io::Error>,
}
impl PublishAttempt {
    /// Actual effect observed, independent of later release/cancellation errors.
    pub fn state(&self) -> PublicationState {
        self.candidate.state()
    }
    /// Original pre-edge or native rename cause, including raw NTSTATUS.
    pub fn error(&self) -> Option<&io::Error> {
        self.error.as_ref()
    }
    /// Transfer the state-guarded owner and its independent original error.
    pub fn into_parts(self) -> (PublicationCandidate, Option<io::Error>) {
        (self.candidate, self.error)
    }
}
impl PublicationCandidate {
    pub(super) fn attempt(
        owned: Creation,
        identity: Identity,
        length: u64,
        sync: SyncReceipt,
        name: &OsStr,
    ) -> PublishAttempt {
        let mut candidate = Self {
            owned,
            identity,
            length,
            sync,
            name: name.to_owned(),
            state: PublicationState::NotPublished,
        };
        let preparation = (|| {
            candidate.owned.validate_file(identity, length)?;
            let maximum = candidate.owned.component_limit()?;
            RenameBuffer::new(name, maximum)
        })();
        let error = match preparation {
            Err(error) => Some(error),
            Ok(mut buffer) => {
                let mut iosb = IO_STATUS_BLOCK::default();
                // SAFETY: generated aligned storage contains one validated basename,
                // NULL RootDirectory and zero replace flags. The same exclusive,
                // DELETE/SYNCHRONIZE-capable synchronous file and held parent remain
                // alive through this always-synchronous SetInformationFile operation.
                // Returned NTSTATUS is authoritative; the IOSB is not a success oracle.
                let result = unsafe {
                    NtSetInformationFile(
                        candidate.owned.file.as_raw_handle(),
                        &mut iosb,
                        buffer.storage.as_mut_ptr().cast(),
                        buffer.bytes,
                        FileRenameInformation,
                    )
                };
                candidate.state = effect(result);
                (result < 0).then(|| status::error(result))
            }
        };
        PublishAttempt { candidate, error }
    }
    /// Publication effect, never downgraded by handle release.
    pub fn state(&self) -> PublicationState {
        self.state
    }
    /// The full identity written and synchronized before the attempt.
    pub fn identity(&self) -> Identity {
        self.identity
    }
    /// Complete verified storage length before the attempt.
    pub fn length(&self) -> u64 {
        self.length
    }
    /// Requested final basename; its presence does not establish an effect.
    pub fn final_name(&self) -> &OsStr {
        &self.name
    }
    /// The actual pre-edge file synchronization receipt.
    pub fn file_sync(&self) -> SyncReceipt {
        self.sync
    }
    /// Clean only a definitely unpublished owned stage, otherwise checked-close
    /// without disposition. Returns the unchanged publication effect alongside
    /// removal/release evidence. No guessed residual path or final-copy claim.
    pub fn cleanup(self) -> (PublicationState, CleanupReceipt) {
        let cleanup = if self.state == PublicationState::NotPublished {
            self.owned.cleanup(Some(self.identity))
        } else {
            let closed = self.owned.release();
            CleanupReceipt {
                state: if self.state == PublicationState::Published && closed.is_ok() {
                    CleanupState::NotNeeded
                } else {
                    CleanupState::Unknown
                },
                residual_name: None,
                native_code: closed
                    .err()
                    .and_then(|error| error.raw_os_error().map(i64::from)),
            }
        };
        (self.state, cleanup)
    }
}
fn effect(status: i32) -> PublicationState {
    if status >= 0 {
        PublicationState::Published
    } else if status == STATUS_OBJECT_NAME_COLLISION {
        PublicationState::NotPublished
    } else {
        PublicationState::Unknown
    }
}
struct RenameBuffer {
    storage: Vec<usize>,
    bytes: u32,
}
impl RenameBuffer {
    fn new(name: &OsStr, maximum: usize) -> io::Result<Self> {
        let units = name.encode_wide().count();
        let overflow =
            || io::Error::new(io::ErrorKind::InvalidInput, "Rename component is too long");
        let name_bytes = units.checked_mul(2).ok_or_else(overflow)?;
        let name_bytes = u32::try_from(name_bytes).map_err(|_| overflow())?;
        validate_component(name, maximum)?;
        let bytes = size_of::<FILE_RENAME_INFORMATION>()
            .checked_add(name_bytes as usize)
            .and_then(|value| value.checked_add(2))
            .ok_or_else(overflow)?;
        let bytes = u32::try_from(bytes).map_err(|_| overflow())?;
        let words = (bytes as usize).div_ceil(size_of::<usize>());
        let mut storage = Vec::<usize>::new();
        storage.try_reserve_exact(words).map_err(io::Error::other)?;
        storage.resize(words, 0);
        let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
        // SAFETY: usize alignment meets the generated structure on both CPUs;
        // checked storage covers its size, full UTF-16 tail and zero padding.
        // All fields use the generated ABI; basename has no terminator/separator.
        unsafe {
            ptr::addr_of_mut!((*info).Anonymous.Flags).write(0);
            ptr::addr_of_mut!((*info).RootDirectory).write(ptr::null_mut());
            ptr::addr_of_mut!((*info).FileNameLength).write(name_bytes);
            let tail = storage
                .as_mut_ptr()
                .cast::<u8>()
                .add(offset_of!(FILE_RENAME_INFORMATION, FileName))
                .cast::<u16>();
            for (index, unit) in name.encode_wide().enumerate() {
                tail.add(index).write(unit);
            }
        }
        Ok(Self { storage, bytes })
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        test_actor::RestrictedToken,
        test_fixture::{directory, operation},
    };
    use super::*;
    use crate::{BoundDirectory, NtStatusError, SyncState, WritableDirectory};
    use std::os::windows::fs::FileExt;

    fn writer(path: &std::path::Path) -> (BoundDirectory, WritableDirectory) {
        let read = BoundDirectory::open(&operation(path)).unwrap();
        let write = WritableDirectory::bind_resolved(&operation(path), read.identity()).unwrap();
        (read, write)
    }
    #[test]
    fn native_no_replace_edge_keeps_complete_same_object_and_release_retains_final_file() {
        let fixture = directory();
        let actor = RestrictedToken::enter().unwrap();
        let (read, write) = writer(fixture.path());
        let bytes = b"Subject: synthetic\r\n\r\ncomplete verified body\r\n";
        let ready = write.stage_bytes(bytes, || Ok(())).unwrap();
        let identity = ready.identity();
        let name = OsStr::new("result.eml");
        let attempt = ready.publish(name);
        assert_eq!(attempt.state(), PublicationState::Published);
        assert!(attempt.error().is_none());
        let (published, error) = attempt.into_parts();
        assert!(error.is_none());
        assert_eq!(published.state(), PublicationState::Published);
        assert_eq!(published.identity(), identity);
        assert_eq!(published.length(), bytes.len() as u64);
        assert_eq!(published.final_name(), name);
        assert_eq!(published.file_sync().state, SyncState::Succeeded);
        let observed = read.observe_child(name).unwrap().file.unwrap();
        assert_eq!(observed.identity, identity);
        assert_eq!(observed.size, bytes.len() as u64);
        let mut actual = [0; 128];
        let mut count = 0;
        loop {
            let got = published
                .owned
                .file
                .seek_read(&mut actual[count..], count as u64)
                .unwrap();
            if got == 0 {
                break;
            }
            count += got;
            assert!(count < actual.len(), "independent readback must reach EOF");
        }
        assert_eq!(&actual[..count], bytes);
        let final_path = operation(&fixture.path().join(name));
        assert_eq!(
            std::fs::File::open(&final_path).unwrap_err().raw_os_error(),
            Some(32)
        );
        // Release cannot undo the already observed edge, including if core has
        // received a stop meanwhile. This is release evidence, not core stop QA.
        let (effect, cleanup) = published.cleanup();
        assert_eq!(effect, PublicationState::Published);
        assert_eq!(cleanup.state, CleanupState::NotNeeded);
        assert!(cleanup.residual_name.is_none());
        assert!(cleanup.native_code.is_none());
        assert_eq!(std::fs::read(&final_path).unwrap(), bytes);
        assert_eq!(read.observe_child(name).unwrap().identity, identity);
        assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 1);
        drop(write);
        drop(read);
        drop(actor);
        fixture.close().unwrap();
    }

    #[test]
    fn native_rename_conflict_and_preedge_name_refusals_only_clean_owned_staging() {
        let fixture = directory();
        std::fs::write(fixture.path().join("winner.eml"), b"synthetic winner").unwrap();
        let actor = RestrictedToken::enter().unwrap();
        let read = BoundDirectory::open(&operation(fixture.path())).unwrap();
        // Obtain the public planning property before any write capability, and
        // compare it with an independently issued direct native DWORD query.
        let maximum = read.name_limit().unwrap();
        let mut native_maximum = 0;
        // SAFETY: live read-only directory and one writable DWORD output;
        // the direct SDK call is independent of the production helper.
        let queried = unsafe {
            windows_sys::Win32::Storage::FileSystem::GetVolumeInformationByHandleW(
                read.file.as_raw_handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut native_maximum,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
            )
        };
        assert_ne!(queried, 0, "{}", io::Error::last_os_error());
        assert!(native_maximum > 0);
        assert_eq!(maximum, native_maximum as usize);
        let write =
            WritableDirectory::bind_resolved(&operation(fixture.path()), read.identity()).unwrap();
        let winner = read.observe_child(OsStr::new("winner.eml")).unwrap();
        let attempt = write
            .stage_bytes(b"synthetic loser", || Ok(()))
            .unwrap()
            .publish(OsStr::new("winner.eml"));
        assert_eq!(attempt.state(), PublicationState::NotPublished);
        assert_eq!(
            attempt
                .error()
                .unwrap()
                .get_ref()
                .unwrap()
                .downcast_ref::<NtStatusError>()
                .unwrap()
                .nt_status(),
            STATUS_OBJECT_NAME_COLLISION
        );
        let (loser, error) = attempt.into_parts();
        assert!(error.is_some());
        let (effect, cleanup) = loser.cleanup();
        assert_eq!(effect, PublicationState::NotPublished);
        assert_eq!(cleanup.state, CleanupState::Complete);
        assert_eq!(
            read.observe_child(OsStr::new("winner.eml")).unwrap(),
            winner
        );
        assert_eq!(
            std::fs::read(fixture.path().join("winner.eml")).unwrap(),
            b"synthetic winner"
        );
        for name in ["", ".", "..", "a/b", r"a\b", "a:b", "NUL", "a\0b"] {
            let attempt = write
                .stage_bytes(b"owned synthetic", || Ok(()))
                .unwrap()
                .publish(OsStr::new(name));
            assert_eq!(attempt.state(), PublicationState::NotPublished, "{name:?}");
            assert_eq!(attempt.error().unwrap().kind(), io::ErrorKind::InvalidInput);
            assert_eq!(
                attempt.into_parts().0.cleanup().1.state,
                CleanupState::Complete
            );
        }
        let ready = write.stage_bytes(b"bounded synthetic", || Ok(())).unwrap();
        let identity = ready.identity();
        assert!((1..1_048_576).contains(&maximum));
        let exact = "x".repeat(maximum);
        let attempt = ready.publish(OsStr::new(&exact));
        assert_eq!(attempt.state(), PublicationState::Published);
        assert!(attempt.error().is_none());
        let (effect, release) = attempt.into_parts().0.cleanup();
        assert_eq!(effect, PublicationState::Published);
        assert_eq!(release.state, CleanupState::NotNeeded);
        assert_eq!(
            read.observe_child(OsStr::new(&exact)).unwrap().identity,
            identity
        );
        assert_eq!(
            std::fs::read(operation(&fixture.path().join(&exact))).unwrap(),
            b"bounded synthetic"
        );
        let oversized = "x".repeat(maximum + 1);
        let attempt = write
            .stage_bytes(b"over-limit control", || Ok(()))
            .unwrap()
            .publish(OsStr::new(&oversized));
        assert_eq!(attempt.state(), PublicationState::NotPublished);
        assert_eq!(attempt.error().unwrap().kind(), io::ErrorKind::InvalidInput);
        assert_eq!(
            attempt.into_parts().0.cleanup().1.state,
            CleanupState::Complete
        );
        assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 2);
        drop(write);
        drop(read);
        drop(actor);
        fixture.close().unwrap();
    }

    #[test]
    fn scripted_unknown_effect_releases_real_owned_storage_without_disposition_or_residual_guess() {
        use windows_sys::Win32::Foundation::STATUS_ACCESS_DENIED;
        assert_eq!(effect(0), PublicationState::Published);
        assert_eq!(effect(1), PublicationState::Published);
        assert_eq!(
            effect(STATUS_OBJECT_NAME_COLLISION),
            PublicationState::NotPublished
        );
        assert_eq!(effect(STATUS_ACCESS_DENIED), PublicationState::Unknown);
        let fixture = directory();
        let actor = RestrictedToken::enter().unwrap();
        let (read, write) = writer(fixture.path());
        let ready = write
            .stage_bytes(b"owned mapping control", || Ok(()))
            .unwrap();
        let (mut candidate, _) = ready.publish(OsStr::new("")).into_parts();
        // Mapping/control only: no claim that a native rename returned an
        // ambiguous status. Real owned storage must survive the Unknown branch.
        candidate.state = effect(STATUS_ACCESS_DENIED);
        let (effect, cleanup) = candidate.cleanup();
        assert_eq!(effect, PublicationState::Unknown);
        assert_eq!(cleanup.state, CleanupState::Unknown);
        assert!(cleanup.residual_name.is_none());
        assert!(cleanup.native_code.is_none());
        let entries: Vec<_> = std::fs::read_dir(fixture.path()).unwrap().collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            std::fs::read(entries[0].as_ref().unwrap().path()).unwrap(),
            b"owned mapping control"
        );
        drop(write);
        drop(read);
        drop(actor);
        fixture.close().unwrap();
    }
}
