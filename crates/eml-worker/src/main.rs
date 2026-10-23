//! Private desktop worker: native request transport and one reusable core run.
#![forbid(unsafe_code)]
#[cfg(all(target_os = "macos", feature = "native-fixtures"))]
mod publication_observation;
use eml_core::RunControl;
use eml_protocol::desktop::{Header, Operation};
use eml_protocol::{JsonSink, decode_request};
use std::{
    io::{self, Write},
    sync::mpsc,
    time::Duration,
};

fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(entry())
}
fn entry() -> u8 {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let version = args == [std::ffi::OsString::from("--version")];
    let desktop = args == [std::ffi::OsString::from("--desktop-operation")];
    if !version && !desktop {
        return failure(
            "Private worker requires --desktop-operation or --version",
            2,
        );
    }
    #[cfg(all(target_os = "macos", feature = "native-fixtures"))]
    let observation = if version {
        None
    } else {
        match load_observation() {
            Ok(value) => value,
            Err(code) => return code,
        }
    };
    #[cfg(all(target_os = "macos", feature = "native-fixtures"))]
    let signal_observation = observation.clone();
    #[cfg(target_os = "macos")]
    let control_pipe = if desktop {
        eml_macos::InheritedControlWriter::duplicate()
            .ok()
            .map(std::sync::Arc::new)
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
    if install_termination_handler(
        &control,
        #[cfg(target_os = "macos")]
        handler_pipe,
        #[cfg(target_os = "macos")]
        handler_ready,
        #[cfg(all(target_os = "macos", feature = "native-fixtures"))]
        signal_observation,
    )
    .is_err()
    {
        return pre_stream_failure("Worker termination handling is unavailable", 1, &control);
    }
    if version {
        return write_identity();
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
    let receiver = match start_request_reader() {
        Ok(receiver) => receiver,
        Err(_) => {
            return pre_stream_failure("Worker could not start request transport", 1, &control);
        }
    };
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
    let (header, bytes) = match receive_request(&receiver, &control) {
        Ok(bytes) => bytes,
        Err(code) => return code,
    };
    match header.operation {
        Operation::Processing => execute_request(
            &bytes,
            working,
            &control,
            #[cfg(all(target_os = "macos", feature = "native-fixtures"))]
            observation,
        ),
        Operation::Export { destination } => {
            execute_export(header.operation_id, &bytes, &destination, &control)
        }
    }
}

#[cfg(all(target_os = "macos", feature = "native-fixtures"))]
fn load_observation() -> Result<Option<std::sync::Arc<publication_observation::Observation>>, u8> {
    publication_observation::load().map_err(|error| {
        failure(
            &format!("Nonshipping publication observation refused: {error}"),
            70,
        )
    })
}

fn write_identity() -> u8 {
    match io::stdout().lock().write_all(
        eml_protocol::worker_control::identity_line(env!("CARGO_PKG_VERSION")).as_bytes(),
    ) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

fn execute_request(
    bytes: &[u8],
    working: std::path::PathBuf,
    control: &RunControl,
    #[cfg(all(target_os = "macos", feature = "native-fixtures"))] observation: Option<
        std::sync::Arc<publication_observation::Observation>,
    >,
) -> u8 {
    if control.is_stopped() {
        return 130;
    }
    let request = match decode_request(bytes, working) {
        Ok(value) => value,
        Err(_) => {
            return pre_stream_failure(
                "Worker request did not satisfy protocol-1 admission",
                2,
                control,
            );
        }
    };
    let mut sink = JsonSink::new(io::stdout().lock());
    #[cfg(all(target_os = "macos", feature = "native-fixtures"))]
    let result = if let Some(observation) = observation {
        eml_core::run_observed(&request, control, &mut sink, observation.hooks())
    } else {
        eml_core::run(&request, control, &mut sink)
    };
    #[cfg(not(all(target_os = "macos", feature = "native-fixtures")))]
    let result = eml_core::run(&request, control, &mut sink);
    match result {
        Ok(outcome) => outcome.exit_class().code(),
        Err(_) => {
            if control.is_stopped() {
                130
            } else {
                pre_stream_failure("Worker core request admission failed", 2, control)
            }
        }
    }
}
fn execute_export(
    operation_id: eml_protocol::desktop::OperationId,
    bytes: &[u8],
    destination: &std::path::Path,
    control: &RunControl,
) -> u8 {
    if eml_app::report_document::validate(bytes).is_err()
        || eml_app::validate_destination(destination).is_err()
    {
        return pre_stream_failure(
            "Worker export document or destination is invalid",
            2,
            control,
        );
    }
    #[cfg(target_os = "macos")]
    {
        let outcome = eml_app::export_report(bytes, destination, control);
        let receipt = match outcome
            .receipt(operation_id)
            .and_then(|r| eml_protocol::desktop::encode_receipt(&r))
        {
            Ok(bytes) => bytes,
            Err(_) => return failure("Worker export receipt could not be encoded", 1),
        };
        if io::stdout().lock().write_all(&receipt).is_err() {
            return 1;
        }
        if outcome.usable() {
            0
        } else if control.is_stopped() {
            130
        } else {
            1
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = operation_id;
        pre_stream_failure(
            "Desktop report export requires native macOS namespace services",
            2,
            control,
        )
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

fn install_termination_handler(
    control: &RunControl,
    #[cfg(target_os = "macos")] handler_pipe: Option<
        std::sync::Arc<eml_macos::InheritedControlWriter>,
    >,
    #[cfg(target_os = "macos")] handler_ready: std::sync::Arc<std::sync::atomic::AtomicBool>,
    #[cfg(all(target_os = "macos", feature = "native-fixtures"))] signal_observation: Option<
        std::sync::Arc<publication_observation::Observation>,
    >,
) -> Result<(), ctrlc::Error> {
    let stopped = control.clone();
    let mut signalled = false;
    ctrlc::set_handler(move || {
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
}

type RequestReceiver = mpsc::Receiver<io::Result<(Header, Vec<u8>)>>;

fn start_request_reader() -> io::Result<RequestReceiver> {
    // This single transport reader never processes messages or publishes copies.
    // A blocked OS read is not joined after cancellation: this isolated process
    // exits, terminating its reader, and the desktop owner observes actual reap.
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("desktop-request".into())
        .spawn(move || {
            let result = eml_protocol::desktop::read_operation(io::stdin().lock());
            drop(sender.send(result));
        })?;
    Ok(receiver)
}

fn receive_request(
    receiver: &RequestReceiver,
    control: &RunControl,
) -> Result<(Header, Vec<u8>), u8> {
    loop {
        if control.is_stopped() {
            return Err(130);
        }
        match receiver.recv_timeout(Duration::from_millis(20)) {
            Ok(Ok(bytes)) => return Ok(bytes),
            Ok(Err(_)) => return Err(pre_stream_failure("Worker request read failed", 2, control)),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(pre_stream_failure(
                    "Worker request transport ended unexpectedly",
                    1,
                    control,
                ));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
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
