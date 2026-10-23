//! Nonshipping first-party worker transport fixture, restricted to its owned directory.
use std::{
    fs,
    io::{Read, Write},
};
fn main() {
    let argument = std::env::args_os().nth(1).unwrap_or_default();
    let executable = std::env::current_exe().unwrap();
    let root = executable.parent().unwrap();
    if argument == "--version" {
        if fs::read(root.join("fixture-mode")).is_ok_and(|mode| mode == b"wrong-identity") {
            assert_eq!(
                fs::read(root.join("fixture-authorized")).unwrap(),
                b"owned transport fixture"
            );
            assert_eq!(
                fs::read(root.join("fixture-mode")).unwrap(),
                b"wrong-identity"
            );
            print!(
                "{}",
                eml_protocol::worker_control::identity_line("0.0.0-wrong")
            );
            std::io::stdout().flush().unwrap();
            fs::write(root.join("prefix-ready"), b"ready").unwrap();
            loop {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        print!(
            "{}",
            eml_protocol::worker_control::identity_line(env!("CARGO_PKG_VERSION"))
        );
        return;
    }
    assert_eq!(argument, "--desktop-operation");
    assert_eq!(
        fs::read(root.join("fixture-authorized")).unwrap(),
        b"owned transport fixture"
    );
    #[cfg(target_os = "macos")]
    if fs::read(root.join("fixture-mode")).is_ok_and(|mode| mode == b"two-lanes") {
        two_lanes(root);
        return;
    }
    #[cfg(target_os = "macos")]
    if fs::read(root.join("fixture-mode")).is_ok_and(|mode| mode == b"no-first-ack") {
        ctrlc::set_handler(|| {}).unwrap();
        let control = eml_macos::InheritedControlWriter::duplicate().unwrap();
        control
            .try_write(eml_protocol::worker_control::READY)
            .unwrap();
        let (_, request) = eml_protocol::desktop::read_operation(std::io::stdin()).unwrap();
        assert!(!request.is_empty() && request.len() <= eml_protocol::MAX_REQUEST_BYTES);
        fs::write(root.join("fed-ready"), b"fed").unwrap();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    if fs::read(root.join("fixture-mode")).is_ok_and(|mode| mode == b"no-ready") {
        fs::write(root.join("prefix-ready"), b"ready").unwrap();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    std::io::stdout()
        .write_all(b"unexpected partial event")
        .unwrap();
    std::io::stdout().flush().unwrap();
    fs::write(root.join("prefix-ready"), b"ready").unwrap();
    // Retain inherited control without emitting R, waiting for the parent's
    // owned cancellation. This fixture neither processes messages nor spawns.
    let mut byte = [0];
    loop {
        if std::io::stdin().read(&mut byte).unwrap() == 0 {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

#[cfg(target_os = "macos")]
fn two_lanes(root: &std::path::Path) {
    use eml_protocol::desktop::Operation;
    let pipe = std::sync::Arc::new(eml_macos::InheritedControlWriter::duplicate().unwrap());
    let handler = pipe.clone();
    let control = eml_core::RunControl::default();
    let stopped = control.clone();
    let mut first = false;
    ctrlc::set_handler(move || {
        if first {
            std::process::exit(130);
        }
        first = true;
        stopped.request_stop();
        handler
            .try_write(eml_protocol::worker_control::FIRST_INTERRUPT)
            .unwrap();
    })
    .unwrap();
    pipe.try_write(eml_protocol::worker_control::READY).unwrap();
    let (header, bytes) = eml_protocol::desktop::read_operation(std::io::stdin()).unwrap();
    let marker = match header.operation {
        Operation::Processing => {
            let request =
                eml_protocol::decode_request(&bytes, std::env::current_dir().unwrap()).unwrap();
            assert!(request.paths.iter().all(|p| p.starts_with(root)));
            if request.paths[0].file_name() != Some(std::ffi::OsStr::new("hold.eml")) {
                let mut sink = eml_protocol::JsonSink::new(std::io::stdout());
                let outcome = eml_core::run(&request, &control, &mut sink).unwrap();
                std::process::exit(i32::from(outcome.exit_class().code()));
            }
            "processing-fed"
        }
        Operation::Export { destination } => {
            assert!(destination.starts_with(root));
            eml_app::report_document::validate(&bytes).unwrap();
            "export-fed"
        }
    };
    fs::write(root.join(marker), b"fed").unwrap();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
