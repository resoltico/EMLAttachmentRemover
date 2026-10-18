//! One release CLI build and frozen owned artifact for archive/app consumers.
use crate::{Result, capture};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) struct FrozenCli {
    bytes: Vec<u8>,
    identity: xtask::archive::ByteIdentity,
    target: String,
    platform: String,
    version: String,
    host: Value,
}
impl FrozenCli {
    pub fn target(&self) -> &str {
        &self.target
    }
    pub fn platform(&self) -> &str {
        &self.platform
    }
    pub fn version(&self) -> &str {
        &self.version
    }
    pub fn host(&self) -> &Value {
        &self.host
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn identity(&self) -> &xtask::archive::ByteIdentity {
        &self.identity
    }
}

pub(crate) fn build(root: &Path, target: &str, out: &Path) -> Result<FrozenCli> {
    build_inner(root, target, out, None)
}

fn build_inner(
    root: &Path,
    target: &str,
    out: &Path,
    additional_input: Option<&Path>,
) -> Result<FrozenCli> {
    if additional_input.is_some() && !cfg!(windows) {
        return Err("Additional native-input control requires Windows".into());
    }
    crate::campaign::require_stock_topology(root)?;
    let plan: Value = serde_json::from_slice(&capture(
        Command::new(std::env::current_exe()?)
            .current_dir(root)
            .arg("distribution"),
    )?)?;
    let row = plan["native_targets"]
        .as_array()
        .ok_or("Missing native target table")?
        .iter()
        .find(|row| row["target"] == target)
        .ok_or("Unsupported native CLI target")?;
    let platform = row["platform"].as_str().ok_or("Missing native platform")?;
    let os = row["os"].as_str().ok_or("Missing native OS")?;
    let architecture = row["architecture"].as_str().ok_or("Missing native CPU")?;
    let host = crate::native_host::inspect(root, target, os, architecture)?;
    #[cfg(target_os = "macos")]
    let mac = crate::macos::load(root)?;
    let authority: toml::Value =
        toml::from_str(&fs::read_to_string(root.join("rust-toolchain.toml"))?)?;
    let required = authority["toolchain"]["channel"]
        .as_str()
        .ok_or("Missing Rust compiler authority")?;
    if !host["compiler"]
        .as_str()
        .ok_or("Missing compiler observation")?
        .lines()
        .any(|line| line == format!("release: {required}"))
    {
        return Err("Native CLI compiler differs from pinned Rust".into());
    }
    fs::create_dir(out)?;
    let out = out.canonicalize()?;
    #[cfg(windows)]
    let work = xtask::archive::create_artifact_directory(&out)?;
    #[cfg(windows)]
    let cargo = work.path().join("cargo");
    #[cfg(target_os = "macos")]
    let cargo = mac.cargo_directory(root);
    #[cfg(not(any(windows, target_os = "macos")))]
    let cargo = out.join("cargo");
    #[cfg(windows)]
    let wrapper = crate::windows_link::prepare(
        root,
        target,
        &work.path().join("native-link"),
        &cargo,
        host["compiler"]
            .as_str()
            .ok_or("Missing observed compiler")?,
    )?;
    #[cfg(target_os = "macos")]
    let mut command = mac.command(root, "cargo");
    #[cfg(not(target_os = "macos"))]
    let mut command = {
        let mut command = Command::new("cargo");
        command.current_dir(root);
        command
    };
    command
        .args([
            if cfg!(windows) { "rustc" } else { "build" },
            "--locked",
            "--release",
            "--package",
            "remove-eml-attachments",
            "--target",
            target,
            "--message-format=json",
        ])
        .env("CARGO_TARGET_DIR", &cargo);
    #[cfg(windows)]
    command
        .args(["--bin", "remove-eml-attachments", "--", "-C"])
        .arg(format!(
            "linker={}",
            wrapper
                .to_str()
                .ok_or("Native wrapper needs a Unicode tooling path")?
        ));
    #[cfg(windows)]
    if let Some(input) = additional_input {
        command.arg("-C").arg(format!(
            "link-arg={}",
            input
                .to_str()
                .ok_or("Native control input needs a Unicode tooling path")?
        ));
    }
    #[cfg(target_os = "macos")]
    command.env("MACOSX_DEPLOYMENT_TARGET", &mac.minimum_text);
    let log = out.join("cargo.jsonl");
    let process = crate::controller()?.run_separate_output(
        command,
        &log,
        std::time::Duration::from_secs(1200),
    )?;
    let mut output = Vec::new();
    fs::File::open(&log)?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut output)?;
    if output.len() > 16 * 1024 * 1024 {
        return Err(
            "Cargo build output exceeds its bounded document size; retained native output".into(),
        );
    }
    if !process.success() {
        for value in serde_json::Deserializer::from_slice(&output)
            .into_iter::<Value>()
            .flatten()
        {
            if value["reason"] == "compiler-message"
                && let Some(rendered) = value["message"]["rendered"].as_str()
            {
                eprintln!("{}", rendered.chars().take(65536).collect::<String>());
            }
        }
    }
    process.require_success(&log)?;
    let compiled = artifact(root, &output)?;
    let compiled_metadata = fs::symlink_metadata(&compiled)?;
    if !compiled_metadata.is_file() || compiled_metadata.len() > 256 * 1024 * 1024 {
        return Err("Native CLI artifact is not an ordinary bounded executable".into());
    }
    let mut profiles = Vec::new();
    for value in serde_json::Deserializer::from_slice(&output).into_iter::<Value>() {
        let value = value?;
        if value["reason"] == "compiler-artifact"
            && value["executable"]
                .as_str()
                .is_some_and(|path| Path::new(path) == compiled)
        {
            profiles.push(value["profile"].clone());
        }
    }
    if profiles.len() != 1 || profiles[0]["test"] != false || profiles[0]["overflow_checks"] != true
    {
        return Err(
            "Actual native release CLI must be a normal executable with overflow checks".into(),
        );
    }
    let destination = out.join(if os == "windows" {
        "remove-eml-attachments.exe"
    } else {
        "remove-eml-attachments"
    });
    let mut source = fs::File::open(&compiled)?;
    let mut owned = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)?;
    std::io::copy(&mut source, &mut owned)?;
    owned.flush()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        owned.set_permissions(fs::Permissions::from_mode(0o755))?;
    }
    owned.sync_all()?;
    drop(owned);
    #[cfg(target_os = "macos")]
    {
        crate::macos::fixture_identity(&fs::read(&destination)?, &mac.architecture, mac.minimum)?;
        let mut sign = mac.command(root, "/usr/bin/codesign");
        sign.args(["--force", "--sign", "-", "--timestamp=none"])
            .arg(&destination);
        crate::macos::checked_run(sign, &out.join("cli-sign.log"), 60)?;
        let mut verify = mac.command(root, "/usr/bin/codesign");
        verify
            .args(["--verify", "--strict", "--verbose=2"])
            .arg(&destination);
        crate::macos::checked_run(verify, &out.join("cli-signature.log"), 60)?;
    }
    let mut bytes = Vec::new();
    fs::File::open(&destination)?
        .take(256 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 256 * 1024 * 1024 {
        return Err("Native CLI exceeds package executable bound".into());
    }
    let identity = xtask::archive::ByteIdentity::of(&bytes);
    #[cfg(windows)]
    {
        let captured = crate::windows_link::captured(
            &work.path().join("native-link"),
            &bytes,
            &crate::git(root, &["rev-parse", "HEAD"])?,
            target,
            host["compiler"]
                .as_str()
                .ok_or("Missing observed compiler")?,
        )?;
        let imports = crate::windows_runtime::inspect(&bytes, architecture)?;
        let input_manifest: Value =
            serde_json::from_slice(&fs::read(work.path().join("native-link/inputs.json"))?)?;
        let input_files: Vec<xtask::linker_reproducer::InputObservation> =
            serde_json::from_value(input_manifest["inputs"].clone())?;
        let official = crate::windows_input_review::official_inputs(
            root,
            target,
            &input_files,
            &crate::windows_link::runtime_directory(&work.path().join("native-link"), target)?,
        )?;
        let selected = crate::windows_input_review::selected_members(
            target,
            &official,
            &fs::read_to_string(work.path().join("native-link/linker.log"))?,
            &fs::read_to_string(work.path().join("native-link/application.map"))?,
        )?;
        let rust_inputs = crate::windows_input_review::rust_inputs(
            root,
            target,
            &input_files,
            &official,
            &output,
            &cargo.join(target).join("release/deps"),
            captured["linked_basename"]
                .as_str()
                .ok_or("Missing actual linked executable basename")?,
        )?;
        fs::write(
            out.join("official-input-identities.json"),
            serde_json::to_vec_pretty(&json!({
            "source":crate::git(root,&["rev-parse","HEAD"])?,"target":target,"inputs":official,"selected_native_members":selected,"rust_inputs":rust_inputs,
            "scope":"official component identities, selected runtime member/source bindings and Cargo/compiler Rust input classes; complete archive-bound admission and real native rejection control remain separate"
            }))?,
        )?;
        fs::write(
            out.join("native-link-capture.json"),
            serde_json::to_vec_pretty(&captured)?,
        )?;
        fs::write(
            out.join("pe-imports.json"),
            serde_json::to_vec_pretty(&imports)?,
        )?;
    }
    let version = plan["version"]
        .as_str()
        .ok_or("Missing Cargo-derived version")?
        .to_owned();
    fs::write(
        out.join("artifact.json"),
        serde_json::to_vec_pretty(
            &json!({"target":target,"platform":platform,"version":version,"host":host,"profile":profiles[0],
        "owned_bytes":identity,"signing":{"mode":if cfg!(target_os="macos") {"adhoc"} else {"not_applicable"},"verified":cfg!(target_os="macos")},"scope":"native release CLI build/conditional signature identity; archive and full platform gates are separate"}),
        )?,
    )?;
    Ok(FrozenCli {
        bytes,
        identity,
        target: target.to_owned(),
        platform: platform.to_owned(),
        version,
        host,
    })
}

