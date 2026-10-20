//! Read-only admission for a later, explicitly authorized owner release.
use crate::{Result, capture, controller, git};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path, process::Command, time::Duration};

fn source_id(source: &str) -> Result<()> {
    if source.len() != 40
        || !source
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("Release source must be an exact full lowercase commit SHA".into());
    }
    Ok(())
}
fn release_section<'a>(changelog: &'a str, version: &str) -> Result<&'a str> {
    if !changelog.lines().any(|line| line == "## [Unreleased]") {
        return Err("Changelog must retain Unreleased".into());
    }
    let prefix = format!("## [{version}]");
    let matching: Vec<_> = changelog
        .lines()
        .filter(|line| line.starts_with(&prefix))
        .collect();
    if matching.len() != 1 {
        return Err(
            "Release needs exactly one dated version section; Unreleased is not a release".into(),
        );
    }
    let header = matching[0];
    let date = header
        .strip_prefix(&format!("{prefix} - "))
        .ok_or("Release section has no exact dated header")?;
    let pieces: Vec<_> = date.split('-').collect();
    if pieces.len() != 3
        || pieces[0].len() != 4
        || pieces[1].len() != 2
        || pieces[2].len() != 2
        || !pieces
            .iter()
            .all(|part| part.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("Release date must be YYYY-MM-DD".into());
    }
    let year: u32 = pieces[0].parse()?;
    let month: usize = pieces[1].parse()?;
    let day: u32 = pieces[2].parse()?;
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if year == 0 || !(1..=12).contains(&month) || day == 0 || day > days[month - 1] {
        return Err("Release date is not a calendar date".into());
    }
    let start = changelog.find(header).ok_or("Missing release section")? + header.len();
    let rest = &changelog[start..];
    let body = &rest[..rest.find("\n## ").unwrap_or(rest.len())];
    if !body.lines().any(|line| {
        line.strip_prefix("- ")
            .is_some_and(|text| !text.trim().is_empty())
    }) {
        return Err("Release section has no recorded release outcomes".into());
    }
    Ok(date)
}
fn release_response(bytes: &[u8], exit: Option<i32>, version: &str, source: &str) -> Result<Value> {
    let text = std::str::from_utf8(bytes)?.replace("\r\n", "\n");
    let (headers, body) = text
        .split_once("\n\n")
        .ok_or("Missing GitHub response headers/body")?;
    let first = headers
        .lines()
        .next()
        .ok_or("Missing GitHub response status")?;
    let mut status = first.split_ascii_whitespace();
    if !matches!(status.next(), Some("HTTP/2.0" | "HTTP/1.1" | "HTTP/2")) {
        return Err("Unknown GitHub HTTP response version".into());
    }
    let code = status.next().ok_or("Missing GitHub response code")?;
    let body: Value = serde_json::from_str(body)?;
    match (code, exit) {
        ("404", Some(1)) if body["message"] == "Not Found" && body["status"] == "404" => {
            Ok(json!({"state":"absent"}))
        }
        ("200", Some(0))
            if body["draft"] == true
                && body["tag_name"] == format!("v{version}")
                && body["target_commitish"] == source =>
        {
            Ok(
                json!({"state":"recoverable_unpublished_draft","id":body["id"],"url":body["html_url"]}),
            )
        }
        _ => Err(
            "Release already published, draft source differs, or GitHub absence is unverified"
                .into(),
        ),
    }
}

