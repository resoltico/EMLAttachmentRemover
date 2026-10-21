//! Bounded parsing of the captured native crash schema and its fault-thread evidence.
use super::*;

pub(super) const MAX_REPORTS: usize = 12;
pub(super) const REPORT_LIMIT: u64 = 8 * 1024 * 1024;
const FRAME_LIMIT: usize = 256;
const METAL_SYMBOL: &str = "MTLLoader sliceIDForDevice:";

fn text(value: &Value, key: &str) -> Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 4096)
        .map(str::to_owned)
        .ok_or_else(|| format!("Missing/invalid native crash string: {key}").into())
}
fn number(value: &Value, key: &str) -> Result<u32> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| format!("Missing/invalid native crash integer: {key}").into())
}
fn array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value]> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("Missing/invalid native crash array: {key}").into())
}
fn frame(value: &Value, images: &[Value]) -> Result<FaultFrame> {
    let index = usize::try_from(number(value, "imageIndex")?)?;
    let image = images
        .get(index)
        .ok_or("Native crash frame image index exceeds image inventory")?;
    let symbol = match value.get("symbol") {
        None => None,
        Some(Value::String(s)) if !s.is_empty() && s.len() <= 4096 => Some(s.clone()),
        _ => return Err("Unknown native crash symbol field".into()),
    };
    Ok(FaultFrame {
        symbol,
        image_name: text(image, "name")?,
        image_path: text(image, "path")?,
    })
}
fn fault_thread(report: &Value) -> Result<FaultThread> {
    let index = usize::try_from(number(report, "faultingThread")?)?;
    let thread = array(report, "threads")?
        .get(index)
        .ok_or("Invalid native faulting-thread index")?;
    if thread.get("triggered") != Some(&Value::Bool(true)) {
        return Err("Native faulting-thread marker is unavailable".into());
    }
    let frames = array(thread, "frames")?;
    if frames.is_empty() || frames.len() > FRAME_LIMIT {
        return Err("Native fault frame inventory exceeds its bound".into());
    }
    let images = array(report, "usedImages")?;
    Ok(FaultThread {
        index,
        triggered: true,
        frames: frames
            .iter()
            .map(|value| frame(value, images))
            .collect::<Result<_>>()?,
    })
}
pub(super) fn parse(file_name: &str, bytes: &[u8]) -> Result<CrashReport> {
    if !file_name.starts_with("iconservicesagent-")
        || !file_name.ends_with(".ips")
        || file_name.contains('/')
        || file_name.contains('\\')
        || file_name.len() > 255
        || bytes.len() as u64 > REPORT_LIMIT
    {
        return Err("Unrecognized or oversized native crash input".into());
    }
    let mut stream = serde_json::Deserializer::from_slice(bytes).into_iter::<Value>();
    let header = stream.next().ok_or("Missing crash metadata object")??;
    let report = stream.next().ok_or("Missing crash report object")??;
    if stream.next().is_some()
        || header["bug_type"] != "309"
        || header["app_name"] != "iconservicesagent"
        || report["version"] != 2
    {
        return Err("Unknown native crash schema or trailing object".into());
    }
    let pid = number(&report, "pid")?;
    if pid == 0 {
        return Err("Native crash PID is invalid".into());
    }
    Ok(CrashReport {
        file_name: file_name.to_owned(),
        identity: ByteIdentity::of(bytes),
        pid,
        user_id: number(&report, "userID")?,
        boot_uuid: text(&report, "bootSessionUUID")?.to_ascii_lowercase(),
        os_build: text(&report["osVersion"], "build")?,
        capture_time: text(&report, "captureTime")?,
        proc_path: text(&report, "procPath")?,
        exception_signal: text(&report["exception"], "signal")?,
        termination: Termination {
            namespace: text(&report["termination"], "namespace")?,
            code: number(&report["termination"], "code")?,
        },
        faulting_thread: fault_thread(&report)?,
    })
}
pub(super) fn known(report: &CrashReport, host: &Host, uid: u32) -> bool {
    report.user_id == uid
        && report.boot_uuid == host.boot_uuid
        && report.os_build == host.build_version
        && report.proc_path == APPLE_PROGRAM
        && report.exception_signal == "SIGABRT"
        && report.termination.namespace == "SIGNAL"
        && report.termination.code == 6
        && report.faulting_thread.triggered
        && report.faulting_thread.frames.iter().any(|f| {
            f.symbol
                .as_deref()
                .is_some_and(|s| s.contains(METAL_SYMBOL))
                && f.image_name == "Metal"
                && f.image_path == METAL
        })
        && report.faulting_thread.frames.iter().any(|f| {
            f.symbol.as_deref().is_some_and(|s| !s.is_empty())
                && f.image_name == "RenderBox"
                && f.image_path == RENDERBOX
        })
}
pub(super) fn require_known(reports: &[CrashReport], host: &Host, uid: u32) -> Result<()> {
    if reports.is_empty()
        || reports.len() > MAX_REPORTS
        || !reports.iter().any(|r| known(r, host, uid))
    {
        return Err("No known current-boot Apple icon-agent Metal/RenderBox SIGABRT; no intervention authorized".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
