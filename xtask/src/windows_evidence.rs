//! Portable final-link observations, independently checked after archive extraction.
use crate::{Result, windows_input_review as policy};
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use std::collections::BTreeMap;
use std::{collections::BTreeSet, fs, path::Path};
use xtask::archive::ByteIdentity;

pub(crate) const MATERIAL: &str = "licenses/windows-runtime/link-evidence.json";
pub(crate) const MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    name: String,
    origin: String,
    identity: ByteIdentity,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Text {
    bytes: String,
    identity: ByteIdentity,
}
impl Text {
    fn new(bytes: String) -> Self {
        Self {
            identity: ByteIdentity::of(bytes.as_bytes()),
            bytes,
        }
    }
    fn checked(&self) -> Result<&str> {
        if self.bytes.is_empty() || self.identity != ByteIdentity::of(self.bytes.as_bytes()) {
            return Err("Changed or empty normalized final-link text".into());
        }
        if self.bytes.contains(":/") || self.bytes.contains('\\') {
            return Err("Final-link text retains an unnormalized native path".into());
        }
        Ok(&self.bytes)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    schema: u8,
    source: String,
    target: String,
    compiler: String,
    linker_version: String,
    executable: ByteIdentity,
    linked_basename: String,
    references: ByteIdentity,
    inputs: Vec<Input>,
    map: Text,
    trace: Text,
}

fn reference_identity(root: &Path) -> Result<ByteIdentity> {
    Ok(ByteIdentity::of(&fs::read(
        root.join("legal/windows-runtime/component-inputs.json"),
    )?))
}

#[cfg(windows)]
pub(crate) struct Capture<'a> {
    pub source: &'a str,
    pub target: &'a str,
    pub compiler: &'a str,
    pub linker_version: &'a str,
    pub executable: &'a [u8],
    pub linked_basename: &'a str,
    pub runtime_directory: &'a Path,
    pub cargo_directory: &'a Path,
    pub link_directory: &'a Path,
    pub inputs: &'a [xtask::linker_reproducer::InputObservation],
    pub official: &'a [policy::OfficialInput],
    pub rust: &'a [policy::RustInput],
    pub map: &'a str,
    pub trace: &'a str,
}

#[cfg(windows)]
pub(crate) fn capture(root: &Path, observed: Capture<'_>) -> Result<Vec<u8>> {
    let NormalizedInputs {
        inputs,
        paths: input_paths,
    } = normalized_inputs(&observed)?;
    let replacements = [
        (observed.runtime_directory, "$RUST_LIB"),
        (observed.cargo_directory, "$CARGO_TARGET"),
        (observed.link_directory, "$LINK_CAPTURE"),
    ]
    .into_iter()
    .map(|(path, symbol)| {
        let path = path.canonicalize()?;
        let text = path
            .to_str()
            .ok_or("Normalization requires a Unicode tooling path")?
            .replace('\\', "/");
        Ok((
            text.strip_prefix("//?/").unwrap_or(&text).to_owned(),
            symbol,
        ))
    })
    .collect::<Result<Vec<_>>>()?;
    let normalize = |text: &str| {
        let mut text = text.replace('\\', "/");
        for (path, name) in &input_paths {
            text = text
                .replace(&format!("//?/{path}"), name)
                .replace(path, name);
        }
        for (path, symbol) in &replacements {
            text = text
                .replace(&format!("//?/{path}"), symbol)
                .replace(path, symbol);
        }
        Text::new(text)
    };
    let evidence = Evidence {
        schema: 1,
        source: observed.source.into(),
        target: observed.target.into(),
        compiler: observed.compiler.into(),
        linker_version: observed.linker_version.into(),
        executable: ByteIdentity::of(observed.executable),
        linked_basename: observed.linked_basename.into(),
        references: reference_identity(root)?,
        inputs,
        map: normalize(observed.map),
        trace: normalize(observed.trace),
    };
    let bytes = serde_json::to_vec_pretty(&evidence)?;
    verify(
        root,
        observed.target,
        observed.source,
        observed.executable,
        &bytes,
    )?;
    Ok(bytes)
}

