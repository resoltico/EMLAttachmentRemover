//! Duplicate the private inherited writer without claiming ownership of raw fd3.
use std::{
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
};

/// A checked, nonblocking private control writer owned by this Rust value.
/// The original inherited descriptor remains open until worker process exit;
/// the worker contract forbids spawning descendants or unrelated fd3 use.
pub struct InheritedControlWriter(OwnedFd);
impl InheritedControlWriter {
    /// Duplicate descriptor3 before starting worker threads. This does not close
    /// or adopt the source descriptor, so it cannot create duplicate Rust owners.
    pub fn duplicate() -> io::Result<Self> {
        // SAFETY: fcntl duplicates an OS descriptor atomically. A successful
        // returned descriptor is fresh and uniquely transferred into OwnedFd.
        let raw = unsafe { libc::fcntl(3, libc::F_DUPFD_CLOEXEC, 4) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful F_DUPFD_CLOEXEC returned a fresh open descriptor.
        let descriptor = unsafe { OwnedFd::from_raw_fd(raw) };
        // SAFETY: initialized stat storage and a live owned descriptor.
        let mut metadata = unsafe { std::mem::zeroed::<libc::stat>() };
        // SAFETY: metadata storage is writable for fstat; ownership stays here.
        if unsafe { libc::fstat(descriptor.as_raw_fd(), &mut metadata) } < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: query the still-owned descriptor's flags without pointer arguments.
        let flags = unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_GETFL) };
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        if metadata.st_mode & libc::S_IFMT != libc::S_IFIFO
            || flags & libc::O_ACCMODE != libc::O_WRONLY
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Private control descriptor must be a write-only pipe",
            ));
        }
        // SAFETY: this pipe's open-file-description IO policy is changed, shared
        // with original3; CLOEXEC above is descriptor-local. There is no global
        // signal change. Nonblocking one-byte writes cannot strand ctrlc.
        if unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_SETNOSIGPIPE, 1) } < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: this is the same live owned pipe descriptor. Preserve all
        // existing status flags while adding nonblocking writes; no ownership moves.
        if unsafe {
            libc::fcntl(
                descriptor.as_raw_fd(),
                libc::F_SETFL,
                flags | libc::O_NONBLOCK,
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(descriptor))
    }
    /// Try exactly one atomic control byte. Errors (including WouldBlock) must
    /// stop the worker/control path; no retry or ignored acknowledgment failure.
    pub fn try_write(&self, byte: u8) -> io::Result<()> {
        // SAFETY: one initialized byte and this live owned write descriptor.
        let written = unsafe { libc::write(self.0.as_raw_fd(), (&byte as *const u8).cast(), 1) };
        if written == 1 {
            Ok(())
        } else if written < 0 {
            Err(io::Error::last_os_error())
        } else {
            Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "Private control byte was not written",
            ))
        }
    }
}
