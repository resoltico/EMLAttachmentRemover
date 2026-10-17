//! CI identity and required-job decisions, separate from artifact execution.
use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::Path,
    process::Command,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) enum Role {
    H,
    M,
    S,
    Q,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Context {
    schema: u32,
    event: Event,
    role: Role,
    source: String,
    event_sha: String,
    workflow_sha: String,
    run_id: u64,
    attempt: u32,
    head: Option<String>,
    base: Option<String>,
    merge: Option<String>,
    pull_request: Option<u64>,
    queue_ref: Option<String>,
}
fn sha(value: &Value) -> Result<String> {
    let value = value.as_str().ok_or("Missing CI commit identity")?;
    if value.len() != 40
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err("Invalid CI commit identity".into());
    }
    Ok(value.to_owned())
}
fn context(
    event: Event,
    payload: &Value,
    event_sha: &str,
    workflow_sha: &str,
    live: &Value,
    run_id: u64,
    attempt: u32,
) -> Result<Context> {
    if run_id == 0 || attempt == 0 {
        return Err("CI run/attempt identity missing".into());
    }
    let event_sha = sha(&json!(event_sha))?;
    let workflow_sha = sha(&json!(workflow_sha))?;
    let mut context = Context {
        schema: 1,
        event,
        role: Role::S,
        source: event_sha.clone(),
        event_sha,
        workflow_sha,
        run_id,
        attempt,
        head: None,
        base: None,
        merge: None,
        pull_request: None,
        queue_ref: None,
    };
    match event {
        Event::PullRequest => {
            if payload["pull_request"]["base"]["ref"] != "main" {
                return Err("PR qualification requires main as its base".into());
            }
            let head = sha(&payload["pull_request"]["head"]["sha"])?;
            let base = sha(&payload["pull_request"]["base"]["sha"])?;
            context.role = Role::H;
            context.source = head.clone();
            context.head = Some(head);
            context.base = Some(base);
            context.merge = Some(context.event_sha.clone());
            context.pull_request = Some(
                payload["number"]
                    .as_u64()
                    .filter(|number| *number > 0)
                    .ok_or("PR number missing")?,
            );
        }
        Event::Push => {
            if payload["ref"] != "refs/heads/main" || sha(&payload["after"])? != context.source {
                return Err("Main push source conflicts with workflow identity".into());
            }
        }
        Event::MergeGroup => {
            if payload["merge_group"]["base_ref"] != "refs/heads/main"
                || sha(&payload["merge_group"]["head_sha"])? != context.source
            {
                return Err("Queue source conflicts with workflow identity".into());
            }
            context.role = Role::Q;
            context.base = Some(sha(&payload["merge_group"]["base_sha"])?);
            let queue_ref = payload["merge_group"]["head_ref"]
                .as_str()
                .filter(|name| {
                    name.starts_with("refs/heads/")
                        && name.len() > 11
                        && name.len() <= 1024
                        && !name.chars().any(char::is_control)
                })
                .ok_or("Queue ref identity missing")?;
            context.queue_ref = Some(queue_ref.to_owned());
        }
    }
    freshness(&context, live)?;
    Ok(context)
}
fn freshness(context: &Context, live: &Value) -> Result<()> {
    if context.schema != 1 || context.run_id == 0 || context.attempt == 0 || !live.is_object() {
        return Err("Unknown CI identity schema".into());
    }
    sha(&json!(context.workflow_sha))?;
    if context.event == Event::PullRequest {
        if context.role != Role::H
            || live["base_ref"] != "main"
            || !context.pull_request.is_some_and(|number| number > 0)
            || context.queue_ref.is_some()
            || context.head.as_deref() != Some(context.source.as_str())
            || context.merge.as_deref() != Some(context.event_sha.as_str())
            || live["number"].as_u64() != context.pull_request
            || Some(sha(&live["head"])?).as_ref() != context.head.as_ref()
            || Some(sha(&live["base"])?).as_ref() != context.base.as_ref()
            || Some(sha(&live["base_tip"])?).as_ref() != context.base.as_ref()
            || Some(sha(&live["merge_ref"])?).as_ref() != context.merge.as_ref()
        {
            return Err("Stale/conflicting PR head, base tip or synthetic merge identity".into());
        }
    } else {
        let role = if context.event == Event::Push {
            Role::S
        } else {
            Role::Q
        };
        if context.role != role
            || context.source != context.event_sha
            || context.head.is_some()
            || context.merge.is_some()
            || context.pull_request.is_some()
        {
            return Err("Stale/conflicting main or queue source identity".into());
        }
        if context.event == Event::MergeGroup {
            sha(&json!(context.base.as_deref().ok_or("Queue base missing")?))?;
            if live["queue_ref"].as_str() != context.queue_ref.as_deref()
                || context.queue_ref.is_none()
                || sha(&live["source_ref"])? != context.source
            {
                return Err("Stale/conflicting live queue ref identity".into());
            }
        } else if context.queue_ref.is_some() || context.base.is_some() {
            return Err("Unexpected pushed-source PR/queue metadata".into());
        }
    }
    Ok(())
}
fn git(repository: &Path, args: &[&str]) -> Result<String> {
    Ok(String::from_utf8(crate::capture(
        Command::new("git")
            .args(["--no-pager", "--no-replace-objects"])
            .current_dir(repository)
            .args(args),
    )?)?
    .trim_end()
    .to_owned())
}
fn merge_parents(raw: &str, base: &str, head: &str) -> Result<()> {
    let parents: Vec<_> = raw
        .lines()
        .take_while(|line| !line.is_empty())
        .filter_map(|line| line.strip_prefix("parent "))
        .collect();
    if parents != [base, head] {
        return Err("Synthetic M does not have the exact ordered base/H parents".into());
    }
    Ok(())
}
fn checkout(repository: &Path, context: &Context, role: Role) -> Result<()> {
    let expected = if role == Role::M {
        if context.role != Role::H {
            return Err("M is required only for PR integration".into());
        }
        context.merge.as_deref().ok_or("Missing synthetic M")?
    } else {
        if role != context.role {
            return Err("Checkout role conflicts with planned source role".into());
        }
        &context.source
    };
    if git(repository, &["rev-parse", "HEAD"])? != expected {
        return Err("Actual checkout differs from declared CI source/role".into());
    }
    if role == Role::M {
        merge_parents(
            &git(repository, &["cat-file", "-p", expected])?,
            context.base.as_deref().ok_or("Missing PR base")?,
            context.head.as_deref().ok_or("Missing PR H")?,
        )?;
    }
    Ok(())
}
#[derive(clap::Args)]
pub(crate) struct ContextArguments {
    #[arg(long)]
    event_name: String,
    #[arg(long)]
    event: std::path::PathBuf,
    #[arg(long)]
    event_sha: String,
    #[arg(long)]
    workflow_sha: String,
    #[arg(long)]
    live: std::path::PathBuf,
    #[arg(long)]
    merge_repository: Option<std::path::PathBuf>,
    #[arg(long)]
    run_id: u64,
    #[arg(long)]
    attempt: u32,
    #[arg(long)]
    out: std::path::PathBuf,
}
pub(crate) fn establish(root: &Path, arguments: ContextArguments) -> Result<()> {
    let event = Event::parse(&arguments.event_name)?;
    let payload: Value = serde_json::from_slice(&document(&arguments.event, 16 * 1024 * 1024)?)?;
    let live: Value = serde_json::from_slice(&document(&arguments.live, 65_536)?)?;
    let context = context(
        event,
        &payload,
        &arguments.event_sha,
        &arguments.workflow_sha,
        &live,
        arguments.run_id,
        arguments.attempt,
    )?;
    checkout(root, &context, context.role)?;
    if context.event == Event::PullRequest {
        checkout(
            arguments
                .merge_repository
                .as_deref()
                .ok_or("PR context requires the actual M checkout")?,
            &context,
            Role::M,
        )?;
    } else if arguments.merge_repository.is_some() {
        return Err("Unexpected non-PR merge checkout".into());
    }
    fs::File::options()
        .write(true)
        .create_new(true)
        .open(&arguments.out)?
        .write_all(&serde_json::to_vec_pretty(&context)?)?;
    Ok(())
}
pub(crate) fn recheck(
    root: &Path,
    context_file: &Path,
    live_file: &Path,
    role: &str,
    run_id: u64,
    attempt: u32,
    workflow_sha: &str,
) -> Result<Context> {
    let bytes = document(context_file, 65_536)?;
    let context = context_document(&bytes, run_id, attempt, workflow_sha)?;
    let live: Value = serde_json::from_slice(&document(live_file, 65_536)?)?;
    freshness(&context, &live)?;
    let role = match role {
        "H" => Role::H,
        "M" => Role::M,
        "S" => Role::S,
        "Q" => Role::Q,
        _ => return Err("Unknown CI checkout role".into()),
    };
    checkout(root, &context, role)?;
    Ok(context)
}
fn context_document(
    bytes: &[u8],
    run_id: u64,
    attempt: u32,
    workflow_sha: &str,
) -> Result<Context> {
    if bytes.iter().find(|byte| !byte.is_ascii_whitespace()) != Some(&b'{') {
        return Err("CI context must be a JSON object".into());
    }
    let context: Context = serde_json::from_slice(bytes)?;
    if context.run_id != run_id
        || context.attempt != attempt
        || context.workflow_sha != sha(&json!(workflow_sha))?
    {
        return Err("CI run/attempt/workflow-definition identity differs".into());
    }
    Ok(context)
}

