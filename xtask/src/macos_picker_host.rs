//! Native picker qualification and shared incident observation guards.
use crate::{Result, host_incidents::IncidentHost};
#[cfg(target_os = "macos")]
use serde::Serialize;
#[cfg(any(target_os = "macos", test))]
use serde_json::Value;
#[cfg(any(target_os = "macos", test))]
use std::collections::BTreeMap;
#[cfg(any(target_os = "macos", test))]
use xtask::archive::ByteIdentity;

pub(crate) mod crashes;
pub(crate) mod evidence;
#[cfg(target_os = "macos")]
mod native;
#[cfg(target_os = "macos")]
mod observations;
#[cfg(target_os = "macos")]
pub(crate) use native::qualify;

const SERVICE: &str = "com.apple.iconservices.iconservicesagent";
const APPLE_PROGRAM: &str = "/System/Library/CoreServices/iconservicesagent";
const APPLE_PLIST: &str =
    "/System/Library/LaunchAgents/com.apple.iconservices.iconservicesagent.plist";
const LAUNCHCTL: &str = "/bin/launchctl";
const METAL: &str = "/System/Library/Frameworks/Metal.framework/Versions/A/Metal";
const RENDERBOX: &str =
    "/System/Library/PrivateFrameworks/RenderBox.framework/Versions/A/RenderBox";
#[cfg(target_os = "macos")]
const DOCUMENT_LIMIT: u64 = 16 * 1024 * 1024;

pub(crate) use crate::picker_evidence::{
    Caller, CrashReport, DisabledOverrideObservation as DisabledObservation, Host,
    HostPreparationReceipt as Preparation, HostVerificationReceipt as Postverification,
    PrintObservation, ProcessObservation, ServiceActionReceipt as CommandObservation,
    ServiceObservation,
};

#[cfg(test)]
use crate::picker_evidence::SecuritySession;
#[cfg(test)]
mod tests;

#[cfg(any(target_os = "macos", test))]
use crate::picker_evidence::{FaultFrame, FaultThread, Termination};

#[cfg(any(target_os = "macos", test))]
use crate::picker_evidence::QualificationBinding as Binding;
