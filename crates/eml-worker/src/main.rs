//! Private desktop worker: native request transport and one reusable core run.
#![forbid(unsafe_code)]
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
    let control = RunControl::default();
    let stopped = control.clone();
    let mut signalled = false;
    if ctrlc::set_handler(move || {
        if signalled {
            std::process::exit(130);
        }
        signalled = true;
        stopped.request_stop();
    })
    .is_err()
    {
        return failure("Worker termination handling is unavailable", 1);
    }
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args == [std::ffi::OsString::from("--version")] {
        return match writeln!(
            io::stdout().lock(),
            "eml-worker {}",
            env!("CARGO_PKG_VERSION")
        ) {
            Ok(()) => 0,
            Err(_) => 1,
        };
    }
    if !args.is_empty() {
        return failure("Private worker accepts only a protocol-1 stdin request", 2);
    }
    let working = match std::env::current_dir() {
        Ok(value) => value,
        Err(_) => return failure("Worker could not capture its working directory", 2),
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
        return failure("Worker could not start request transport", 1);
    }
    let bytes = loop {
        if control.is_stopped() {
            return 130;
        }
        match receiver.recv_timeout(Duration::from_millis(20)) {
            Ok(Ok(bytes)) => break bytes,
            Ok(Err(_)) => return failure("Worker request read failed", 2),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return failure("Worker request transport ended unexpectedly", 1);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    };
    if control.is_stopped() {
        return 130;
    }
    let request = match decode_request(&bytes, working) {
        Ok(value) => value,
        Err(_) => return failure("Worker request did not satisfy protocol-1 admission", 2),
    };
    match eml_core::run(&request, &control, &mut JsonSink::new(io::stdout().lock())) {
        Ok(outcome) => outcome.exit_class().code(),
        Err(_) => {
            if control.is_stopped() {
                130
            } else {
                failure("Worker core request admission failed", 2)
            }
        }
    }
}
fn failure(message: &str, code: u8) -> u8 {
    if writeln!(io::stderr().lock(), "{message}").is_ok() {
        code
    } else {
        1
    }
}
