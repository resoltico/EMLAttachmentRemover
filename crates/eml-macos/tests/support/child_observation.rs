//! Nonshipping owned child for actual signal-mask, stdio and lifetime checks.
#[cfg(not(target_os = "macos"))]
compile_error!("Native macOS child fixtures require a macOS host");
#[cfg(target_os = "macos")]
#[allow(unsafe_code, reason = "quality:fixture-signal-mask-observation")]
fn main() {
    use std::io::{Read, Write};
    let argument = std::env::args_os().nth(1).unwrap_or_default();
    if argument == "--full-control" || argument == "--broken-control" {
        let control = eml_macos::InheritedControlWriter::duplicate().unwrap();
        default_sigpipe();
        if argument == "--broken-control" {
            let mut release = [0];
            std::io::stdin().read_exact(&mut release).unwrap();
            assert_eq!(release, [1]);
            assert_eq!(
                control.try_write(b'R').unwrap_err().kind(),
                std::io::ErrorKind::BrokenPipe
            );
            println!("broken-control-survived");
            return;
        }
        for count in 0..=1024 * 1024 {
            match control.try_write(b'R') {
                Ok(()) => {}
                Err(error) => {
                    assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
                    println!("full-control:{count}");
                    return;
                }
            }
        }
        panic!("Private pipe did not reach its bounded nonblocking boundary");
    }
    if std::env::args_os()
        .nth(1)
        .is_some_and(|argument| argument == "--closed-stdio")
    {
        closed_stdio::run();
        return;
    }
    let _control = if std::env::args_os()
        .nth(1)
        .is_some_and(|argument| argument == "--control")
    {
        let control = eml_macos::InheritedControlWriter::duplicate().unwrap();
        control.try_write(b'R').unwrap();
        Some(control)
    } else {
        None
    };
    // SAFETY: query writes a valid sigset_t; no parent/global signal state changes.
    let blocked = unsafe {
        let mut set = std::mem::zeroed::<libc::sigset_t>();
        assert_eq!(libc::sigprocmask(0, std::ptr::null(), &mut set), 0);
        libc::sigismember(&set, libc::SIGINT)
    };
    println!("int-blocked={blocked}");
    std::io::stdout().flush().unwrap();
    let mut input = Vec::new();
    std::io::stdin()
        .lock()
        .take(65537)
        .read_to_end(&mut input)
        .unwrap();
    assert!(input.len() <= 65536);
    std::io::stdout().write_all(&input).unwrap();
    eprintln!("owned fixture reached EOF");
    std::process::exit(7);
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code, reason = "quality:fixture-sigpipe-default")]
fn default_sigpipe() {
    // SAFETY: this single-threaded fixture owns its entire process; no parent
    // or test-runner signal disposition changes. Remove Rust startup's ignored
    // SIGPIPE as a possible explanation for a successful EPIPE observation.
    assert_ne!(
        unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) },
        libc::SIG_ERR
    );
}

#[cfg(target_os = "macos")]
#[path = "../../../../tests/support/native_child.rs"]
mod native_child;
#[cfg(target_os = "macos")]
#[allow(unsafe_code, reason = "quality:fixture-close-stdio")]
mod closed_stdio {
    use std::{ffi::OsStr, io::Write, os::fd::FromRawFd, time::Duration};
    pub(crate) fn run() {
        let executable = std::env::current_exe().unwrap();
        // SAFETY: duplicate this fixture's stdout to a fresh CLOEXEC descriptor
        // above the collision range, transferring that unique copy to File.
        let descriptor = unsafe { libc::fcntl(1, libc::F_DUPFD_CLOEXEC, 10) };
        assert!(descriptor >= 10);
        let mut log = unsafe { std::fs::File::from_raw_fd(descriptor) };
        for descriptor in 0..=3 {
            // SAFETY: this isolated fixture has no IO threads and exclusively
            // changes its own process descriptors. The preserved log is >=10.
            if unsafe { libc::close(descriptor) } != 0 {
                assert_eq!(
                    std::io::Error::last_os_error().raw_os_error(),
                    Some(libc::EBADF)
                );
            }
        }
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let observation =
                eml_macos::spawn_with_control_pipe(&executable, &[OsStr::new("--control")])
                    .unwrap();
            let child = super::native_child::SettleWithLog(observation.child.unwrap(), &mut log);
            assert!(observation.issues.is_empty(), "{:?}", observation.issues);
            let output = super::native_child::drain(observation.pipes.output);
            let errors = super::native_child::drain(observation.pipes.errors);
            let control = super::native_child::drain(observation.pipes.control.unwrap());
            let mut input = observation.pipes.input;
            input.write_all(b"owned descriptor collision\n").unwrap();
            eml_macos::close_writer(input).unwrap();
            assert_eq!(
                super::native_child::terminal(&child.0),
                eml_macos::ChildExit {
                    code: Some(7),
                    signal: None
                }
            );
            assert_eq!(
                output.recv_timeout(Duration::from_secs(2)).unwrap(),
                b"int-blocked=0\nowned descriptor collision\n"
            );
            assert_eq!(
                errors.recv_timeout(Duration::from_secs(2)).unwrap(),
                b"owned fixture reached EOF\n"
            );
            assert_eq!(control.recv_timeout(Duration::from_secs(2)).unwrap(), b"R");
            assert!(child.0.authority_revoked().unwrap());
        }));
        if let Err(error) = outcome {
            let detail = error
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| error.downcast_ref::<&str>().copied())
                .unwrap_or("unknown fixture failure");
            let _ = writeln!(log, "closed-stdio-control-failed: {detail}");
            std::process::exit(1);
        }
        log.write_all(b"closed-stdio-control-ok\n").unwrap();
    }
}