/// Wire identities and text substitutions derived from the same admitted input origins.
#[cfg(windows)]
struct NormalizedInputs {
    inputs: Vec<Input>,
    paths: Vec<(String, String)>,
}
#[cfg(windows)]
fn normalized_inputs(observed: &Capture<'_>) -> Result<NormalizedInputs> {
    let mut names = BTreeMap::new();
    for input in observed.official {
        names.insert(
            input.member.as_str(),
            (
                format!("$RUST_LIB/{}", input.relative),
                format!("official:{}", input.component),
            ),
        );
    }
    for input in observed.rust {
        if names
            .insert(&input.member, (input.name.clone(), input.origin.clone()))
            .is_some()
        {
            return Err("Actual final-link input has multiple origins".into());
        }
    }
    let mut inputs = Vec::new();
    let mut input_paths = Vec::new();
    for input in observed.inputs {
        let (name, origin) = if input.member == "reproducer/response.txt" {
            (
                "$LINKER/response.txt".into(),
                "linker command record".into(),
            )
        } else {
            names
                .remove(input.member.as_str())
                .ok_or("Actual final-link input lacks an admitted origin")?
        };
        if input.member != "reproducer/response.txt" {
            let ordinary = if let Some(path) = input.member.strip_prefix("reproducer/?") {
                path.to_owned()
            } else {
                let path = input
                    .member
                    .strip_prefix("reproducer/")
                    .ok_or("Invalid reproducer input root")?;
                let (drive, tail) = path
                    .split_once('/')
                    .ok_or("Invalid reproducer drive address")?;
                if drive.len() != 1 || !drive.as_bytes()[0].is_ascii_alphabetic() {
                    return Err("Invalid reproducer drive letter".into());
                }
                format!("{drive}:/{tail}")
            };
            input_paths.push((ordinary, name.clone()));
        }
        inputs.push(Input {
            name,
            origin,
            identity: input.identity.clone(),
        });
    }
    if !names.is_empty() {
        return Err("Admitted input is missing from the actual final link".into());
    }
    input_paths.sort_by_key(|(path, _)| std::cmp::Reverse(path.len()));
    Ok(NormalizedInputs {
        inputs,
        paths: input_paths,
    })
}
fn reconcile_inputs(inputs: &[Input], trace: &str) -> Result<()> {
    let expected = inputs
        .iter()
        .filter(|input| input.name != "$LINKER/response.txt")
        .map(|input| input.name.as_str())
        .collect::<BTreeSet<_>>();
    let mut reads = BTreeSet::new();
    for line in trace.lines() {
        let Some(name) = line.strip_prefix("rust-lld: Reading ") else {
            continue;
        };
        let archive = name
            .split_once(".rlib(")
            .map(|(prefix, _)| format!("{prefix}.rlib"))
            .or_else(|| {
                name.split_once(".a(")
                    .map(|(prefix, _)| format!("{prefix}.a"))
            });
        if let Some(archive) = archive {
            if !name.ends_with(')')
                || expected
                    .iter()
                    .filter(|input| input.rsplit('/').next() == Some(archive.as_str()))
                    .count()
                    != 1
            {
                return Err("Archive-member read has no unique admitted final-link archive".into());
            }
        } else if !expected.contains(name) || !reads.insert(name) {
            return Err("Unknown or duplicate direct input in actual linker read trace".into());
        }
    }
    if reads.is_empty() || reads != expected {
        return Err("Final-link inventory differs from the complete direct-read trace".into());
    }
    Ok(())
}

