//! Independent official-component identity checks; not a complete native-input approval.
use crate::Result;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};
use xtask::{archive::ByteIdentity, linker_reproducer::InputObservation};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    component: String,
    identity: ByteIdentity,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Component {
    target: String,
    package: String,
    archive: ByteIdentity,
    code_files: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct References {
    schema: u8,
    rust: String,
    components: Vec<Component>,
    targets: BTreeMap<String, BTreeMap<String, Reference>>,
    scope: String,
}
#[derive(Serialize)]
pub(crate) struct OfficialInput {
    pub member: String,
    pub relative: String,
    pub component: String,
    pub identity: ByteIdentity,
}

#[derive(Serialize)]
pub(crate) struct SelectedMember {
    pub archive: String,
    pub member: String,
    pub source: String,
    pub reason: String,
}

#[derive(Serialize)]
#[cfg(windows)]
pub(crate) struct RustInput {
    pub member: String,
    pub name: String,
    pub origin: String,
    pub identity: ByteIdentity,
}

fn reproducer_names(path: &Path) -> Result<[String; 2]> {
    let text = path
        .to_str()
        .ok_or("Native build input needs a Unicode tooling path")?
        .replace('\\', "/");
    let ordinary = text.strip_prefix("//?/").unwrap_or(&text);
    let bytes = ordinary.as_bytes();
    if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || &bytes[1..3] != b":/" {
        return Err("Native build input needs an absolute local-drive tooling path".into());
    }
    // Exact spellings emitted by current LLD for ordinary and verbatim DOS
    // addresses. They are evidence keys only, never operation paths.
    Ok([
        format!("reproducer/{}{}", &ordinary[..1], &ordinary[2..]),
        format!("reproducer/?{ordinary}"),
    ])
}

#[cfg(windows)]
pub(crate) fn rust_inputs(
    root: &Path,
    target: &str,
    inputs: &[InputObservation],
    official: &[OfficialInput],
    cargo_output: &[u8],
    directory: &Path,
    linked_basename: &str,
) -> Result<Vec<RustInput>> {
    use std::io::Read;
    let directory = directory.canonicalize()?;
    let stem = application_stem(linked_basename)?;
    let root = root.canonicalize()?;
    let libraries = cargo_runtime_libraries(&root, &directory, cargo_output)?;
    let prefixes = reproducer_names(&directory)?;
    let mut result = Vec::new();
    let mut application_objects = 0;
    let mut symbols = 0;
    for input in inputs {
        if official.iter().any(|known| known.member == input.member)
            || input.member == "reproducer/response.txt"
        {
            continue;
        }
        if let Some((origin, path)) = libraries.get(&input.member) {
            let mut bytes = Vec::new();
            fs::File::open(path)?
                .take(256 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 256 * 1024 * 1024 || ByteIdentity::of(&bytes) != input.identity {
                return Err("Cargo Rust archive changed after the actual link".into());
            }
            result.push(RustInput {
                member: input.member.clone(),
                name: format!(
                    "$CARGO_TARGET/{target}/release/deps/{}",
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .ok_or("Invalid Rust archive name")?
                ),
                origin: origin.clone(),
                identity: input.identity.clone(),
            });
            continue;
        }
        let relative = prefixes
            .iter()
            .find_map(|prefix| input.member.strip_prefix(&format!("{prefix}/")));
        if let Some(relative) = relative {
            if !relative.contains('/')
                && relative.starts_with(&format!("{stem}."))
                && relative.ends_with(".rcgu.o")
            {
                application_objects += 1;
                result.push(RustInput {
                    member: input.member.clone(),
                    name: format!("$CARGO_TARGET/{target}/release/deps/{relative}"),
                    origin: "application Rust compiler code generation".into(),
                    identity: input.identity.clone(),
                });
                continue;
            }
            if let Some(folder) = relative.strip_suffix("/symbols.o")
                && let Some(suffix) = folder.strip_prefix("rustc")
                && !suffix.is_empty()
                && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
            {
                symbols += 1;
                result.push(RustInput {
                    member: input.member.clone(),
                    name: "$RUSTC_TEMP/symbols.o".into(),
                    origin: "Rust compiler symbol object".into(),
                    identity: input.identity.clone(),
                });
                continue;
            }
        }
        return Err(format!("Unreviewed actual final-link input: {:?}", input.member).into());
    }
    if application_objects == 0
        || symbols != 1
        || result.iter().all(|input| !input.name.ends_with(".rlib"))
    {
        return Err(
            "Actual link omits required application/compiler/Cargo Rust input classes".into(),
        );
    }
    Ok(result)
}

#[cfg(windows)]
fn cargo_runtime_libraries(
    root: &Path,
    directory: &Path,
    cargo_output: &[u8],
) -> Result<BTreeMap<String, (String, std::path::PathBuf)>> {
    let mut libraries = BTreeMap::new();
    for document in
        serde_json::Deserializer::from_slice(cargo_output).into_iter::<serde_json::Value>()
    {
        let value = document?;
        if value["reason"] == "build-script-executed" {
            for key in ["linked_libs", "linked_paths"] {
                if value[key]
                    .as_array()
                    .is_some_and(|values| !values.is_empty())
                {
                    return Err(
                        "Cargo build script introduced an unreviewed native dependency".into(),
                    );
                }
            }
        }
        if value["reason"] != "compiler-artifact" || value["profile"]["test"] != false {
            continue;
        }
        for filename in value["filenames"]
            .as_array()
            .ok_or("Missing Cargo artifact filenames")?
        {
            let path = Path::new(filename.as_str().ok_or("Invalid Cargo artifact filename")?);
            if path.extension().is_none_or(|extension| extension != "rlib") {
                continue;
            }
            let actual = path.canonicalize()?;
            if actual.parent() != Some(directory) {
                continue;
            } // host proc-macro inputs are not the target runtime
            let package = value["package_id"]
                .as_str()
                .ok_or("Missing Cargo package identity")?;
            let origin = if package.starts_with("path+") {
                let manifest = Path::new(
                    value["manifest_path"]
                        .as_str()
                        .ok_or("Missing Cargo manifest identity")?,
                )
                .canonicalize()?;
                format!(
                    "workspace:{}",
                    manifest
                        .strip_prefix(root)
                        .map_err(|_| "Cargo path dependency lies outside the reviewed source")?
                        .to_string_lossy()
                        .replace('\\', "/")
                )
            } else if package.starts_with("registry+") {
                package.to_owned()
            } else {
                return Err("Unreviewed Cargo dependency source kind".into());
            };
            for name in reproducer_names(&actual)? {
                if libraries
                    .insert(name, (origin.clone(), actual.clone()))
                    .is_some()
                {
                    return Err("Cargo runtime artifact identity is duplicated".into());
                }
            }
        }
    }
    Ok(libraries)
}
pub(crate) fn application_stem(basename: &str) -> Result<&str> {
    let stem = basename
        .strip_suffix(".exe")
        .ok_or("Linked CLI name lacks its executable suffix")?;
    let hash = stem
        .strip_prefix("remove_eml_attachments-")
        .ok_or("Linked CLI name differs from its Cargo binary target")?;
    if hash.len() != 16 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Linked CLI name lacks a bounded Cargo artifact hash".into());
    }
    Ok(stem)
}

fn member_source(target: &str, archive: &str, member: &str) -> Option<(String, String)> {
    let arch = match target {
        "x86_64-pc-windows-gnullvm" => "lib64",
        "aarch64-pc-windows-gnullvm" => "libarm64",
        _ => return None,
    };
    if archive == "libunwind.a"
        && [
            "Unwind-seh.cpp.obj",
            "UnwindLevel1-gcc-ext.c.obj",
            "UnwindLevel1.c.obj",
            "libunwind.cpp.obj",
            "UnwindRegistersSave.S.obj",
        ]
        .contains(&member)
    {
        return Some((
            format!("libunwind/src/{}", member.strip_suffix(".obj")?),
            "LLVM Apache-2.0 WITH LLVM-exception; full retained libunwind license".into(),
        ));
    }
    if archive == "libmsvcrt.a" && member == "api-ms-win-crt-runtime-l1-1-0.dll" {
        return Some(("UCRT runtime import members in the checksum-pinned official archive".into(), "OS import descriptors, not a CRT implementation DLL redistribution; repeated member-name ambiguity retained".into()));
    }
    let prefix = match archive {
        "libmingw32.a" => format!("{arch}_libmingw32_a-"),
        "libmingwex.a" => format!("{arch}_libmingwex_a-"),
        "libmsvcrt.a" => format!("{arch}_libucrt_extra_a-"),
        _ => return None,
    };
    let stem = member.strip_prefix(&prefix)?.strip_suffix(".o")?;
    let path = match (archive, stem) {
        (
            "libmingw32.a",
            "tlssup" | "xthdloc" | "gccmain" | "mingw_helpers" | "crt_handler" | "usermatherr"
            | "natstart" | "cinitexe" | "xncommod" | "wildcard" | "xtxtmode" | "merr" | "_newmode"
            | "pseudo-reloc" | "dllargv" | "tlsthrd" | "tlsmcrt" | "pesect" | "pseudo-reloc-list",
        ) => format!("crt/{stem}.c"),
        ("libmingw32.a", "CRT_fp10") if arch == "lib64" => "crt/CRT_fp10.c".into(),
        ("libmingwex.a", "mingw_matherr") => "misc/mingw_matherr.c".into(),
        ("libmingwex.a", "mingw_cfguard_support") => "cfguard/mingw_cfguard_support.c".into(),
        ("libmingwex.a", "loadcfg") => "crt/loadcfg.S".into(),
        ("libmingwex.a", "guard_dispatch") if arch == "lib64" => "cfguard/guard_dispatch.S".into(),
        ("libmsvcrt.a", "ucrt__getmainargs" | "__p___initenv" | "ucrt_amsg_exit" | "__initenv") => {
            format!("misc/{stem}.c")
        }
        ("libmsvcrt.a", "ucrt_fprintf" | "ucrt_vfprintf" | "ucrt___local_stdio_printf_options") => {
            format!("stdio/{stem}.c")
        }
        _ => return None,
    };
    Some((format!("mingw-w64-crt/{path}"), "Reviewed MinGW runtime source; retained COPYING/runtime/disclaimer and source-specific pseudo-relocation terms".into()))
}

pub(crate) fn selected_members(
    target: &str,
    official: &[OfficialInput],
    trace: &str,
    map: &str,
) -> Result<Vec<SelectedMember>> {
    if !map
        .lines()
        .any(|line| line.split_ascii_whitespace().last() == Some("mainCRTStartup"))
    {
        return Err("Actual final map omits the required console startup sentinel".into());
    }
    let mut result = Vec::new();
    for line in trace.lines() {
        let Some(loaded) = line.strip_prefix("rust-lld: Loaded ") else {
            continue;
        };
        let Some((archive, remainder)) = loaded.split_once('(') else {
            return Err("Malformed selected archive-member trace".into());
        };
        if !archive.ends_with(".a") {
            continue;
        }
        let (member, symbol) = remainder
            .split_once(") for ")
            .ok_or("Malformed selected native member trace")?;
        if symbol.is_empty()
            || !official
                .iter()
                .any(|input| input.relative == format!("self-contained/{archive}"))
        {
            return Err("Selected native archive has no official component identity".into());
        }
        let (source, reason) = member_source(target, archive, member).ok_or_else(|| {
            format!("Unreviewed selected native archive member: {archive}({member})")
        })?;
        result.push(SelectedMember {
            archive: archive.into(),
            member: member.into(),
            source,
            reason,
        });
    }
    if result.is_empty() {
        return Err("Actual selected native member trace is empty".into());
    }
    Ok(result)
}

fn references(root: &Path) -> Result<References> {
    let references: References = serde_json::from_slice(&fs::read(
        root.join("legal/windows-runtime/component-inputs.json"),
    )?)?;
    let sources: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("legal/windows-runtime/source.json"))?)?;
    let toolchain: toml::Value =
        toml::from_str(&fs::read_to_string(root.join("rust-toolchain.toml"))?)?;
    if references.schema != 1
        || references.rust
            != toolchain["toolchain"]["channel"]
                .as_str()
                .ok_or("Missing Rust authority")?
        || sources["producer"]["rust"] != references.rust
        || references.scope.is_empty()
    {
        return Err("Official component references differ from source/toolchain authority".into());
    }
    let expected = sources["components"]
        .as_array()
        .ok_or("Missing official component sources")?;
    if expected.len() != references.components.len() || references.targets.len() != 2 {
        return Err("Official component reference set is incomplete".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for component in &references.components {
        if !seen.insert((&component.target, &component.package))
            || component.code_files == 0
            || !expected.iter().any(|source| {
                source["target"] == component.target
                    && source["package"] == component.package
                    && source["sha256"] == component.archive.sha256
            })
        {
            return Err(
                "Official component reference source is missing, duplicated or changed".into(),
            );
        }
        let files = references
            .targets
            .get(&component.target)
            .ok_or("Missing component target references")?;
        if files
            .values()
            .filter(|file| file.component == component.package)
            .count()
            != component.code_files
        {
            return Err("Official component code-file count differs from its references".into());
        }
    }
    Ok(references)
}

pub(crate) fn check_reference(
    root: &Path,
    target: &str,
    relative: &str,
    identity: &ByteIdentity,
) -> Result<String> {
    let references = references(root)?;
    let reference = references
        .targets
        .get(target)
        .and_then(|files| files.get(relative))
        .ok_or("Unreviewed official runtime input")?;
    if reference.identity != *identity {
        return Err("Runtime input differs from official component reference".into());
    }
    Ok(reference.component.clone())
}

pub(crate) fn official_inputs(
    root: &Path,
    target: &str,
    inputs: &[InputObservation],
    runtime_directory: &Path,
) -> Result<Vec<OfficialInput>> {
    let references = references(root)?;
    let expected = references
        .targets
        .get(target)
        .ok_or("Missing target official input references")?;
    let prefixes = reproducer_names(runtime_directory)?;
    let mut result = Vec::new();
    for input in inputs {
        let Some(relative) = prefixes
            .iter()
            .find_map(|prefix| input.member.strip_prefix(&format!("{prefix}/")))
        else {
            continue;
        };
        let reference = expected
            .get(relative)
            .ok_or("Unreferenced toolchain input in actual linker reproducer")?;
        if reference.identity != input.identity {
            return Err(format!(
                "Actual toolchain input differs from its official component bytes: {relative}"
            )
            .into());
        }
        result.push(OfficialInput {
            member: input.member.clone(),
            relative: relative.to_owned(),
            component: reference.component.clone(),
            identity: input.identity.clone(),
        });
    }
    if !result
        .iter()
        .any(|input| input.relative == "self-contained/crt2.o" && input.component == "rust-std")
    {
        return Err("Actual final link omits its required official console startup object".into());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_owned()
    }
    #[test]
    fn application_objects_use_the_linked_deps_name_not_the_public_binary_name() {
        for hash in ["43ccdf81f9fa5803", "babb91a3bf518500"] {
            let basename = format!("remove_eml_attachments-{hash}.exe");
            let stem = application_stem(&basename).unwrap();
            let object = format!("{stem}.remove_eml_attachments.2f7a-cgu.0.rcgu.o");
            assert!(object.starts_with(&format!("{stem}.")));
            assert!(!object.starts_with("remove-eml-attachments."));
        }
        for invalid in [
            "remove-eml-attachments.exe",
            "../remove_eml_attachments-43ccdf81f9fa5803.exe",
            "C:remove_eml_attachments-43ccdf81f9fa5803.exe",
            "remove_eml_attachments-43ccdf81f9fa5803/other.exe",
            "remove_eml_attachments-xyz.exe",
        ] {
            assert!(application_stem(invalid).is_err(), "{invalid}");
        }
    }
    #[test]
    fn reproducer_keys_match_current_dos_and_verbatim_spelling() {
        let expected = [
            "reproducer/C/Users/u/object.o".to_owned(),
            "reproducer/?C:/Users/u/object.o".to_owned(),
        ];
        assert_eq!(
            reproducer_names(Path::new(r"C:\Users\u\object.o")).unwrap(),
            expected
        );
        assert_eq!(
            reproducer_names(Path::new(r"\\?\C:\Users\u\object.o")).unwrap(),
            expected
        );
        assert!(reproducer_names(Path::new("relative/object.o")).is_err());
    }
    #[test]
    fn official_reference_identity_rejects_changed_bytes_and_missing_startup() {
        let root = root();
        let references = references(&root).unwrap();
        for (target, files) in references.targets {
            let runtime = format!("C:/observed-sysroot/lib/rustlib/{target}/lib");
            let reference = &files["self-contained/crt2.o"];
            let mut input = InputObservation {
                member: format!(
                    "reproducer/C/observed-sysroot/lib/rustlib/{target}/lib/self-contained/crt2.o"
                ),
                identity: reference.identity.clone(),
            };
            assert_eq!(
                official_inputs(
                    &root,
                    &target,
                    std::slice::from_ref(&input),
                    Path::new(&runtime)
                )
                .unwrap()
                .len(),
                1
            );
            assert!(
                official_inputs(
                    &root,
                    &target,
                    &[input],
                    Path::new(&runtime.replace("observed-sysroot", "another-sysroot"))
                )
                .is_err()
            );
            input = InputObservation {
                member: format!(
                    "reproducer/C/observed-sysroot/lib/rustlib/{target}/lib/self-contained/crt2.o"
                ),
                identity: ByteIdentity::of(b"changed startup bytes"),
            };
            assert!(official_inputs(&root, &target, &[input], Path::new(&runtime)).is_err());
            assert!(official_inputs(&root, &target, &[], Path::new(&runtime)).is_err());
        }
    }
    #[test]
    fn selected_member_admission_rejects_unknown_support_and_missing_startup_map() {
        let root = root();
        let references = references(&root).unwrap();
        for (target, files) in references.targets {
            let prefix = if target.starts_with("x86_64") {
                "lib64"
            } else {
                "libarm64"
            };
            let official =
                ["self-contained/crt2.o", "self-contained/libmingw32.a"].map(|relative| {
                    let reference = &files[relative];
                    OfficialInput {
                        member: relative.into(),
                        relative: relative.into(),
                        component: reference.component.clone(),
                        identity: reference.identity.clone(),
                    }
                });
            let trace = format!(
                "rust-lld: Loaded libmingw32.a({prefix}_libmingw32_a-gccmain.o) for __main\r\n"
            );
            let map = "00001000 00000000 0 mainCRTStartup\n";
            assert_eq!(
                selected_members(&target, &official, &trace, map)
                    .unwrap()
                    .len(),
                1
            );
            assert!(selected_members(&target, &official, &trace, "").is_err());
            let unknown = trace.replace("gccmain.o", "unreviewed-support.o");
            assert!(selected_members(&target, &official, &unknown, map).is_err());
        }
    }
}
