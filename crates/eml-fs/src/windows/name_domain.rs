//! Read-only local-NTFS qualification on an exclusively owned synchronous query handle.
use super::handle;
use crate::{BoundDirectory, Identity};
use std::{fs::File, io, mem::size_of, os::windows::io::AsRawHandle, ptr};
use windows_sys::{
    Wdk::{
        Storage::FileSystem::{FileFsDeviceInformation, NtQueryVolumeInformationFile},
        System::SystemServices::{FILE_FS_DEVICE_INFORMATION, FILE_REMOTE_DEVICE},
    },
    Win32::{
        Foundation::{STATUS_PENDING, STATUS_SUCCESS},
        Storage::FileSystem::{
            FILE_DEVICE_DISK, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE,
            GetVolumeInformationByHandleW, SYNCHRONIZE,
        },
        System::{
            IO::IO_STATUS_BLOCK,
            Threading::{INFINITE, WaitForSingleObject},
        },
    },
};
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "Generated destinations require a qualified local NTFS volume; choose a supported private folder",
    )
}
fn completed(
    status: i32,
    completed_status: i32,
    information: usize,
    device: &FILE_FS_DEVICE_INFORMATION,
) -> io::Result<()> {
    if status < 0 {
        return Err(super::status::error(status));
    }
    if completed_status < 0 {
        return Err(super::status::error(completed_status));
    }
    if status != STATUS_SUCCESS
        || completed_status != STATUS_SUCCESS
        || information != size_of::<FILE_FS_DEVICE_INFORMATION>()
        || device.DeviceType != FILE_DEVICE_DISK
        || device.Characteristics & FILE_REMOTE_DEVICE != 0
    {
        return Err(unsupported());
    }
    Ok(())
}
// No timeout or failed wait can end ownership of outstanding native buffers.
fn retain_until_settled(mut pending: impl FnMut() -> bool, mut wait: impl FnMut()) {
    while pending() {
        wait();
    }
}
struct NativeQuery {
    owner: File,
    identity: Identity,
    status: i32,
    io_status: Box<IO_STATUS_BLOCK>,
    device: Box<FILE_FS_DEVICE_INFORMATION>,
    #[cfg(test)]
    filesystem_units: Vec<u16>,
}

pub(super) fn admit(file: &File) -> io::Result<()> {
    let query = query(file)?;
    // Retain this dedicated handle and both buffers through all qualification.
    let _query_owner = &query.owner;
    // SAFETY: query() retains pending storage until completion; a nonpending
    // native return also completes all writes to the generated status block.
    let completed_status = unsafe { query.io_status.Anonymous.Status };
    completed(
        query.status,
        completed_status,
        query.io_status.Information,
        &query.device,
    )?;
    if handle::directory_identity(file)? != query.identity {
        return Err(unsupported());
    }
    let Identity::Windows { .. } = query.identity else {
        return Err(unsupported());
    };
    Ok(())
}

