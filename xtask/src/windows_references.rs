//! Derive byte references from pinned official components, without extracting them.
use crate::{Result, capture, controller};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::Path,
    process::Command,
    time::Duration,
};
use xtask::archive::ByteIdentity;

#[derive(Serialize)]
struct Reference {
    component: String,
    identity: ByteIdentity,
}
fn bounded(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    if !fs::symlink_metadata(path)?.is_file() {
        return Err("Component reference input is not an ordinary file".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(maximum + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        return Err("Component reference bytes exceed their bound".into());
    }
    Ok(bytes)
}
pub(crate) fn generate(root: &Path, archives: &Path, out: &Path) -> Result<()> {
    if !cfg!(unix) {
        return Err("Reference derivation currently requires the system Unix tar; this is not native Windows qualification".into());
    }
    let authority: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("legal/windows-runtime/source.json"))?)?;
    let toolchain: toml::Value =
        toml::from_str(&fs::read_to_string(root.join("rust-toolchain.toml"))?)?;
    let version = toolchain["toolchain"]["channel"]
        .as_str()
        .ok_or("Missing Rust authority")?;
    if authority["producer"]["rust"] != version {
        return Err("Runtime source and Rust toolchain versions differ".into());
    }
    let parent = root.join("target/windows-reference-generation");
    fs::create_dir_all(&parent)?;
    let work = xtask::archive::create_artifact_directory(&parent)?;
    let mut targets: BTreeMap<String, BTreeMap<String, Reference>> = BTreeMap::new();
    let mut components = Vec::new();
    for (index, component) in authority["components"]
        .as_array()
        .ok_or("Missing official components")?
        .iter()
        .enumerate()
    {
        components.push(derive_component(
            root,
            archives,
            work.path(),
            version,
            index,
            component,
            &mut targets,
        )?);
    }
    if targets.len() != 2 || components.len() != 4 {
        return Err("Official component reference set is incomplete".into());
    }
    let result = serde_json::json!({"schema":1,"rust":version,"components":components,"targets":targets,
        "scope":"byte references derived from checksum-pinned official component archives; selected input/member/source/legal approval and native execution are separate"});
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)?;
    output.write_all(&serde_json::to_vec_pretty(&result)?)?;
    output.flush()?;
    Ok(())
}

fn derive_component(
    root: &Path,
    archives: &Path,
    work: &Path,
    version: &str,
    index: usize,
    component: &serde_json::Value,
    targets: &mut BTreeMap<String, BTreeMap<String, Reference>>,
) -> Result<serde_json::Value> {
    let target = component["target"]
        .as_str()
        .ok_or("Missing component target")?;
    let package = component["package"]
        .as_str()
        .ok_or("Missing component package")?;
    if !matches!(
        target,
        "x86_64-pc-windows-gnullvm" | "aarch64-pc-windows-gnullvm"
    ) || !matches!(package, "rust-std" | "rust-mingw")
    {
        return Err("Unreviewed official component identity".into());
    }
    let url = component["url"]
        .as_str()
        .ok_or("Missing component source URL")?;
    let name = url
        .rsplit('/')
        .next()
        .ok_or("Missing component archive basename")?;
    if name != format!("{package}-{version}-{target}.tar.xz") {
        return Err("Component archive name differs from source authority".into());
    }
    let bytes = bounded(&archives.join(name), 512 * 1024 * 1024)?;
    let identity = ByteIdentity::of(&bytes);
    if component["sha256"] != identity.sha256 {
        return Err("Official component archive checksum differs".into());
    }
    let snapshot = work.join(format!("component-{index}.tar.xz"));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&snapshot)?;
    file.write_all(&bytes)?;
    file.flush()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o400))?;
    }
    drop(file);
    let inner = if package == "rust-std" {
        format!("{package}-{target}")
    } else {
        package.to_owned()
    };
    let prefix = format!("{package}-{version}-{target}/{inner}/lib/rustlib/{target}/lib/");
    let count = derive_members(root, &snapshot, index, &prefix, package, target, targets)?;
    if count == 0 {
        return Err("Official component yielded no native/Rust library references".into());
    }
    Ok(serde_json::json!({"target":target,"package":package,"archive":identity,"code_files":count}))
}

fn derive_members(
    root: &Path,
    snapshot: &Path,
    index: usize,
    prefix: &str,
    package: &str,
    target: &str,
    targets: &mut BTreeMap<String, BTreeMap<String, Reference>>,
) -> Result<usize> {
    let listing = capture(
        Command::new("/usr/bin/tar")
            .current_dir(root)
            .arg("-tf")
            .arg(snapshot),
    )?;
    if listing.len() > 1024 * 1024 {
        return Err("Official component member listing exceeds its bound".into());
    }
    let mut names = BTreeSet::new();
    let mut count = 0;
    for (member_index, member) in std::str::from_utf8(&listing)?.lines().enumerate() {
        if member.is_empty()
            || member.len() > 32768
            || member.starts_with('/')
            || member.contains(['\\', '\0'])
            || member
                .trim_end_matches('/')
                .split('/')
                .any(|part| matches!(part, "" | "." | ".."))
            || !names.insert(member)
        {
            return Err(
                "Official component contains an ambiguous or duplicated member name".into(),
            );
        }
        if names.len() > 4096 {
            return Err("Official component member count exceeds its bound".into());
        }
        let Some(relative) = member.strip_prefix(prefix) else {
            continue;
        };
        if ![".rlib", ".o", ".a"]
            .iter()
            .any(|extension| relative.ends_with(extension))
        {
            continue;
        }
        let log = snapshot
            .parent()
            .ok_or("Missing owned component snapshot parent")?
            .join(format!("component-{index}-member-{member_index}.bin"));
        // stdout goes to a fixed owned pathname. No archive member is ever
        // extracted to its advertised name or followed as an operation path.
        let mut command = Command::new("/usr/bin/tar");
        command
            .current_dir(root)
            .arg("-xOf")
            .arg(snapshot)
            .arg(member);
        let process = controller()?.run_separate_output(command, &log, Duration::from_secs(120))?;
        process.require_success(&log)?;
        let identity = ByteIdentity::of(&bounded(&log, 256 * 1024 * 1024)?);
        let prior = targets.entry(target.to_owned()).or_default().insert(
            relative.to_owned(),
            Reference {
                component: package.to_owned(),
                identity,
            },
        );
        if prior.is_some() {
            return Err("Official components claim the same target library twice".into());
        }
        count += 1;
        fs::remove_file(log)?;
    }
    Ok(count)
}
