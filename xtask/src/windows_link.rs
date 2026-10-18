//! Actual final application link capture; review of selected native inputs follows.
use crate::{Result, capture};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
};
use xtask::archive::ByteIdentity;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Control {
    schema: u8,
    source: String,
    target: String,
    linker: PathBuf,
    map: PathBuf,
    trace: PathBuf,
    receipt: PathBuf,
    output_directory: PathBuf,
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    Ok(())
}
fn bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err("Native link evidence is not an ordinary bounded file".into());
    }
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err("Native link evidence grew past its bound".into());
    }
    Ok(bytes)
}
pub(crate) fn prepare(root: &Path, target: &str, work: &Path, cargo: &Path) -> Result<PathBuf> {
    fs::create_dir(work)?;
    let work = work.canonicalize()?;
    let sysroot = PathBuf::from(
        String::from_utf8(capture(
            Command::new("rustc")
                .current_dir(root)
                .args(["--print", "sysroot"]),
        )?)?
        .trim(),
    );
    let linker = sysroot
        .join("lib/rustlib")
        .join(target)
        .join("bin/rust-lld.exe")
        .canonicalize()?;
    let wrapper = work.join("eml-native-link.exe");
    write_new(
        &wrapper,
        &bounded(&std::env::current_exe()?, 256 * 1024 * 1024)?,
    )?;
    let control = Control {
        schema: 1,
        source: crate::git(root, &["rev-parse", "HEAD"])?,
        target: target.to_owned(),
        linker,
        map: work.join("application.map"),
        trace: work.join("linker.log"),
        receipt: work.join("link.json"),
        output_directory: cargo.join(target).join("release/deps"),
    };
    write_new(
        &work.join("control.json"),
        &serde_json::to_vec_pretty(&control)?,
    )?;
    Ok(wrapper)
}
fn run_helper() -> Result<()> {
    let executable = std::env::current_exe()?;
    // GetModuleFileName/current_exe may return the ordinary drive spelling of
    // the same executable whose control used canonical verbatim paths. Compare
    // both against the native canonical parent; do not weaken parent admission.
    let work = executable
        .parent()
        .ok_or("Missing native link enclosure")?
        .canonicalize()?;
    let control: Control =
        serde_json::from_slice(&bounded(&work.join("control.json"), 64 * 1024)?)?;
    if control.schema != 1
        || control.map.parent() != Some(work.as_path())
        || control.trace.parent() != Some(work.as_path())
        || control.receipt.parent() != Some(work.as_path())
    {
        return Err("Native link control does not match its owned enclosure".into());
    }
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    let mut command = Command::new(&control.linker);
    // rustc's explicit ld.lld flavor supplies GNU arguments to this wrapper;
    // rust-lld itself dispatches its selected driver through -flavor gnu.
    if arguments.first().is_none_or(|arg| arg != "-flavor") {
        command.args(["-flavor", "gnu"]);
    }
    command
        .args(&arguments)
        .arg(format!(
            "--Map={}",
            control
                .map
                .to_str()
                .ok_or("Native link map needs a Unicode tooling path")?
        ))
        .arg("--verbose");
    let trace = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&control.trace)?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(trace.try_clone()?))
        .stderr(Stdio::from(trace));
    // The outer verification Job already owns this exact wrapper and inherited
    // linker descendant. No shell or independent name/PID cleanup is introduced.
    let status = command.spawn()?.wait()?;
    let linker = ByteIdentity::of(&bounded(&control.linker, 256 * 1024 * 1024)?);
    let trace = bounded(&control.trace, 64 * 1024 * 1024)?;
    let mut record = serde_json::json!({"source":control.source,"target":control.target,"actual_linker":linker,"actual_linker_status":status.code(),"trace":ByteIdentity::of(&trace),"scope":"actual final link capture; selected-input review remains separate"});
    if !status.success() {
        write_new(&control.receipt, &serde_json::to_vec_pretty(&record)?)?;
        return Err(format!(
            "Native linker failed with actual status {status}; retained {}",
            control.trace.display()
        )
        .into());
    }
    let map = bounded(&control.map, 64 * 1024 * 1024)?;
    if trace.is_empty() || map.is_empty() {
        return Err("Native final-link map/verbose trace is empty".into());
    }
    let outputs = fs::read_dir(&control.output_directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .filter(|path| {
            path.extension()
                .is_some_and(|name| name.eq_ignore_ascii_case("exe"))
        })
        .collect::<Vec<_>>();
    if outputs.len() != 1 {
        return Err(
            "Fresh selected CLI link did not produce exactly one target-deps executable".into(),
        );
    }
    let image = bounded(&outputs[0], 256 * 1024 * 1024)?;
    record["map"] = serde_json::to_value(ByteIdentity::of(&map))?;
    record["linked_image"] = serde_json::to_value(ByteIdentity::of(&image))?;
    write_new(&control.receipt, &serde_json::to_vec_pretty(&record)?)?;
    Ok(())
}
pub(crate) fn helper_entry() -> Option<ExitCode> {
    if std::env::current_exe().ok()?.file_name()
        != Some(std::ffi::OsStr::new("eml-native-link.exe"))
    {
        return None;
    }
    Some(match run_helper() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Native final link: {error}");
            ExitCode::FAILURE
        }
    })
}
pub(crate) fn captured(
    work: &Path,
    bytes: &[u8],
    source: &str,
    target: &str,
) -> Result<serde_json::Value> {
    let record: serde_json::Value =
        serde_json::from_slice(&bounded(&work.join("link.json"), 1024 * 1024)?)?;
    if record["source"] != source
        || record["target"] != target
        || record["actual_linker_status"] != 0
        || record["linked_image"] != serde_json::to_value(ByteIdentity::of(bytes))?
        || record["map"]
            != serde_json::to_value(ByteIdentity::of(&bounded(
                &work.join("application.map"),
                64 * 1024 * 1024,
            )?))?
        || record["trace"]
            != serde_json::to_value(ByteIdentity::of(&bounded(
                &work.join("linker.log"),
                64 * 1024 * 1024,
            )?))?
    {
        return Err(
            "Missing, changed or stale final link evidence does not bind the actual CLI bytes"
                .into(),
        );
    }
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_link_capture_cannot_bind_another_image_source_target_or_trace() {
        let root = tempfile::tempdir().unwrap();
        let work = root.path();
        let source = "a".repeat(40);
        let target = "x86_64-pc-windows-gnullvm";
        let image = b"owned synthetic final image identity; not executable";
        fs::write(work.join("application.map"), b"owned map").unwrap();
        fs::write(work.join("linker.log"), b"owned trace").unwrap();
        let record = serde_json::json!({"source":source,"target":target,"actual_linker_status":0,"linked_image":ByteIdentity::of(image),"map":ByteIdentity::of(b"owned map"),"trace":ByteIdentity::of(b"owned trace")});
        fs::write(work.join("link.json"), serde_json::to_vec(&record).unwrap()).unwrap();
        captured(work, image, &source, target).unwrap();
        assert!(captured(work, b"different final bytes", &source, target).is_err());
        assert!(captured(work, image, &"b".repeat(40), target).is_err());
        assert!(captured(work, image, &source, "aarch64-pc-windows-gnullvm").is_err());
        fs::write(work.join("linker.log"), b"changed trace").unwrap();
        assert!(captured(work, image, &source, target).is_err());
    }
}
