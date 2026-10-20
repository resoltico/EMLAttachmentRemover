//! Authored-source policy and centrally justified exceptions.
mod discovery;
mod policy;
mod references;
mod rust;
#[cfg(target_os = "macos")]
mod swift;
#[cfg(test)]
mod tests;

use crate::Result;
use serde::Deserialize;
use serde_json::json;
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    schema: u32,
    max_source_lines: usize,
    vendor: Vec<String>,
    #[serde(default)]
    exceptions: Vec<Exception>,
    #[serde(default)]
    command_exceptions: Vec<CommandException>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Exception {
    id: String,
    file: String,
    scope: String,
    lints: Vec<String>,
    rationale: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandException {
    id: String,
    file: String,
    scope: String,
    argument: String,
    rationale: String,
}
#[derive(Debug, PartialEq, Eq)]
struct Suppression {
    id: String,
    scope: String,
    lints: Vec<String>,
}

pub(crate) fn run(root: &Path) -> Result<serde_json::Value> {
    let policy: Policy = toml::from_str(&fs::read_to_string(root.join("quality.toml"))?)?;
    policy::validate(root, &policy)?;
    let paths: std::collections::BTreeSet<_> = discovery::sources(root)?
        .into_iter()
        .filter(|path| {
            !policy
                .vendor
                .iter()
                .any(|prefix| path.starts_with(&format!("{prefix}/")))
        })
        .collect();
    let mut matched = BTreeMap::<String, usize>::new();
    let mut counts = BTreeMap::<String, usize>::new();
    let mut errors = Vec::new();
    for path in &paths {
        let source = fs::read_to_string(root.join(path))?;
        let extension = Path::new(path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        *counts.entry(extension.into()).or_default() += 1;
        if source.lines().count() > policy.max_source_lines {
            errors.push(format!("{path}: {} physical lines exceed {}; split responsibilities, not arbitrary text chunks", source.lines().count(), policy.max_source_lines));
        }
        let result = match extension {
            "rs" => rust::check(root, path, &source, &paths, &policy, &mut matched),
            "swift" | "sh" => policy::check_directives(path, &source),
            _ => policy::check_directives(path, &source),
        };
        if let Err(error) = result {
            errors.push(error.to_string());
        }
    }
    errors.extend(unbound_exceptions(&policy, &matched));
    if !errors.is_empty() {
        return Err(errors.join("\n").into());
    }
    Ok(
        json!({"passed":true,"sources":counts,"exceptions":matched,"physical_source_limit":policy.max_source_lines,"scope":"authored source structure, compiler/formatter suppressions, shared lint configuration and owned vendor boundaries; native Swift syntax checks run separately"}),
    )
}

fn unbound_exceptions(policy: &Policy, matched: &BTreeMap<String, usize>) -> Vec<String> {
    policy
        .exceptions
        .iter()
        .map(|entry| &entry.id)
        .chain(policy.command_exceptions.iter().map(|entry| &entry.id))
        .filter(|id| matched.get(*id) != Some(&1))
        .map(|id| {
            format!(
                "Quality exception {id} must match exactly one current source scope; observed {}",
                matched.get(id).unwrap_or(&0)
            )
        })
        .collect()
}

fn bind(
    policy: &Policy,
    file: &str,
    observed: Suppression,
    matched: &mut BTreeMap<String, usize>,
) -> Result<()> {
    let entry = policy
        .exceptions
        .iter()
        .find(|entry| entry.id == observed.id)
        .ok_or_else(|| format!("{file}: unregistered lint exception {}", observed.id))?;
    let mut expected = entry.lints.clone();
    expected.sort();
    if entry.file != file || entry.scope != observed.scope || expected != observed.lints {
        return Err(format!(
            "{file}: exception {} scope/lints differ from quality.toml: observed {} {:?}",
            entry.id, observed.scope, observed.lints
        )
        .into());
    }
    *matched.entry(entry.id.clone()).or_default() += 1;
    Ok(())
}

#[cfg(target_os = "macos")]
pub(crate) fn check_swift_structure(
    root: &Path,
    out: &Path,
    build: &crate::macos::MacBuild,
    paths: &std::collections::BTreeSet<&str>,
) -> Result<serde_json::Value> {
    swift::check(root, out, build, paths)
}

pub(crate) fn shell_sources(root: &Path) -> Result<Vec<String>> {
    let policy: Policy = toml::from_str(&fs::read_to_string(root.join("quality.toml"))?)?;
    Ok(discovery::shells(root)?
        .into_iter()
        .filter(|path| {
            !policy
                .vendor
                .iter()
                .any(|prefix| path.starts_with(&format!("{prefix}/")))
        })
        .collect())
}
