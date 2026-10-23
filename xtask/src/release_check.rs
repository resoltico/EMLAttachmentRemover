//! Read-only admission for a later, explicitly authorized owner release.
use crate::distribution::BuildPlan;
use crate::{Result, capture, controller, git};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command, time::Duration};

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
fn response_json(bytes: &[u8]) -> Result<Value> {
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
    if code != "200" {
        return Err("GitHub release lookup did not return HTTP 200; absence is unverified".into());
    }
    Ok(serde_json::from_str(body)?)
}

fn release_id(body: &Value, repository: &str) -> Result<Option<u64>> {
    if body.get("errors").is_some() {
        return Err("GraphQL release lookup reported errors; absence is unverified".into());
    }
    let repo = body
        .pointer("/data/repository")
        .filter(|value| value.is_object())
        .ok_or("GraphQL release lookup omitted the repository")?;
    if !repo["nameWithOwner"]
        .as_str()
        .is_some_and(|name| name.eq_ignore_ascii_case(repository))
    {
        return Err("GraphQL release lookup returned a different repository".into());
    }
    let release = repo
        .get("release")
        .ok_or("GraphQL release lookup omitted the release field")?;
    if release.is_null() {
        return Ok(None);
    }
    release["databaseId"]
        .as_u64()
        .filter(|id| *id > 0)
        .map(Some)
        .ok_or_else(|| "GraphQL release lookup omitted a positive integer release ID".into())
}

fn draft_release(body: &Value, id: u64, version: &str, source: &str) -> Result<Value> {
    if body["id"].as_u64() != Some(id)
        || body["draft"] != true
        || body["tag_name"] != format!("v{version}")
        || body["target_commitish"] != source
        || !body["html_url"].as_str().is_some_and(|url| !url.is_empty())
    {
        return Err(
            "Release ID/source differs, release is published, or draft identity is incomplete"
                .into(),
        );
    }
    Ok(json!({"state":"recoverable_unpublished_draft","id":id,"url":body["html_url"]}))
}

fn github_read(root: &Path, out: &Path, label: &str, args: &[String]) -> Result<Value> {
    let log = out.join(format!("{label}.log"));
    let mut command = Command::new("gh");
    command
        .current_dir(root)
        .args(["api", "--include"])
        .args(args);
    let process = controller()?.run_separate_output(command, &log, Duration::from_secs(30))?;
    fs::write(
        out.join(format!("{label}-process.json")),
        serde_json::to_vec_pretty(&process.evidence())?,
    )?;
    process.require_success(&log)?;
    if fs::metadata(&log)?.len() > 1024 * 1024 {
        return Err("GitHub release response exceeds its bound".into());
    }
    let bytes = fs::read(&log)?;
    if bytes.len() > 1024 * 1024 {
        return Err("GitHub release response grew beyond its bound".into());
    }
    response_json(&bytes)
}