#[derive(clap::Args)]
pub(crate) struct IntegrationArguments {
    #[arg(long)]
    context: std::path::PathBuf,
    #[arg(long)]
    live: std::path::PathBuf,
    #[arg(long)]
    run_id: u64,
    #[arg(long)]
    attempt: u32,
    #[arg(long)]
    workflow_sha: String,
    #[arg(long)]
    out: std::path::PathBuf,
}
pub(crate) fn integration(root: &Path, arguments: IntegrationArguments) -> Result<()> {
    if std::env::consts::OS != "linux" || std::env::consts::ARCH != "x86_64" {
        return Err("M integration requires the declared native Linux x64 host".into());
    }
    let context = recheck(
        root,
        &arguments.context,
        &arguments.live,
        "M",
        arguments.run_id,
        arguments.attempt,
        &arguments.workflow_sha,
    )?;
    fs::create_dir(&arguments.out)?;
    let tests = crate::qualify::checked_tests(root, None, false, &arguments.out)?;
    fs::write(
        arguments.out.join("result.json"),
        serde_json::to_vec_pretty(
            &json!({"role":"M","source":context.merge,"head":context.head,"base":context.base,"event_sha":context.event_sha,"workflow_sha":context.workflow_sha,"run_id":context.run_id,"attempt":context.attempt,"tests":tests,"scope":"actual M Linux workspace/core tests only; not six-platform M packaging or final live-ref observation"}),
        )?,
    )?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Event {
    PullRequest,
    Push,
    MergeGroup,
}
impl Event {
    fn parse(name: &str) -> Result<Self> {
        match name {
            "pull_request" => Ok(Self::PullRequest),
            "push" => Ok(Self::Push),
            "merge_group" => Ok(Self::MergeGroup),
            _ => Err("Unsupported CI event identity".into()),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Conclusion {
    Success,
    Failure,
    Cancelled,
    Skipped,
    TimedOut,
}
struct Need {
    result: Conclusion,
}
impl<'de> Deserialize<'de> for Need {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct Object;
        impl<'de> serde::de::Visitor<'de> for Object {
            type Value = Need;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("one GitHub need object")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Need, A::Error> {
                let mut result = None;
                while let Some(key) = map.next_key::<String>()? {
                    if key == "result" {
                        if result.is_some() {
                            return Err(serde::de::Error::duplicate_field("result"));
                        }
                        let value = map.next_value::<String>()?;
                        result = Some(match value.as_str() {
                            "success" => Conclusion::Success,
                            "failure" => Conclusion::Failure,
                            "cancelled" => Conclusion::Cancelled,
                            "skipped" => Conclusion::Skipped,
                            "timed_out" => Conclusion::TimedOut,
                            _ => {
                                return Err(serde::de::Error::custom(
                                    "Unknown terminal GitHub conclusion",
                                ));
                            }
                        });
                    } else {
                        map.next_value::<serde::de::IgnoredAny>()?;
                    }
                }
                Ok(Need {
                    result: result.ok_or_else(|| serde::de::Error::missing_field("result"))?,
                })
            }
        }
        deserializer.deserialize_map(Object)
    }
}
struct Needs(BTreeMap<String, Need>);
impl<'de> Deserialize<'de> for Needs {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct Object;
        impl<'de> serde::de::Visitor<'de> for Object {
            type Value = Needs;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("one unique-key GitHub needs object")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Needs, A::Error> {
                let mut needs = BTreeMap::new();
                while let Some((key, value)) = map.next_entry::<String, Need>()? {
                    if needs.insert(key, value).is_some() {
                        return Err(serde::de::Error::custom("Duplicate GitHub job key"));
                    }
                }
                Ok(Needs(needs))
            }
        }
        deserializer.deserialize_map(Object)
    }
}
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Rejection {
    code: &'static str,
    job: &'static str,
}
impl std::fmt::Display for Rejection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "CI gate {}: {}", self.code, self.job)
    }
}
impl std::error::Error for Rejection {}

