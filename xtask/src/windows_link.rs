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
    linker_version: String,
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
fn checked_linker_version(compiler: &str, bytes: &[u8]) -> Result<String> {
    let expected = compiler
        .lines()
        .find_map(|line| line.strip_prefix("LLVM version: "))
        .ok_or("Compiler has no observed LLVM version")?;
    if bytes.len() > 4096 {
        return Err("Linker version output exceeds its bound".into());
    }
    let version = std::str::from_utf8(bytes)?.trim_end_matches(['\r', '\n']);
    if version.lines().count() != 1
        || version
            .strip_prefix("LLD ")
            .and_then(|value| value.split_ascii_whitespace().next())
            != Some(expected)
    {
        return Err(
            "Selected actual linker version differs from the compiler's LLVM version".into(),
        );
    }
    Ok(version.to_owned())
}
pub(crate) fn prepare(
    root: &Path,
    target: &str,
    work: &Path,
    cargo: &Path,
    compiler: &str,
) -> Result<PathBuf> {
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
    let linker_version = checked_linker_version(
        compiler,
        &capture(
            Command::new(&linker)
                .current_dir(root)
                .args(["-flavor", "gnu", "--version"]),
        )?,
    )?;
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
        linker_version,
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
    let reproducer_path = work.join("reproducer.tar");
    match fs::symlink_metadata(&reproducer_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        _ => return Err("Native reproducer output is not a fresh absent address".into()),
    }
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
        .arg("--verbose")
        .arg(format!(
            "--reproduce={}",
            reproducer_path
                .to_str()
                .ok_or("Native reproducer needs a Unicode tooling path")?
        ));
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
    let mut record = serde_json::json!({"source":control.source,"target":control.target,"actual_linker":linker,"actual_linker_version":control.linker_version,"actual_linker_status":status.code(),"trace":ByteIdentity::of(&trace),"scope":"actual final link capture; selected-input review remains separate"});
    if !status.success() {
        write_new(&control.receipt, &serde_json::to_vec_pretty(&record)?)?;
        return Err(format!(
            "Native linker failed with actual status {status}; retained {}",
            control.trace.display()
        )
        .into());
    }
    // LLD expands rustc's response file itself and preserves actual input bytes
    // before rustc can remove transient objects. Never replay the command file.
    let reproducer = bounded(&reproducer_path, 512 * 1024 * 1024)?;
    let inputs = xtask::linker_reproducer::inspect(&reproducer)?;
    let input_bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "source":control.source,"target":control.target,"inputs":inputs,
        "scope":"actual linker reproducer member byte identities; selected-member, source and legal review remain separate"
    }))?;
    if input_bytes.len() > 4 * 1024 * 1024 {
        return Err("Native input manifest exceeds its bound".into());
    }
    write_new(&work.join("inputs.json"), &input_bytes)?;
    record["inputs"] = serde_json::to_value(ByteIdentity::of(&input_bytes))?;
    record["reproducer"] = serde_json::to_value(ByteIdentity::of(&reproducer))?;
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
    let basename = outputs[0]
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Linked CLI lacks a Unicode basename")?;
    crate::windows_input_review::application_stem(basename)?;
    record["linked_basename"] = serde_json::to_value(basename)?;
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
    compiler: &str,
) -> Result<serde_json::Value> {
    let record: serde_json::Value =
        serde_json::from_slice(&bounded(&work.join("link.json"), 1024 * 1024)?)?;
    checked_linker_version(
        compiler,
        record["actual_linker_version"]
            .as_str()
            .ok_or("Missing actual linker version")?
            .as_bytes(),
    )?;
    if record["source"] != source
        || record["target"] != target
        || record["actual_linker_status"] != 0
        || record["reproducer"]
            != serde_json::to_value(ByteIdentity::of(&bounded(
                &work.join("reproducer.tar"),
                512 * 1024 * 1024,
            )?))?
        || record["inputs"]
            != serde_json::to_value(ByteIdentity::of(&bounded(
                &work.join("inputs.json"),
                4 * 1024 * 1024,
            )?))?
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

pub(crate) fn runtime_directory(work: &Path, target: &str) -> Result<PathBuf> {
    let control: Control =
        serde_json::from_slice(&bounded(&work.join("control.json"), 64 * 1024)?)?;
    if control.schema != 1 || control.target != target {
        return Err("Selected sysroot control differs from the native build".into());
    }
    let bin = control
        .linker
        .parent()
        .ok_or("Missing selected linker directory")?;
    if bin.file_name() != Some(std::ffi::OsStr::new("bin")) {
        return Err("Selected linker is outside its target bin directory".into());
    }
    let runtime = bin
        .parent()
        .ok_or("Missing selected linker target directory")?;
    if runtime.file_name() != Some(std::ffi::OsStr::new(target)) {
        return Err("Selected linker target directory differs from the build".into());
    }
    Ok(runtime.join("lib").canonicalize()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_link_capture_cannot_bind_another_image_source_target_or_trace() {
        const COMPILER: &str = "LLVM version: 23.1.1";
        let root = tempfile::tempdir().unwrap();
        let work = root.path();
        let source = "a".repeat(40);
        let target = "x86_64-pc-windows-gnullvm";
        let image = b"owned synthetic final image identity; not executable";
        fs::write(work.join("application.map"), b"owned map").unwrap();
        fs::write(work.join("linker.log"), b"owned trace").unwrap();
        fs::write(
            work.join("reproducer.tar"),
            b"owned synthetic reproducer identity; not native input",
        )
        .unwrap();
        fs::write(
            work.join("inputs.json"),
            b"owned synthetic input record; not native provenance",
        )
        .unwrap();
        let record = serde_json::json!({"source":source,"target":target,"actual_linker_version":"LLD 23.1.1 (compatible with GNU linkers)","actual_linker_status":0,"reproducer":ByteIdentity::of(b"owned synthetic reproducer identity; not native input"),"inputs":ByteIdentity::of(b"owned synthetic input record; not native provenance"),"linked_image":ByteIdentity::of(image),"map":ByteIdentity::of(b"owned map"),"trace":ByteIdentity::of(b"owned trace")});
        fs::write(work.join("link.json"), serde_json::to_vec(&record).unwrap()).unwrap();
        captured(work, image, &source, target, COMPILER).unwrap();
        assert!(captured(work, image, &source, target, "LLVM version: 22.0.0").is_err());
        assert!(captured(work, b"different final bytes", &source, target, COMPILER).is_err());
        assert!(captured(work, image, &"b".repeat(40), target, COMPILER).is_err());
        assert!(captured(work, image, &source, "aarch64-pc-windows-gnullvm", COMPILER).is_err());
        fs::write(work.join("linker.log"), b"changed trace").unwrap();
        assert!(captured(work, image, &source, target, COMPILER).is_err());
        fs::write(work.join("linker.log"), b"owned trace").unwrap();
        fs::write(work.join("inputs.json"), b"changed input observations").unwrap();
        assert!(captured(work, image, &source, target, COMPILER).is_err());
        fs::write(
            work.join("inputs.json"),
            b"owned synthetic input record; not native provenance",
        )
        .unwrap();
        fs::write(work.join("reproducer.tar"), b"changed reproducer").unwrap();
        assert!(captured(work, image, &source, target, COMPILER).is_err());
    }
}
