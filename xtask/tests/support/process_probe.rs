//! Synthetic worker for real developer-controller lifetime probes; never distributed.
#![forbid(unsafe_code)]
use std::{path::PathBuf, process::Command, time::Duration};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let mode = args.next().ok_or("mode")?;
    let root = PathBuf::from(args.next().ok_or("root")?);
    if mode == "marker" {
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
        let mut child = Command::new(std::env::current_exe()?);
        child.arg("background").arg(&root);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            child.process_group(0);
        }
        let _ = child.spawn()?.wait()?;
    } else if mode == "nested-race" {
        let mut child = Command::new(std::env::current_exe()?);
        child.arg("wait-fork").arg(&root);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            child.process_group(0);
        }
        let _ = child.spawn()?.wait()?;
    } else if mode == "wait-fork" {
        std::fs::write(root.join("first-ready"), b"waiting")?;
        let began = std::time::Instant::now();
        while !root.join("inspect.fork-now").exists() {
            if began.elapsed() > Duration::from_secs(5) {
                return Err("fork trigger missing".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let mut child = Command::new(std::env::current_exe()?);
        child.arg("ignore-term").arg(&root);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            child.process_group(0);
        }
        let _ = child.spawn()?.wait()?;
    } else if mode == "nested" || mode == "nested-wait" {
        let mut child = Command::new(std::env::current_exe()?);
        child.arg("ignore-term").arg(&root);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            child.process_group(0);
        }
        let mut child = child.spawn()?;
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
