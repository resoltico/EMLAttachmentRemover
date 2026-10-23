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

fn upstream_open(record: &IncidentRecord, observation: &serde_json::Value) -> Result<()> {
    if observation["html_url"] != record.upstream_issue
        || observation["state"] != "open"
        || !observation["updated_at"]
            .as_str()
            .is_some_and(|v| !v.is_empty() && v.len() <= 64)
    {
        return Err("Declared upstream incident is closed or its read-only freshness observation is unverified".into());
    }
    Ok(())
}
pub(crate) fn check(root: &Path, out: &Path) -> Result<()> {
    fs::create_dir(out)?;
    let now = SystemTime::now();
    let mut result = serde_json::json!({"schema":1,"checked_at":timestamp(now)?,"scope":"read-only incident policy/upstream maintenance; no qualification, renewal or service action","policy_sha256":null,"incidents":[],"failure":null});
    let checked = (|| -> Result<()> {
        let registry = Registry::load(root)?;
        result["policy_sha256"] = serde_json::json!(registry.digest());
        let mut failed = false;
        for record in registry.records() {
            let observation = (|| -> Result<serde_json::Value> {
                record.current(now)?;
                let number = record
                    .upstream_issue
                    .strip_prefix("https://github.com/actions/runner-images/issues/")
                    .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
                    .ok_or("Incident upstream issue is not the fixed read-only repository")?;
                let bytes =
                    crate::capture(std::process::Command::new("gh").current_dir(root).args([
                        "api",
                        &format!("repos/actions/runner-images/issues/{number}"),
                        "--jq",
                        "{state:.state,html_url:.html_url,updated_at:.updated_at}",
                    ]))?;
                if bytes.len() > 16 * 1024 {
                    return Err("Upstream maintenance response exceeds its bound".into());
                }
                let value: serde_json::Value = serde_json::from_slice(&bytes)?;
                upstream_open(record, &value)?;
                Ok(value)
            })();
            if observation.is_err() {
                failed = true;
            }
            result["incidents"].as_array_mut().expect("maintenance array").push(serde_json::json!({"id":record.id,"reviewed_at":record.reviewed_at,"expires_at":record.expires_at,"upstream":observation.as_ref().ok(),"failure":observation.as_ref().err().map(ToString::to_string)}));
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
}