fn required_jobs(
    event: Event,
    needs: &BTreeMap<String, Need>,
) -> std::result::Result<(), Rejection> {
    if needs.len() > 4
        || needs
            .keys()
            .any(|job| !["context", "repository", "native", "merge"].contains(&job.as_str()))
    {
        return Err(Rejection {
            code: "required_job_set_mismatch",
            job: "all",
        });
    }
    for job in ["context", "repository", "native", "merge"] {
        let result = needs
            .get(job)
            .ok_or(Rejection {
                code: "required_job_missing",
                job,
            })?
            .result;
        if job == "merge" && event != Event::PullRequest {
            if result != Conclusion::Skipped {
                return Err(Rejection {
                    code: "unexpected_merge_job",
                    job,
                });
            }
            continue;
        }
        let code = match result {
            Conclusion::Success => continue,
            Conclusion::Failure => "required_job_failed",
            Conclusion::Cancelled => "required_job_cancelled",
            Conclusion::Skipped => "required_job_skipped",
            Conclusion::TimedOut => "required_job_timed_out",
        };
        return Err(Rejection { code, job });
    }
    Ok(())
}
fn document(path: &Path, limit: u64) -> Result<Vec<u8>> {
    if !fs::symlink_metadata(path)?.is_file() || fs::metadata(path)?.len() > limit {
        return Err("CI input is not a bounded ordinary document".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err("CI document exceeds its bound".into());
    }
    Ok(bytes)
}
pub(crate) fn needs(event: &str, input: &Path, out: &Path) -> Result<()> {
    let event = Event::parse(event)?;
    let Needs(needs) = serde_json::from_slice(&document(input, 65_536)?)?;
    let decision = required_jobs(event, &needs);
    let result = match &decision {
        Ok(()) => {
            json!({"scope":"required GitHub job conclusions only; source/artifact qualification is separate","passed":true})
        }
        Err(rejection) => json!({"passed":false,"rejection":rejection}),
    };
    let bytes = serde_json::to_vec_pretty(&result)?;
    let recorded = (|| -> Result<()> {
        fs::File::options()
            .write(true)
            .create_new(true)
            .open(out)?
            .write_all(&bytes)?;
        Ok(())
    })();
    if let Err(error) = recorded {
        eprintln!("CI decision: {}", String::from_utf8_lossy(&bytes));
        return Err(format!(
            "{}; result write failed: {error}",
            decision
                .as_ref()
                .err()
                .map_or_else(|| "CI needs accepted".to_owned(), ToString::to_string)
        )
        .into());
    }
    decision.map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_context_requires_object_and_exact_run_attempt_and_definition() {
        let definition = commit('f');
        let valid = context(
            Event::Push,
            &json!({"ref":"refs/heads/main","after":commit('a')}),
            &commit('a'),
            &definition,
            &json!({}),
            12,
            3,
        )
        .unwrap();
        let bytes = serde_json::to_vec(&valid).unwrap();
        assert_eq!(context_document(&bytes, 12, 3, &definition).unwrap(), valid);
        for (run, attempt, workflow) in [
            (13, 3, definition.clone()),
            (12, 4, definition.clone()),
            (12, 3, commit('e')),
        ] {
            assert!(context_document(&bytes, run, attempt, &workflow).is_err());
        }
        let array = serde_json::to_vec(&json!([valid])).unwrap();
        assert!(context_document(&array, 12, 3, &definition).is_err());
        let text = String::from_utf8(bytes).unwrap();
        let duplicate = text.replacen('{', "{\"run_id\":12,", 1);
        assert!(context_document(duplicate.as_bytes(), 12, 3, &definition).is_err());
    }
    #[test]
    fn needs_wire_boundary_is_object_only_and_rejects_duplicate_results_and_jobs() {
        let valid = r#"{"context":{"result":"success","outputs":{}},"repository":{"result":"success"},"native":{"result":"success"},"merge":{"result":"skipped"}}"#;
        assert!(serde_json::from_str::<Needs>(valid).is_ok());
        for invalid in [
            "[]".to_owned(),
            valid.replace(r#"{"result":"success","outputs":{}}"#, r#"["success"]"#),
            valid.replacen(r#""result":"success""#, r#""result":{"success":null}""#, 1),
            valid.replacen(
                r#""result":"success""#,
                r#""result":"failure","result":"success""#,
                1,
            ),
            valid.replacen('{', r#"{"repository":{"result":"failure"},"#, 1),
        ] {
            assert!(
                serde_json::from_str::<Needs>(&invalid).is_err(),
                "Malformed needs were admitted"
            );
        }
    }
    #[test]
    fn known_rejection_survives_a_failed_record_write() {
        let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/xtask-tests");
        fs::create_dir_all(&parent).unwrap();
        let owned = tempfile::tempdir_in(parent).unwrap();
        let input = owned.path().join("needs.json");
        let output = owned.path().join("decision.json");
        fs::write(&input,br#"{"context":{"result":"success"},"repository":{"result":"skipped"},"native":{"result":"success"},"merge":{"result":"skipped"}}"#).unwrap();
        fs::write(&output, b"owned earlier evidence\n").unwrap();
        let error = needs("push", &input, &output).unwrap_err().to_string();
        assert!(error.contains("required_job_skipped: repository"));
        assert!(error.contains("result write failed"));
        assert_eq!(fs::read(output).unwrap(), b"owned earlier evidence\n");
    }
    fn commit(byte: char) -> String {
        byte.to_string().repeat(40)
    }
    #[test]
    fn pr_context_distinguishes_h_base_m_and_rejects_every_stale_ref() {
        let h = commit('a');
        let b = commit('b');
        let m = commit('c');
        let event =
            json!({"number":39,"pull_request":{"head":{"sha":h},"base":{"sha":b,"ref":"main"}}});
        let live =
            json!({"number":39,"head":h,"base":b,"base_tip":b,"merge_ref":m,"base_ref":"main"});
        let valid = context(Event::PullRequest, &event, &m, &commit('f'), &live, 1, 1).unwrap();
        assert_eq!(valid.role, Role::H);
        assert_eq!(valid.source, h);
        assert_eq!(valid.event_sha, m);
        assert_eq!(valid.workflow_sha, commit('f'));
        for field in ["head", "base", "base_tip", "merge_ref"] {
            let mut changed = live.clone();
            changed[field] = json!(commit('d'));
            assert!(freshness(&valid, &changed).is_err());
        }
        let mut retargeted = live.clone();
        retargeted["base_ref"] = json!("another-branch-at-the-same-commit");
        assert!(freshness(&valid, &retargeted).is_err());
        assert!(context(Event::PullRequest, &event, &h, &commit('f'), &live, 1, 1).is_err());
        for raw in [
            format!("tree {}\nparent {h}\nparent {b}\n\nmessage", commit('e')),
            format!("tree {}\nparent {b}\n\nparent {h}\n", commit('e')),
            format!(
                "tree {}\nparent {b}\nparent {h}\nparent {}\n",
                commit('e'),
                commit('f')
            ),
        ] {
            assert!(merge_parents(&raw, &b, &h).is_err());
        }
        assert!(
            merge_parents(
                &format!("tree {}\nparent {b}\nparent {h}\n\nmessage", commit('e')),
                &b,
                &h
            )
            .is_ok()
        );
    }
    #[test]
    fn main_and_queue_keep_exact_source_roles_without_a_single_pr_assumption() {
        let s = commit('a');
        let b = commit('b');
        let live = json!({"source_ref":s,"queue_ref":"refs/heads/gh-readonly-queue/main/owned"});
        let pushed = context(
            Event::Push,
            &json!({"ref":"refs/heads/main","after":s}),
            &s,
            &commit('f'),
            &live,
            1,
            1,
        )
        .unwrap();
        assert_eq!(pushed.role, Role::S);
        assert!(pushed.merge.is_none());
        assert!(freshness(&pushed, &json!({"source_ref":b})).is_ok());
        let queued = context(
            Event::MergeGroup,
            &json!({"merge_group":{"head_sha":s,"base_sha":b,"base_ref":"refs/heads/main","head_ref":"refs/heads/gh-readonly-queue/main/owned"}}),
            &s,
            &commit('f'),
            &live,
            1,
            1,
        )
        .unwrap();
        assert_eq!(queued.role, Role::Q);
        assert!(queued.head.is_none());
        assert!(queued.pull_request.is_none());
        assert!(
            context(
                Event::MergeGroup,
                &json!({"merge_group":{"head_sha":s,"base_sha":b,"base_ref":"refs/heads/main"}}),
                &s,
                &commit('f'),
                &live,
                1,
                1
            )
            .is_err()
        );
        assert!(
            context(
                Event::Push,
                &json!({"ref":"refs/heads/other","after":s}),
                &s,
                &commit('f'),
                &live,
                1,
                1
            )
            .is_err()
        );
        let mut changed = live.clone();
        changed["source_ref"] = json!(b);
        assert!(freshness(&queued, &changed).is_err());
        changed = live.clone();
        changed["queue_ref"] = json!("refs/heads/other");
        assert!(freshness(&queued, &changed).is_err());
        assert!(sha(&json!("bad")).is_err());
    }
    fn complete(event: Event) -> BTreeMap<String, Need> {
        ["context", "repository", "native", "merge"]
            .into_iter()
            .map(|job| {
                (
                    job.into(),
                    Need {
                        result: if job == "merge" && event != Event::PullRequest {
                            Conclusion::Skipped
                        } else {
                            Conclusion::Success
                        },
                    },
                )
            })
            .collect()
    }
    #[test]
    fn every_required_failed_cancelled_skipped_or_timed_out_job_refuses() {
        for event in [Event::PullRequest, Event::Push, Event::MergeGroup] {
            assert!(required_jobs(event, &complete(event)).is_ok());
            for job in ["context", "repository", "native", "merge"] {
                if job == "merge" && event != Event::PullRequest {
                    continue;
                }
                for (result, code) in [
                    (Conclusion::Failure, "required_job_failed"),
                    (Conclusion::Cancelled, "required_job_cancelled"),
                    (Conclusion::Skipped, "required_job_skipped"),
                    (Conclusion::TimedOut, "required_job_timed_out"),
                ] {
                    let mut needs = complete(event);
                    needs.get_mut(job).unwrap().result = result;
                    assert_eq!(
                        required_jobs(event, &needs).unwrap_err(),
                        Rejection { code, job }
                    );
                }
            }
        }
    }
    #[test]
    fn omission_is_only_the_explicit_non_pr_merge_role() {
        for event in [Event::PullRequest, Event::Push, Event::MergeGroup] {
            let mut needs = complete(event);
            needs.remove("native");
            assert!(required_jobs(event, &needs).is_err());
            needs = complete(event);
            needs.insert(
                "extra".into(),
                Need {
                    result: Conclusion::Success,
                },
            );
            assert!(required_jobs(event, &needs).is_err());
            needs = complete(event);
            needs.get_mut("merge").unwrap().result = Conclusion::Skipped;
            assert_eq!(
                required_jobs(event, &needs).is_ok(),
                event != Event::PullRequest
            );
        }
        assert!(required_jobs(Event::Push, &BTreeMap::new()).is_err());
        assert!(Event::parse("pull_request_target").is_err());
        assert!(
            serde_json::from_value::<BTreeMap<String, Need>>(
                json!({"native":{"result":"in_progress"}})
            )
            .is_err()
        );
    }
}