fn query(file: &File) -> io::Result<NativeQuery> {
    let identity = handle::directory_identity(file)?;
    let observation = BoundDirectory {
        file: file.try_clone()?,
        identity,
    };
    let resolved = observation.resolved_path()?;
    let query = handle::open(
        &resolved,
        FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
    )?;
    if handle::directory_identity(&query)? != identity {
        return Err(unsupported());
    }
    // This dedicated handle is exclusively owned by this call; the query buffers
    // remain owned until completion even if a driver unexpectedly returns pending.
    let mut filesystem = [0u16; 32];
    // SAFETY: live held directory and fixed writable name buffer; unused outputs are null.
    if unsafe {
        GetVolumeInformationByHandleW(
            query.as_raw_handle(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            filesystem.as_mut_ptr(),
            filesystem.len() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let length = filesystem
        .iter()
        .position(|&u| u == 0)
        .ok_or_else(unsupported)?;
    if filesystem[..length] != [78, 84, 70, 83] {
        return Err(unsupported());
    }
    let mut io_status = Box::new(IO_STATUS_BLOCK::default());
    io_status.Anonymous.Status = STATUS_PENDING;
    let mut device = Box::new(FILE_FS_DEVICE_INFORMATION::default());
    // SAFETY: generated ABI, exact initialized storage and byte size. Both heap
    // owners and the file remain alive until the operation is actually settled.
    let status = unsafe {
        NtQueryVolumeInformationFile(
            query.as_raw_handle(),
            &mut *io_status,
            (&mut *device as *mut FILE_FS_DEVICE_INFORMATION).cast(),
            size_of::<FILE_FS_DEVICE_INFORMATION>() as u32,
            FileFsDeviceInformation,
        )
    };
    if status == STATUS_PENDING {
        // The fixed query profile is synchronous; unexpected pending never grants
        // creation. Retain all memory and the handle through actual settlement.
        retain_until_settled(
            || {
                // SAFETY: owned generated status storage remains live through
                // kernel completion; volatile reads observe driver settlement.
                unsafe {
                    ptr::read_volatile(&raw const io_status.Anonymous.Status) == STATUS_PENDING
                }
            },
            || {
                // SAFETY: dedicated live synchronous query handle. No other I/O
                // can make another operation's completion appear to settle this one.
                let waited = unsafe { WaitForSingleObject(query.as_raw_handle(), INFINITE) };
                if waited != 0 {
                    // A failed wait is not completion. Keep owners and permit the
                    // enclosing owned worker's explicit termination if the driver
                    // cannot settle; never return borrowed kernel storage early.
                    std::thread::park_timeout(std::time::Duration::from_millis(10));
                }
            },
        );
    }
    Ok(NativeQuery {
        owner: query,
        identity,
        status,
        io_status,
        device,
        #[cfg(test)]
        filesystem_units: filesystem[..length].to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(name: &'static str, case: &str, origin: &str, fields: &str) {
        if let Some(directory) = std::env::var_os("EML_WINDOWS_EVIDENCE_DIR") {
            let document = format!(
                "{{\"schema\":1,\"case\":\"{case}\",\"origin\":\"{origin}\",\"process_os\":\"windows\",\"process_architecture\":\"{}\",{fields}}}",
                std::env::consts::ARCH
            );
            crate::fixture_evidence::write_record(
                &std::path::PathBuf::from(directory).join("ntfs-query"),
                name,
                &document,
            )
            .unwrap();
        }
    }
    #[test]
    fn native_locality_query_retains_actual_completion_fields() {
        use windows_sys::Win32::Foundation::{GetHandleInformation, GetLastError};
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fs-tests");
        std::fs::create_dir_all(&base).unwrap();
        let root = tempfile::tempdir_in(base).unwrap();
        let file = handle::open(
            root.path(),
            FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        )
        .unwrap();
        let before = handle::directory_identity(&file).unwrap();
        let queried = query(&file).unwrap();
        let query_identity = handle::directory_identity(&queried.owner).unwrap();
        let after = handle::directory_identity(&file).unwrap();
        // SAFETY: query() retained the block until settlement; it initializes Status.
        let completion_status = unsafe { queried.io_status.Anonymous.Status };
        let mut flags = 0;
        // SAFETY: the query owner and one writable DWORD remain live.
        let handle_return =
            unsafe { GetHandleInformation(queried.owner.as_raw_handle(), &mut flags) };
        // SAFETY: capture this thread's native error immediately, before formatting/I/O.
        let handle_error = (handle_return == 0).then(|| unsafe { GetLastError() });
        let accepted = completed(
            queried.status,
            completion_status,
            queried.io_status.Information,
            &queried.device,
        )
        .is_ok();
        record(
            "ntfs-query-native.json",
            "native-local-ntfs-query",
            "native_ntqueryvolumeinformationfile",
            &format!(
                "\"directory_identity_before\":{},\"directory_identity_after\":{},\"query_identity\":{},\"filesystem_units\":{:?},\"return_status\":{},\"completion_status\":{completion_status},\"information\":{},\"device_type\":{},\"characteristics\":{},\"handle_information_return\":{handle_return},\"handle_information_error\":{},\"handle_information_flags\":{flags},\"validator_accepted\":{accepted}",
                crate::fixture_evidence::identity_json(before),
                crate::fixture_evidence::identity_json(after),
                crate::fixture_evidence::identity_json(query_identity),
                queried.filesystem_units,
                queried.status,
                queried.io_status.Information,
                queried.device.DeviceType,
                queried.device.Characteristics,
                crate::fixture_evidence::native_code_json(handle_error.map(i64::from))
            ),
        );
        assert_eq!(before, after);
        assert_eq!(before, query_identity);
        assert_ne!(handle_return, 0);
        assert!(accepted);
        drop(queried);
        drop(file);
        root.close().unwrap();
    }

    #[test]
    fn native_failed_wait_retains_concrete_query_owners_until_event_and_thread_settle() {
        use std::{
            cell::RefCell,
            os::windows::io::FromRawHandle,
            sync::{
                Arc,
                atomic::{AtomicUsize, Ordering},
                mpsc,
            },
        };
        use windows_sys::Win32::{
            Foundation::{
                ERROR_INVALID_HANDLE, GetHandleInformation, GetLastError, WAIT_FAILED,
                WAIT_OBJECT_0,
            },
            System::Threading::{CreateEventW, SetEvent},
        };

        struct QueryOwners {
            query: File,
            io_status: Box<IO_STATUS_BLOCK>,
            device: Box<FILE_FS_DEVICE_INFORMATION>,
            drops: Arc<AtomicUsize>,
        }
        impl Drop for QueryOwners {
            fn drop(&mut self) {
                self.drops.fetch_add(1, Ordering::SeqCst);
            }
        }
        // SAFETY: no security descriptor/name, manual-reset event starts nonsignalled.
        let event = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
        assert!(!event.is_null(), "{}", io::Error::last_os_error());
        let drops = Arc::new(AtomicUsize::new(0));
        let mut io_status = Box::new(IO_STATUS_BLOCK::default());
        io_status.Anonymous.Status = STATUS_PENDING;
        let owners = RefCell::new(QueryOwners {
            // SAFETY: successful CreateEventW transfers this unique CloseHandle owner.
            query: unsafe { File::from_raw_handle(event) },
            io_status,
            device: Box::new(FILE_FS_DEVICE_INFORMATION::default()),
            drops: drops.clone(),
        });
        let addresses = {
            let held = owners.borrow();
            (&raw const *held.io_status, &raw const *held.device)
        };
        let (release, released) = mpsc::channel();
        // Only the raw value crosses threads. The sole event owner above remains
        // retained through SetEvent and actual thread join, not elapsed time.
        let event_value = event as usize;
        let mut completion = Some(std::thread::spawn(move || {
            released.recv().expect("test owner must release completion");
            // SAFETY: main-thread concrete owner outlives this thread's join.
            assert_ne!(unsafe { SetEvent(event_value as _) }, 0);
        }));
        let initial = {
            let held = owners.borrow();
            // SAFETY: initialized Status arm, confined to this test thread.
            let status = unsafe { held.io_status.Anonymous.Status };
            format!(
                "{{\"completion_status\":{status},\"information\":{},\"device_type\":{},\"characteristics\":{},\"owner_drop_count\":{}}}",
                held.io_status.Information,
                held.device.DeviceType,
                held.device.Characteristics,
                drops.load(Ordering::SeqCst)
            )
        };
        let wait_observations = RefCell::new(Vec::new());
        let waits = RefCell::new(0);
        retain_until_settled(
            || {
                let held = owners.borrow();
                assert_eq!(drops.load(Ordering::SeqCst), 0);
                // SAFETY: generated union contains the initialized Status arm;
                // only this thread reads/writes it, after actual worker join.
                unsafe { held.io_status.Anonymous.Status == STATUS_PENDING }
            },
            || {
                let mut held = owners.borrow_mut();
                assert_eq!(
                    addresses,
                    (&raw const *held.io_status, &raw const *held.device)
                );
                assert_eq!(drops.load(Ordering::SeqCst), 0);
                let mut flags = 0;
                // SAFETY: the concrete owner still holds the live kernel event.
                // SAFETY: concrete owner retains the live event and writable DWORD.
                let handle_return =
                    unsafe { GetHandleInformation(held.query.as_raw_handle(), &mut flags) };
                // SAFETY: capture native failure immediately, before assertions or I/O.
                let handle_error = (handle_return == 0).then(|| unsafe { GetLastError() });
                assert_ne!(handle_return, 0);
                *waits.borrow_mut() += 1;
                let wait_return;
                let wait_error;
                if *waits.borrow() == 1 {
                    // SAFETY: NULL has no associated object or user storage. Unlike
                    // -1 (the current-process pseudo handle), it is an invalid wait
                    // target; the owned query event remains held independently.
                    let failed_wait = unsafe { WaitForSingleObject(ptr::null_mut(), 0) };
                    // SAFETY: reads this thread's native error immediately after
                    // the failed wait, before assertions or other native calls.
                    let failed_wait_error = unsafe { GetLastError() };
                    wait_return = failed_wait;
                    wait_error = Some(failed_wait_error);
                    assert_eq!(failed_wait, WAIT_FAILED);
                    assert_eq!(failed_wait_error, ERROR_INVALID_HANDLE);
                    // A failed native wait cannot alter pending status or release
                    // any owner. Release the controlled completer only afterward.
                    // SAFETY: initialized Status arm, confined to this thread.
                    assert_eq!(unsafe { held.io_status.Anonymous.Status }, STATUS_PENDING);
                    assert_eq!(held.io_status.Information, 0);
                    assert_eq!(held.device.DeviceType, 0);
                    release
                        .send(())
                        .expect("controlled completer remains owned");
                } else {
                    assert_eq!(*waits.borrow(), 2);
                    // SAFETY: live owned kernel event; the controlled thread signals
                    // this event, then is joined before modeled completion is stored.
                    // SAFETY: sole owner retains the event until real completion and join.
                    wait_return =
                        unsafe { WaitForSingleObject(held.query.as_raw_handle(), INFINITE) };
                    // SAFETY: capture a native failure immediately if this wait did fail.
                    wait_error = (wait_return == WAIT_FAILED).then(|| unsafe { GetLastError() });
                    assert_eq!(wait_return, WAIT_OBJECT_0);
                    completion
                        .take()
                        .expect("one owned completion thread")
                        .join()
                        .expect("native event completion");
                    held.io_status.Anonymous.Status = STATUS_SUCCESS;
                    held.io_status.Information = size_of::<FILE_FS_DEVICE_INFORMATION>();
                    held.device.DeviceType = FILE_DEVICE_DISK;
                }
                let stable = addresses == (&raw const *held.io_status, &raw const *held.device);
                // SAFETY: initialized Status arm, read only on this thread after join.
                let completion_status = unsafe { held.io_status.Anonymous.Status };
                wait_observations.borrow_mut().push(format!("{{\"ordinal\":{},\"native_wait_return\":{wait_return},\"native_wait_error\":{},\"completion_status\":{completion_status},\"information\":{},\"device_type\":{},\"characteristics\":{},\"handle_information_return\":{handle_return},\"handle_information_error\":{},\"handle_information_flags\":{flags},\"owner_drop_count\":{},\"buffers_stable\":{stable}}}", *waits.borrow(), crate::fixture_evidence::native_code_json(wait_error.map(i64::from)), held.io_status.Information, held.device.DeviceType, held.device.Characteristics, crate::fixture_evidence::native_code_json(handle_error.map(i64::from)), drops.load(Ordering::SeqCst)));
            },
        );
        assert!(completion.is_none());
        assert_eq!(*waits.borrow(), 2);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        let final_state = {
            let held = owners.borrow();
            // SAFETY: controlled completion is stored only after the actual thread join.
            let completion_status = unsafe { held.io_status.Anonymous.Status };
            completed(
                STATUS_SUCCESS,
                completion_status,
                held.io_status.Information,
                &held.device,
            )
            .unwrap();
            let pending_accepted = completed(
                STATUS_PENDING,
                completion_status,
                held.io_status.Information,
                &held.device,
            )
            .is_ok();
            let remote = FILE_FS_DEVICE_INFORMATION {
                DeviceType: held.device.DeviceType,
                Characteristics: FILE_REMOTE_DEVICE,
            };
            let remote_accepted = completed(
                STATUS_SUCCESS,
                completion_status,
                held.io_status.Information,
                &remote,
            )
            .is_ok();
            assert!(!pending_accepted && !remote_accepted);
            format!(
                "{{\"completion_status\":{completion_status},\"information\":{},\"device_type\":{},\"characteristics\":{},\"pending_return_validator_accepted\":{pending_accepted},\"remote_validator_accepted\":{remote_accepted}}}",
                held.io_status.Information, held.device.DeviceType, held.device.Characteristics
            )
        };
        drop(owners);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        record(
            "ntfs-query-pending-owners.json",
            "pending-owner-lifetime",
            "controlled_completion_real_event",
            &format!(
                "\"initial\":{initial},\"waits\":[{}],\"completion_thread_joined\":{},\"final\":{final_state},\"owner_drop_count_after\":{}",
                wait_observations.borrow().join(","),
                completion.is_none(),
                drops.load(Ordering::SeqCst)
            ),
        );
    }
    #[test]
    fn only_exact_complete_local_disk_success_qualifies() {
        let rows = [
            ("exact-local-success", 0, 0, 8, FILE_DEVICE_DISK, 0, true),
            (
                "return-pending",
                STATUS_PENDING,
                0,
                8,
                FILE_DEVICE_DISK,
                0,
                false,
            ),
            (
                "completion-pending",
                0,
                STATUS_PENDING,
                8,
                FILE_DEVICE_DISK,
                0,
                false,
            ),
            ("return-positive", 1, 0, 8, FILE_DEVICE_DISK, 0, false),
            ("completion-positive", 0, 1, 8, FILE_DEVICE_DISK, 0, false),
            ("information-zero", 0, 0, 0, FILE_DEVICE_DISK, 0, false),
            ("information-long", 0, 0, 9, FILE_DEVICE_DISK, 0, false),
            (
                "information-overflow",
                0,
                0,
                usize::MAX,
                FILE_DEVICE_DISK,
                0,
                false,
            ),
            ("return-failed", -1, 0, 8, FILE_DEVICE_DISK, 0, false),
            ("completion-failed", 0, -1, 8, FILE_DEVICE_DISK, 0, false),
            ("information-short", 0, 0, 7, FILE_DEVICE_DISK, 0, false),
            (
                "remote-characteristic",
                0,
                0,
                8,
                FILE_DEVICE_DISK,
                FILE_REMOTE_DEVICE,
                false,
            ),
            ("unknown-device", 0, 0, 8, u32::MAX, 0, false),
        ];
        let mut observed = Vec::new();
        let mut accepted = Vec::new();
        for (case, status, completion, length, kind, characteristics, expected) in rows {
            let actual = completed(
                status,
                completion,
                length,
                &FILE_FS_DEVICE_INFORMATION {
                    DeviceType: kind,
                    Characteristics: characteristics,
                },
            )
            .is_ok();
            observed.push(format!("{{\"case\":\"{case}\",\"return_status\":{status},\"completion_status\":{completion},\"information\":{length},\"device_type\":{kind},\"characteristics\":{characteristics},\"validator_accepted\":{actual}}}"));
            accepted.push((case, actual, expected));
        }
        record(
            "ntfs-query-adapters.json",
            "completed-status-adapters",
            "controlled_completed_status_inputs",
            &format!("\"cases\":[{}]", observed.join(",")),
        );
        for (case, actual, expected) in accepted {
            assert_eq!(actual, expected, "{case}");
        }
    }
}
