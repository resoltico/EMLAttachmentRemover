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
pub(super) fn admit(file: &File) -> io::Result<()> {
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
        return Err(unsupported());
    }
    // SAFETY: nonpending native return completes writes to the IO_STATUS_BLOCK.
    let completed_status = unsafe { io_status.Anonymous.Status };
    completed(status, completed_status, io_status.Information, &device)?;
    if handle::directory_identity(file)? != identity {
        return Err(unsupported());
    }
    let Identity::Windows { .. } = identity else {
        return Err(unsupported());
    };
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
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
            Foundation::{GetHandleInformation, INVALID_HANDLE_VALUE, WAIT_FAILED, WAIT_OBJECT_0},
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
                assert_ne!(
                    // SAFETY: concrete owner retains this live event and writable flags.
                    unsafe { GetHandleInformation(held.query.as_raw_handle(), &mut flags) },
                    0
                );
                *waits.borrow_mut() += 1;
                if *waits.borrow() == 1 {
                    // SAFETY: deliberately invalid kernel handle produces a real
                    // failed wait; it is not the owned query event and is not closed.
                    assert_eq!(
                        // SAFETY: deliberate invalid handle has no associated user storage.
                        unsafe { WaitForSingleObject(INVALID_HANDLE_VALUE, 0) },
                        WAIT_FAILED
                    );
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
                    assert_eq!(
                        // SAFETY: sole owner retains this event through completer join.
                        unsafe { WaitForSingleObject(held.query.as_raw_handle(), INFINITE) },
                        WAIT_OBJECT_0
                    );
                    completion
                        .take()
                        .expect("one owned completion thread")
                        .join()
                        .expect("native event completion");
                    held.io_status.Anonymous.Status = STATUS_SUCCESS;
                    held.io_status.Information = size_of::<FILE_FS_DEVICE_INFORMATION>();
                    held.device.DeviceType = FILE_DEVICE_DISK;
                }
            },
        );
        assert!(completion.is_none());
        assert_eq!(*waits.borrow(), 2);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        {
            let mut held = owners.borrow_mut();
            // SAFETY: initialized generated Status arm, after actual thread join.
            completed(
                STATUS_SUCCESS,
                unsafe { held.io_status.Anonymous.Status },
                held.io_status.Information,
                &held.device,
            )
            .unwrap();
            assert!(
                completed(
                    STATUS_PENDING,
                    STATUS_SUCCESS,
                    held.io_status.Information,
                    &held.device
                )
                .is_err()
            );
            held.device.Characteristics = FILE_REMOTE_DEVICE;
            assert!(
                completed(
                    STATUS_SUCCESS,
                    STATUS_SUCCESS,
                    held.io_status.Information,
                    &held.device
                )
                .is_err()
            );
        }
        drop(owners);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn only_exact_complete_local_disk_success_qualifies() {
        let good = FILE_FS_DEVICE_INFORMATION {
            DeviceType: FILE_DEVICE_DISK,
            Characteristics: 0,
        };
        assert!(
            completed(
                STATUS_SUCCESS,
                STATUS_SUCCESS,
                size_of::<FILE_FS_DEVICE_INFORMATION>(),
                &good
            )
            .is_ok()
        );
        for (status, completion, length) in [
            (STATUS_PENDING, STATUS_SUCCESS, 8),
            (STATUS_SUCCESS, STATUS_PENDING, 8),
            (1, STATUS_SUCCESS, 8),
            (STATUS_SUCCESS, 1, 8),
            (STATUS_SUCCESS, STATUS_SUCCESS, 0),
            (STATUS_SUCCESS, STATUS_SUCCESS, 9),
            (STATUS_SUCCESS, STATUS_SUCCESS, usize::MAX),
            (-1, STATUS_SUCCESS, 8),
            (STATUS_SUCCESS, -1, 8),
            (STATUS_SUCCESS, STATUS_SUCCESS, 7),
        ] {
            assert!(completed(status, completion, length, &good).is_err());
        }
        assert!(
            completed(
                0,
                0,
                8,
                &FILE_FS_DEVICE_INFORMATION {
                    DeviceType: FILE_DEVICE_DISK,
                    Characteristics: FILE_REMOTE_DEVICE
                }
            )
            .is_err()
        );
        assert!(
            completed(
                0,
                0,
                8,
                &FILE_FS_DEVICE_INFORMATION {
                    DeviceType: u32::MAX,
                    Characteristics: 0
                }
            )
            .is_err()
        );
    }
}
