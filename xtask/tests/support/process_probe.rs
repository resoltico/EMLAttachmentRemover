//! Synthetic worker for real developer-controller lifetime probes; never distributed.
#![forbid(unsafe_code)]
use std::{path::PathBuf, process::Command, time::Duration};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let mode = args.next().ok_or("mode")?;
    if mode == "--eml-process-pin" {
        use std::io::Write;
        std::thread::sleep(Duration::from_millis(300));
        std::io::stdout().write_all(b"ready\n")?;
        return Ok(());
    }
    let root = PathBuf::from(args.next().ok_or("root")?);
    if mode == "clipboard-abort" {
        use std::io::{Read, Write};
        let path = root.join("one.eml");
        use base64::Engine;
        let path = path
            .to_str()
            .ok_or("native clipboard fixture path is not UTF8")?;
        let payload = base64::engine::general_purpose::STANDARD.encode(path.as_bytes());
        writeln!(
            std::io::stdout(),
            "EML_NATIVE_PANEL_PATH_PREPARE 1 {payload}"
        )?;
        std::io::stdout().flush()?;
        let began = std::time::Instant::now();
        let expected = b"owned panel path ready 1\n";
        loop {
            let mut options = std::fs::OpenOptions::new();
            options.read(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.custom_flags(
                    (rustix::fs::OFlags::NONBLOCK | rustix::fs::OFlags::NOFOLLOW).bits() as i32,
                );
            }
            match options.open(root.join("panel-path-1.ready")) {
                Ok(file) => {
                    if !file.metadata()?.is_file() {
                        return Err(
                            "parent clipboard acknowledgment is not an ordinary file".into()
                        );
                    }
                    let mut bytes = Vec::new();
                    file.take(expected.len() as u64 + 1)
                        .read_to_end(&mut bytes)?;
                    if bytes == expected {
                        break;
                    }
                    if bytes.len() >= expected.len() || !expected.starts_with(&bytes) {
                        return Err("parent clipboard acknowledgment differs".into());
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(error.into()),
            }
            if began.elapsed() > Duration::from_secs(10) {
                return Err("parent did not prepare the owned clipboard lease".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        #[cfg(unix)]
        rustix::process::setrlimit(
            rustix::process::Resource::Core,
            rustix::process::Rlimit {
                current: Some(0),
                maximum: Some(0),
            },
        )?;
        std::process::abort();
    }
    if mode == "abort" {
        #[cfg(unix)]
        rustix::process::setrlimit(
            rustix::process::Resource::Core,
            rustix::process::Rlimit {
                current: Some(0),
                maximum: Some(0),
            },
        )?;
        std::process::abort();
    }
    if mode == "streams-exit" {
        use std::io::Write;
        std::io::stdout().write_all(b"owned stdout\n")?;
        std::io::stdout().flush()?;
        std::io::stderr().write_all(b"owned stderr\n")?;
        std::io::stderr().flush()?;
        std::process::exit(23);
    }
    if mode == "cooperative-resource" {
        cooperative_resource(&root)?;
    } else if mode == "marker" {
        #[cfg(unix)]
        std::fs::write(
            root.join("worker-group"),
            rustix::process::getpgrp()
                .as_raw_nonzero()
                .get()
                .to_string(),
        )?;
        std::thread::sleep(Duration::from_millis(800));
        std::fs::write(root.join("invoked"), b"not terminated")?;
    } else if mode == "background" {
        let _ = Command::new(std::env::current_exe()?)
            .arg("marker")
            .arg(&root)
            .spawn()?;
    } else if mode == "nested-leader-exit" {
        let _ = spawn_group("background", &root)?.wait()?;
    } else if mode == "nested-race" {
        let _ = spawn_group("wait-fork", &root)?.wait()?;
    } else if mode == "wait-fork" {
        std::fs::write(root.join("first-ready"), b"waiting")?;
        let began = std::time::Instant::now();
        while !root.join("inspect.fork-now").exists() {
            if began.elapsed() > Duration::from_secs(5) {
                return Err("fork trigger missing".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let _ = spawn_group("ignore-term", &root)?.wait()?;
    } else if mode == "nested" || mode == "nested-wait" {
        let mut child = spawn_group("ignore-term", &root)?;
        if mode == "nested-wait" {
            let _ = child.wait()?;
        }
    } else if mode == "ignore-term" {
        ctrlc::set_handler(|| {})?;
        std::fs::write(root.join("worker-ready"), std::process::id().to_string())?;
        std::thread::sleep(Duration::from_secs(5));
        std::fs::write(root.join("invoked"), b"not terminated")?;
    } else if mode == "supervise" {
        let helper = PathBuf::from(args.next().ok_or("helper")?);
        let controller = xtask::process::Controller::new(&root, &helper)?;
        controller.install_signals()?;
        let mut worker = Command::new(std::env::current_exe()?);
        worker.arg("nested-wait").arg(&root);
        let result = controller.run(
            worker,
            &root.join("supervised.log"),
            Duration::from_secs(10),
        );
        std::fs::write(
            root.join("parent-result"),
            if result.as_ref().is_ok_and(|result| result.success()) {
                b"success".as_slice()
            } else {
                b"failed"
            },
        )?;
        if !result?.success() {
            return Err("Owned supervised worker did not qualify".into());
        }
    } else {
        return Err("unknown mode".into());
    }
    Ok(())
}

fn cooperative_resource(root: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let stopped = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&stopped);
    ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))?;
    std::fs::write(
        root.join("external-resource"),
        b"owned resource requiring cleanup",
    )?;
    std::fs::write(root.join("worker-ready"), b"handler installed")?;
    let began = std::time::Instant::now();
    while !stopped.load(Ordering::Relaxed) && !root.join("control-release").exists() {
        if began.elapsed() > Duration::from_secs(10) {
            return Err("Cooperative stop was not delivered".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    std::fs::remove_file(root.join("external-resource"))?;
    std::fs::write(
        root.join("cooperative-stop-observed"),
        b"owned resource cleanup completed",
    )?;
    Ok(())
}

fn spawn_group(
    mode: &str,
    root: &std::path::Path,
) -> Result<std::process::Child, Box<dyn std::error::Error>> {
    let mut child = Command::new(std::env::current_exe()?);
    child.arg(mode).arg(root);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        child.process_group(0);
    }
    Ok(child.spawn()?)
}
