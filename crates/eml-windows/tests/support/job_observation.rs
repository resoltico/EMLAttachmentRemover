//! Synthetic native Job/process fixture; never part of a product archive.
#![forbid(unsafe_code)]
#[cfg(not(windows))]
fn main() {
    std::process::exit(2);
}
#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(exit) = eml_windows::console_helper_entry() {
        return if exit == std::process::ExitCode::SUCCESS {
            Ok(())
        } else {
            Err("Private console sender failed".into())
        };
    }
    use std::{fs, path::PathBuf, process::Command, time::Duration};
    let mut args = std::env::args_os().skip(1);
    let mode = args.next().ok_or("Missing fixture mode")?;
    let root = PathBuf::from(args.next().ok_or("Missing owned fixture root")?);
    if mode == "streams" {
        println!("owned stdout");
        eprintln!("owned stderr");
        std::process::exit(23);
    } else if mode == "echo" {
        use std::os::windows::ffi::OsStrExt;
        for argument in args {
            println!("{:?}", argument.encode_wide().collect::<Vec<_>>());
        }
    } else if mode == "delayed-write" {
        fs::write(
            root.join("child-ready"),
            b"native child entered before delay",
        )?;
        std::thread::sleep(Duration::from_millis(1500));
        fs::write(root.join("invoked"), b"owned native child actually ran")?;
    } else if mode == "background" {
        let _child = Command::new(std::env::current_exe()?)
            .arg("delayed-write")
            .arg(&root)
            .spawn()?;
    } else if mode == "parent-exit" {
        let job = eml_windows::Job::new()?;
        let executable = std::env::current_exe()?;
        let arguments = vec!["delayed-write".into(), root.as_os_str().to_owned()];
        let environment = std::env::vars_os().collect::<Vec<_>>();
        let output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join("child.stdout"))?;
        let errors = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join("child.stderr"))?;
        let _process = job.launch(
            &eml_windows::ProcessSpec {
                executable: &executable,
                arguments: &arguments,
                directory: &root,
                environment: &environment,
            },
            &output,
            &errors,
        )?;
        let began = std::time::Instant::now();
        while !root.join("child-ready").exists() {
            if began.elapsed() > Duration::from_secs(5) {
                return Err("Contained child never entered its actual body".into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        fs::write(
            root.join("parent-ready"),
            b"child entered inside private job",
        )?;
        // Deliberately bypass Rust Drop. Kernel closure of the non-inherited Job
        // handle must still terminate the actual contained child.
        std::process::exit(0);
    } else {
        return Err("Unknown fixture mode".into());
    }
    Ok(())
}
