//! One physical no-replace edge; final-copy/address qualification is separate.
use super::{containment, creation::Creation, path::validate_component};
use crate::{CleanupReceipt, CleanupState, Identity, PublicationState, SyncReceipt};
use std::{
    ffi::{OsStr, OsString},
    io,
    mem::{offset_of, size_of},
    os::windows::io::AsRawHandle,
    ptr,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_RENAME_INFO, FileRenameInfo, SetFileInformationByHandle,
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
    #[cfg(any(test, feature = "native-fixtures"))]
    native_edge: Option<crate::NativePublicationReceipt>,
}
impl PublishAttempt {
    /// Actual native call-return facts; absent when a pre-edge check refused.
    #[cfg(any(test, feature = "native-fixtures"))]
    pub fn native_edge(&self) -> Option<crate::NativePublicationReceipt> {
        self.native_edge
    }

    /// Exact native existing-name collision, independent of publication effects.
    pub fn is_name_collision(&self) -> bool {
        self.error.as_ref().is_some_and(crate::name_collision)
    }
    /// Documented unsupported native primitive result; no fallback is authorized.
    pub fn is_unsupported(&self) -> bool {
        self.error
            .as_ref()
            .is_some_and(crate::unsupported_publication)
    }
    /// Actual effect observed, independent of later release/cancellation errors.
    pub fn state(&self) -> PublicationState {
        self.candidate.state()
    }
    /// Original pre-edge or native rename cause, including the actual Win32 cause.
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
            RenameBuffer::new(name, maximum, &candidate.owned.parent_path()?)
        })();
        #[cfg(any(test, feature = "native-fixtures"))]
        let mut native_edge = None;
        let error = match preparation {
            Err(error) => Some(error),
            Ok(mut buffer) => {
                // SAFETY: generated aligned buffer holds an absolute validated
                // native path, null RootDirectory and replace=false. The exact
                // exclusive DELETE-capable candidate remains held throughout.
                let succeeded = unsafe {
                    SetFileInformationByHandle(
                        candidate.owned.file.as_raw_handle(),
                        FileRenameInfo,
                        buffer.storage.as_mut_ptr().cast(),
                        buffer.bytes,
                    )
                };
                let error = (succeeded == 0).then(io::Error::last_os_error);
                #[cfg(any(test, feature = "native-fixtures"))]
                {
                    native_edge = Some(crate::NativePublicationReceipt {
                        primitive: "SetFileInformationByHandle_FileRenameInfo",
                        succeeded: succeeded != 0,
                        raw_return: Some(succeeded),
                        native_code: error
                            .as_ref()
                            .and_then(|error| error.raw_os_error().map(i64::from)),
                    });
                }
                candidate.state = effect(error.as_ref());
                error
            }
        };
        PublishAttempt {
            candidate,
            error,
            #[cfg(any(test, feature = "native-fixtures"))]
            native_edge,
        }
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
fn effect(error: Option<&io::Error>) -> PublicationState {
    match error {
        None => PublicationState::Published,
        Some(error) if crate::name_collision(error) => PublicationState::NotPublished,
        Some(_) => PublicationState::Unknown,
    }
}
struct RenameBuffer {
    storage: Vec<usize>,
    bytes: u32,
}
impl RenameBuffer {
    fn new(name: &OsStr, maximum: usize, parent: &[u16]) -> io::Result<Self> {
        validate_component(name, maximum)?;
        let full_path = containment::append(parent, name)?;
        let units = full_path.len();
        let overflow =
            || io::Error::new(io::ErrorKind::InvalidInput, "Rename component is too long");
        let name_bytes = units.checked_mul(2).ok_or_else(overflow)?;
        let name_bytes = u32::try_from(name_bytes).map_err(|_| overflow())?;
        let bytes = offset_of!(FILE_RENAME_INFO, FileName)
            .checked_add(name_bytes as usize)
            .and_then(|value| value.checked_add(2))
            .ok_or_else(overflow)?;
        let bytes =
            u32::try_from(bytes.max(size_of::<FILE_RENAME_INFO>())).map_err(|_| overflow())?;
        let words = (bytes as usize).div_ceil(size_of::<usize>());
        let mut storage = Vec::<usize>::new();
        storage.try_reserve_exact(words).map_err(io::Error::other)?;
        storage.resize(words, 0);
        let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
        // SAFETY: usize alignment meets the generated structure on both CPUs;
        // checked storage covers its size, full UTF-16 tail and zero padding.
        // All fields use the generated ABI; the complete absolute native tail is terminated by zero padding.
        unsafe {
            ptr::addr_of_mut!((*info).Anonymous.ReplaceIfExists).write(false);
            ptr::addr_of_mut!((*info).RootDirectory).write(ptr::null_mut());
            ptr::addr_of_mut!((*info).FileNameLength).write(name_bytes);
            let tail = storage
                .as_mut_ptr()
                .cast::<u8>()
                .add(offset_of!(FILE_RENAME_INFO, FileName))
                .cast::<u16>();
            for (index, unit) in full_path.into_iter().enumerate() {
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
    use crate::{BoundDirectory, CreationDirectory, SyncState};
    use std::os::windows::fs::FileExt;

    fn writer(path: &std::path::Path) -> (BoundDirectory, CreationDirectory) {
        let read = BoundDirectory::open(&operation(path)).unwrap();
        let write = CreationDirectory::bind_resolved(&operation(path), read.identity()).unwrap();
        (read, write)
    }
    fn publication_record(name: &'static str, case: &str, origin: &str, fields: &str) {
        if let Some(directory) = std::env::var_os("EML_NATIVE_PUBLICATION_EVIDENCE_DIR") {
            let document = format!(
                "{{\"schema\":1,\"case\":\"{case}\",\"origin\":\"{origin}\",\"process_os\":\"windows\",\"process_architecture\":\"{}\",{fields}}}",
                std::env::consts::ARCH
            );
            crate::fixture_evidence::write_record(
                std::path::Path::new(&directory),
                name,
                &document,
            )
            .unwrap();
        }
    }
    #[test]
    fn native_no_replace_edge_keeps_complete_same_object_and_release_retains_final_file() {
        let fixture = directory();
        let actor = RestrictedToken::enter().unwrap();
        let (read, write) = writer(fixture.path());
        let bytes = b"Subject: synthetic\r\n\r\ncomplete verified body\r\n";
        let ready = write.stage_bytes(bytes, || Ok(())).unwrap();
        let sync = ready.file_sync();
        let parent_before = read.identity();
        let inventory_before = crate::fixture_evidence::inventory_json(fixture.path()).unwrap();
        let mut events = vec!["stage_returned"];
        let denied =
            std::fs::rename(fixture.path(), fixture.path().with_extension("moved")).unwrap_err();
        let actor_native_code = denied.raw_os_error();
        events.push("parent_rename_denied");
        assert_eq!(actor_native_code, Some(32));
        let identity = ready.identity();
        let name = OsStr::new("result.eml");
        let attempt = ready.publish(name);
        let edge = attempt.native_edge();
        let publication = attempt.state();
        events.push("native_publication_returned");

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
        // A separate metadata file object can coexist with the candidate's
        // zero sharing while retaining identity across candidate release.
        let identity_pin = read.pin_child(name).unwrap();
        assert_eq!(identity_pin.observation().identity, identity);
        assert_eq!(identity_pin.observation().kind, crate::EntryKind::File);
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
        events.push("cleanup_returned");

        assert_eq!(effect, PublicationState::Published);
        assert_eq!(cleanup.state, CleanupState::NotNeeded);
        assert!(cleanup.residual_name.is_none());
        assert!(cleanup.native_code.is_none());
        assert_eq!(std::fs::read(&final_path).unwrap(), bytes);
        assert_eq!(read.observe_child(name).unwrap().identity, identity);
        assert_eq!(identity_pin.observation().identity, identity);
        identity_pin.close().unwrap();
        assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 1);
        use crate::fixture_evidence::{
            cleanup_json, identity_json, inventory_json, native_code_json, native_edge_json,
            sync_json,
        };
        let requested = BoundDirectory::open(&operation(fixture.path())).unwrap();
        let events = events
            .iter()
            .map(|event| format!("\"{event}\""))
            .collect::<Vec<_>>()
            .join(",");
        publication_record(
            "native-parent-binding.json",
            "native-parent-binding",
            "actual_native_parent_interference",
            &format!(
                "\"events\":[{events}],\"parent_before\":{},\"held_parent_after\":{},\"requested_parent_after\":{},\"actor_native_code\":{},\"actor_changed_address\":{},\"requested_address_verified\":{},\"candidate_identity\":{},\"candidate_expected\":{bytes:?},\"candidate_actual\":{:?},\"file_sync\":{},\"namespace_sync\":null,\"native_edge\":{},\"publication\":\"{publication:?}\",\"cleanup\":{},\"inventory_before\":{inventory_before},\"inventory_after\":{},\"substitute_expected\":null,\"substitute_after\":null",
                identity_json(parent_before),
                identity_json(read.identity()),
                identity_json(requested.identity()),
                native_code_json(actor_native_code.map(i64::from)),
                requested.identity() != parent_before,
                requested.identity() == parent_before,
                identity_json(identity),
                &actual[..count],
                sync_json(sync),
                native_edge_json(edge),
                cleanup_json(&cleanup),
                inventory_json(fixture.path()).unwrap()
            ),
        );
        drop(requested);
        drop(write);
        drop(read);
        drop(actor);
        fixture.close().unwrap();
    }

    fn native_name_limit(read: &BoundDirectory) -> usize {
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
        native_maximum as usize
    }

    #[test]
    fn native_rename_conflict_and_preedge_name_refusals_only_clean_owned_staging() {
        let fixture = directory();
        let actor = RestrictedToken::enter().unwrap();
        let read = BoundDirectory::open(&operation(fixture.path())).unwrap();
        // Obtain the public planning property before any write capability, and
        // compare it with an independently issued direct native DWORD query.
        let maximum = read.name_limit().unwrap();
        assert_eq!(maximum, native_name_limit(&read));
        let write =
            CreationDirectory::bind_resolved(&operation(fixture.path()), read.identity()).unwrap();
        let ready = write.stage_bytes(b"synthetic loser", || Ok(())).unwrap();
        let candidate_identity = ready.identity();
        let sync = ready.file_sync();
        let mut events = vec!["stage_returned"];
        std::io::Write::write_all(
            &mut std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(fixture.path().join("winner.eml"))
                .unwrap(),
            b"synthetic winner",
        )
        .unwrap();
        events.push("winner_created");
        let winner = read.observe_child(OsStr::new("winner.eml")).unwrap();
        let winner_bytes = std::fs::read(fixture.path().join("winner.eml")).unwrap();
        let inventory_before = crate::fixture_evidence::inventory_json(fixture.path()).unwrap();
        let attempt = ready.publish(OsStr::new("winner.eml"));
        let edge = attempt.native_edge();
        let publication = attempt.state();
        events.push("native_publication_returned");
        assert_eq!(attempt.state(), PublicationState::NotPublished);
        assert_eq!(
            attempt.error().unwrap().raw_os_error(),
            Some(windows_sys::Win32::Foundation::ERROR_ALREADY_EXISTS as i32)
        );
        let (loser, error) = attempt.into_parts();
        assert!(error.is_some());
        let candidate_actual = crate::fixture_evidence::held_file_bytes(&loser.owned.file).unwrap();
        let (effect, cleanup) = loser.cleanup();
        events.push("cleanup_returned");
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
        use crate::fixture_evidence::{
            cleanup_json, identity_json, inventory_json, native_edge_json, signature_json,
            sync_json,
        };
        let after = read
            .observe_child(OsStr::new("winner.eml"))
            .unwrap()
            .file
            .unwrap();
        let after_bytes = std::fs::read(fixture.path().join("winner.eml")).unwrap();
        let events = events
            .iter()
            .map(|event| format!("\"{event}\""))
            .collect::<Vec<_>>()
            .join(",");
        publication_record(
            "native-racing-winner.json",
            "native-racing-winner",
            "actual_native_publication",
            &format!(
                "\"events\":[{events}],\"candidate_identity\":{},\"candidate_expected\":{:?},\"candidate_actual\":{candidate_actual:?},\"file_sync\":{},\"native_edge\":{},\"publication\":\"{publication:?}\",\"cleanup\":{},\"winner_before\":{{\"signature\":{},\"bytes\":{winner_bytes:?}}},\"winner_after\":{{\"signature\":{},\"bytes\":{after_bytes:?}}},\"inventory_before\":{inventory_before},\"inventory_after\":{}",
                identity_json(candidate_identity),
                b"synthetic loser",
                sync_json(sync),
                native_edge_json(edge),
                cleanup_json(&cleanup),
                signature_json(winner.file.unwrap()),
                signature_json(after),
                inventory_json(fixture.path()).unwrap()
            ),
        );
        for name in ["", ".", "..", "a/b", r"a\b", "a:b", "NUL", "a\0b"] {
            let attempt = write
                .stage_bytes(b"owned synthetic", || Ok(()))
                .unwrap()
                .publish(OsStr::new(name));
            assert_eq!(attempt.state(), PublicationState::NotPublished, "{name:?}");
            assert!(
                attempt.native_edge().is_none(),
                "Name prechecks must not invent a native call"
            );
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
        use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS};
        assert_eq!(effect(None), PublicationState::Published);
        assert_eq!(
            effect(Some(&io::Error::from_raw_os_error(
                ERROR_ALREADY_EXISTS as i32
            ))),
            PublicationState::NotPublished
        );
        assert_eq!(
            effect(Some(&io::Error::from_raw_os_error(
                ERROR_ACCESS_DENIED as i32
            ))),
            PublicationState::Unknown
        );
        let fixture = directory();
        let actor = RestrictedToken::enter().unwrap();
        let (read, write) = writer(fixture.path());
        let ready = write
            .stage_bytes(b"owned mapping control", || Ok(()))
            .unwrap();
        let (mut candidate, _) = ready.publish(OsStr::new("")).into_parts();
        // Mapping/control only: no claim that a native rename returned an
        // ambiguous status. Real owned storage must survive the Unknown branch.
        candidate.state = effect(Some(&io::Error::from_raw_os_error(
            ERROR_ACCESS_DENIED as i32,
        )));
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