pub(crate) fn run(root: &Path, plan: &BuildPlan, source: &str, version: &str) -> Result<()> {
    // Design: validate immutable source and local release text before remote
    // reads. Absence requires a successful draft-aware lookup with push-capable
    // credentials; an HTTP/CLI error or inaccessible ID cannot prove it. Never mutate
    // tags, releases, assets, remote refs, changelog or repository settings.
    source_id(source)?;
    if git(root, &["rev-parse", "HEAD"])? != source
        || !git(root, &["status", "--porcelain=v1", "--untracked-files=all"])?.is_empty()
    {
        return Err("Release checks require the exact clean source checkout".into());
    }
    if plan.version() != version {
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
    let names = plan.archive_names();
    let parent = root.join("target/release-check");
    fs::create_dir_all(&parent)?;
    let out = tempfile::Builder::new()
        .prefix("observation-")
        .tempdir_in(parent)?
        .keep();
    let (owner, name) = repository
        .split_once('/')
        .ok_or("Missing repository owner/name")?;
    let lookup = github_read(root, &out, "github-release-lookup", &[
        "graphql".into(), "-f".into(),
        "query=query($owner:String!,$name:String!,$tag:String!){repository(owner:$owner,name:$name){nameWithOwner release(tagName:$tag){databaseId}}}".into(),
        "-f".into(), format!("owner={owner}"), "-f".into(), format!("name={name}"), "-f".into(), format!("tag={tag}"),
    ])?;
    let release = match release_id(&lookup, repository)? {
        None => json!({"state":"absent"}),
        Some(id) => {
            let body = github_read(
                root,
                &out,
                "github-release-response",
                &[format!("repos/{repository}/releases/{id}")],
            )?;
            draft_release(&body, id, version, source)?
        }
    };
    let result = json!({"source":source,"main":local_main,"version":version,"date":date,"repository":repository,"release":release,"expected_archives":names,"release_visibility":"requires push-capable credentials to include unpublished drafts","scope":"read-only source/version/changelog/tag/release-state/expected-name admission; no archive, attestation or publication qualification"});
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
    if git(root, &["rev-parse", &format!("{source}:.github/workflows")])?
        != git(
            root,
            &["rev-parse", &format!("{local_main}:.github/workflows")],
        )?
    {
        return Err("Release source workflow tree differs from current main; the release token cannot create that tag".into());
    }
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
    fn only_successful_http_json_can_supply_release_observations() {
        assert_eq!(
            response_json(b"HTTP/2.0 200 OK\r\nX-Test: owned\r\n\r\n{\"id\":7}").unwrap()["id"],
            7
        );
        for bytes in [
            "HTTP/2.0 404 Not Found\n\n{\"message\":\"Not Found\",\"status\":\"404\"}",
            "HTTP/2.0 403 Forbidden\n\n{\"message\":\"Forbidden\"}",
            "HTTP/2.0 500 Server Error\n\n{}",
            "HTTP/2.0 200 OK\n\nnot JSON",
            "HTTP/2.0 200 OK",
            "unknown 200 OK\n\n{}",
        ] {
            assert!(response_json(bytes.as_bytes()).is_err(), "{bytes}");
        }
    }
    #[test]
    fn only_explicit_draft_aware_null_proves_absence() {
        let absent = json!({"data":{"repository":{"nameWithOwner":"owner/repo","release":null}}});
        assert_eq!(release_id(&absent, "owner/repo").unwrap(), None);
        assert_eq!(release_id(&absent, "OWNER/Repo").unwrap(), None);
        let present = json!({"data":{"repository":{"nameWithOwner":"owner/repo","release":{"databaseId":7}}}});
        assert_eq!(release_id(&present, "owner/repo").unwrap(), Some(7));
        for body in [
            json!({}),
            json!({"data":null}),
            json!({"data":{"repository":null}}),
            json!({"data":{"repository":{"nameWithOwner":"owner/repo"}}}),
            json!({"data":{"repository":{"release":null}}}),
            json!({"data":{"repository":{"nameWithOwner":"other/repo","release":null}}}),
            json!({"data":absent["data"],"errors":[{"message":"inaccessible"}]}),
            json!({"data":absent["data"],"errors":[]}),
        ] {
            assert!(release_id(&body, "owner/repo").is_err(), "{body}");
        }
        for id in [json!(null), json!(0), json!(-1), json!(1.5), json!("7")] {
            let mut body = present.clone();
            body["data"]["repository"]["release"]["databaseId"] = id;
            assert!(release_id(&body, "owner/repo").is_err());
        }
        for release in [json!({}), json!(false), json!([])] {
            let mut body = present.clone();
            body["data"]["repository"]["release"] = release;
            assert!(release_id(&body, "owner/repo").is_err());
        }
    }
    #[test]
    fn resolved_draft_id_source_and_publication_state_must_agree() {
        let draft = json!({"id":7,"draft":true,"tag_name":"v6.0.0","target_commitish":"source","html_url":"https://github.com/owner/repo/releases/tag/untagged"});
        assert_eq!(
            draft_release(&draft, 7, "6.0.0", "source").unwrap()["state"],
            "recoverable_unpublished_draft"
        );
        for (field, value) in [
            ("id", json!(8)),
            ("id", json!(null)),
            ("draft", json!(false)),
            ("tag_name", json!("v5.0.0")),
            ("target_commitish", json!("other")),
            ("html_url", json!(null)),
            ("html_url", json!("")),
        ] {
            let mut body = draft.clone();
            body[field] = value;
            assert!(
                draft_release(&body, 7, "6.0.0", "source").is_err(),
                "{field}"
            );
        }
    }
}
