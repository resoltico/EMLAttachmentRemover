//! Actual owned Darwin children; no windows, preferences or unrelated processes.
#![cfg(all(target_os = "macos", feature = "native-fixtures"))]
use eml_macos::{ChildExit, ChildState, StopSignal, spawn};
use std::{
    io::{Read, Write},
    path::Path,
    sync::mpsc,
    thread,
    time::Duration,
};
const CHILD: &str = env!("CARGO_BIN_EXE_macos-child-observation");
#[path = "../../../tests/support/native_child.rs"]
mod native_child;
use native_child::{Settle, drain, terminal};
#[test]
fn native_spawn_pipes_eof_exit_and_signal_revocation_are_actual() {
    let result = spawn(Path::new(CHILD), &[]).unwrap();
    let child = result.child.map(Settle);
    assert!(result.issues.is_empty(), "{:?}", result.issues);
    let child = child.unwrap();
    assert!(!child.0.authority_revoked().unwrap());
    assert!(
        child
            .0
            .observation_pid()
            .unwrap()
            .is_some_and(|pid| pid > 0)
    );
    let output = drain(result.pipes.output);
    let errors = drain(result.pipes.errors);
    let mut input = result.pipes.input;
    input.write_all(b"synthetic owned request\n").unwrap();
    drop(input);
    assert_eq!(
        terminal(&child.0),
        ChildExit {
            code: Some(7),
            signal: None
        }
    );
    assert_eq!(
        output.recv_timeout(Duration::from_secs(2)).unwrap(),
        b"int-blocked=0\nsynthetic owned request\n"
    );
    assert_eq!(
        errors.recv_timeout(Duration::from_secs(2)).unwrap(),
        b"owned fixture reached EOF\n"
    );
    assert!(!child.0.signal(StopSignal::Kill).unwrap());
    assert!(child.0.authority_revoked().unwrap());
    assert_eq!(child.0.observation_pid().unwrap(), None);
    assert!(!child.0.signal(StopSignal::Interrupt).unwrap());
}
#[test]
fn child_waiting_for_request_can_be_stopped_without_a_blocking_reaper() {
    stop_waiting(StopSignal::Interrupt, 2);
}
#[test]
fn live_child_kill_uses_launch_bound_authority_before_reap() {
    stop_waiting(StopSignal::Kill, 9);
}
fn stop_waiting(signal: StopSignal, expected: i32) {
    let result = spawn(Path::new(CHILD), &[]).unwrap();
    let child = result.child.map(Settle);
    assert!(result.issues.is_empty(), "{:?}", result.issues);
    let child = child.unwrap();
    let mut stdout = result.pipes.output;
    let errors = drain(result.pipes.errors);
    let (ready, receive) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut bytes = [0; 14];
        stdout.read_exact(&mut bytes).unwrap();
        drop(ready.send((bytes, stdout)));
    });
    let (bytes, stdout) = receive.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(&bytes, b"int-blocked=0\n");
    let output = drain(stdout);
    assert_eq!(child.0.poll().unwrap(), ChildState::Running);
    assert!(child.0.signal(signal).unwrap());
    assert_eq!(
        terminal(&child.0),
        ChildExit {
            code: None,
            signal: Some(expected)
        }
    );
    drop(result.pipes.input);
    assert!(
        output
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .is_empty()
    );
    assert!(
        errors
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .is_empty()
    );
    assert!(!child.0.signal(StopSignal::Kill).unwrap());
}
#[allow(
    unsafe_code,
    reason = "Block/restore only this owned test thread's signal mask"
)]
mod spawning_thread_mask {
    pub(crate) fn blocked<T>(observe: impl FnOnce() -> T) -> T {
        struct Restore(libc::sigset_t);
        impl Drop for Restore {
            fn drop(&mut self) {
                // SAFETY: restore the exact mask captured on this same thread.
                if unsafe {
                    libc::pthread_sigmask(libc::SIG_SETMASK, &self.0, std::ptr::null_mut())
                } != 0
                {
                    eprintln!("Owned fixture thread mask restoration failed");
                    std::process::exit(1);
                }
            }
        }
        // SAFETY: initialized local sigsets; only the calling fixture thread is
        // changed, and the RAII guard restores it through ordinary unwinding.
        let guard = unsafe {
            let mut block = std::mem::zeroed::<libc::sigset_t>();
            let mut previous = std::mem::zeroed::<libc::sigset_t>();
            assert_eq!(libc::sigemptyset(&mut block), 0);
            assert_eq!(libc::sigaddset(&mut block, libc::SIGINT), 0);
            assert_eq!(
                libc::pthread_sigmask(libc::SIG_BLOCK, &block, &mut previous),
                0
            );
            Restore(previous)
        };
        // SAFETY: read the same thread's current mask; assert the negative
        // precondition before spawning rather than assuming inheritance differs.
        unsafe {
            let mut actual = std::mem::zeroed::<libc::sigset_t>();
            assert_eq!(libc::pthread_sigmask(0, std::ptr::null(), &mut actual), 0);
            assert_eq!(libc::sigismember(&actual, libc::SIGINT), 1);
        }
        let result = observe();
        drop(guard);
        result
    }
}
#[test]
fn child_signal_policy_resets_a_deliberately_blocked_spawning_thread_mask() {
    let result = spawning_thread_mask::blocked(|| spawn(Path::new(CHILD), &[]).unwrap());
    let child = result.child.map(Settle);
    assert!(result.issues.is_empty(), "{:?}", result.issues);
    let child = child.unwrap();
    let output = drain(result.pipes.output);
    let errors = drain(result.pipes.errors);
    drop(result.pipes.input);
    assert_eq!(
        terminal(&child.0),
        ChildExit {
            code: Some(7),
            signal: None
        }
    );
    assert_eq!(
        output.recv_timeout(Duration::from_secs(2)).unwrap(),
        b"int-blocked=0\n"
    );
    assert_eq!(
        errors.recv_timeout(Duration::from_secs(2)).unwrap(),
        b"owned fixture reached EOF\n"
    );
}
#[test]
fn failed_spawn_retains_no_child_authority_and_parent_endpoints_can_close() {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/native-child-tests");
    std::fs::create_dir_all(&parent).unwrap();
    let owned = tempfile::tempdir_in(parent).unwrap();
    let result = spawn(&owned.path().join("absent-owned-child"), &[]).unwrap();
    let child = result.child.map(Settle);
    assert!(child.is_none());
    assert_eq!(result.issues.len(), 1);
    assert_eq!(result.issues[0].operation, "posix-spawn");
    assert_eq!(result.issues[0].native_code, 2);
    drop(result.pipes.input);
    assert!(
        drain(result.pipes.output)
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .is_empty()
    );
    assert!(
        drain(result.pipes.errors)
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn closed_standard_descriptors_do_not_alias_control_or_stdio_spawn_sources() {
    use std::{ffi::OsStr, time::Instant};
    let observation = spawn(Path::new(CHILD), &[OsStr::new("--closed-stdio")]).unwrap();
    let child = observation.child.map(Settle).unwrap();
    assert!(observation.issues.is_empty(), "{:?}", observation.issues);
    let output = drain(observation.pipes.output);
    let errors = drain(observation.pipes.errors);
    eml_macos::close_writer(observation.pipes.input).unwrap();
    // The inner fixture has its own five-second bounded cleanup. Give it time
    // to settle its exact owned child before the outer cleanup allowance.
    let deadline = Instant::now() + Duration::from_secs(20);
    let exit = loop {
        if let ChildState::Exited(exit) = child.0.poll().unwrap() {
            break exit;
        }
        assert!(
            Instant::now() < deadline,
            "Owned descriptor fixture did not settle"
        );
        thread::sleep(Duration::from_millis(5));
    };
    let bytes = output.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(
        exit,
        ChildExit {
            code: Some(0),
            signal: None
        },
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    assert_eq!(bytes, b"closed-stdio-control-ok\n");
    assert!(
        errors
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .is_empty()
    );
    assert!(child.0.authority_revoked().unwrap());
}

#[test]
fn private_control_full_and_broken_pipes_are_nonblocking_errors_without_sigpipe_exit() {
    use std::ffi::OsStr;
    for mode in ["--full-control", "--broken-control"] {
        let observation =
            eml_macos::spawn_with_control_pipe(Path::new(CHILD), &[OsStr::new(mode)]).unwrap();
        let child = observation.child.map(Settle).unwrap();
        assert!(observation.issues.is_empty(), "{:?}", observation.issues);
        let output = drain(observation.pipes.output);
        let errors = drain(observation.pipes.errors);
        let mut input = observation.pipes.input;
        let control = observation.pipes.control.unwrap();
        let control = if mode == "--broken-control" {
            eml_macos::close_reader(control).unwrap();
            input.write_all(&[1]).unwrap();
            None
        } else {
            Some(control)
        };
        eml_macos::close_writer(input).unwrap();
        let exit = terminal(&child.0);
        let bytes = output.recv_timeout(Duration::from_secs(2)).unwrap();
        let diagnostics = errors.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            exit,
            ChildExit {
                code: Some(0),
                signal: None
            },
            "{mode}: {}",
            String::from_utf8_lossy(&diagnostics)
        );
        assert!(diagnostics.is_empty());
        if let Some(mut control) = control {
            let mut received = Vec::new();
            (&mut control)
                .take(1024 * 1024 + 1)
                .read_to_end(&mut received)
                .unwrap();
            assert!(!received.is_empty() && received.len() <= 1024 * 1024);
            assert!(received.iter().all(|byte| *byte == b'R'));
            assert_eq!(
                bytes,
                format!("full-control:{}\n", received.len()).as_bytes()
            );
            eml_macos::close_reader(control).unwrap();
        } else {
            assert_eq!(bytes, b"broken-control-survived\n");
        }
        assert!(child.0.authority_revoked().unwrap());
    }
}
