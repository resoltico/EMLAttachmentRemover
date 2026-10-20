//! Exact package layouts and cross-member path claims.
use super::*;

pub(super) struct CliLayout {
    pub(super) files: BTreeSet<String>,
    pub(super) directories: BTreeSet<String>,
}

pub(super) fn producer_layout(
    root_name: &str,
    executable: &str,
    binary: &[u8],
    materials: &BTreeMap<String, Vec<u8>>,
) -> io::Result<CliLayout> {
    let layout = cli_layout(root_name, executable, materials)?;
    if binary.is_empty()
        || binary.len() as u64 > MAX_MEMBER_BYTES
        || layout.files.len() + layout.directories.len() > usize::from(MAX_ENTRIES)
    {
        return Err(invalid(
            "CLI archive producer input exceeds its member/count bounds",
        ));
    }
    let mut total = binary.len() as u64;
    for bytes in materials.values() {
        if bytes.len() as u64 > MAX_MEMBER_BYTES {
            return Err(invalid("CLI material exceeds archive member bound"));
        }
        total = total
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| invalid("CLI archive size overflow"))?;
    }
    if total > MAX_EXPANDED_BYTES {
        return Err(invalid("CLI archive payload exceeds its bound"));
    }
    Ok(layout)
}
pub(super) fn cli_layout(
    root_name: &str,
    executable: &str,
    materials: &BTreeMap<String, Vec<u8>>,
) -> io::Result<CliLayout> {
    for name in [root_name, executable] {
        validate_member_name(name)?;
        if name.contains('/') {
            return Err(invalid("CLI root/executable must be one component"));
        }
    }
    for name in [
        "README.txt",
        "LICENSE",
        "THIRD-PARTY-NOTICES.txt",
        "licenses/rust/COPYRIGHT-library.html",
        "licenses/rust/MIT.txt",
        "licenses/rust/Apache-2.0.txt",
        "licenses/rust/BSD-2-Clause.txt",
        "licenses/rust/Unicode-3.0.txt",
        "licenses/rust/source.json",
    ] {
        if !materials.get(name).is_some_and(|bytes| !bytes.is_empty()) {
            return Err(invalid("Expected CLI material inventory is incomplete"));
        }
    }
    let mut files = BTreeSet::from([format!("{root_name}/{executable}")]);
    for (name, bytes) in materials {
        validate_member_name(name)?;
        if name.ends_with('/')
            || name == executable
            || bytes.is_empty()
            || (!matches!(
                name.as_str(),
                "README.txt" | "LICENSE" | "THIRD-PARTY-NOTICES.txt"
            ) && !name.starts_with("licenses/"))
        {
            return Err(invalid("Invalid expected CLI material path or bytes"));
        }
        files.insert(format!("{root_name}/{name}"));
    }
    let mut directories = BTreeSet::new();
    for name in &files {
        let mut ancestor = name.as_str();
        while let Some((parent, _)) = ancestor.rsplit_once('/') {
            directories.insert(format!("{parent}/"));
            ancestor = parent;
        }
    }
    Ok(CliLayout { files, directories })
}

#[derive(Default)]
pub(super) struct PathClaims {
    pub(super) by_name: BTreeMap<String, (bool, String)>,
    pub(super) text_bytes: usize,
}
pub(super) fn claim_path(claims: &mut PathClaims, name: &str, directory: bool) -> io::Result<()> {
    let components: Vec<_> = name.trim_end_matches('/').split('/').collect();
    if components.len() > MAX_PATH_DEPTH {
        return Err(invalid("Archive package path is too deep"));
    }
    let mut path = String::new();
    for (index, component) in components.iter().enumerate() {
        if index != 0 {
            path.push('/');
        }
        path.push_str(component);
        let is_directory = index + 1 != components.len() || directory;
        let key = path.to_ascii_lowercase();
        if let Some((prior_directory, spelling)) = claims.by_name.get(&key) {
            if !prior_directory || !is_directory || spelling != &path {
                return Err(invalid("Archive file/directory claims collide"));
            }
        } else {
            let text_bytes = claims
                .text_bytes
                .checked_add(key.len() + path.len())
                .ok_or_else(|| invalid("Archive path claim budget overflow"))?;
            if text_bytes > MAX_CLAIM_TEXT_BYTES {
                return Err(invalid("Archive path claim text exceeds its bound"));
            }
            claims.by_name.insert(key, (is_directory, path.clone()));
            claims.text_bytes = text_bytes;
        }
    }
    Ok(())
}

/// Require a relative ASCII package path with no platform-dependent aliases.
pub fn validate_member_name(name: &str) -> io::Result<()> {
    if name.is_empty()
        || name.len() > MAX_NAME_BYTES
        || !name.is_ascii()
        || name.contains(['\\', '\0', ':', '<', '>', '"', '|', '?', '*'])
        || name.starts_with('/')
    {
        return Err(invalid("Invalid archive member spelling"));
    }
    let path = name.strip_suffix('/').unwrap_or(name);
    if path.split('/').any(|part| {
        part.is_empty()
            || matches!(part, "." | "..")
            || part.ends_with(['.', ' '])
            || part.chars().any(char::is_control)
            || reserved_windows_component(part)
    }) {
        return Err(invalid("Invalid archive path component"));
    }
    Ok(())
}

pub(super) fn reserved_windows_component(component: &str) -> bool {
    let base = component
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
}

#[cfg(unix)]
pub(super) fn set_directory_modes(root: &Path, directories: &BTreeSet<String>) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    for directory in directories {
        fs::set_permissions(
            root.join(directory.trim_end_matches('/')),
            fs::Permissions::from_mode(0o755),
        )?;
    }
    Ok(())
}
