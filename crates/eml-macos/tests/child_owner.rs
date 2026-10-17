//! Actual owned Darwin children; no windows, preferences or unrelated processes.
#![cfg(all(target_os = "macos", feature = "native-fixtures"))]
use eml_macos::{ChildExit, ChildState, OwnedChild, StopSignal, spawn};
use std::{
    io::{Read, Write},
    path::Path,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
const CHILD: &str = env!("CARGO_BIN_EXE_macos-child-observation");
struct Settle(OwnedChild);
impl Drop for Settle {
    fn drop(&mut self) {
        match self.0.poll() {
            Ok(ChildState::Exited(_)) => return,
            Err(error) => unconfirmed(&format!("initial cleanup poll: {error}")),
            Ok(ChildState::Running) => {}
        }
        if let Err(error) = self.0.signal(StopSignal::Kill) {
            eprintln!(
                "Owned native fixture cleanup Kill failed: {error}; still checking terminal state"
            );
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            match self.0.poll() {
                Ok(ChildState::Exited(_)) => return,
                Err(error) => unconfirmed(&format!("cleanup poll: {error}")),
                Ok(ChildState::Running) => thread::sleep(Duration::from_millis(5)),
            }
        }
        unconfirmed("cleanup allowance exhausted");
    }
}
fn unconfirmed(reason: &str) -> ! {
    eprintln!(
        "Owned native fixture cleanup is unconfirmed ({reason}); any earlier failure remains; abort later tests and retain owned resources"
    );
    std::process::exit(1);
}
fn terminal(child: &OwnedChild) -> ChildExit {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.poll().unwrap() {
            ChildState::Exited(status) => return status,
            ChildState::Running => {}
        }
        assert!(
            Instant::now() < deadline,
            "Owned child terminal settlement timed out"
        );
        thread::sleep(Duration::from_millis(5));
    }
}
fn drain(mut reader: impl Read + Send + 'static) -> mpsc::Receiver<Vec<u8>> {
    let (send, receive) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut bytes = Vec::new();
        reader.by_ref().take(65537).read_to_end(&mut bytes).unwrap();
        assert!(bytes.len() <= 65536);
        drop(send.send(bytes));
    });
    receive
}
#[test]
fn native_spawn_pipes_eof_exit_and_signal_revocation_are_actual() {
    let result = spawn(Path::new(CHILD), &[]).unwrap();
    let child = result.child.map(Settle);
    assert!(result.issues.is_empty(), "{:?}", result.issues);
    let child = child.unwrap();
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
    pub fn blocked<T>(observe: impl FnOnce() -> T) -> T {
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