#[cfg(windows)]
pub(crate) fn unknown_input_control(
    root: &Path,
    target: &str,
    baseline: &FrozenCli,
    out: &Path,
) -> Result<Value> {
    use std::time::Duration;
    fs::create_dir(out)?;
    let out = out.canonicalize()?;
    let source = out.join("owned_unknown_native.rs");
    let object = out.join("owned_unknown_native.o");
    let mut source_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&source)?;
    source_file.write_all(
        b"#[unsafe(no_mangle)] pub extern \"C\" fn eml_unreviewed_native_probe() -> u64 { 17 }\n",
    )?;
    drop(source_file);
    let mut compile = Command::new("rustc");
    compile
        .current_dir(root)
        .args([
            "--crate-name",
            "eml_unreviewed_native_probe",
            "--crate-type",
            "lib",
            "--edition",
            "2024",
            "--emit",
            "obj",
            "--target",
            target,
        ])
        .arg(&source)
        .arg("-o")
        .arg(&object);
    let log = out.join("object-build.log");
    let compiled = crate::controller()?.run(compile, &log, Duration::from_secs(120))?;
    compiled.require_success(&log)?;
    let object_identity = xtask::archive::ByteIdentity::of(&fs::read(&object)?);
    let attempt = out.join("with-unknown-input");
    let rejection = match build_inner(root, target, &attempt, Some(&object)) {
        Ok(_) => return Err("Actual unreviewed native object was accepted".into()),
        Err(error)
            if error
                .to_string()
                .contains("Unreviewed actual final-link input:")
                && error.to_string().contains("owned_unknown_native.o") =>
        {
            error.to_string()
        }
        Err(error) => {
            return Err(format!(
                "Native unknown-input control failed for a different reason: {error}"
            )
            .into());
        }
    };
    let mut receipts = Vec::new();
    for entry in fs::read_dir(&attempt)? {
        let directory = entry?.path().join("native-link");
        if directory.join("link.json").is_file() {
            receipts.push(directory);
        }
    }
    if receipts.len() != 1 {
        return Err("Native unknown-input control has no unique actual linker receipt".into());
    }
    let receipt: Value = serde_json::from_slice(&fs::read(receipts[0].join("link.json"))?)?;
    let inputs: Value = serde_json::from_slice(&fs::read(receipts[0].join("inputs.json"))?)?;
    if receipt["actual_linker_status"] != 0
        || inputs["target"] != target
        || !inputs["inputs"]
            .as_array()
            .ok_or("Missing actual negative-control inputs")?
            .iter()
            .any(|input| {
                input["member"]
                    .as_str()
                    .is_some_and(|name| name.ends_with("/owned_unknown_native.o"))
                    && input["identity"]
                        == serde_json::to_value(&object_identity)
                            .expect("Byte identity serialization")
            })
    {
        return Err(
            "Unknown-input rejection did not follow an actual successful link consuming the probe"
                .into(),
        );
    }
    let rejected =
        xtask::archive::ByteIdentity::of(&fs::read(attempt.join("remove-eml-attachments.exe"))?);
    if receipt["linked_image"] != serde_json::to_value(&rejected)? {
        return Err(
            "Rejected native input trace is not bound to the actual control executable".into(),
        );
    }
    let result = json!({"source":crate::git(root,&["rev-parse","HEAD"])?,"target":target,"baseline_image":baseline.identity(),"object":object_identity,"object_compile":compiled.evidence(),"actual_linker_status":0,"rejected_image":rejected,"rejection":rejection,
        "scope":"real native rustc object and successful final CLI link consumed the unreviewed input; independent input admission rejected it before packaging"});
    fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
    Ok(result)
}
pub(crate) fn artifact(root: &Path, output: &[u8]) -> Result<PathBuf> {
    named_artifact(
        root,
        output,
        "crates/eml-cli/Cargo.toml",
        "crates/eml-cli/src/main.rs",
        "remove-eml-attachments",
    )
}
pub(crate) fn named_artifact(
    root: &Path,
    output: &[u8],
    manifest: &str,
    source: &str,
    name: &str,
) -> Result<PathBuf> {
    let manifest = root.join(manifest).canonicalize()?;
    let source = root.join(source).canonicalize()?;
    let mut candidates = Vec::new();
    let mut finished = 0;
    for document in serde_json::Deserializer::from_slice(output).into_iter::<serde_json::Value>() {
        let value = document?;
        if value["reason"] == "build-finished" {
            if value["success"] != true {
                return Err("Cargo reported an unsuccessful build".into());
            }
            finished += 1;
        }
        if value["reason"] != "compiler-artifact"
            || value["target"]["name"] != name
            || value["target"]["kind"] != serde_json::json!(["bin"])
        {
            continue;
        }
        let path = value["manifest_path"]
            .as_str()
            .ok_or("Artifact manifest missing")?;
        let main = value["target"]["src_path"]
            .as_str()
            .ok_or("Artifact source missing")?;
        if Path::new(path).canonicalize()? != manifest
            || Path::new(main).canonicalize()? != source
            || value["profile"]["test"] != false
        {
            continue;
        }
        candidates.push(PathBuf::from(
            value["executable"]
                .as_str()
                .ok_or("Native executable artifact missing")?,
        ));
    }
    if finished != 1 || candidates.len() != 1 {
        return Err(
            "Expected one successful Cargo build and one actual workspace executable".into(),
        );
    }
    Ok(candidates.remove(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cargo_selection_requires_one_real_workspace_bin_and_successful_completion() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap();
        let artifact = serde_json::json!({"reason":"compiler-artifact","manifest_path":root.join("crates/eml-cli/Cargo.toml"),"target":{"name":"remove-eml-attachments","kind":["bin"],"src_path":root.join("crates/eml-cli/src/main.rs")},"profile":{"test":false},"executable":root.join("target/owned-test-executable")});
        let finished = serde_json::json!({"reason":"build-finished","success":true});
        let encode = |values: &[serde_json::Value]| {
            values
                .iter()
                .map(|value| serde_json::to_string(value).unwrap())
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert_eq!(
            super::artifact(
                &root,
                encode(&[artifact.clone(), finished.clone()]).as_bytes()
            )
            .unwrap(),
            root.join("target/owned-test-executable")
        );
        for values in [
            vec![],
            vec![artifact.clone()],
            vec![finished.clone()],
            vec![artifact.clone(), artifact.clone(), finished.clone()],
            vec![
                artifact.clone(),
                serde_json::json!({"reason":"build-finished","success":false}),
            ],
        ] {
            assert!(super::artifact(&root, encode(&values).as_bytes()).is_err());
        }
        let mut harness = artifact.clone();
        harness["profile"]["test"] = true.into();
        assert!(super::artifact(&root, encode(&[harness, finished.clone()]).as_bytes()).is_err());
        let mut foreign = artifact;
        foreign["manifest_path"] = root.join("xtask/Cargo.toml").to_str().unwrap().into();
        assert!(super::artifact(&root, encode(&[foreign, finished]).as_bytes()).is_err());
    }
}
