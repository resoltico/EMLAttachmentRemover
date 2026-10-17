//! Permission-model evidence is required only at actual namespace mutation parents.
use std::{io, os::fd::AsFd};

fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "This destination does not provide the reviewed namespace ownership model; choose a supported private folder",
    )
}

#[cfg(target_os = "linux")]
fn linux_model(kind: i64) -> io::Result<()> {
    if [
        libc::EXT4_SUPER_MAGIC,
        libc::TMPFS_MAGIC,
        libc::XFS_SUPER_MAGIC,
        libc::BTRFS_SUPER_MAGIC,
    ]
    .contains(&kind)
    {
        Ok(())
    } else {
        Err(unsupported())
    }
}

#[cfg(target_os = "linux")]
fn overflow_uid(reader: impl io::Read) -> io::Result<u32> {
    use io::Read;
    let mut bytes = Vec::new();
    reader.take(33).read_to_end(&mut bytes)?;
    if bytes.len() > 32 {
        return Err(unsupported());
    }
    let raw = std::str::from_utf8(&bytes).map_err(|_| unsupported())?;
    let raw = raw.strip_suffix('\n').unwrap_or(raw);
    if raw.is_empty() || !raw.bytes().all(|c| c.is_ascii_digit()) {
        return Err(unsupported());
    }
    raw.parse().map_err(|_| unsupported())
}

#[cfg(target_os = "linux")]
pub(crate) fn admit_model(fd: impl AsFd, owner: u32) -> io::Result<()> {
    let actual = rustix::fs::fstatfs(fd)?;
    linux_model(actual.f_type as i64)?;
    // Ownership translations can report this configurable sentinel for unmapped
    // owners. Such a number must never supply our current-user/root trust proof.
    let overflow = overflow_uid(std::fs::File::open("/proc/sys/kernel/overflowuid")?)?;
    if owner == overflow {
        return Err(unsupported());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn ownership_enabled(flags: u32) -> io::Result<()> {
    if flags & libc::MNT_IGNORE_OWNERSHIP as u32 != 0 {
        Err(unsupported())
    } else {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn admit_model(fd: impl AsFd, _owner: u32) -> io::Result<()> {
    ownership_enabled(rustix::fs::fstatfs(fd)?.f_flags)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "macos")]
    #[test]
    fn ignoring_ownership_is_refused_even_with_unrelated_flags() {
        assert!(ownership_enabled(0).is_ok());
        assert!(ownership_enabled(libc::MNT_IGNORE_OWNERSHIP as u32).is_err());
        assert!(ownership_enabled(libc::MNT_IGNORE_OWNERSHIP as u32 | 1).is_err());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn only_reviewed_linux_permission_models_are_admitted() {
        for kind in [0xef53, 0x01021994, 0x58465342, 0x9123683e] {
            assert!(linux_model(kind).is_ok());
        }
        for kind in [0, -1, 0x794c7630, 0x65735546, 0x6969, 0xff534d42, 0x4d44] {
            assert!(linux_model(kind).is_err());
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn overflow_identity_evidence_is_bounded_numeric_and_fails_closed() {
        assert_eq!(overflow_uid(&b"65534\n"[..]).unwrap(), 65534);
        assert_eq!(overflow_uid(&b"0\n"[..]).unwrap(), 0);
        for bytes in [
            b"".as_slice(),
            b"-1",
            b" 42",
            b"42\n\n",
            b"4294967296",
            &[b'1'; 33],
        ] {
            assert!(overflow_uid(bytes).is_err());
        }
        struct Failed;
        impl io::Read for Failed {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("injected read failure"))
            }
        }
        assert!(overflow_uid(Failed).is_err());
    }
}
