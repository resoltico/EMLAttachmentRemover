//! One compiler policy, explicit ownership and validated exception metadata.
use super::Policy;
use crate::{Result, capture};
use std::{collections::BTreeSet, fs, path::Path, process::Command};

pub(super) fn validate(root: &Path, policy: &Policy) -> Result<()> {
    if policy.schema != 1 || policy.max_source_lines == 0 {
        return Err("Unsupported or empty quality policy".into());
    }
    let mut ids = BTreeSet::new();
    for (id, file, scope, rationale) in policy
        .exceptions
        .iter()
        .map(|entry| (&entry.id, &entry.file, &entry.scope, &entry.rationale))
        .chain(
            policy
                .command_exceptions
                .iter()
                .map(|entry| (&entry.id, &entry.file, &entry.scope, &entry.rationale)),
        )
    {
        if id.is_empty()
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            || !ids.insert(id)
            || scope.trim().is_empty()
            || rationale.trim().is_empty()
            || !root.join(file).is_file()
        {
            return Err(format!("Invalid, duplicate or stale quality exception: {id}").into());
        }
    }
    for entry in &policy.exceptions {
        if entry.lints.is_empty()
            || entry.lints.iter().any(|lint| {
                matches!(
                    lint.as_str(),
                    "warnings" | "clippy::all" | "clippy::pedantic" | "clippy::restriction"
                )
            })
        {
            return Err(format!("Blanket lint suppression is prohibited: {}", entry.id).into());
        }
        let unique: BTreeSet<_> = entry.lints.iter().collect();
        if unique.len() != entry.lints.len() {
            return Err(format!("Duplicate lint in exception {}", entry.id).into());
        }
    }
    manifests(root, policy)?;
    tool_configs(root)
}
fn manifests(root: &Path, policy: &Policy) -> Result<()> {
    let authority: toml::Value = toml::from_str(&fs::read_to_string(root.join("Cargo.toml"))?)?;
    let workspace = &authority["workspace"];
    let lints = &workspace["lints"];
    strict_levels(lints)?;
    let members = workspace["members"]
        .as_array()
        .ok_or("Missing workspace members")?;
    let excluded = workspace["exclude"]
        .as_array()
        .ok_or("Missing workspace exclusions")?;
    let patches = authority["patch"]["crates-io"]
        .as_table()
        .ok_or("Missing path-patch ownership")?;
    for vendor in &policy.vendor {
        let path = Path::new(vendor);
        let components: Vec<_> = path.components().collect();
        if components.len() != 2
            || !vendor.starts_with("vendor/")
            || components
                .iter()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
            || !root.join(vendor).join("Cargo.toml").is_file()
            || fs::symlink_metadata(root.join(vendor))?
                .file_type()
                .is_symlink()
            || members.iter().any(|value| value.as_str() == Some(vendor))
            || !excluded.iter().any(|value| value.as_str() == Some(vendor))
        {
            return Err(format!(
                "Invalid or authored-member-overlapping vendor ownership: {vendor}"
            )
            .into());
        }
    }
    let patched: BTreeSet<_> = patches
        .values()
        .filter_map(|entry| entry.get("path").and_then(toml::Value::as_str))
        .collect();
    let vendors: BTreeSet<_> = policy.vendor.iter().map(String::as_str).collect();
    if patched != vendors || vendors.len() != policy.vendor.len() {
        return Err("Vendor ownership must exactly match Cargo path patches".into());
    }
    let mut known = BTreeSet::from(["Cargo.toml".to_owned()]);
    for value in members.iter().chain(excluded) {
        let directory = value.as_str().ok_or("Workspace paths must be explicit")?;
        if vendors.contains(directory) {
            continue;
        }
        let path = format!("{directory}/Cargo.toml");
        let manifest: toml::Value = toml::from_str(&fs::read_to_string(root.join(&path))?)?;
        lint_table(&path, &manifest, lints, members.contains(value))?;
        known.insert(path);
    }
    let listing = capture(Command::new("git").current_dir(root).args([
        "ls-files",
        "--cached",
        "--others",
        "--exclude-standard",
        "-z",
        "--",
        "Cargo.toml",
        "**/Cargo.toml",
    ]))?;
    for raw in listing
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let path = std::str::from_utf8(raw)?;
        if root.join(path).is_file()
            && !known.contains(path)
            && !vendors
                .iter()
                .any(|prefix| path.starts_with(&format!("{prefix}/")))
        {
            return Err(format!("Unclassified authored Cargo manifest: {path}").into());
        }
    }
    Ok(())
}
fn tool_configs(root: &Path) -> Result<()> {
    let shell = fs::read_to_string(root.join(".shellcheckrc"))?;
    let settings: BTreeSet<_> = shell
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    if settings != BTreeSet::from(["external-sources=true", "enable=all", "severity=style"]) {
        return Err(
            "ShellCheck policy must enable all checks at style severity without exclusions".into(),
        );
    }
    let clippy: toml::Value = toml::from_str(&fs::read_to_string(root.join("clippy.toml"))?)?;
    let thresholds = [
        "too-many-lines-threshold",
        "cognitive-complexity-threshold",
        "too-many-arguments-threshold",
        "type-complexity-threshold",
    ];
    let table = clippy.as_table().ok_or("Invalid Clippy configuration")?;
    if table.len() != thresholds.len()
        || thresholds.iter().any(|key| {
            table
                .get(*key)
                .and_then(toml::Value::as_integer)
                .is_none_or(|value| value <= 0)
        })
    {
        return Err(
            "Clippy configuration must contain only the four positive structural thresholds".into(),
        );
    }
    let cargo_config = fs::read_to_string(root.join(".cargo/config.toml"))?;
    check_tool_options(".cargo/config.toml", &cargo_config)?;
    for entry in fs::read_dir(root.join(".github/workflows"))? {
        let path = entry?.path();
        if matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("yml" | "yaml")
        ) {
            check_tool_options(&path.display().to_string(), &fs::read_to_string(path)?)?;
        }
    }
    for name in ["rustfmt.toml", ".rustfmt.toml"] {
        if root.join(name).exists() {
            return Err(format!("{name}: formatter configuration needs an explicit reviewed policy; default formatting currently has no exclusions").into());
        }
    }
    Ok(())
}
pub(super) fn check_directives(path: &str, source: &str) -> Result<()> {
    for line in source.lines() {
        let line = line.trim();
        if (path.ends_with(".swift")
            && (line.starts_with("//") || line.starts_with("/*") || line.starts_with('*'))
            && line.contains("swift-format-ignore"))
            || (line.starts_with('#')
                && line.contains("shellcheck")
                && (line.contains("disable") || line.contains("exclude")))
        {
            return Err(
                format!("{path}: unregistered formatter/linter suppression directive").into(),
            );
        }
    }
    Ok(())
}

