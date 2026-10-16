//! Locked builds of pinned repository tools without global installations.
use crate::{Result, campaign, capture, controller, tool_sources};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::Command,
    time::Duration,
};

fn document(path: &Path, limit: u64) -> Result<Vec<u8>> {
    if !fs::symlink_metadata(path)?.is_file() || fs::metadata(path)?.len() > limit {
        return Err("Repository tool input is not an admitted ordinary file".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err("Repository tool input exceeds bound".into());
    }
    Ok(bytes)
}
fn configuration_snapshot(cwd: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>> {
    let mut paths: Vec<_> = campaign::effective_configs(cwd)?
        .into_iter()
        .map(|path| path.canonicalize())
        .collect::<std::io::Result<_>>()?;
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .map(|path| document(&path, 4 * 1024 * 1024).map(|bytes| (path, bytes)))
        .collect()
}
fn inputs(root: &Path, name: &str) -> Result<Value> {
    let value: Value = serde_json::from_slice(&document(
        &root.join("xtask/tool-locks/build-inputs.json"),
        65_536,
    )?)?;
    let matching: Vec<_> = value["tools"]
        .as_array()
        .ok_or("Missing tool build inputs")?
        .iter()
        .filter(|record| record["package"] == format!("cargo-{name}"))
        .collect();
    if matching.len() != 1
        || value["schema"] != 1
        || value["review_status"] != "source inputs reviewed"
    {
        return Err("Missing unique reviewed tool input".into());
    }
    let record = matching[0];
    let filename = format!("cargo-{name}.Cargo.lock");
    let lock = document(
        &root.join("xtask/tool-locks").join(&filename),
        4 * 1024 * 1024,
    )?;
    let identity = xtask::archive::ByteIdentity::of(&lock);
    if record["build_lock"]["path"] != filename
        || record["build_lock"]["sha256"] != identity.sha256
        || record["build_lock"]["byte_length"] != identity.byte_length
    {
        return Err("Current tool lock differs from reviewed identity".into());
    }
    Ok(record.clone())
}
pub(crate) fn path(root: &Path, name: &str) -> Result<PathBuf> {
    let selected: Value = serde_json::from_slice(
        &document(&root.join("target/repository-tools/current.json"), 65_536)
            .map_err(|_| "Build the private pinned tools first: cargo xtask tools")?,
    )?;
    selection_path(root, name, &selected)
}
fn selection_path(root: &Path, name: &str, selected: &Value) -> Result<PathBuf> {
    let required_compiler = tool_sources::compiler(root)?;
    let pin = tool_sources::CARGO_TOOLS
        .iter()
        .find(|(tool, _)| *tool == name)
        .ok_or("Unknown repository tool")?
        .1;
    let records: Vec<_> = selected["tools"]
        .as_array()
        .ok_or("Missing selected tool builds")?
        .iter()
        .filter(|record| record["package"] == format!("cargo-{name}"))
        .collect();
    if records.len() != 1
        || selected["schema"] != 1
        || records[0]["version"] != campaign::pin(root, pin)?
        || records[0]["inputs"] != inputs(root, name)?
        || selected["process_os"] != std::env::consts::OS
        || selected["process_architecture"] != std::env::consts::ARCH
        || !records[0]["compiler"].as_str().is_some_and(|compiler| {
            compiler
                .lines()
                .any(|line| line == format!("release: {}", required_compiler))
        })
    {
        return Err(
            "Selected tool build differs from reviewed inputs/native host; run cargo xtask tools"
                .into(),
        );
    }
    let relative = Path::new(
        records[0]["binary"]["path"]
            .as_str()
            .ok_or("Missing tool binary path")?,
    );
    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
        || relative.file_name()
            != Some(std::ffi::OsStr::new(&format!(
                "cargo-{name}{}",
                std::env::consts::EXE_SUFFIX
            )))
    {
        return Err("Invalid selected repository tool path".into());
    }
    let parent = root.join("target/repository-tools").canonicalize()?;
    let path = parent.join(relative);
    if !path.canonicalize()?.starts_with(&parent) {
        return Err("Selected repository tool escaped its owned root".into());
    }
    let bytes = document(&path, 128 * 1024 * 1024)?;
    let identity = xtask::archive::ByteIdentity::of(&bytes);
    if records[0]["binary"]["sha256"] != identity.sha256
        || records[0]["binary"]["byte_length"] != identity.byte_length
    {
        return Err("Selected repository tool bytes changed".into());
    }
    Ok(path)
}
pub(crate) fn command(root: &Path, name: &str) -> Result<Command> {
    let mut command = Command::new(path(root, name)?);
    command
        .current_dir(root)
        .arg(name)
        .env("CARGO_HOME", campaign::cargo_home()?)
        .env("RUSTUP_TOOLCHAIN", tool_sources::compiler(root)?);
    Ok(command)
}
pub(crate) fn run(root: &Path) -> Result<()> {
    campaign::require_stock_topology(root)?;
    let mut authority_paths = vec![
        root.join("rust-toolchain.toml"),
        root.join("xtask/tools.toml"),
        root.join("xtask/tool-locks/build-inputs.json"),
        root.join("xtask/dependencies.toml"),
        root.join("xtask/distribution.toml"),
    ];
    for (name, _) in tool_sources::CARGO_TOOLS {
        authority_paths.push(
            root.join("xtask/tool-locks")
                .join(format!("cargo-{name}.Cargo.lock")),
        );
    }
    let mut configurations = vec![(root.to_owned(), configuration_snapshot(root)?)];
    let authorities: Vec<_> = authority_paths
        .into_iter()
        .map(|path| document(&path, 4 * 1024 * 1024).map(|bytes| (path, bytes)))
        .collect::<Result<_>>()?;
    let parent = root.join("target/repository-tools");
    fs::create_dir_all(&parent)?;
    let directory = tempfile::Builder::new()
        .prefix("build-")
        .tempdir_in(&parent)?
        .keep();
    let directory = directory.canonicalize()?;
    fs::create_dir(directory.join("bin"))?;
    eprintln!(
        "Retained locked repository tool build: {}",
        directory.display()
    );
    let compiler = String::from_utf8(capture(Command::new("rustc").current_dir(root).arg("-vV"))?)?;
    let required = tool_sources::compiler(root)?;
    if !compiler
        .lines()
        .any(|line| line == format!("release: {required}"))
    {
        return Err("Tool build compiler differs from its authority".into());
    }
    let host = compiler
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .ok_or("Tool build compiler host missing")?;
    let plan: Value = serde_json::from_slice(&capture(
        Command::new(std::env::current_exe()?)
            .current_dir(root)
            .arg("distribution"),
    )?)?;
    let row = plan["native_targets"]
        .as_array()
        .ok_or("Missing native tool build policy")?
        .iter()
        .find(|row| {
            row["os"] == std::env::consts::OS && row["architecture"] == std::env::consts::ARCH
        })
        .ok_or("Unsupported native tool host")?;
    if row["target"] != host {
        return Err("Tool compiler host differs from actual native policy".into());
    }
    let native_host =
        crate::native_host::inspect(root, host, std::env::consts::OS, std::env::consts::ARCH)?;
    let mut builds = Vec::new();
    let mut manifests = Vec::new();
    for (name, pin) in tool_sources::CARGO_TOOLS {
        let selected_source = tool_sources::build_source(root, name, pin, &directory)?;
        let used_input = selected_source.input_record;
        let manifest = selected_source.manifest;
        let cargo_cwd = manifest
            .parent()
            .and_then(Path::parent)
            .ok_or("Missing actual tool Cargo CWD")?;
        configurations.push((cargo_cwd.to_owned(), configuration_snapshot(cargo_cwd)?));
        let lock = fs::read(
            manifest
                .parent()
                .ok_or("Missing tool source")?
                .join("Cargo.lock"),
        )?;
        let used_lock = xtask::archive::ByteIdentity::of(&lock);
        if used_input["build_lock"]["sha256"] != used_lock.sha256
            || used_input["build_lock"]["byte_length"] != used_lock.byte_length
        {
            return Err("Actually selected tool lock differs from frozen input record".into());
        }
        let source_receipt: Value = serde_json::from_slice(&fs::read(
            directory.join(format!("tool-{name}-source.json")),
        )?)?;
        let mut metadata = tool_sources::cargo(root, &manifest)?;
        metadata
            .args([
                "metadata",
                "--locked",
                "--no-deps",
                "--format-version",
                "1",
                "--manifest-path",
            ])
            .arg(&manifest);
        if name == "about" {
            metadata.args(["--features", "cli"]);
        }
        let metadata: Value = serde_json::from_slice(&capture(&mut metadata)?)?;
        let package = metadata["packages"]
            .as_array()
            .ok_or("Tool metadata packages missing")?
            .iter()
            .find(|package| package["name"] == format!("cargo-{name}"))
            .ok_or("Tool metadata root missing")?;
        let targets: Vec<_> = package["targets"]
            .as_array()
            .ok_or("Tool targets missing")?
            .iter()
            .filter(|target| {
                target["name"] == format!("cargo-{name}") && target["kind"] == json!(["bin"])
            })
            .collect();
        if targets.len() != 1 {
            return Err("Expected one pinned tool binary target".into());
        }
        let source = Path::new(
            targets[0]["src_path"]
                .as_str()
                .ok_or("Tool binary source missing")?,
        )
        .strip_prefix(manifest.parent().unwrap())?;
        let mut build = tool_sources::cargo(root, &manifest)?;
        build
            .args([
                "build",
                "--locked",
                "--release",
                "--jobs",
                "2",
                "--config",
                "profile.release.overflow-checks=true",
                "--target",
                host,
                "--manifest-path",
            ])
            .arg(&manifest)
            .args([
                "--bin",
                &format!("cargo-{name}"),
                "--message-format=json",
                "--target-dir",
            ])
            .arg(directory.join("cargo"));
        if name == "about" {
            build.args(["--features", "cli"]);
        }
        let build_command = json!({"program":"cargo","arguments":build.get_args().map(|argument|argument.to_string_lossy().into_owned()).collect::<Vec<_>>(),"cwd":build.get_current_dir(),"cargo_home":campaign::cargo_home()?});
        if build.get_args().next() != Some(std::ffi::OsStr::new(&format!("+{required}"))) {
            return Err("Tool compiler selection drifted before build".into());
        }
        let transcript = capture(&mut build)?;
        fs::write(directory.join(format!("{name}-build.jsonl")), &transcript)?;
        let executable = crate::cli_build::named_artifact(
            manifest.parent().unwrap(),
            &transcript,
            "Cargo.toml",
            source.to_str().ok_or("Non-UTF-8 published source path")?,
            &format!("cargo-{name}"),
        )?;
        let profiles: Vec<Value> = serde_json::Deserializer::from_slice(&transcript)
            .into_iter::<Value>()
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|value| {
                value["reason"] == "compiler-artifact"
                    && value["target"]["kind"] == json!(["bin"])
                    && value["target"]["name"] == format!("cargo-{name}")
            })
            .map(|value| value["profile"].clone())
            .collect();
        if profiles.len() != 1
            || profiles[0]["overflow_checks"] != true
            || profiles[0]["test"] != false
            || profiles[0]["opt_level"] != "3"
        {
            return Err("Actual tool artifact profile differs from declared release checks".into());
        }
        if fs::read(manifest.parent().unwrap().join("Cargo.lock"))? != lock {
            return Err("Locked tool build changed selected lock".into());
        }
        let bytes = document(&executable, 128 * 1024 * 1024)?;
        let identity = xtask::archive::ByteIdentity::of(&bytes);
        let filename = format!("cargo-{name}{}", std::env::consts::EXE_SUFFIX);
        let destination = directory.join("bin").join(&filename);
        let mut output = fs::File::options()
            .create_new(true)
            .write(true)
            .open(&destination)?;
        output.write_all(&bytes)?;
        output.sync_all()?;
        drop(output);
        if document(&destination, 128 * 1024 * 1024)? != bytes {
            return Err("Owned tool copy readback differs from compiled bytes".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&destination, fs::Permissions::from_mode(0o700))?;
        }
        let mut version = Command::new(&destination);
        version.current_dir(root).args([name, "--version"]);
        let actual = String::from_utf8(capture(&mut version)?)?;
        let expected = campaign::pin(root, pin)?;
        if actual.trim() != format!("cargo-{name} {expected}") {
            return Err("Built tool version differs".into());
        }
        builds.push(json!({"package":format!("cargo-{name}"),"version":expected,"inputs":used_input,"source":source_receipt,"binary":{"path":destination.strip_prefix(parent.canonicalize()?)?,"byte_length":identity.byte_length,"sha256":identity.sha256},"build_command":build_command,"actual_profile":profiles[0],"compiler":compiler,"host":host,"features":if name=="about" {json!(["cli"])} else {json!([])},"default_features":true,"locked":true,"overflow_checks":true}));
        manifests.push((name, manifest));
    }
    let deny = directory
        .join("bin")
        .join(format!("cargo-deny{}", std::env::consts::EXE_SUFFIX));
    for (name, manifest) in &manifests {
        let mut audit = Command::new(&deny);
        audit
            .current_dir(manifest.parent().and_then(Path::parent).unwrap())
            .args(["deny", "--locked", "--manifest-path"])
            .arg(manifest)
            .arg("--config")
            .arg(root.join("xtask/dependencies.toml"))
            .env("CARGO_HOME", campaign::cargo_home()?)
            .env("RUSTUP_TOOLCHAIN", &required);
        if *name == "about" {
            audit.args(["--features", "cli"]);
        }
        audit.args(["check", "advisories", "sources", "licenses"]);
        let log = directory.join(format!("{name}-audit.log"));
        let result = controller()?.run(audit, &log, Duration::from_secs(600))?;
        result.require_success(&log)?;
    }
    let selected = json!({"schema":1,"process_os":std::env::consts::OS,"process_architecture":std::env::consts::ARCH,"native_host":native_host,"tools":builds,"scope":"actual locked native repository tool builds and selected source graphs; not product qualification"});
    let serialized = serde_json::to_vec_pretty(&selected)?;
    if serialized.len() > 65_536 {
        return Err("Selected tool receipt exceeds its consumer bound".into());
    }
    let pending = directory.join("selected.json");
    fs::write(&pending, &serialized)?;
    let readback: Value = serde_json::from_slice(&document(&pending, 65_536)?)?;
    if readback != selected {
        return Err("Pending tool selection readback differs".into());
    }
    for (name, _) in tool_sources::CARGO_TOOLS {
        let _ = selection_path(root, name, &readback)?;
    }
    fs::write(directory.join("result.json"), &serialized)?;
    for (path, bytes) in &authorities {
        if document(path, 4 * 1024 * 1024)? != *bytes {
            return Err(
                "Tool input authority changed during build; selected tools remain unchanged".into(),
            );
        }
    }
    for (cwd, before) in &configurations {
        if configuration_snapshot(cwd)? != *before {
            return Err("Effective Cargo configuration paths/bytes changed during tool build; prior selection preserved".into());
        }
    }
    fs::rename(pending, parent.join("current.json"))?;
    Ok(())
}
