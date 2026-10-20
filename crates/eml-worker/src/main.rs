//! Private desktop worker: native request transport and one reusable core run.
#![forbid(unsafe_code)]
#[cfg(all(target_os = "macos", feature = "native-fixtures"))]
mod publication_observation;
use eml_core::RunControl;
use eml_protocol::{JsonSink, MAX_REQUEST_BYTES, decode_request};
use std::{
    io::{self, Read, Write},
    sync::mpsc,
    time::Duration,
};

fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(entry())
}
fn entry() -> u8 {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let version = args == [std::ffi::OsString::from("--version")];
    let desktop = args == [std::ffi::OsString::from("--desktop-control")];
    if !args.is_empty() && !version && !desktop {
        return failure(
            "Private worker accepts a protocol-1 stdin request or exact private control/version invocation",
            2,
        );
    }
    #[cfg(all(target_os = "macos", feature = "native-fixtures"))]
    let observation = if version {
        None
    } else {
        match publication_observation::load() {
            Ok(value) => value,
            Err(error) => {
                return failure(
                    &format!("Nonshipping publication observation refused: {error}"),
                    70,
                );
            }
        }
    };
    #[cfg(all(target_os = "macos", feature = "native-fixtures"))]
    let signal_observation = observation.clone();
    #[cfg(not(target_os = "macos"))]
    if desktop {
        return failure("Desktop control requires the native macOS worker", 2);
    }
    #[cfg(target_os = "macos")]
    let control_pipe = if desktop {
        match eml_macos::InheritedControlWriter::duplicate() {
            Ok(writer) => Some(std::sync::Arc::new(writer)),
            Err(_) => return failure("Worker private control pipe is unavailable", 1),
        }
    } else {
        None
    };
    #[cfg(target_os = "macos")]
    let ready = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    #[cfg(target_os = "macos")]
    let handler_pipe = control_pipe.clone();
    #[cfg(target_os = "macos")]
    let handler_ready = std::sync::Arc::clone(&ready);
    let control = RunControl::default();
    let stopped = control.clone();
    let mut signalled = false;
    if ctrlc::set_handler(move || {
        if signalled {
            #[cfg(all(target_os = "macos", feature = "native-fixtures"))]
            if let Some(observation) = &signal_observation {
                observation.interrupt(true);
            }
            std::process::exit(130);
        }
        signalled = true;
        stopped.request_stop();
        #[cfg(target_os = "macos")]
        if let Some(writer) = &handler_pipe {
            // The owner waits for real R. An external signal during startup may
            // terminate or produce F before R; the parent rejects that sequence
            // without feeding input. Acknowledgment failure cannot strand ctrlc.
            if !handler_ready.load(std::sync::atomic::Ordering::Acquire)
                || writer
                    .try_write(eml_protocol::worker_control::FIRST_INTERRUPT)
                    .is_err()
            {
                std::process::exit(130);
            }
        }
        #[cfg(all(target_os = "macos", feature = "native-fixtures"))]
        if let Some(observation) = &signal_observation {
            // Only expose the first observation after the real Stop flag and
            // private acknowledgment write, before a fixture actor releases Core.
            observation.interrupt(false);
        }
    })
    .is_err()
    {
        return pre_stream_failure("Worker termination handling is unavailable", 1, &control);
    }
    if version {
        return match io::stdout().lock().write_all(
            eml_protocol::worker_control::identity_line(env!("CARGO_PKG_VERSION")).as_bytes(),
        ) {
            Ok(()) => 0,
            Err(_) => 1,
        };
    }
    let working = match std::env::current_dir() {
        Ok(value) => value,
        Err(_) => {
            return pre_stream_failure(
                "Worker could not capture its working directory",
                2,
                &control,
            );
        }
    };
    // This single transport reader never processes messages or publishes copies.
    // A blocked OS read is not joined after cancellation: this isolated process
    // exits, terminating its reader, and the desktop owner observes actual reap.
    let (sender, receiver) = mpsc::sync_channel(1);
    if std::thread::Builder::new()
        .name("desktop-request".into())
        .spawn(move || {
            let mut bytes = Vec::new();
            let result = io::stdin()
                .lock()
                .take((MAX_REQUEST_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .map(|_| bytes);
            drop(sender.send(result));
        })
        .is_err()
    {
        return pre_stream_failure("Worker could not start request transport", 1, &control);
    }
    #[cfg(target_os = "macos")]
    if let Some(writer) = &control_pipe {
        if control.is_stopped() {
            return 130;
        }
        // Arm before writing R: once the parent can read R, its immediate INT
        // must observe an armed handler rather than a post-write flag race.
        ready.store(true, std::sync::atomic::Ordering::Release);
        if writer
            .try_write(eml_protocol::worker_control::READY)
            .is_err()
        {
            return pre_stream_failure("Worker readiness could not be delivered", 1, &control);
        }
    }
    let bytes = loop {
        if control.is_stopped() {
            return 130;
        }
        match receiver.recv_timeout(Duration::from_millis(20)) {
            Ok(Ok(bytes)) => break bytes,
            Ok(Err(_)) => return pre_stream_failure("Worker request read failed", 2, &control),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return pre_stream_failure(
                    "Worker request transport ended unexpectedly",
                    1,
                    &control,
                );
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    };
    if control.is_stopped() {
        return 130;
    }
    let request = match decode_request(&bytes, working) {
        Ok(value) => value,
        Err(_) => {
            return pre_stream_failure(
                "Worker request did not satisfy protocol-1 admission",
                2,
                &control,
            );
        }
    };
    let mut sink = JsonSink::new(io::stdout().lock());
    #[cfg(all(target_os = "macos", feature = "native-fixtures"))]
    let result = if let Some(observation) = observation {
        eml_core::run_observed(&request, &control, &mut sink, observation.hooks())
    } else {
        eml_core::run(&request, &control, &mut sink)
    };
    #[cfg(not(all(target_os = "macos", feature = "native-fixtures")))]
    let result = eml_core::run(&request, &control, &mut sink);
    match result {
        Ok(outcome) => outcome.exit_class().code(),
        Err(_) => {
            if control.is_stopped() {
                130
            } else {
                pre_stream_failure("Worker core request admission failed", 2, &control)
            }
        }
    }
}
fn pre_stream_failure(message: &str, code: u8, control: &RunControl) -> u8 {
    pre_stream_failure_to(io::stderr().lock(), message, code, control)
}
fn pre_stream_failure_to(
    mut diagnostics: impl Write,
    message: &str,
    code: u8,
    control: &RunControl,
) -> u8 {
    if control.is_stopped() {
        return 130;
    }
    let result = if writeln!(diagnostics, "{message}").is_ok() {
        code
    } else {
        1
    };
    if control.is_stopped() { 130 } else { result }
}

fn failure(message: &str, code: u8) -> u8 {
    if writeln!(io::stderr().lock(), "{message}").is_ok() {
        code
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct StopOnWrite {
        control: RunControl,
        fail: bool,
    }
    impl Write for StopOnWrite {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.control.request_stop();
            if self.fail {
                Err(io::Error::other("Owned diagnostic failure"))
            } else {
                Ok(bytes.len())
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn stop_during_pre_stream_diagnostic_delivery_preserves_exit130_even_if_delivery_fails() {
        for fail in [false, true] {
            let control = RunControl::default();
            assert_eq!(
                pre_stream_failure_to(
                    StopOnWrite {
                        control: control.clone(),
                        fail
                    },
                    "Synthetic request refusal",
                    2,
                    &control
                ),
                130
            );
            assert!(control.is_stopped());
        }
        let control = RunControl::default();
        let mut bytes = Vec::new();
        assert_eq!(
            pre_stream_failure_to(&mut bytes, "Synthetic request refusal", 2, &control),
            2
        );
        assert_eq!(bytes, b"Synthetic request refusal\n");
        control.request_stop();
        let mut untouched = Vec::new();
        assert_eq!(
            pre_stream_failure_to(&mut untouched, "No delivery", 2, &control),
            130
        );
        assert!(untouched.is_empty());
    }
}
