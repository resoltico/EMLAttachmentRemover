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
    assert_eq!(argument, "--desktop-control");
    assert_eq!(
        fs::read(root.join("fixture-authorized")).unwrap(),
        b"owned transport fixture"
    );
    #[cfg(target_os = "macos")]
    if fs::read(root.join("fixture-mode")).is_ok_and(|mode| mode == b"no-first-ack") {
        ctrlc::set_handler(|| {}).unwrap();
        let control = eml_macos::InheritedControlWriter::duplicate().unwrap();
        control
            .try_write(eml_protocol::worker_control::READY)
            .unwrap();
        let mut request = Vec::new();
        std::io::stdin()
            .take((eml_protocol::MAX_REQUEST_BYTES + 1) as u64)
            .read_to_end(&mut request)
            .unwrap();
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