fn cargo_origin(root: &Path, origin: &str) -> Result<String> {
    if let Some(manifest) = origin.strip_prefix("workspace:") {
        // Only checked-in workspace manifests authorize local runtime code.
        if manifest.contains(['\\', ':'])
            || manifest
                .split('/')
                .any(|part| matches!(part, "" | "." | ".."))
        {
            return Err("Invalid workspace runtime origin".into());
        }
        let metadata: serde_json::Value = serde_json::from_slice(&crate::capture(
            std::process::Command::new("cargo").current_dir(root).args([
                "metadata",
                "--locked",
                "--no-deps",
                "--format-version",
                "1",
            ]),
        )?)?;
        let root = root.canonicalize()?;
        let found = metadata["packages"]
            .as_array()
            .ok_or("Missing workspace metadata")?
            .iter()
            .find(|package| {
                package["manifest_path"]
                    .as_str()
                    .and_then(|path| Path::new(path).canonicalize().ok())
                    .is_some_and(|path| path == root.join(manifest))
            });
        return Ok(
            found.ok_or("Unreviewed workspace runtime manifest")?["name"]
                .as_str()
                .ok_or("Missing workspace package name")?
                .replace('-', "_"),
        );
    }
    let (source, package) = origin
        .rsplit_once('#')
        .ok_or("Missing registry runtime identity")?;
    let (name, version) = package
        .rsplit_once('@')
        .ok_or("Missing registry runtime version")?;
    let lock: toml::Value = toml::from_str(&fs::read_to_string(root.join("Cargo.lock"))?)?;
    if !lock["package"]
        .as_array()
        .ok_or("Missing locked packages")?
        .iter()
        .any(|package| {
            package["name"].as_str() == Some(name)
                && package["version"].as_str() == Some(version)
                && package.get("source").and_then(toml::Value::as_str) == Some(source)
        })
    {
        return Err("Runtime Rust dependency is outside the checked lockfile".into());
    }
    Ok(name.replace('-', "_"))
}

pub(crate) fn verify(
    root: &Path,
    target: &str,
    source: &str,
    executable: &[u8],
    bytes: &[u8],
) -> Result<()> {
    if bytes.len() > MAX_BYTES {
        return Err("Windows link evidence exceeds its bound".into());
    }
    let evidence: Evidence = serde_json::from_slice(bytes)?;
    if evidence.schema != 1
        || evidence.source != source
        || evidence.target != target
        || source.len() != 40
        || !source.bytes().all(|byte| byte.is_ascii_hexdigit())
        || evidence.executable != ByteIdentity::of(executable)
        || evidence.references != reference_identity(root)?
    {
        return Err("Missing, changed or stale Windows final-link evidence".into());
    }
    verify_compiler_identity(root, target, &evidence)?;
    let stem = policy::application_stem(&evidence.linked_basename)?;
    let map = evidence.map.checked()?;
    let trace = evidence.trace.checked()?;
    reconcile_inputs(&evidence.inputs, trace)?;
    let mut unique = BTreeSet::new();
    let mut official = Vec::new();
    let mut objects = 0;
    let mut symbols = 0;
    let mut libraries = 0;
    let mut commands = 0;
    let mut origins = std::collections::BTreeMap::new();
    let deps = format!("$CARGO_TARGET/{target}/release/deps/");
    for input in &evidence.inputs {
        if !unique.insert(&input.name)
            || input.identity.byte_length == 0
            || input.identity.sha256.len() != 64
            || !input
                .identity
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("Duplicate, empty or invalid normalized final-link input".into());
        }
        if let Some(relative) = input.name.strip_prefix("$RUST_LIB/") {
            let component = policy::check_reference(root, target, relative, &input.identity)?;
            if input.origin != format!("official:{component}") {
                return Err("Official runtime origin differs from component policy".into());
            }
            official.push(policy::OfficialInput {
                member: input.name.clone(),
                relative: relative.into(),
                component,
                identity: input.identity.clone(),
            });
        } else if let Some(relative) = input.name.strip_prefix(&deps) {
            if relative.contains('/') || relative.contains('\\') {
                return Err("Invalid normalized Cargo input basename".into());
            }
            if relative.starts_with("lib") && relative.ends_with(".rlib") {
                let crate_name = match origins.get(&input.origin) {
                    Some(name) => name,
                    None => origins
                        .entry(&input.origin)
                        .or_insert(cargo_origin(root, &input.origin)?),
                };
                let hash = relative
                    .strip_prefix(&format!("lib{crate_name}-"))
                    .and_then(|name| name.strip_suffix(".rlib"))
                    .ok_or("Rust archive name differs from its package origin")?;
                if hash.len() != 16 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return Err("Rust archive lacks its bounded Cargo hash".into());
                }
                libraries += 1;
            } else if relative.starts_with(&format!("{stem}."))
                && relative.ends_with(".rcgu.o")
                && input.origin == "application Rust compiler code generation"
            {
                objects += 1;
            } else {
                return Err("Unreviewed normalized Cargo final-link input".into());
            }
        } else if input.name == "$RUSTC_TEMP/symbols.o"
            && input.origin == "Rust compiler symbol object"
        {
            symbols += 1;
        } else if input.name == "$LINKER/response.txt" && input.origin == "linker command record" {
            commands += 1;
        } else {
            return Err("Unreviewed normalized final-link input".into());
        }
    }
    if objects == 0
        || symbols != 1
        || commands != 1
        || libraries == 0
        || !official
            .iter()
            .any(|input| input.relative == "self-contained/crt2.o")
    {
        return Err("Normalized final link omits required input classes/startup".into());
    }
    policy::selected_members(target, &official, trace, map)?;
    Ok(())
}

