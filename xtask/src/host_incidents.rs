//! Exact, expiring host-incident data. Healthy qualification never loads this file.
use crate::Result;
use serde::{Deserialize, Serialize};
#[cfg(any(target_os = "macos", test))]
use std::time::{Duration, Instant};
use std::{
    fs,
    io::Read,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
const MAX_POLICY_BYTES: usize = 64 * 1024;
const MAX_LIFETIME: u64 = 30 * 24 * 60 * 60;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IncidentHost {
    pub(crate) image_os: String,
    pub(crate) image_version: String,
    pub(crate) product_version: String,
    pub(crate) build_version: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IncidentIdentity {
    pub(crate) id: String,
    pub(crate) policy_sha256: String,
    pub(crate) source: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IncidentRecord {
    pub(crate) id: String,
    pub(crate) host: IncidentHost,
    pub(crate) architecture: String,
    pub(crate) classifier: String,
    pub(crate) service: String,
    pub(crate) upstream_issue: String,
    pub(crate) reviewed_at: u64,
    pub(crate) expires_at: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema: u8,
    incidents: Vec<IncidentRecord>,
}
pub(crate) struct Registry {
    records: Vec<IncidentRecord>,
    digest: String,
}
fn dotted_digits(value: &str, count: usize, maximum: usize) -> bool {
    let pieces: Vec<_> = value.split('.').collect();
    value.len() <= maximum
        && pieces.len() == count
        && pieces
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}
fn timestamp(time: SystemTime) -> Result<u64> {
    Ok(time.duration_since(UNIX_EPOCH)?.as_secs())
}
impl IncidentRecord {
    fn validate(&self) -> Result<()> {
        if self.id.is_empty()
            || self.id.len() > 100
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || self.architecture != "x86_64"
            || self.host.image_os != "macos26"
            || !dotted_digits(&self.host.image_version, 3, 32)
            || !dotted_digits(&self.host.product_version, 3, 16)
            || !self.host.product_version.starts_with("26.")
            || self.host.build_version.is_empty()
            || self.host.build_version.len() > 32
            || !self
                .host
                .build_version
                .bytes()
                .all(|b| b.is_ascii_alphanumeric())
            || self.classifier != "iconservices-metal-renderbox-panel"
            || self.service != "com.apple.iconservices.iconservicesagent"
            || self.upstream_issue != "https://github.com/actions/runner-images/issues/14751"
            || self
                .expires_at
                .checked_sub(self.reviewed_at)
                .is_none_or(|d| d == 0 || d > MAX_LIFETIME)
        {
            return Err("Incident must be the exact reviewed host/classifier/service and a positive interval of at most thirty days".into());
        }
        Ok(())
    }
    pub(crate) fn current(&self, now: SystemTime) -> Result<()> {
        let now = timestamp(now)?;
        if now < self.reviewed_at || now >= self.expires_at {
            return Err("Incident review interval is not currently valid".into());
        }
        Ok(())
    }
}
impl Registry {
    pub(crate) fn load(root: &Path) -> Result<Self> {
        let path = root.join("xtask/host-incidents.toml");
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_file() || metadata.len() > MAX_POLICY_BYTES as u64 {
            return Err("Incident policy must be a bounded ordinary file".into());
        }
        let mut bytes = Vec::new();
        fs::File::open(&path)?
            .take((MAX_POLICY_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_POLICY_BYTES {
            return Err("Incident policy grew beyond its bound".into());
        }
        Self::parse(&bytes)
    }
    fn parse(bytes: &[u8]) -> Result<Self> {
        let document: Document = toml::from_str(std::str::from_utf8(bytes)?)?;
        if document.schema != 1 || document.incidents.len() > 1 {
            return Err("Only the single reviewed incident is supported".into());
        }
        for row in &document.incidents {
            row.validate()?;
        }
        Ok(Self {
            records: document.incidents,
            digest: xtask::archive::ByteIdentity::of(bytes).sha256,
        })
    }
    pub(crate) fn select(
        &self,
        host: &IncidentHost,
        architecture: &str,
        source: &str,
        now: SystemTime,
    ) -> Result<(IncidentIdentity, IncidentRecord)> {
        let mut matches = self
            .records
            .iter()
            .filter(|record| record.host == *host && record.architecture == architecture);
        let record = matches
            .next()
            .ok_or("No exact incident matches this unhealthy host")?;
        if matches.next().is_some() {
            return Err("Incident match is ambiguous".into());
        }
        record.current(now)?;
        Ok((
            IncidentIdentity {
                id: record.id.clone(),
                policy_sha256: self.digest.clone(),
                source: source.to_owned(),
            },
            record.clone(),
        ))
    }
    pub(crate) fn recheck(
        &self,
        identity: &IncidentIdentity,
        record: &IncidentRecord,
        now: SystemTime,
    ) -> Result<()> {
        let (current, row) =
            self.select(&record.host, &record.architecture, &identity.source, now)?;
        if current != *identity
            || row.reviewed_at != record.reviewed_at
            || row.expires_at != record.expires_at
        {
            return Err("Incident identity or review dates changed after admission".into());
        }
        Ok(())
    }
    pub(crate) fn records(&self) -> &[IncidentRecord] {
        &self.records
    }
    pub(crate) fn digest(&self) -> &str {
        &self.digest
    }
}
// A concrete clock guard used by the private native admission owner. It is not
// serializable, cloneable or a service-mutation capability by itself.
#[cfg(any(target_os = "macos", test))]
pub(crate) struct ActionDeadline {
    admitted: Instant,
    deadline: Instant,
    reviewed_at: u64,
    expires_at: u64,
}
#[cfg(any(target_os = "macos", test))]
impl ActionDeadline {
    pub(crate) fn admit(
        record: &IncidentRecord,
        wall: SystemTime,
        monotonic: Instant,
    ) -> Result<Self> {
        record.current(wall)?;
        let expiry = UNIX_EPOCH
            .checked_add(Duration::from_secs(record.expires_at))
            .ok_or("Incident expiry overflow")?;
        let remaining = expiry.duration_since(wall)?;
        let deadline = monotonic
            .checked_add(remaining.min(Duration::from_secs(600)))
            .ok_or("Incident monotonic deadline overflow")?;
        Ok(Self {
            admitted: monotonic,
            deadline,
            reviewed_at: record.reviewed_at,
            expires_at: record.expires_at,
        })
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn elapsed_ns(&self, now: Instant) -> Result<u64> {
        Ok(now
            .checked_duration_since(self.admitted)
            .ok_or("Monotonic observation precedes admission")?
            .as_nanos()
            .try_into()?)
    }
    pub(crate) fn check(&self, wall: SystemTime, monotonic: Instant) -> Result<()> {
        let now = timestamp(wall)?;
        if now < self.reviewed_at
            || now >= self.expires_at
            || monotonic < self.admitted
            || monotonic >= self.deadline
        {
            return Err(
                "Incident action authority expired; no further service mutation is admitted".into(),
            );
        }
        Ok(())
    }
}

const MAX_UPSTREAM_BYTES: usize = 16 * 1024;
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct UpstreamIssue {
    state: String,
    html_url: String,
    updated_at: String,
}
fn upstream_document(bytes: &[u8]) -> Result<UpstreamIssue> {
    if bytes.len() > MAX_UPSTREAM_BYTES {
        return Err("Upstream maintenance response exceeds its bound".into());
    }
    Ok(serde_json::from_slice(bytes)?)
}
fn upstream_open(record: &IncidentRecord, observation: &UpstreamIssue) -> Result<()> {
    if observation.html_url != record.upstream_issue
        || observation.state != "open"
        || observation.updated_at.is_empty()
        || observation.updated_at.len() > 64
    {
        return Err("Declared upstream incident is closed or its read-only freshness observation is unverified".into());
    }
    Ok(())
}
pub(crate) fn check(root: &Path, out: &Path) -> Result<()> {
    check_with_observation(root, out, SystemTime::now(), |record| {
        let number = record
            .upstream_issue
            .strip_prefix("https://github.com/actions/runner-images/issues/")
            .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            .ok_or("Incident upstream issue is not the fixed read-only repository")?;
        crate::capture(std::process::Command::new("gh").current_dir(root).args([
            "api",
            &format!("repos/actions/runner-images/issues/{number}"),
            "--jq",
            "{state:.state,html_url:.html_url,updated_at:.updated_at}",
        ]))
    })
}
fn check_with_observation(
    root: &Path,
    out: &Path,
    now: SystemTime,
    mut observe: impl FnMut(&IncidentRecord) -> Result<Vec<u8>>,
) -> Result<()> {
    fs::create_dir(out)?;
    let mut result = serde_json::json!({"schema":1,"checked_at":timestamp(now)?,"scope":"read-only incident policy/upstream maintenance; no qualification, renewal or service action","policy_sha256":null,"incidents":[],"failure":null});
    let checked = (|| -> Result<()> {
        let registry = Registry::load(root)?;
        result["policy_sha256"] = serde_json::json!(registry.digest());
        let mut failed = false;
        for record in registry.records() {
            let mut upstream = None;
            let observation = (|| -> Result<()> {
                record.current(now)?;
                // Retain a parsed response before interpreting its open/closed
                // state: an actual closed observation is still evidence.
                upstream = Some(upstream_document(&observe(record)?)?);
                upstream_open(
                    record,
                    upstream.as_ref().expect("observed upstream response"),
                )
            })();
            if observation.is_err() {
                failed = true;
            }
            result["incidents"].as_array_mut().expect("maintenance array").push(serde_json::json!({"id":record.id,"reviewed_at":record.reviewed_at,"expires_at":record.expires_at,"upstream":upstream,"failure":observation.as_ref().err().map(ToString::to_string)}));
        }
        if failed {
            return Err("Incident maintenance failed; policy is not renewed or changed".into());
        }
        Ok(())
    })();
    if let Err(error) = &checked {
        result["failure"] = serde_json::json!(error.to_string());
    }
    fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
    checked
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record() -> IncidentRecord {
        IncidentRecord {
            id: "intel-iconservices-panel-2026-10".into(),
            host: IncidentHost {
                image_os: "macos26".into(),
                image_version: "20260824.0517.1".into(),
                product_version: "26.6.1".into(),
                build_version: "25G76".into(),
            },
            architecture: "x86_64".into(),
            classifier: "iconservices-metal-renderbox-panel".into(),
            service: "com.apple.iconservices.iconservicesagent".into(),
            upstream_issue: "https://github.com/actions/runner-images/issues/14751".into(),
            reviewed_at: 100,
            expires_at: 1000,
        }
    }
    #[test]
    fn both_clocks_expire_without_reset_and_short_lifetime_wins() {
        let start = Instant::now();
        let r = record();
        let deadline =
            ActionDeadline::admit(&r, UNIX_EPOCH + Duration::from_secs(200), start).unwrap();
        assert!(
            deadline
                .check(
                    UNIX_EPOCH + Duration::from_secs(201),
                    start + Duration::from_secs(599)
                )
                .is_ok()
        );
        assert!(
            deadline
                .check(
                    UNIX_EPOCH + Duration::from_secs(150),
                    start + Duration::from_secs(600)
                )
                .is_err()
        );
        assert!(
            deadline
                .check(UNIX_EPOCH + Duration::from_secs(1000), start)
                .is_err()
        );
        let deadline =
            ActionDeadline::admit(&r, UNIX_EPOCH + Duration::from_secs(950), start).unwrap();
        assert!(
            deadline
                .check(
                    UNIX_EPOCH + Duration::from_secs(951),
                    start + Duration::from_secs(50)
                )
                .is_err()
        );
    }
    #[test]
    fn exact_host_identity_dates_and_policy_digest_are_required() {
        let r = record();
        assert!(r.validate().is_ok());
        for mutation in 0..4 {
            let mut changed = r.clone();
            match mutation {
                0 => changed.host.image_version = "future-image".into(),
                1 => changed.architecture = "aarch64".into(),
                2 => changed.expires_at = changed.reviewed_at,
                _ => changed.expires_at = changed.reviewed_at + MAX_LIFETIME + 1,
            };
            assert!(changed.validate().is_err());
        }
    }
    #[test]
    fn fractional_remaining_lifetime_is_not_rounded_up_after_wall_rollback() {
        let start = Instant::now();
        let r = record();
        let deadline =
            ActionDeadline::admit(&r, UNIX_EPOCH + Duration::from_millis(950_900), start).unwrap();
        assert!(
            deadline
                .check(
                    UNIX_EPOCH + Duration::from_secs(951),
                    start + Duration::from_millis(49_099)
                )
                .is_ok()
        );
        assert!(
            deadline
                .check(
                    UNIX_EPOCH + Duration::from_secs(951),
                    start + Duration::from_millis(49_100)
                )
                .is_err()
        );
    }
    const MAINTENANCE_POLICY: &[u8] = br#"schema = 1
[[incidents]]
id = "intel-iconservices-panel-2026-10"
architecture = "x86_64"
classifier = "iconservices-metal-renderbox-panel"
service = "com.apple.iconservices.iconservicesagent"
upstream_issue = "https://github.com/actions/runner-images/issues/14751"
reviewed_at = 100
expires_at = 1000
[incidents.host]
image_os = "macos26"
image_version = "20260824.0517.1"
product_version = "26.6.1"
build_version = "25G76"
"#;
    fn maintenance_fixture(policy: &[u8]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("xtask")).unwrap();
        fs::write(root.path().join("xtask/host-incidents.toml"), policy).unwrap();
        root
    }
    fn retained_maintenance(root: &Path) -> serde_json::Value {
        serde_json::from_slice(&fs::read(root.join("observation/result.json")).unwrap()).unwrap()
    }
    #[test]
    fn maintenance_retains_open_closed_and_unverified_literal_upstream_observations() {
        let cases: &[(&[u8], bool, &str)] = &[
            (br#"{"state":"open","html_url":"https://github.com/actions/runner-images/issues/14751","updated_at":"2026-10-23T12:00:00Z"}"#, true, "open"),
            (br#"{"state":"closed","html_url":"https://github.com/actions/runner-images/issues/14751","updated_at":"2026-10-23T12:00:00Z"}"#, false, "closed"),
            (br#"{"state":"open","html_url":"https://github.com/actions/runner-images/issues/1","updated_at":"2026-10-23T12:00:00Z"}"#, false, "open"),
            (br#"{"state":"open","html_url":"https://github.com/actions/runner-images/issues/14751","updated_at":""}"#, false, "open"),
        ];
        for &(bytes, expected_success, expected_state) in cases {
            let root = maintenance_fixture(MAINTENANCE_POLICY);
            let before = fs::read(root.path().join("xtask/host-incidents.toml")).unwrap();
            let mut calls = 0;
            let checked = check_with_observation(
                root.path(),
                &root.path().join("observation"),
                UNIX_EPOCH + Duration::from_secs(500),
                |record| {
                    calls += 1;
                    assert_eq!(
                        record.upstream_issue,
                        "https://github.com/actions/runner-images/issues/14751"
                    );
                    Ok(bytes.to_vec())
                },
            );
            assert_eq!(checked.is_ok(), expected_success);
            assert_eq!(calls, 1);
            let result = retained_maintenance(root.path());
            assert_eq!(result["checked_at"], 500);
            assert_eq!(result["failure"].is_null(), expected_success);
            assert_eq!(
                result["incidents"][0]["failure"].is_null(),
                expected_success
            );
            assert_eq!(result["incidents"][0]["upstream"]["state"], expected_state);
            assert_eq!(
                result["policy_sha256"],
                xtask::archive::ByteIdentity::of(&before).sha256
            );
            assert_eq!(
                fs::read(root.path().join("xtask/host-incidents.toml")).unwrap(),
                before
            );
            assert_eq!(fs::read_dir(root.path().join("xtask")).unwrap().count(), 1);
        }
        let root = maintenance_fixture(MAINTENANCE_POLICY);
        let response = format!(
            r#"{{"state":"open","html_url":"https://github.com/actions/runner-images/issues/14751","updated_at":"{}"}}"#,
            "x".repeat(65)
        );
        assert!(
            check_with_observation(
                root.path(),
                &root.path().join("observation"),
                UNIX_EPOCH + Duration::from_secs(500),
                |_| Ok(response.as_bytes().to_vec())
            )
            .is_err()
        );
        assert_eq!(
            retained_maintenance(root.path())["incidents"][0]["upstream"]["updated_at"],
            "x".repeat(65)
        );
    }
    #[test]
    fn maintenance_rejects_unavailable_oversize_malformed_duplicate_and_mistyped_responses() {
        let mut cases = vec![
            br#"{"state":"open","html_url":"https://github.com/actions/runner-images/issues/14751","updated_at":"now""#.to_vec(),
            br#"{"state":"closed","state":"open","html_url":"https://github.com/actions/runner-images/issues/14751","updated_at":"now"}"#.to_vec(),
            br#"{"state":"open","html_url":"https://github.com/actions/runner-images/issues/14751"}"#.to_vec(),
            br#"{"state":"open","html_url":"https://github.com/actions/runner-images/issues/14751","updated_at":12}"#.to_vec(),
            br#"{"state":"open","html_url":"https://github.com/actions/runner-images/issues/14751","updated_at":null}"#.to_vec(),
            br#"{"state":true,"html_url":"https://github.com/actions/runner-images/issues/14751","updated_at":"now"}"#.to_vec(),
            br#"{"state":"open","html_url":"https://github.com/actions/runner-images/issues/14751","updated_at":"now","unverified":true}"#.to_vec(),
        ];
        cases.push(vec![b' '; 16 * 1024 + 1]);
        for bytes in cases {
            let root = maintenance_fixture(MAINTENANCE_POLICY);
            let mut calls = 0;
            assert!(
                check_with_observation(
                    root.path(),
                    &root.path().join("observation"),
                    UNIX_EPOCH + Duration::from_secs(500),
                    |_| {
                        calls += 1;
                        Ok(bytes.clone())
                    }
                )
                .is_err()
            );
            assert_eq!(calls, 1);
            let result = retained_maintenance(root.path());
            assert!(result["failure"].is_string());
            assert!(result["incidents"][0]["failure"].is_string());
            assert!(result["incidents"][0]["upstream"].is_null());
            assert_eq!(
                fs::read(root.path().join("xtask/host-incidents.toml")).unwrap(),
                MAINTENANCE_POLICY
            );
        }
        let root = maintenance_fixture(MAINTENANCE_POLICY);
        let mut calls = 0;
        assert!(
            check_with_observation(
                root.path(),
                &root.path().join("observation"),
                UNIX_EPOCH + Duration::from_secs(500),
                |_| {
                    calls += 1;
                    Err("Independent read-only upstream API unavailable".into())
                }
            )
            .is_err()
        );
        assert_eq!(calls, 1);
        let result = retained_maintenance(root.path());
        assert!(result["incidents"][0]["upstream"].is_null());
        assert!(
            result["incidents"][0]["failure"]
                .as_str()
                .unwrap()
                .contains("upstream API unavailable")
        );
        assert!(result["failure"].is_string());
        assert_eq!(
            fs::read(root.path().join("xtask/host-incidents.toml")).unwrap(),
            MAINTENANCE_POLICY
        );
    }
    #[test]
    fn maintenance_never_queries_or_rewrites_expired_or_malformed_policy() {
        let invalid = [
            b"schema = [".to_vec(),
            String::from_utf8(MAINTENANCE_POLICY.to_vec())
                .unwrap()
                .replace("schema = 1", "schema = 2")
                .into_bytes(),
            String::from_utf8(MAINTENANCE_POLICY.to_vec())
                .unwrap()
                .replace("expires_at = 1000", "expires_at = 100")
                .into_bytes(),
            String::from_utf8(MAINTENANCE_POLICY.to_vec())
                .unwrap()
                .replace("expires_at = 1000", "expires_at = 2592101")
                .into_bytes(),
        ];
        for (policy, now) in invalid
            .iter()
            .map(|policy| (policy.as_slice(), 500))
            .chain([(MAINTENANCE_POLICY, 99), (MAINTENANCE_POLICY, 1000)])
        {
            let root = maintenance_fixture(policy);
            let mut calls = 0;
            assert!(
                check_with_observation(
                    root.path(),
                    &root.path().join("observation"),
                    UNIX_EPOCH + Duration::from_secs(now),
                    |_| {
                        calls += 1;
                        Err("An inadmissible policy must not query upstream".into())
                    }
                )
                .is_err()
            );
            assert_eq!(calls, 0);
            assert!(retained_maintenance(root.path())["failure"].is_string());
            assert_eq!(
                fs::read(root.path().join("xtask/host-incidents.toml")).unwrap(),
                policy
            );
            assert_eq!(fs::read_dir(root.path().join("xtask")).unwrap().count(), 1);
        }
    }
}
