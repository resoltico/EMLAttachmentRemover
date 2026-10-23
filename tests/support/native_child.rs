//! Shared bounded cleanup and EOF observations for actual owned native children.
use eml_macos::{ChildExit, ChildState, OwnedChild, StopSignal};
use std::{
    io::{Read, Write},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
#[allow(
    dead_code,
    reason = "Shared test support: ordinary harnesses use stderr; the closed-stdio fixture uses its preserved log"
)]
pub(crate) struct Settle(pub OwnedChild);
impl Drop for Settle {
    fn drop(&mut self) {
        // Acquire no diagnostic lock until native poll/Kill/reap has finished.
        if let Err(error) = settle_native(&self.0) {
            unconfirmed(&error, &mut std::io::stderr().lock());
        }
    }
}
/// The descriptor-collision fixture preserves its diagnostic writer above fd3.
#[allow(
    dead_code,
    reason = "Only the separate closed-stdio fixture target needs a preserved diagnostic descriptor"
)]
pub(crate) struct SettleWithLog<'a>(pub OwnedChild, pub &'a mut dyn Write);
impl Drop for SettleWithLog<'_> {
    fn drop(&mut self) {
        if let Err(error) = settle_native(&self.0) {
            unconfirmed(&error, self.1);
        }
    }
}
fn settle_native(child: &OwnedChild) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    match child.poll() {
        Ok(ChildState::Exited(_)) => return Ok(()),
        Err(error) => return Err(format!("initial cleanup poll: {error}")),
        Ok(ChildState::Running) => {}
    }
    // Keep an earlier native failure even if a later reap succeeds. Logging
    // happens only after the critical native cleanup allowance, never within it.
    let kill_error = child
        .signal(StopSignal::Kill)
        .err()
        .map(|error| format!("cleanup Kill: {error}"));
    while Instant::now() < deadline {
        match child.poll() {
            Ok(ChildState::Exited(_)) => return kill_error.map_or(Ok(()), Err),
            Err(error) => {
                return Err(format!(
                    "{}cleanup poll: {error}",
                    kill_error.map_or_else(String::new, |error| format!("{error}; "))
                ));
            }
            Ok(ChildState::Running) => thread::sleep(Duration::from_millis(5)),
        }
    }
    Err(format!(
        "{}cleanup allowance exhausted",
        kill_error.map_or_else(String::new, |error| format!("{error}; "))
    ))
}
fn unconfirmed(reason: &str, diagnostics: &mut dyn Write) -> ! {
    let _ = writeln!(
        diagnostics,
        "Owned native fixture cleanup failed ({reason}); any earlier failure remains; abort later tests and retain owned resources"
    );
    std::process::exit(1);
}
pub(crate) fn terminal(child: &OwnedChild) -> ChildExit {
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
pub(crate) fn drain(mut reader: impl Read + Send + 'static) -> mpsc::Receiver<Vec<u8>> {
    let (send, receive) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut bytes = Vec::new();
        reader.by_ref().take(65537).read_to_end(&mut bytes).unwrap();
        assert!(bytes.len() <= 65536);
        drop(send.send(bytes));
    });
    receive
}