fn verify_compiler_identity(root: &Path, target: &str, evidence: &Evidence) -> Result<()> {
    let authority: toml::Value =
        toml::from_str(&fs::read_to_string(root.join("rust-toolchain.toml"))?)?;
    let release = authority["toolchain"]["channel"]
        .as_str()
        .ok_or("Missing Rust authority")?;
    let lines = evidence.compiler.lines().collect::<BTreeSet<_>>();
    let llvm = evidence
        .compiler
        .lines()
        .find_map(|line| line.strip_prefix("LLVM version: "))
        .ok_or("Missing compiler LLVM version")?;
    if !lines.contains(format!("release: {release}").as_str())
        || !lines.contains(format!("host: {target}").as_str())
        || evidence
            .linker_version
            .split_ascii_whitespace()
            .collect::<Vec<_>>()
            .get(..2)
            != Some(["LLD", llvm].as_slice())
    {
        return Err("Final-link compiler/linker identity differs from the native toolchain".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
    }
    fn fixture(target: &str) -> Evidence {
        let root = root();
        let references: serde_json::Value = serde_json::from_slice(
            &fs::read(root.join("legal/windows-runtime/component-inputs.json")).unwrap(),
        )
        .unwrap();
        let mut inputs = Vec::new();
        for relative in ["self-contained/crt2.o", "self-contained/libmingw32.a"] {
            let reference = &references["targets"][target][relative];
            inputs.push(Input {
                name: format!("$RUST_LIB/{relative}"),
                origin: format!("official:{}", reference["component"].as_str().unwrap()),
                identity: serde_json::from_value(reference["identity"].clone()).unwrap(),
            });
        }
        let lock: toml::Value =
            toml::from_str(&fs::read_to_string(root.join("Cargo.lock")).unwrap()).unwrap();
        let package = lock["package"]
            .as_array()
            .unwrap()
            .iter()
            .find(|package| package.get("source").is_some())
            .unwrap();
        let package_name = package["name"].as_str().unwrap();
        let stem = "emlattachmentremover-4707473ea7d9d8bd";
        let deps = format!("$CARGO_TARGET/{target}/release/deps");
        for (name, origin) in [
            (
                format!("{deps}/{stem}.emlattachmentremover.165be93e914db964-cgu.00.rcgu.o"),
                "application Rust compiler code generation".into(),
            ),
            (
                format!("{deps}/{stem}.emlattachmentremover.165be93e914db964-cgu.01.rcgu.o"),
                "application Rust compiler code generation".into(),
            ),
            (
                "$RUSTC_TEMP/symbols.o".into(),
                "Rust compiler symbol object".into(),
            ),
            (
                "$LINKER/response.txt".into(),
                "linker command record".into(),
            ),
            (
                format!(
                    "{deps}/lib{}-43ccdf81f9fa5803.rlib",
                    package_name.replace('-', "_")
                ),
                format!(
                    "{}#{package_name}@{}",
                    package["source"].as_str().unwrap(),
                    package["version"].as_str().unwrap()
                ),
            ),
        ] {
            inputs.push(Input {
                name,
                origin,
                identity: ByteIdentity::of(b"owned synthetic input; no execution evidence"),
            });
        }
        let release: toml::Value =
            toml::from_str(&fs::read_to_string(root.join("rust-toolchain.toml")).unwrap()).unwrap();
        let arch = if target.starts_with("x86_64") {
            "lib64"
        } else {
            "libarm64"
        };
        let reads = inputs
            .iter()
            .filter(|input| input.name != "$LINKER/response.txt")
            .map(|input| format!("rust-lld: Reading {}\n", input.name))
            .collect::<String>();
        Evidence {
            schema: 1,
            source: "a".repeat(40),
            target: target.into(),
            compiler: format!(
                "release: {}\nhost: {target}\nLLVM version: 23.1.1\n",
                release["toolchain"]["channel"].as_str().unwrap()
            ),
            linker_version: "LLD 23.1.1 (compatible with GNU linkers)".into(),
            executable: ByteIdentity::of(b"owned synthetic image; not an executable"),
            linked_basename: format!("{stem}.exe"),
            references: reference_identity(root).unwrap(),
            inputs,
            map: Text::new("00001000 00000000 0 mainCRTStartup\n".into()),
            trace: Text::new(format!(
                "{reads}rust-lld: Loaded libmingw32.a({arch}_libmingw32_a-gccmain.o) for __main\n"
            )),
        }
    }
    fn check(evidence: &Evidence) -> Result<()> {
        verify(
            root(),
            &evidence.target,
            &"a".repeat(40),
            b"owned synthetic image; not an executable",
            &serde_json::to_vec(evidence)?,
        )
    }
    #[test]
    fn former_rust_crate_namespace_cannot_qualify_current_product_link_evidence() {
        for target in ["x86_64-pc-windows-gnullvm", "aarch64-pc-windows-gnullvm"] {
            let mut evidence = fixture(target);
            // Keep the synthetic basename, input records and actual-read trace
            // mutually consistent; the obsolete product identity must refuse.
            evidence.linked_basename = evidence
                .linked_basename
                .replace("emlattachmentremover", "remove_eml_attachments");
            for input in &mut evidence.inputs {
                input.name = input
                    .name
                    .replace("emlattachmentremover", "remove_eml_attachments");
            }
            evidence.trace = Text::new(
                evidence
                    .trace
                    .bytes
                    .replace("emlattachmentremover", "remove_eml_attachments"),
            );
            let error = check(&evidence).unwrap_err().to_string();
            assert!(error.contains("Cargo binary target"), "{target}: {error}");
        }
    }
    #[test]
    fn recorded_link_consistency_rejects_stale_changed_and_unreviewed_inputs() {
        assert!(MATERIAL.starts_with("licenses/windows-runtime/"));
        for target in ["x86_64-pc-windows-gnullvm", "aarch64-pc-windows-gnullvm"] {
            let evidence = fixture(target);
            check(&evidence).unwrap();
            assert!(
                verify(
                    root(),
                    target,
                    &evidence.source,
                    b"changed image",
                    &serde_json::to_vec(&evidence).unwrap()
                )
                .is_err()
            );
            for change in 0..11 {
                let mut evidence = fixture(target);
                match change {
                    0 => evidence.source = "b".repeat(40),
                    1 => evidence
                        .trace
                        .bytes
                        .push_str("changed without updating identity"),
                    2 => evidence.inputs[0].identity = ByteIdentity::of(b"changed official input"),
                    3 => evidence.inputs[0].name = "$RUST_LIB/self-contained/unreviewed.o".into(),
                    4 => evidence.inputs[1].name = evidence.inputs[0].name.clone(),
                    5 => evidence.map = Text::new("missing console startup sentinel".into()),
                    6 => {
                        evidence.trace =
                            Text::new(evidence.trace.bytes.replace("gccmain.o", "unreviewed.o"))
                    }
                    7 => evidence.inputs[2].name = "$CARGO_TARGET/unknown.o".into(),
                    8 => evidence.trace = Text::new("C:/owner/private/file.o".into()),
                    9 => {
                        evidence.inputs.remove(2);
                    }
                    10 => {
                        evidence.trace = Text::new(format!(
                            "{}rust-lld: Reading $CARGO_TARGET/unreviewed.o\n",
                            evidence.trace.bytes
                        ))
                    }
                    _ => unreachable!(),
                }
                assert!(check(&evidence).is_err(), "accepted changed case {change}");
            }
            assert!(
                verify(
                    root(),
                    "wrong-target",
                    &evidence.source,
                    b"owned synthetic image; not an executable",
                    &serde_json::to_vec(&evidence).unwrap()
                )
                .is_err()
            );
        }
    }
}
