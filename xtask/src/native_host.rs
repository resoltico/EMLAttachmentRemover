//! Native execution observations, distinct from compiler/source qualification.
use crate::Result;
#[cfg(unix)]
use crate::capture;
use serde_json::Value;
#[cfg(unix)]
use serde_json::json;
use std::path::Path;
#[cfg(unix)]
use std::{fs, process::Command};
pub(crate) fn inspect(root: &Path, target: &str, os: &str, architecture: &str) -> Result<Value> {
    if std::env::consts::OS != os || std::env::consts::ARCH != architecture {
        return Err("Archive execution requires its actual native OS/CPU; cross execution is not qualification".into());
    }
    #[cfg(not(unix))]
    {
        let _ = (root, target);
        Err("Windows archive verification awaits its owned process/private extraction and final-link boundaries".into())
    }
    #[cfg(unix)]
    {
        let compiler =
            String::from_utf8(capture(Command::new("rustc").current_dir(root).arg("-vV"))?)?;
        if !compiler
            .lines()
            .any(|line| line == format!("host: {target}"))
        {
            return Err("Compiler host differs from requested native target".into());
        }
        let machine = String::from_utf8(capture(Command::new("/usr/bin/uname").arg("-m"))?)?;
        let expected = if architecture == "aarch64" && os == "macos" {
            "arm64"
        } else {
            architecture
        };
        if machine.trim() != expected {
            return Err("Runtime kernel machine differs from requested target".into());
        }
        #[cfg(target_os = "macos")]
        let translation = translation_inspector(root, architecture)?;
        #[cfg(not(target_os = "macos"))]
        let translation = Value::Null;
        let actual_os = if os == "macos" {
            String::from_utf8(capture(&mut Command::new("/usr/bin/sw_vers"))?)?
        } else {
            fs::read_to_string("/etc/os-release")?
        };
        Ok(
            json!({"target":target,"process_os":os,"process_architecture":architecture,"kernel_machine":machine.trim(),"actual_os":actual_os,"compiler":compiler,"translation_inspector":translation,
            "scope":"actual host observations; no original-minimum-OS boot claim"}),
        )
    }
}

#[cfg(target_os = "macos")]
fn translation_response(
    architecture: &str,
    code: Option<i32>,
    stdout: &[u8],
    stderr: &[u8],
) -> Result<&'static str> {
    if code == Some(0) && stdout == b"0\n" && stderr.is_empty() {
        return Ok("same-cpu-inspector-not-translated");
    }
    if architecture == "x86_64"
        && code == Some(1)
        && stdout.is_empty()
        && stderr == b"sysctl: unknown oid 'sysctl.proc_translated'\n"
    {
        return Ok("intel-same-cpu-inspector-enoent");
    }
    Err("Translated or unverified CPU-bound macOS inspector result".into())
}

#[cfg(target_os = "macos")]
fn translation_inspector(root: &Path, architecture: &str) -> Result<Value> {
    let own = fs::read(std::env::current_exe()?)?;
    let file = object::File::parse(own.as_slice())?;
    if crate::macos::macho_architecture(&file)? != architecture {
        return Err("Running xtask thin CPU differs from bound inspector".into());
    }
    let parent = root.join("target/native-host");
    fs::create_dir_all(&parent)?;
    let work = tempfile::Builder::new()
        .prefix("inspector-")
        .tempdir_in(parent)?
        .keep();
    let mut command = Command::new("/usr/bin/arch");
    command
        .arg(if architecture == "x86_64" {
            "-x86_64"
        } else {
            "-arm64"
        })
        .args(["/usr/sbin/sysctl", "-n", "sysctl.proc_translated"])
        .env("LC_ALL", "C");
    let log = work.join("translation.log");
    let result = crate::controller()?.run_separate_output(
        command,
        &log,
        std::time::Duration::from_secs(10),
    )?;
    if result.timed_out
        || result.cancelled
        || result.unexpected_survivors != 0
        || !result.cleanup_complete
    {
        return Err("Translation inspector lifetime/cleanup is unqualified".into());
    }
    if fs::metadata(&log)?.len() > 512 || fs::metadata(log.with_extension("stderr"))?.len() > 512 {
        return Err("Translation inspector output exceeds its bound".into());
    }
    let basis = translation_response(
        architecture,
        result.status.code(),
        &fs::read(&log)?,
        &fs::read(log.with_extension("stderr"))?,
    )?;
    Ok(
        json!({"basis":basis,"bound_architecture":architecture,"scope":"same-CPU inspector inference plus running thin executable identity; not a direct parent sysctl query"}),
    )
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    #[test]
    fn translation_output_contract_rejects_blank_success_other_error_and_arm_missing_key() {
        let missing = b"sysctl: unknown oid 'sysctl.proc_translated'\n";
        assert!(translation_response("aarch64", Some(0), b"0\n", b"").is_ok());
        assert!(translation_response("x86_64", Some(1), b"", missing).is_ok());
        for (arch, code, out, err) in [
            ("aarch64", Some(1), b"".as_slice(), missing.as_slice()),
            ("x86_64", Some(0), b"".as_slice(), b"".as_slice()),
            ("x86_64", Some(0), b"1\n".as_slice(), b"".as_slice()),
            ("x86_64", Some(2), b"".as_slice(), missing.as_slice()),
            (
                "x86_64",
                Some(1),
                b"".as_slice(),
                b"permission denied\n".as_slice(),
            ),
        ] {
            assert!(translation_response(arch, code, out, err).is_err());
        }
    }
}