pub(super) fn lint_table(
    path: &str,
    manifest: &toml::Value,
    lints: &toml::Value,
    member: bool,
) -> Result<()> {
    let inherited = manifest
        .get("lints")
        .and_then(|value| value.get("workspace"))
        .and_then(toml::Value::as_bool)
        == Some(true);
    if member {
        if !inherited
            || manifest["lints"]
                .as_table()
                .is_none_or(|table| table.len() != 1)
        {
            return Err(format!(
                "{path}: every member must inherit the entire workspace lint policy"
            )
            .into());
        }
    } else if manifest.get("lints") != Some(lints) {
        return Err(
            format!("{path}: standalone workspace lints differ from root authority").into(),
        );
    }
    Ok(())
}

pub(super) fn check_tool_options(path: &str, source: &str) -> Result<()> {
    for line in source
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
    {
        let weak = line
            .split(|character: char| {
                character.is_whitespace() || ['\'', '"', ',', '[', ']', '='].contains(&character)
            })
            .any(|word| {
                let word = word.trim_matches(['\'', '"', ',', '[', ']']);
                word.starts_with("-A")
                    || word.starts_with("--allow")
                    || word.contains("--cap-lints")
                    || word.contains("--force-warn")
                    || word.contains("RUSTFLAGS=-A")
            });
        let excluded = (line.contains("shellcheck")
            && (line.contains("--exclude") || line.contains(" -e ")))
            || (line.contains("actionlint") && line.contains("-ignore"))
            || (line.contains("swift-format") && line.contains("--ignore"));
        if weak || excluded {
            return Err(format!(
                "{path}: undocumented lint-weakening command/config option: {line}"
            )
            .into());
        }
    }
    Ok(())
}

fn strict_levels(lints: &toml::Value) -> Result<()> {
    let groups = lints
        .as_table()
        .ok_or("Missing authoritative lint policy")?;
    for group in groups.values() {
        let rules = group.as_table().ok_or("Lint groups must be tables")?;
        for (name, value) in rules {
            let level = value
                .as_str()
                .or_else(|| value.get("level").and_then(toml::Value::as_str));
            if !matches!(level, Some("deny" | "forbid")) {
                return Err(format!("Root lint {name} is not strict; every necessary exception belongs in quality.toml at its actual source scope").into());
            }
        }
    }
    Ok(())
}
