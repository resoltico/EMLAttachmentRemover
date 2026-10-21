//! Explicit known-image picker host provisioning; never product acceptance by itself.
use crate::{Result, distribution::PickerHostPolicy};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use xtask::archive::ByteIdentity;

mod crashes;
mod evidence;
#[cfg(target_os = "macos")]
mod native;
#[cfg(target_os = "macos")]
mod observations;
#[cfg(target_os = "macos")]
pub(crate) use native::{prepare, require_context, verify};

const SERVICE: &str = "com.apple.iconservices.iconservicesagent";
const APPLE_PROGRAM: &str = "/System/Library/CoreServices/iconservicesagent";
const APPLE_PLIST: &str =
    "/System/Library/LaunchAgents/com.apple.iconservices.iconservicesagent.plist";
const LAUNCHCTL: &str = "/bin/launchctl";
const METAL: &str = "/System/Library/Frameworks/Metal.framework/Versions/A/Metal";
const RENDERBOX: &str =
    "/System/Library/PrivateFrameworks/RenderBox.framework/Versions/A/RenderBox";
const DOCUMENT_LIMIT: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Binding {
    source: String,
    target: String,
    run: String,
    attempt: String,
    workflow_ref: String,
    workflow_sha: String,
    archive: ByteIdentity,
    reference: ByteIdentity,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Host {
    target: String,
    process_os: String,
    process_architecture: String,
    kernel_machine: String,
    image_os: String,
    image_version: String,
    product_version: String,
    build_version: String,
    boot_uuid: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SecuritySession {
    status: i32,
    security_session_id: Option<u32>,
    graphic_access: Option<bool>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Caller {
    uid: u32,
    euid: u32,
    console_uid: u32,
    security_session: SecuritySession,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandObservation {
    program: String,
    arguments: Vec<String>,
    process: Value,
    stdout_path: String,
    stderr_path: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrintObservation {
    program: String,
    arguments: Vec<String>,
    process: Value,
    selected_fields: BTreeMap<String, String>,
    absence_diagnostic: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DisabledObservation {
    program: String,
    arguments: Vec<String>,
    process: Value,
    r#override: Option<bool>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceObservation {
    target: String,
    print: PrintObservation,
    disabled: DisabledObservation,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FaultFrame {
    symbol: Option<String>,
    image_name: String,
    image_path: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FaultThread {
    index: usize,
    triggered: bool,
    frames: Vec<FaultFrame>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Termination {
    namespace: String,
    code: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CrashReport {
    file_name: String,
    identity: ByteIdentity,
    pid: u32,
    user_id: u32,
    boot_uuid: String,
    os_build: String,
    capture_time: String,
    proc_path: String,
    exception_signal: String,
    termination: Termination,
    faulting_thread: FaultThread,
}
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Preparation {
    stage: String,
    #[serde(flatten)]
    binding: Binding,
    picker_host: PickerHostPolicy,
    host: Host,
    caller: Caller,
    service_before: ServiceObservation,
    crash_reports: Vec<CrashReport>,
    bootout: CommandObservation,
    bootout_absence: Option<PrintObservation>,
    disable: CommandObservation,
    service_after: ServiceObservation,
}
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Postverification {
    stage: String,
    #[serde(flatten)]
    binding: Binding,
    picker_host: PickerHostPolicy,
    host: Host,
    caller: Caller,
    service: ServiceObservation,
}

#[cfg(test)]
mod tests;
