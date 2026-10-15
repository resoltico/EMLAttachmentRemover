//! Complete, positive-discovery mutation campaigns for exactly two real guards.
use crate::{
    Result,
    campaign::{Campaign, require_unix_campaign, run_logged, tool_version},
    capture, git,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::Path,
    process::Command,
    time::Duration,
};
const GUARDS: &[&str] = &["authorize_deletions", "check_edit_ranges"];
const SELECTION: &str = "(authorize_deletions|check_edit_ranges)";
fn command(campaign: &Campaign, selection: &str) -> Result<Command> {
    let mut command = campaign.command("cargo")?;
    command.args([
        "mutants",
        "--no-config",
        "--package",
        "eml-core",
        "--re",
        selection,
    ]);
    Ok(command)
}
fn discovery(campaign: &Campaign, selection: &str) -> Result<Vec<Value>> {
    let bytes = capture(command(campaign, selection)?.args(["--list", "--json"]))?;
    let mutants: Vec<Value> = serde_json::from_slice(&bytes)?;
    if mutants.len() > 400 {
        return Err(
            "Scoped selection exceeds 400 mutants; review its design rather than truncating it"
                .into(),
        );
    }
    Ok(mutants)
}
fn function(mutant: &Value) -> Result<&str> {
    mutant
        .pointer("/function/function_name")
        .and_then(Value::as_str)
        .ok_or_else(|| "Mutation discovery lacks a function name".into())
}
fn positive_guards(mutants: &[Value]) -> Result<BTreeMap<String, usize>> {
    let mut counts = GUARDS
        .iter()
        .map(|name| (name.to_string(), 0))
        .collect::<BTreeMap<_, _>>();
    for mutant in mutants {
        let function = function(mutant)?;
        *counts
            .get_mut(function)
            .ok_or("Mutation selection escaped the two required guards")? += 1;
    }
    if counts.values().any(|count| *count == 0) {
        return Err("Both named guards require positive mutant discovery".into());
    }
    Ok(counts)
}
// A changed guard is selected as a whole function; no partial/first-N selection.
fn changed_guards(diff: &str, mutants: &[Value]) -> Result<BTreeSet<String>> {
    let mut changed = BTreeSet::new();
    let mut file = String::new();
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            file = path.to_owned();
        }
        if line.starts_with("@@ ") {
            let added = line
                .split_whitespace()
                .nth(2)
                .and_then(|field| field.strip_prefix('+'))
                .ok_or("Invalid source diff hunk")?;
            let mut numbers = added.split(',');
            let start: usize = numbers.next().ok_or("Missing diff line")?.parse()?;
            let length: usize = numbers.next().unwrap_or("1").parse()?;
            let end = start
                .checked_add(length.max(1) - 1)
                .ok_or("Diff line overflow")?;
            for mutant in mutants {
                if mutant.get("file").and_then(Value::as_str) != Some(file.as_str()) {
                    continue;
                }
                let first = mutant
                    .pointer("/function/span/start/line")
                    .and_then(Value::as_u64)
                    .ok_or("Missing guard source span")? as usize;
                let last = mutant
                    .pointer("/function/span/end/line")
                    .and_then(Value::as_u64)
                    .ok_or("Missing guard source span")? as usize;
                if start <= last && end >= first {
                    changed.insert(function(mutant)?.to_owned());
                }
            }
        }
    }
    Ok(changed)
}
#[derive(Deserialize, Serialize)]
struct Outcome {
    total_mutants: usize,
    missed: usize,
    caught: usize,
    timeout: usize,
    unviable: usize,
    success: usize,
    end_time: Option<Value>,
    cargo_mutants_version: String,
    outcomes: Vec<Value>,
}
fn validate(outcome: &Outcome, selected: &[Value], version: &str) -> Result<()> {
    if outcome.cargo_mutants_version != version || outcome.end_time.is_none() {
        return Err("Missing/stale/incomplete mutation summary".into());
    }
    let total = [
        outcome.caught,
        outcome.missed,
        outcome.timeout,
        outcome.unviable,
        outcome.success,
    ]
    .iter()
    .try_fold(0usize, |sum, count| sum.checked_add(*count))
    .ok_or("Mutation counter overflow")?;
    if outcome.total_mutants != selected.len() || total != selected.len() {
        return Err(
            "Mutation campaign did not completely account for its admitted selection".into(),
        );
    }
    let expected = selected
        .iter()
        .map(|mutant| mutant["name"].as_str().ok_or("Mutant name missing"))
        .collect::<std::result::Result<BTreeSet<_>, _>>()?;
    let mut observed = BTreeSet::new();
    let mut baseline = 0;
    let mut classified = [0usize; 5];
    for result in &outcome.outcomes {
        if result["scenario"] == "Baseline" {
            if result["summary"] != "Success" {
                return Err("Mutation baseline failed".into());
            }
            baseline += 1;
        } else {
            let name = result
                .pointer("/scenario/Mutant/name")
                .and_then(Value::as_str)
                .ok_or("Unknown mutation outcome scenario")?;
            if !expected.contains(name) || !observed.insert(name) {
                return Err("Mutation outcome is duplicate or outside admitted selection".into());
            }
            let index = match result["summary"].as_str() {
                Some("CaughtMutant") => 0,
                Some("MissedMutant") => 1,
                Some("Timeout") => 2,
                Some("Unviable") => 3,
                Some("Success") => 4,
                _ => return Err("Unclassified mutation outcome".into()),
            };
            classified[index] += 1;
        }
    }
    if baseline != 1 || observed != expected {
        return Err("Missing baseline or executed mutation outcomes".into());
    }
    if classified
        != [
            outcome.caught,
            outcome.missed,
            outcome.timeout,
            outcome.unviable,
            outcome.success,
        ]
    {
        return Err("Mutation summary disagrees with individual executed outcomes".into());
    }
    if outcome.missed > 0 || outcome.timeout > 0 || outcome.success > 0 {
        return Err("Missed, timed-out or unexplained mutations fail qualification".into());
    }
    if outcome.unviable > 0 {
        return Err("Unviable mutations are not caught; inspect the retained compiler evidence and review them before qualification".into());
    }
    Ok(())
}
pub(crate) fn run(root: &Path, initial: bool, base: Option<&str>) -> Result<()> {
    require_unix_campaign()?;
    let version = tool_version(root, "mutants", "cargo_mutants")?;
    let resolved_base = if initial {
        None
    } else {
        let base = base.ok_or("Use --initial or --diff BASE")?;
        Some(git(
            root,
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{base}^{{commit}}"),
            ],
        )?)
    };
    let campaign = Campaign::create(root, "mutations")?;
    let all = discovery(&campaign, SELECTION)?;
    let guard_counts = positive_guards(&all)?;
    let mut merge_base = None;
    let selection = if initial {
        SELECTION.to_owned()
    } else {
        let resolved = resolved_base
            .as_deref()
            .ok_or("Diff base was not admitted")?;
        capture(
            Command::new("git")
                .current_dir(&campaign.source)
                .args(["fetch", "--no-tags", "--no-recurse-submodules"])
                .arg(root)
                .arg(resolved),
        )?;
        let common = git(&campaign.source, &["merge-base", "HEAD", resolved])?;
        let diff = git(
            &campaign.source,
            &[
                "diff",
                "--no-ext-diff",
                "--no-renames",
                "--unified=0",
                &common,
                "HEAD",
                "--",
                "crates/eml-core/src",
            ],
        )?;
        fs::write(campaign.directory.join("source.diff"), &diff)?;
        merge_base = Some(common);
        let guards = changed_guards(&diff, &all)?;
        if guards.is_empty() {
            campaign.report(&serde_json::json!({"source":campaign.sha,"merge_base":merge_base,"status":"not_applicable","reason":"Neither selected guard changed; other code is not claimed mutation-covered","guard_discovery":guard_counts}))?;
            return Ok(());
        }
        format!("({})", guards.into_iter().collect::<Vec<_>>().join("|"))
    };
    let selected = discovery(&campaign, &selection)?;
    if selected.is_empty() {
        return Err("Changed guards require positive mutant discovery".into());
    }
    fs::write(
        campaign.directory.join("discovery.json"),
        serde_json::to_vec_pretty(&selected)?,
    )?;
    writeln!(
        std::io::stderr(),
        "Running complete scoped selection: {} mutants at {}; evidence {}",
        selected.len(),
        campaign.sha,
        campaign.directory.display()
    )?;
    let mut child = campaign.command("cargo")?;
    child
        .args([
            "mutants",
            "--no-config",
            "--package",
            "eml-core",
            "--re",
            &selection,
            "--cap-lints=true",
            "--profile",
            "mutants",
            "--baseline",
            "run",
            "--jobs",
            "1",
            "--copy-target=false",
            "--test-workspace=false",
            "--cargo-arg=--locked",
            "--timeout",
            "15",
            "--build-timeout",
            "120",
            "--no-times",
            "--colors",
            "never",
            "--output",
        ])
        .arg(campaign.directory.join("output"));
    let process = run_logged(
        child,
        &campaign.directory.join("campaign.log"),
        Duration::from_secs(45 * 60),
    )?;
    let outcome: Outcome = serde_json::from_slice(&fs::read(
        campaign.directory.join("output/mutants.out/outcomes.json"),
    )?)?;
    let validation = validate(&outcome, &selected, &version);
    campaign.report(&serde_json::json!({"source":campaign.sha,"merge_base":merge_base,"tool_version":version,"discovered":selected.len(),"guard_discovery":guard_counts,
        "executed":outcome.total_mutants,"caught":outcome.caught,"missed":outcome.missed,"unviable":outcome.unviable,"timed_out":outcome.timeout,
        "command_exit":process.status.code(),"campaign_deadline_exceeded":process.timed_out,"elapsed_ms":process.elapsed.as_millis(),"qualified":validation.is_ok()&&process.success()}))?;
    if !process.success() {
        return Err(format!(
            "Mutation process failed; retain {}",
            campaign.directory.display()
        )
        .into());
    }
    validation
}
#[cfg(test)]
mod tests {
    use super::*;
    fn selected() -> Vec<Value> {
        vec![serde_json::json!({"name":"guard mutation"})]
    }
    fn outcome() -> Outcome {
        Outcome {
            total_mutants: 1,
            missed: 0,
            caught: 1,
            timeout: 0,
            unviable: 0,
            success: 0,
            end_time: Some(Value::String("finished".into())),
            cargo_mutants_version: "pin".into(),
            outcomes: vec![
                serde_json::json!({"scenario":"Baseline","summary":"Success"}),
                serde_json::json!({"scenario":{"Mutant":{"name":"guard mutation"}},"summary":"CaughtMutant"}),
            ],
        }
    }
    #[test]
    fn incomplete_missed_and_timeout_are_not_caught() {
        assert!(validate(&outcome(), &selected(), "pin").is_ok());
        let mut incomplete = outcome();
        incomplete.total_mutants = 0;
        assert!(validate(&incomplete, &selected(), "pin").is_err());
        let mut missed = outcome();
        missed.caught = 0;
        missed.missed = 1;
        assert!(validate(&missed, &selected(), "pin").is_err());
        let mut timeout = outcome();
        timeout.caught = 0;
        timeout.timeout = 1;
        assert!(validate(&timeout, &selected(), "pin").is_err());
        let mut baseline = outcome();
        baseline.outcomes[0]["summary"] = "Failure".into();
        assert!(validate(&baseline, &selected(), "pin").is_err());
        let mut duplicate = outcome();
        duplicate.outcomes.push(duplicate.outcomes[1].clone());
        assert!(validate(&duplicate, &selected(), "pin").is_err());
        let mut inconsistent = outcome();
        inconsistent.outcomes[1]["summary"] = "Timeout".into();
        assert!(validate(&inconsistent, &selected(), "pin").is_err());
    }
    #[test]
    fn empty_guard_discovery_is_a_failure() {
        assert!(positive_guards(&[]).is_err());
    }
}
