//! Pure admission of the finite exact-binary Linux preparation campaign.
use crate::{Result, picker_evidence::ProcessObservation};
use serde_json::Value;
use xtask::archive::ByteIdentity;
const MIB: u64 = 1024 * 1024;
fn field<'a>(v: &'a Value, key: &str) -> Result<&'a Value> {
    v.get(key)
        .ok_or_else(|| format!("Missing resource observation {key}").into())
}
fn integer(v: &Value, key: &str) -> Result<u64> {
    field(v, key)?
        .as_u64()
        .ok_or_else(|| format!("Invalid integer resource observation {key}").into())
}
fn seconds(v: &Value, key: &str) -> Result<f64> {
    field(v, key)?
        .as_f64()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .ok_or_else(|| format!("Invalid finite resource timing {key}").into())
}
fn null(v: &Value, key: &str) -> bool {
    v.get(key) == Some(&Value::Null)
}
fn digest(v: &Value) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
pub(crate) fn validate(value: &Value, source: &str, binary: &ByteIdentity) -> Result<()> {
    let process: ProcessObservation = serde_json::from_value(field(value, "process")?.clone())?;
    if value["source"] != source
        || serde_json::from_value::<ByteIdentity>(field(value, "binary")?.clone())? != *binary
        || process.clean()? != 0
        || !null(value, "process_failure")
        || !null(value, "measurement_read_failure")
    {
        return Err("Resource campaign source/binary/process binding failed".into());
    }
    let m = field(value, "measurements")?;
    if m["source"] != source
        || m["binary_sha256"] != binary.sha256
        || m["passed"] != true
        || !null(m, "failure")
        || m["host"]["system"] != "Linux"
        || m["host"]["machine"] != "x86_64"
        || !m["filesystem"].as_str().is_some_and(|s| !s.is_empty())
        || !["case-sensitive", "case-insensitive"]
            .iter()
            .any(|mode| m["case_mode"] == *mode)
    {
        return Err("Resource measurements lack exact native successful campaign binding".into());
    }
    let fixtures = field(m, "fixtures")?
        .as_object()
        .ok_or("Resource fixtures are not an object")?;
    if fixtures
        .keys()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>()
        != std::collections::BTreeSet::from(["shallow", "deep", "limit"])
    {
        return Err("Resource fixture set differs".into());
    }
    for (name, wrappers) in [("shallow", 1), ("deep", 63), ("limit", 63)] {
        let f = &fixtures[name];
        let raw = integer(f, "raw_bytes")?;
        let body = integer(f, "body_bytes")?;
        if integer(f, "wrappers")? != wrappers
            || !digest(field(f, "sha256")?)
            || raw <= body
            || (name != "limit" && body != 32 * MIB)
            || (name == "limit" && raw != 256 * MIB)
            || raw - body > 64 * 1024
        {
            return Err("Resource fixture bytes/depth/thin-header shape differs".into());
        }
    }
    let rows = field(m, "runs")?
        .as_array()
        .ok_or("Resource run list is absent")?;
    let mut expected = Vec::new();
    for i in 1..=5 {
        for name in ["shallow", "deep"] {
            expected.push((format!("{name}-{i}"), name));
        }
    }
    expected.push(("limit".into(), "limit"));
    if rows.len() != expected.len() {
        return Err("Finite resource campaign run set differs".into());
    }
    let (mut shallow, mut deep) = (Vec::new(), Vec::new());
    for (row, (name, fixture)) in rows.iter().zip(expected) {
        if row["name"] != name
            || row["fixture"] != fixture
            || row["exit_code"] != 0
            || row["exact_existing_bytes_matched"] != true
        {
            return Err("Resource run identity/order/byte oracle failed".into());
        }
        let elapsed = seconds(row, "elapsed_seconds")?;
        seconds(row, "user_seconds")?;
        seconds(row, "system_seconds")?;
        let rss = integer(row, "max_rss_kib")?
            .checked_mul(1024)
            .ok_or("Resource RSS multiplication overflow")?;
        if rss == 0 {
            return Err("Resource RSS is unobserved".into());
        }
        match fixture {
            "shallow" => shallow.push(elapsed),
            "deep" => deep.push(elapsed),
            "limit" => {
                let raw = integer(&fixtures["limit"], "raw_bytes")?;
                let bound = raw
                    .checked_mul(2)
                    .and_then(|n| n.checked_add(96 * MIB))
                    .ok_or("Resource RSS bound overflow")?;
                if rss > bound || integer(m, "rss_limit_bytes")? != bound {
                    return Err("Exact-limit resource RSS budget failed".into());
                }
            }
            _ => unreachable!(),
        }
    }
    shallow.sort_by(f64::total_cmp);
    deep.sort_by(f64::total_cmp);
    let a = shallow[shallow.len() / 2];
    let b = deep[deep.len() / 2];
    let bound = 3.0 * a + 0.2;
    let medians = field(m, "medians")?;
    if b > bound
        || seconds(medians, "shallow_seconds")? != a
        || seconds(medians, "deep_seconds")? != b
        || seconds(medians, "maximum_deep_seconds")? != bound
    {
        return Err("Independent resource median qualification failed".into());
    }
    Ok(())
}
