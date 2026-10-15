//! Real owned-fixture mount-point creation, shared by native reparse controls.
use std::{
    fs::{File, OpenOptions},
    io,
    mem::{offset_of, size_of},
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle},
    path::Path,
    ptr,
};
use windows_sys::{
    Wdk::Storage::FileSystem::{REPARSE_DATA_BUFFER, REPARSE_DATA_BUFFER_0_1},
    Win32::{
        Foundation::GENERIC_WRITE,
        Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
            FILE_SHARE_READ, FILE_SHARE_WRITE,
        },
        System::{IO::DeviceIoControl, Ioctl::FSCTL_SET_REPARSE_POINT},
    },
};

pub(crate) fn writer(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .access_mode(GENERIC_WRITE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}
pub(crate) fn set_mount_point(file: &File, target: &Path) -> io::Result<()> {
    let target = std::fs::canonicalize(target)?;
    let target: Vec<_> = target.as_os_str().encode_wide().collect();
    if !target.starts_with(&[92, 92, 63, 92]) || target.get(5) != Some(&58) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Owned junction control requires a resolved DOS target",
        ));
    }
    let mut substitute = vec![92, 63, 63, 92];
    substitute.extend_from_slice(&target[4..]);
    let print = &target[4..];
    let path_units = substitute
        .len()
        .checked_add(print.len())
        .and_then(|n| n.checked_add(2))
        .ok_or_else(|| io::Error::other("Reparse length overflow"))?;
    let header = offset_of!(REPARSE_DATA_BUFFER, Anonymous);
    let path_offset = header + offset_of!(REPARSE_DATA_BUFFER_0_1, PathBuffer);
    let total = path_offset
        .checked_add(
            path_units
                .checked_mul(2)
                .ok_or_else(|| io::Error::other("Reparse length overflow"))?,
        )
        .ok_or_else(|| io::Error::other("Reparse length overflow"))?;
    if total > 16 * 1024 {
        return Err(io::Error::other(
            "Owned reparse control exceeds native bound",
        ));
    }
    let data_len = u16::try_from(total - header).map_err(io::Error::other)?;
    let sub_len = u16::try_from(substitute.len() * 2).map_err(io::Error::other)?;
    let print_offset = u16::try_from((substitute.len() + 1) * 2).map_err(io::Error::other)?;
    let print_len = u16::try_from(print.len() * 2).map_err(io::Error::other)?;
    let mut storage = vec![
        0u64;
        total
            .max(size_of::<REPARSE_DATA_BUFFER>())
            .div_ceil(size_of::<u64>())
    ];
    let buffer = storage.as_mut_ptr().cast::<REPARSE_DATA_BUFFER>();
    let mut returned = 0;
    // SAFETY: generated structures supply all offsets; u64 storage meets their
    // alignment and covers full struct plus checked UTF-16 tail. All lengths are
    // bounded above, buffers remain live throughout this synchronous native call.
    // The handle owns the exact freshly created test directory, never owner data.
    let success = unsafe {
        buffer.write(REPARSE_DATA_BUFFER::default());
        ptr::addr_of_mut!((*buffer).ReparseTag)
            .write(windows_sys::Win32::System::SystemServices::IO_REPARSE_TAG_MOUNT_POINT);
        ptr::addr_of_mut!((*buffer).ReparseDataLength).write(data_len);
        let point = ptr::addr_of_mut!((*buffer).Anonymous.MountPointReparseBuffer);
        ptr::addr_of_mut!((*point).SubstituteNameOffset).write(0);
        ptr::addr_of_mut!((*point).SubstituteNameLength).write(sub_len);
        ptr::addr_of_mut!((*point).PrintNameOffset).write(print_offset);
        ptr::addr_of_mut!((*point).PrintNameLength).write(print_len);
        let path = storage
            .as_mut_ptr()
            .cast::<u8>()
            .add(path_offset)
            .cast::<u16>();
        ptr::copy_nonoverlapping(substitute.as_ptr(), path, substitute.len());
        ptr::copy_nonoverlapping(print.as_ptr(), path.add(substitute.len() + 1), print.len());
        DeviceIoControl(
            file.as_raw_handle(),
            FSCTL_SET_REPARSE_POINT,
            buffer.cast(),
            total as u32,
            ptr::null_mut(),
            0,
            &mut returned,
            ptr::null_mut(),
        )
    };
    if success == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