pub(crate) fn run(root: &Path, source: &str, version: &str) -> Result<()> {
    // Design: validate immutable source and local release text before remote
    // reads. Challenge: a CLI error is not proof of absence; accept only an
    // observed404 body/status, or a matching unpublished draft. Never mutate
    // tags, releases, assets, remote refs, changelog or repository settings.
    source_id(source)?;
    if git(root, &["rev-parse", "HEAD"])? != source
        || !git(root, &["status", "--porcelain=v1", "--untracked-files=all"])?.is_empty()
    {
        return Err("Release checks require the exact clean source checkout".into());
    }
    let plan: Value = serde_json::from_slice(&capture(
        Command::new(std::env::current_exe()?)
            .current_dir(root)
            .arg("distribution"),
    )?)?;
    if plan["version"] != version {
        return Err("Release version differs from the authoritative Cargo workspace".into());
    }
    let changelog = fs::read_to_string(root.join("CHANGELOG.md"))?;
    let date = release_section(&changelog, version)?;
    let remote = git(root, &["remote", "get-url", "origin"])?;
    let repository = repository_identity(&remote)?;
    let local_main = verify_main_ancestry(root, source)?;
    let tag = format!("v{version}");
    if !git(root, &["tag", "--list", &tag])?.is_empty() {
        return Err(
            "Release tag already exists locally; it must not be replaced or retargeted".into(),
        );
    }
    if !git(
        root,
        &[
            "ls-remote",
            "--tags",
            "origin",
            &format!("refs/tags/{tag}"),
            &format!("refs/tags/{tag}^{{}}"),
        ],
    )?
    .is_empty()
    {
        return Err("Release tag already exists; it must not be replaced or retargeted".into());
    }
    let expected = plan["expected_artifacts"]
        .as_array()
        .ok_or("Missing artifact policy")?;
    let names = expected
        .iter()
        .map(|item| {
            item["name"]
                .as_str()
                .ok_or("Missing expected artifact name")
        })
        .collect::<std::result::Result<BTreeSet<_>, _>>()?;
    if expected.len() != 8 || names.len() != 8 {
        return Err("Release requires exactly eight distinct product archives".into());
    }
    let parent = root.join("target/release-check");
    fs::create_dir_all(&parent)?;
    let out = tempfile::Builder::new()
        .prefix("observation-")
        .tempdir_in(parent)?
        .keep();
    let log = out.join("github-release-response.log");
    let mut command = Command::new("gh");
    command.current_dir(root).args([
        "api",
        "--include",
        &format!("repos/{repository}/releases/tags/{tag}"),
    ]);
    let process = controller()?.run_separate_output(command, &log, Duration::from_secs(30))?;
    fs::write(
        out.join("process.json"),
        serde_json::to_vec_pretty(&process.evidence())?,
    )?;
    if process.cancelled
        || process.timed_out
        || !process.cleanup_complete
        || process.unexpected_survivors != 0
    {
        return Err("GitHub release read did not settle cleanly".into());
    }
    if fs::metadata(&log)?.len() > 1024 * 1024 {
        return Err("GitHub release response exceeds its bound".into());
    }
    let release = release_response(&fs::read(&log)?, process.status.code(), version, source)?;
    let result = json!({"source":source,"main":local_main,"version":version,"date":date,"repository":repository,"release":release,"expected_archives":names,"scope":"read-only source/version/changelog/tag/release-state/expected-name admission; no archive, attestation or publication qualification"});
    fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

fn repository_identity(remote: &str) -> Result<&str> {
    let repository = remote
        .strip_prefix("https://github.com/")
        .or_else(|| remote.strip_prefix("git@github.com:"))
        .ok_or("Release origin is not a supported GitHub repository URL")?
        .trim_end_matches(".git");
    let parts: Vec<_> = repository.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
    {
        return Err("Ambiguous GitHub repository identity".into());
    }
    Ok(repository)
}

fn verify_main_ancestry(root: &Path, source: &str) -> Result<String> {
    let current_main = git(
        root,
        &["ls-remote", "--exit-code", "origin", "refs/heads/main"],
    )?;
    let local_main = git(
        root,
        &["rev-parse", "--verify", "refs/remotes/origin/main^{commit}"],
    )?;
    if current_main.split_ascii_whitespace().collect::<Vec<_>>()
        != [local_main.as_str(), "refs/heads/main"]
    {
        return Err("Local main is stale; fetch main before checking release ancestry".into());
    }
    capture(Command::new("git").current_dir(root).args([
        "merge-base",
        "--is-ancestor",
        source,
        &local_main,
    ]))?;
    Ok(local_main)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unreleased_empty_duplicate_and_invalid_calendar_dates_cannot_admit_a_release() {
        assert!(release_section("## [Unreleased]\n- Work in progress\n", "6.0.0").is_err());
        for tail in [
            "## [6.0.0] - 2026-02-30\n- Outcome",
            "## [6.0.0]\n- Outcome",
            "## [6.0.0] - 2026-10-18\n",
            "## [6.0.0] - 2026-10-18\n- Outcome\n## [6.0.0] - 2026-10-19\n- Duplicate",
        ] {
            assert!(release_section(&format!("## [Unreleased]\n{tail}\n"), "6.0.0").is_err());
        }
        assert_eq!(release_section("## [Unreleased]\n\n## [6.0.0] - 2026-10-18\n- Outcome\n## [5.0.0] - 2026-10-15\n- History","6.0.0").unwrap(),"2026-10-18");
    }
    #[test]
    fn remote_errors_and_published_releases_never_count_as_absence() {
        let absent=b"HTTP/2.0 404 Not Found\nX-Test: owned\r\n\r\n{\"message\":\"Not Found\",\"status\":\"404\"}";
        assert_eq!(
            release_response(absent, Some(1), "6.0.0", "source").unwrap()["state"],
            "absent"
        );
        assert!(release_response(absent, Some(0), "6.0.0", "source").is_err());
        for response in [
            "HTTP/2.0 403 Forbidden\n\n{\"message\":\"Forbidden\"}",
            "HTTP/2.0 200 OK\n\n{\"draft\":false,\"tag_name\":\"v6.0.0\",\"target_commitish\":\"source\"}",
            "HTTP/2.0 200 OK\n\n{\"draft\":true,\"tag_name\":\"v6.0.0\",\"target_commitish\":\"wrong-source\"}",
        ] {
            assert!(release_response(response.as_bytes(), Some(0), "6.0.0", "source").is_err());
        }
    }
}
