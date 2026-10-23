//! Strict, data-only picker qualification. No receipt authorizes native mutation.
use crate::{
    Result,
    distribution::BuildPlan,
    host_incidents::{IncidentIdentity, IncidentRecord, Registry},
};
use serde::{
    Deserialize, Serialize,
    de::{self, MapAccess, Visitor},
};
use std::{collections::BTreeMap, fmt, path::Path, time::SystemTime};
use xtask::archive::ByteIdentity;
pub(crate) const MAX_RECEIPT_BYTES: usize = 16 * 1024 * 1024;

// Every record uses a map visitor: derived struct deserializers also accept JSON
// positional arrays, and derived Option fields silently accept missing facts.
macro_rules! record {
    ($name:ident { $($field:ident : $ty:ty $(=> $decoder:path)?),* $(,)? }) => {
        #[derive(Clone,Debug,PartialEq,serde::Serialize)]
        pub(crate) struct $name {$(pub(crate) $field:$ty),*}
        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D:serde::Deserializer<'de>>(deserializer:D)->std::result::Result<Self,D::Error> {
                struct Object;
                impl<'de> serde::de::Visitor<'de> for Object {
                    type Value=$name;
                    fn expecting(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.write_str(concat!(stringify!($name)," object with every required observation"))}
                    fn visit_map<A:serde::de::MapAccess<'de>>(self,mut map:A)->std::result::Result<Self::Value,A::Error> {
                        $(let mut $field:Option<$ty>=None;)*
                        while let Some(key)=map.next_key::<String>()? {
                            match key.as_str() {
                                $(name if name==stringify!($field).strip_prefix("r#").unwrap_or(stringify!($field)) => {
                                    if $field.is_some() {return Err(serde::de::Error::custom(format!("Duplicate field {name}")));}
                                    $field=Some(record!(@next map,$ty $(,$decoder)?));
                                },)*
                                _=>return Err(serde::de::Error::custom(format!("Unknown field {key} in {}",stringify!($name)))),
                            }
                        }
                        Ok($name {$($field:$field.ok_or_else(||serde::de::Error::missing_field(stringify!($field)))?),*})
                    }
                }
                deserializer.deserialize_map(Object)
            }
        }
    };
    (@next $map:ident,$ty:ty,$decoder:path)=>{$decoder(&mut $map)?};
    (@next $map:ident,$ty:ty)=>{$map.next_value::<$ty>()?};
}
record!(IdentityObject {
    byte_length: usize,
    sha256: String
});
fn identity<'de, A: MapAccess<'de>>(map: &mut A) -> std::result::Result<ByteIdentity, A::Error> {
    let value: IdentityObject = map.next_value()?;
    Ok(ByteIdentity {
        byte_length: value.byte_length,
        sha256: value.sha256,
    })
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub(crate) struct StringMap<T>(pub(crate) BTreeMap<String, T>);
impl<'de, T: Deserialize<'de>> Deserialize<'de> for StringMap<T> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Object<T>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for Object<T> {
            type Value = StringMap<T>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("object without duplicate keys")
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut result = BTreeMap::new();
                while let Some(key) = map.next_key::<String>()? {
                    if result.contains_key(&key) {
                        return Err(de::Error::custom("Duplicate map key"));
                    }
                    result.insert(key, map.next_value()?);
                }
                Ok(StringMap(result))
            }
        }
        d.deserialize_map(Object(std::marker::PhantomData))
    }
}
impl<T> std::ops::Deref for StringMap<T> {
    type Target = BTreeMap<String, T>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<T> std::ops::DerefMut for StringMap<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl<T> From<BTreeMap<String, T>> for StringMap<T> {
    fn from(value: BTreeMap<String, T>) -> Self {
        Self(value)
    }
}
record!(QualificationBinding {source:String,target:String,run:String,attempt:String,workflow_ref:String,workflow_sha:String,archive:ByteIdentity=>self::identity,reference:ByteIdentity=>self::identity});
record!(ProcessObservation {exit_code:Option<i32>,signal:Option<i32>,timed_out:bool,cancelled:bool,unexpected_survivors:usize,cleanup_complete:bool,elapsed_seconds:f64,cleanup_seconds:f64});
impl ProcessObservation {
    #[cfg(target_os = "macos")]
    pub(crate) fn from_process(process: &xtask::process::ProcessResult) -> Result<Self> {
        Ok(serde_json::from_value(process.evidence())?)
    }
    pub(crate) fn clean(&self) -> Result<i32> {
        if self.timed_out
            || self.cancelled
            || self.signal.is_some()
            || !self.cleanup_complete
            || self.unexpected_survivors != 0
            || !self.elapsed_seconds.is_finite()
            || !self.cleanup_seconds.is_finite()
            || self.elapsed_seconds < 0.0
            || self.cleanup_seconds < 0.0
        {
            return Err("Native process did not settle cleanly".into());
        }
        self.exit_code
            .filter(|code| *code >= 0)
            .ok_or_else(|| "Native process exit is unconfirmed".into())
    }
}
record!(Host {
    target: String,
    process_os: String,
    process_architecture: String,
    kernel_machine: String,
    image_os: String,
    image_version: String,
    product_version: String,
    build_version: String,
    boot_uuid: String
});
record!(SecuritySession {status:i32,security_session_id:Option<u32>,graphic_access:Option<bool>});
record!(Caller {
    uid: u32,
    euid: u32,
    console_uid: u32,
    security_session: SecuritySession
});
record!(DisabledOverrideObservation {program:String,arguments:Vec<String>,process:ProcessObservation,r#override:Option<bool>});
record!(NormalHostSnapshot {
    host: Host,
    caller: Caller,
    disabled_override: DisabledOverrideObservation
});
record!(ProbeFiles {info:ByteIdentity=>self::identity,executable:ByteIdentity=>self::identity,resources:ByteIdentity=>self::identity});
record!(ProbeIdentity {
    path: String,
    files: ProbeFiles,
    signature_display: String,
    scope: String
});
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PanelObservation {
    Healthy,
    KnownFailure,
}
record!(PanelReceipt {identity:ProbeIdentity,identity_after:ProbeIdentity,process:ProcessObservation,phases:Vec<String>,observation:PanelObservation});
record!(IncidentHostObject {
    image_os: String,
    image_version: String,
    product_version: String,
    build_version: String
});
fn incident_host<'de, A: MapAccess<'de>>(
    map: &mut A,
) -> std::result::Result<crate::host_incidents::IncidentHost, A::Error> {
    let h: IncidentHostObject = map.next_value()?;
    Ok(crate::host_incidents::IncidentHost {
        image_os: h.image_os,
        image_version: h.image_version,
        product_version: h.product_version,
        build_version: h.build_version,
    })
}
record!(IncidentIdentityObject {
    id: String,
    policy_sha256: String,
    source: String
});
fn incident_identity<'de, A: MapAccess<'de>>(
    map: &mut A,
) -> std::result::Result<IncidentIdentity, A::Error> {
    let i: IncidentIdentityObject = map.next_value()?;
    Ok(IncidentIdentity {
        id: i.id,
        policy_sha256: i.policy_sha256,
        source: i.source,
    })
}
record!(IncidentPolicyObject {id:String,host:crate::host_incidents::IncidentHost=>incident_host,architecture:String,classifier:String,service:String,upstream_issue:String,reviewed_at:u64,expires_at:u64});
fn incident_policy<'de, A: MapAccess<'de>>(
    map: &mut A,
) -> std::result::Result<IncidentRecord, A::Error> {
    let p: IncidentPolicyObject = map.next_value()?;
    Ok(IncidentRecord {
        id: p.id,
        host: p.host,
        architecture: p.architecture,
        classifier: p.classifier,
        service: p.service,
        upstream_issue: p.upstream_issue,
        reviewed_at: p.reviewed_at,
        expires_at: p.expires_at,
    })
}
record!(IncidentAdmissionReceipt {identity:IncidentIdentity=>incident_identity,policy:IncidentRecord=>incident_policy,admitted_at:u64,admitted_subsec_ns:u32,action_deadline_wall:u64});
record!(ActionGuardObservation {
    wall_at: u64,
    monotonic_elapsed_ns: u64
});
record!(ServiceActionReceipt {program:String,arguments:Vec<String>,process:ProcessObservation,stdout_path:String,stderr_path:String,guard:ActionGuardObservation});
record!(PrintObservation {program:String,arguments:Vec<String>,process:ProcessObservation,selected_fields:StringMap<String>,absence_diagnostic:Option<String>});
record!(ServiceObservation {
    target: String,
    print: PrintObservation,
    disabled: DisabledOverrideObservation
});
record!(FaultFrame {symbol:Option<String>,image_name:String,image_path:String});
record!(FaultThread {index:u32,triggered:bool,frames:Vec<FaultFrame>});
record!(Termination {
    namespace: String,
    code: u32
});
record!(CrashReport {file_name:String,identity:ByteIdentity=>self::identity,pid:u32,user_id:u32,boot_uuid:String,os_build:String,capture_time:String,proc_path:String,exception_signal:String,termination:Termination,faulting_thread:FaultThread});
record!(HostPreparationReceipt {stage:String,binding:QualificationBinding,picker_host:crate::host_incidents::IncidentHost=>incident_host,host:Host,caller:Caller,service_before:ServiceObservation,crash_reports:Vec<CrashReport>,bootout:ServiceActionReceipt,bootout_absence:Option<PrintObservation>,disable:ServiceActionReceipt,service_after:ServiceObservation});
record!(HostVerificationReceipt {stage:String,binding:QualificationBinding,picker_host:crate::host_incidents::IncidentHost=>incident_host,host:Host,caller:Caller,service:ServiceObservation});
record!(TestSummary {
    total: u32,
    passed: u32,
    failed: u32,
    skipped: u32,
    expected_failures: u32
});
record!(OwnershipReceipt {
    pid: u32,
    launch_date: f64,
    bundle_path: String,
    pasteboard: String
});
record!(LifetimeOwner {failure:Option<String>,source_check_failure:Option<String>,timed_out:bool,cancelled:bool,runner_reaped:bool,runner_status:Option<i32>,owned_app_terminated:bool,forced_app_cleanup:bool,originals_captured:u32,originals_unchanged:bool,owned_pasteboard_released:bool,unexpected_application_pids:Vec<u32>,ownership_receipt:OwnershipReceipt});
record!(PickerObservation {
    schema: u8,
    scenario: String,
    owned_pid: u32,
    message: String,
    folder: String
});
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum NativePreference {
    Data {
        base64: String,
    },
    Date {
        reference_seconds_bits: String,
    },
    Boolean {
        value: bool,
    },
    Real {
        representation: String,
        bits: String,
    },
    Integer {
        representation: String,
        value: String,
    },
    String {
        value: String,
    },
    Array {
        values: Vec<NativePreference>,
    },
    Dictionary {
        values: StringMap<NativePreference>,
    },
}
record!(PermittedPreferenceChange {
    key: String,
    before_present: bool,
    native_type: String
});
record!(PreferenceComparison {whole_domain_unchanged:bool,application_and_other_preferences_unchanged:bool,real_pickers_observed:bool,permitted_system_changes:Vec<PermittedPreferenceChange>});
record!(PersistentPreferences {fresh_process_before:StringMap<NativePreference>,fresh_process_after:StringMap<NativePreference>,comparison:PreferenceComparison,argument_only_controls:PreferenceComparison});
record!(PickerProductReceipt {stage:String,source:String,target:String,host:Host,snapshot:ByteIdentity=>self::identity,raw_product_identity:ByteIdentity=>self::identity,case:String,test_method:String,app:String,summary:TestSummary,process:ProcessObservation,lifetime_owner:LifetimeOwner,ownership_receipt:OwnershipReceipt,picker_observation:PickerObservation,persistent_preferences:PersistentPreferences});
record!(UnmodifiedHostReceipt {binding:QualificationBinding,before:NormalHostSnapshot,panel:PanelReceipt,product:PickerProductReceipt,after:NormalHostSnapshot,actions:Vec<ServiceActionReceipt>});
record!(IncidentModifiedHostReceipt {
    binding: QualificationBinding,
    failed_panel: PanelReceipt,
    admission: IncidentAdmissionReceipt,
    preparation: HostPreparationReceipt,
    recovered_panel: PanelReceipt,
    product: PickerProductReceipt,
    postverification: HostVerificationReceipt
});
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum PickerQualification {
    UnmodifiedHost(Box<UnmodifiedHostReceipt>),
    IncidentModifiedHost(Box<IncidentModifiedHostReceipt>),
}
record!(VerifiedPickerReceipt {receipt:PickerQualification,identity:ByteIdentity=>self::identity,document:String});
impl VerifiedPickerReceipt {
    pub(crate) fn identity(&self) -> &ByteIdentity {
        &self.identity
    }
}
const HEALTHY: &[&str] = &[
    "constructor entry",
    "constructor returned",
    "presentation returned",
    "cancel retained panel; visible=true",
    "callback response 0",
];
const FAILED: &[&str] = &[
    "constructor entry",
    "constructor returned",
    "presentation returned",
    "cancel retained panel; visible=false",
    "callback deadline exceeded",
];
fn require(ok: bool, message: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(message.into()) }
}
fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn byte_identity(id: &ByteIdentity) -> Result<()> {
    require(
        id.byte_length > 0 && hex(&id.sha256, 64),
        "Invalid exact byte identity",
    )
}
fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}
fn binding(value: &QualificationBinding, expected: &QualificationBinding) -> Result<()> {
    require(
        value == expected && hex(&value.source, 40) && hex(&value.workflow_sha, 40),
        "Picker source/workflow/archive/current-run binding differs",
    )?;
    for number in [&value.run, &value.attempt] {
        require(
            number.bytes().all(|b| b.is_ascii_digit())
                && number.parse::<u64>().is_ok_and(|n| n > 0),
            "Invalid qualification run/attempt",
        )?;
    }
    require(
        ["ci.yml", "release.yml"].iter().any(|file| {
            value
                .workflow_ref
                .strip_prefix(&format!(
                    "resoltico/EMLAttachmentRemover/.github/workflows/{file}@"
                ))
                .is_some_and(|suffix| !suffix.is_empty())
        }),
        "Unrecognized qualification workflow",
    )?;
    byte_identity(&value.archive)?;
    byte_identity(&value.reference)
}
fn host(value: &Host, plan: &BuildPlan, expected: &QualificationBinding) -> Result<()> {
    let target = plan.lookup_target(&expected.target)?;
    let picker = target
        .desktop()
        .and_then(|d| d.picker())
        .ok_or("Target has no picker qualification role")?;
    let arch = target.platform().architecture().as_str();
    require(
        target.platform().family().as_str() == "macos"
            && value.target == expected.target
            && value.process_os == "macos"
            && value.process_architecture == arch
            && value.kernel_machine == if arch == "aarch64" { "arm64" } else { arch }
            && value
                .product_version
                .split('.')
                .next()
                .and_then(|s| s.parse::<u32>().ok())
                == Some(picker.os_major())
            && !value.image_os.is_empty()
            && !value.image_version.is_empty()
            && !value.build_version.is_empty()
            && uuid(&value.boot_uuid),
        "Unsupported native picker OS/CPU/image/boot observation",
    )
}
fn caller(value: &Caller) -> Result<()> {
    require(
        value.uid > 0
            && value.uid == value.euid
            && value.uid == value.console_uid
            && value.security_session.status == 0
            && value.security_session.graphic_access == Some(true)
            && value
                .security_session
                .security_session_id
                .is_some_and(|n| n > 0 && n < u32::MAX),
        "Picker caller lacks observed non-root console graphical authority",
    )
}
fn command(
    program: &str,
    args: &[String],
    expected: &[String],
    process: &ProcessObservation,
) -> Result<i32> {
    require(
        program == "/bin/launchctl" && args == expected,
        "Service command differs from exact authorized operation",
    )?;
    process.clean()
}
fn disabled(value: &DisabledOverrideObservation, uid: u32, required: bool) -> Result<()> {
    require(
        command(
            &value.program,
            &value.arguments,
            &["print-disabled".into(), format!("gui/{uid}")],
            &value.process,
        )? == 0
            && if required {
                value.r#override == Some(true)
            } else {
                value.r#override != Some(true)
            },
        "Disabled override observation is missing, uncertain or disallowed",
    )
}
fn normal(
    value: &NormalHostSnapshot,
    plan: &BuildPlan,
    expected: &QualificationBinding,
) -> Result<()> {
    host(&value.host, plan, expected)?;
    caller(&value.caller)?;
    disabled(&value.disabled_override, value.caller.uid, false)
}
fn panel(value: &PanelReceipt, expected: PanelObservation) -> Result<()> {
    require(
        value.identity == value.identity_after && value.observation == expected,
        "Panel probe identity or classification changed",
    )?;
    for id in [
        &value.identity.files.info,
        &value.identity.files.executable,
        &value.identity.files.resources,
    ] {
        byte_identity(id)?;
    }
    require(
        value.identity.path.starts_with('/')
            && value.identity.path.ends_with("/NativePanelDiagnostic.app")
            && value
                .identity
                .signature_display
                .lines()
                .any(|line| line == "Signature=adhoc")
            && !value.identity.scope.is_empty(),
        "Panel probe lacks exact signed native identity",
    )?;
    let (exit, phases) = match expected {
        PanelObservation::Healthy => (0, HEALTHY),
        PanelObservation::KnownFailure => (2, FAILED),
    };
    require(
        value.process.clean()? == exit
            && value
                .phases
                .iter()
                .map(String::as_str)
                .eq(phases.iter().copied()),
        "Panel probe phases/process do not match exact classification",
    )
}
fn product(
    value: &PickerProductReceipt,
    plan: &BuildPlan,
    expected: &QualificationBinding,
    snapshot: &Host,
) -> Result<()> {
    // The native-case catalog owns production spellings; independent literal
    // fixtures reject redefining this product test into a readiness probe.
    require(
        value.stage == "picker_compatibility"
            && value.source == expected.source
            && value.target == expected.target
            && value.snapshot == expected.archive
            && value.case == crate::desktop_evidence::DesktopCase::Picker.name()
            && value.test_method == crate::desktop_evidence::DesktopCase::Picker.method(),
        "Product receipt is not the exact frozen actual-picker scenario",
    )?;
    host(&value.host, plan, expected)?;
    require(
        value.host == *snapshot,
        "Product host/boot differs from qualifying host",
    )?;
    byte_identity(&value.raw_product_identity)?;
    require(
        value.summary.total == 1
            && value.summary.passed == 1
            && value.summary.failed == 0
            && value.summary.skipped == 0
            && value.summary.expected_failures == 0
            && value.process.clean()? == 0,
        "Product test discovery/execution/settlement is incomplete",
    )?;
    let owner = &value.lifetime_owner;
    require(
        owner.failure.is_none()
            && owner.source_check_failure.is_none()
            && !owner.timed_out
            && !owner.cancelled
            && owner.runner_reaped
            && owner.runner_status == Some(0)
            && owner.owned_app_terminated
            && !owner.forced_app_cleanup
            && owner.originals_captured == 4
            && owner.originals_unchanged
            && owner.owned_pasteboard_released
            && owner.unexpected_application_pids.is_empty(),
        "Actual product lifetime/original-byte ownership did not settle normally",
    )?;
    let target = plan.lookup_target(&expected.target)?;
    let expected_bundle_root = plan
        .lookup_artifact(
            target.platform(),
            crate::distribution::ProductRole::DesktopApp,
        )?
        .root();
    let identity = &value.ownership_receipt;
    require(
        identity == &owner.ownership_receipt
            && identity.pid > 0
            && identity.launch_date.is_finite()
            && identity.launch_date > 0.0
            && identity.bundle_path == value.app
            && value.app.starts_with('/')
            && Path::new(&value.app).file_name()
                == Some(std::ffi::OsStr::new(expected_bundle_root))
            && !identity.pasteboard.is_empty()
            && identity.pasteboard != "Apple CFPasteboard general",
        "Actual product process ownership differs",
    )?;
    let picker = &value.picker_observation;
    require(
        picker.schema == 1
            && picker.scenario == "picker"
            && picker.owned_pid == identity.pid
            && !picker.message.is_empty()
            && !picker.folder.is_empty(),
        "Actual picker observation is missing or belongs to another process",
    )?;
    validate_preferences(&value.persistent_preferences)
}
pub(crate) fn validate_preferences(p: &PersistentPreferences) -> Result<()> {
    let before = &p.fresh_process_before.0;
    let after = &p.fresh_process_after.0;
    let mut actual = Vec::new();
    for key in before
        .keys()
        .chain(after.keys())
        .collect::<std::collections::BTreeSet<_>>()
    {
        if before.get(key) == after.get(key) {
            continue;
        }
        let ty = match key.as_str() {
            "NSNavPanelExpandedSizeForOpenMode"
            | "NSWindow Frame GoToSheet"
            | "NSWindow Frame NSNavPanelAutosaveName" => "string",
            "NSOSPLastRootDirectory" => "data",
            "NSNavLastUserSetHideExtensionButtonState" => "boolean",
            _ => return Err("Application or unrecognized persistent preference changed".into()),
        };
        let matches = |value: &NativePreference| {
            matches!(
                (ty, value),
                ("string", NativePreference::String { .. })
                    | ("data", NativePreference::Data { .. })
                    | ("boolean", NativePreference::Boolean { .. })
            )
        };
        require(
            after.get(key).is_some_and(matches) && before.get(key).is_none_or(matches),
            "Native picker preference change has an incorrect native type/presence",
        )?;
        actual.push(PermittedPreferenceChange {
            key: key.clone(),
            before_present: before.contains_key(key),
            native_type: ty.into(),
        });
    }
    require(
        p.comparison.real_pickers_observed
            && p.comparison.application_and_other_preferences_unchanged
            && p.comparison.whole_domain_unchanged == (before == after)
            && p.comparison.permitted_system_changes == actual,
        "Persistent preference comparison does not match complete before/after facts",
    )?;
    let controls = &p.argument_only_controls;
    require(
        !controls.real_pickers_observed
            && controls.application_and_other_preferences_unchanged
            && controls.whole_domain_unchanged
            && controls.permitted_system_changes.is_empty(),
        "Argument-only native ownership controls changed persistent preferences",
    )
}
pub(crate) fn validate_crash(value: &CrashReport) -> Result<()> {
    require(
        value.file_name.starts_with("iconservicesagent-")
            && value.file_name.ends_with(".ips")
            && value.file_name.len() <= 255
            && !value.file_name.contains(['/', '\\'])
            && value.pid > 0
            && value.identity.byte_length <= 8 * 1024 * 1024
            && value.faulting_thread.triggered
            && !value.faulting_thread.frames.is_empty()
            && value.faulting_thread.frames.len() <= 256,
        "Selected crash report structure is invalid",
    )?;
    byte_identity(&value.identity)?;
    for text in [
        &value.boot_uuid,
        &value.os_build,
        &value.capture_time,
        &value.proc_path,
        &value.exception_signal,
        &value.termination.namespace,
    ] {
        require(
            !text.is_empty() && text.len() <= 4096,
            "Crash observation text is invalid",
        )?;
    }
    for frame in &value.faulting_thread.frames {
        require(
            !frame.image_name.is_empty()
                && frame.image_name.len() <= 4096
                && !frame.image_path.is_empty()
                && frame.image_path.len() <= 4096
                && frame
                    .symbol
                    .as_ref()
                    .is_none_or(|v| !v.is_empty() && v.len() <= 4096),
            "Crash frame metadata is invalid",
        )?;
    }
    Ok(())
}
fn modified(
    root: &Path,
    plan: &BuildPlan,
    expected: &QualificationBinding,
    receipt: &IncidentModifiedHostReceipt,
    now: SystemTime,
) -> Result<()> {
    let IncidentModifiedHostReceipt {
        binding: _,
        failed_panel: failed,
        admission,
        preparation: prepared,
        recovered_panel: recovered,
        product: product_receipt,
        postverification: post,
    } = receipt;
    panel(failed, PanelObservation::KnownFailure)?;
    panel(recovered, PanelObservation::Healthy)?;
    require(
        failed.identity == recovered.identity,
        "Recovery used a different signed probe",
    )?;
    binding(&prepared.binding, expected)?;
    binding(&post.binding, expected)?;
    require(
        admission.identity.source == expected.source
            && admission.identity.id == admission.policy.id
            && prepared.stage == "picker_host_preparation"
            && post.stage == "picker_host_verification"
            && prepared.picker_host == admission.policy.host
            && post.picker_host == admission.policy.host
            && prepared.host == post.host
            && prepared.caller == post.caller,
        "Incident binding, host or caller changed",
    )?;
    require(
        prepared.service_before.disabled.r#override != Some(true),
        "Incident evidence started with a pre-disabled host",
    )?;
    host(&prepared.host, plan, expected)?;
    caller(&prepared.caller)?;
    let policy = &admission.policy;
    require(
        prepared.host.image_os == policy.host.image_os
            && prepared.host.image_version == policy.host.image_version
            && prepared.host.product_version == policy.host.product_version
            && prepared.host.build_version == policy.host.build_version
            && prepared.host.process_architecture == policy.architecture
            && admission.admitted_at >= policy.reviewed_at
            && admission.admitted_at < policy.expires_at
            && admission.action_deadline_wall
                == admission
                    .admitted_at
                    .checked_add(600)
                    .ok_or("Admission wall deadline overflow")?
                    .min(policy.expires_at),
        "Incident tuple/admission dates differ",
    )?;
    Registry::load(root)?.recheck(&admission.identity, policy, now)?;
    require(
        admission.admitted_subsec_ns < 1_000_000_000,
        "Invalid admission wall-clock fractional observation",
    )?;
    let budget_ns = policy
        .expires_at
        .checked_sub(admission.admitted_at)
        .ok_or("Incident admitted after expiry")?
        .checked_mul(1_000_000_000)
        .and_then(|ns| ns.checked_sub(u64::from(admission.admitted_subsec_ns)))
        .ok_or("Incident action duration overflow")?
        .min(600_000_000_000);
    for action in [&prepared.bootout, &prepared.disable] {
        require(
            action.guard.wall_at >= policy.reviewed_at
                && action.guard.wall_at < policy.expires_at
                && action.guard.monotonic_elapsed_ns < budget_ns,
            "An incident action lacked an unexpired wall/monotonic guard observation",
        )?;
    }
    require(
        prepared.disable.guard.monotonic_elapsed_ns >= prepared.bootout.guard.monotonic_elapsed_ns,
        "Incident action observations do not belong to one increasing monotonic clock",
    )?;
    crate::macos_picker_host::evidence::validate_preparation(prepared)?;
    crate::macos_picker_host::evidence::validate_post(prepared, post)?;
    product(product_receipt, plan, expected, &prepared.host)
}
fn validate(
    root: &Path,
    plan: &BuildPlan,
    receipt: &PickerQualification,
    expected: &QualificationBinding,
    now: SystemTime,
) -> Result<()> {
    match receipt {
        PickerQualification::UnmodifiedHost(receipt) => {
            let UnmodifiedHostReceipt {
                binding: actual,
                before,
                panel: probe,
                product: actual_product,
                after,
                actions,
            } = &**receipt;
            binding(actual, expected)?;
            normal(before, plan, expected)?;
            normal(after, plan, expected)?;
            require(
                before.host == after.host && before.caller == after.caller && actions.is_empty(),
                "Ordinary host/caller changed or service intervention was recorded",
            )?;
            panel(probe, PanelObservation::Healthy)?;
            product(actual_product, plan, expected, &before.host)
        }
        PickerQualification::IncidentModifiedHost(receipt) => {
            binding(&receipt.binding, expected)?;
            modified(root, plan, expected, receipt, now)
        }
    }
}

pub(crate) fn verify_receipt(
    root: &Path,
    plan: &BuildPlan,
    bytes: &[u8],
    expected: &QualificationBinding,
    now: SystemTime,
) -> Result<VerifiedPickerReceipt> {
    require(
        !bytes.is_empty() && bytes.len() <= MAX_RECEIPT_BYTES,
        "Picker receipt exceeds its bounded document size",
    )?;
    let receipt: PickerQualification = serde_json::from_slice(bytes)?;
    validate(root, plan, &receipt, expected, now)?;
    Ok(VerifiedPickerReceipt {
        receipt,
        identity: ByteIdentity::of(bytes),
        document: std::str::from_utf8(bytes)?.into(),
    })
}
pub(crate) fn recheck_release(
    root: &Path,
    plan: &BuildPlan,
    receipt: &VerifiedPickerReceipt,
    expected: &QualificationBinding,
    now: SystemTime,
) -> Result<()> {
    require(
        receipt.document.len() <= MAX_RECEIPT_BYTES
            && ByteIdentity::of(receipt.document.as_bytes()) == receipt.identity,
        "Retained picker receipt bytes changed",
    )?;
    let decoded: PickerQualification = serde_json::from_str(&receipt.document)?;
    require(
        decoded == receipt.receipt,
        "Retained picker document differs from its typed receipt",
    )?;
    validate(root, plan, &decoded, expected, now)
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn validate_snapshot(
    plan: &BuildPlan,
    binding: &QualificationBinding,
    value: &NormalHostSnapshot,
) -> Result<()> {
    normal(value, plan, binding)
}
#[cfg(any(target_os = "macos", test))]
pub(crate) fn validate_panel(value: &PanelReceipt, healthy: bool) -> Result<()> {
    panel(
        value,
        if healthy {
            PanelObservation::Healthy
        } else {
            PanelObservation::KnownFailure
        },
    )
}
// Full native result artifacts contain additional diagnostic observations. Keep
// that artifact intact, reject duplicate keys throughout it, and project only
// the explicitly typed qualification facts. New receipt decoding never uses it.
struct ObservationJson(serde_json::Value);
impl<'de> Deserialize<'de> for ObservationJson {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Json;
        impl<'de> Visitor<'de> for Json {
            type Value = ObservationJson;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("native product JSON without duplicate object fields")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<Self::Value, E> {
                Ok(ObservationJson(v.into()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Self::Value, E> {
                Ok(ObservationJson(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Self::Value, E> {
                Ok(ObservationJson(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<Self::Value, E> {
                Ok(ObservationJson(
                    serde_json::Number::from_f64(v)
                        .ok_or_else(|| E::custom("Nonfinite product number"))?
                        .into(),
                ))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Self::Value, E> {
                Ok(ObservationJson(v.into()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<Self::Value, E> {
                Ok(ObservationJson(v.into()))
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(ObservationJson(serde_json::Value::Null))
            }
            fn visit_seq<A: de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(ObservationJson(value)) = seq.next_element()? {
                    values.push(value);
                }
                Ok(ObservationJson(values.into()))
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(de::Error::custom(format!(
                            "Duplicate native product field {key}"
                        )));
                    }
                    let ObservationJson(value) = map.next_value()?;
                    values.insert(key, value);
                }
                Ok(ObservationJson(values.into()))
            }
        }
        d.deserialize_any(Json)
    }
}
// The producer projects already-executed native evidence, never arbitrary new
// receipt input. No default can replace an absent actual observation here.
pub(crate) fn observation_document(bytes: &[u8]) -> Result<serde_json::Value> {
    require(
        !bytes.is_empty() && bytes.len() <= MAX_RECEIPT_BYTES,
        "Native observation document exceeds its bound",
    )?;
    let ObservationJson(value) = serde_json::from_slice(bytes)?;
    require(
        value.is_object(),
        "Native observation document must be an object",
    )?;
    Ok(value)
}
fn observed_field<'a>(value: &'a serde_json::Value, name: &str) -> Result<&'a serde_json::Value> {
    value
        .get(name)
        .ok_or_else(|| format!("Missing actual product observation {name}").into())
}
fn observed_text<'a>(value: &'a serde_json::Value, name: &str) -> Result<&'a str> {
    observed_field(value, name)?
        .as_str()
        .ok_or_else(|| format!("Invalid actual product text {name}").into())
}
pub(crate) fn project_lifetime_owner(raw: &serde_json::Value) -> Result<LifetimeOwner> {
    use serde_json::json;
    let identity = observed_field(raw, "ownership_receipt")?;
    // Additional Settings ownership in appearance scenarios belongs to that
    // scenario's separate proof; these are the common actual app-owner facts.
    let app_identity = json!({"pid":observed_field(identity,"pid")?,"launch_date":observed_field(identity,"launch_date")?,"bundle_path":observed_field(identity,"bundle_path")?,"pasteboard":observed_field(identity,"pasteboard")?});
    Ok(serde_json::from_value(
        json!({"failure":observed_field(raw,"failure")?,"source_check_failure":observed_field(raw,"source_check_failure")?,"timed_out":observed_field(raw,"timed_out")?,"cancelled":observed_field(raw,"cancelled")?,"runner_reaped":observed_field(raw,"runner_reaped")?,"runner_status":observed_field(raw,"runner_status")?,"owned_app_terminated":observed_field(raw,"owned_app_terminated")?,"forced_app_cleanup":observed_field(raw,"forced_app_cleanup")?,"originals_captured":observed_field(raw,"originals_captured")?,"originals_unchanged":observed_field(raw,"originals_unchanged")?,"owned_pasteboard_released":observed_field(raw,"owned_pasteboard_released")?,"unexpected_application_pids":observed_field(raw,"unexpected_application_pids")?,"ownership_receipt":app_identity}),
    )?)
}
pub(crate) fn project_product(
    raw_bytes: &[u8],
    binding: &QualificationBinding,
    native: &Host,
) -> Result<PickerProductReceipt> {
    use serde_json::json;
    require(
        !raw_bytes.is_empty() && raw_bytes.len() <= MAX_RECEIPT_BYTES,
        "Raw product receipt exceeds bound",
    )?;
    let raw = observation_document(raw_bytes)?;
    let observed_host = observed_field(&raw, "host")?;
    for (name, expected) in [
        ("target", native.target.as_str()),
        ("process_os", native.process_os.as_str()),
        ("process_architecture", native.process_architecture.as_str()),
        ("kernel_machine", native.kernel_machine.as_str()),
        ("product_version", native.product_version.as_str()),
        ("runner_image", native.image_os.as_str()),
        ("runner_image_version", native.image_version.as_str()),
    ] {
        require(
            observed_text(observed_host, name)? == expected,
            "Actual product native host differs from bracketing snapshot",
        )?;
    }
    let os = observed_text(observed_host, "actual_os")?;
    for (name, expected) in [
        ("ProductVersion:", native.product_version.as_str()),
        ("BuildVersion:", native.build_version.as_str()),
    ] {
        let values: Vec<_> = os
            .lines()
            .filter_map(|line| line.strip_prefix(name).map(str::trim))
            .collect();
        require(
            values == [expected],
            "Actual product OS/build output is missing, ambiguous or changed",
        )?;
    }
    let verification = observed_field(&raw, "verification")?;
    let picker = observed_field(verification, "picker")?;
    require(
        observed_field(picker, "source_archive")? == &serde_json::to_value(&binding.archive)?,
        "Actual picker case did not execute the bound frozen source archive",
    )?;
    let summary = observed_field(picker, "summary")?;
    let owner = observed_field(picker, "lifetime_owner")?;
    let preferences = observed_field(picker, "persistent_preferences")?;
    let selected_owner = project_lifetime_owner(owner)?;
    let product: PickerProductReceipt = serde_json::from_value(
        json!({"stage":observed_field(&raw,"stage")?,"source":observed_field(&raw,"source")?,"target":observed_field(&raw,"target")?,"host":native,"snapshot":observed_field(verification,"snapshot")?,"raw_product_identity":ByteIdentity::of(raw_bytes),"case":observed_field(picker,"case")?,"test_method":observed_field(picker,"test_method")?,"app":observed_field(picker,"app")?,"summary":{"total":observed_field(summary,"totalTestCount")?,"passed":observed_field(summary,"passedTests")?,"failed":observed_field(summary,"failedTests")?,"skipped":observed_field(summary,"skippedTests")?,"expected_failures":observed_field(summary,"expectedFailures")?},"process":observed_field(picker,"process")?,"lifetime_owner":selected_owner,"ownership_receipt":observed_field(picker,"ownership_receipt")?,"picker_observation":observed_field(picker,"picker_observation")?,"persistent_preferences":preferences}),
    )?;
    require(
        observed_text(verification, "stage")? == "picker_compatibility"
            && observed_text(picker, "source")? == binding.source,
        "Actual product inner stage/source differs",
    )?;
    // Full supported-role validation runs with the loaded plan at final admission.
    require(
        product.source == binding.source
            && product.target == binding.target
            && product.snapshot == binding.archive,
        "Projected product binding differs",
    )?;
    Ok(product)
}
impl VerifiedPickerReceipt {
    pub(crate) fn check_raw_product(
        &self,
        raw_bytes: &[u8],
        expected: &QualificationBinding,
    ) -> Result<()> {
        let (stored, native, actual_binding) = match &self.receipt {
            PickerQualification::UnmodifiedHost(receipt) => {
                (&receipt.product, &receipt.before.host, &receipt.binding)
            }
            PickerQualification::IncidentModifiedHost(receipt) => (
                &receipt.product,
                &receipt.preparation.host,
                &receipt.binding,
            ),
        };
        binding(actual_binding, expected)?;
        require(
            project_product(raw_bytes, expected, native)? == *stored,
            "Retained raw product observations differ from the verified projection",
        )
    }
    pub(crate) fn raw_product_identity(&self) -> &ByteIdentity {
        match &self.receipt {
            PickerQualification::UnmodifiedHost(receipt) => &receipt.product.raw_product_identity,
            PickerQualification::IncidentModifiedHost(receipt) => {
                &receipt.product.raw_product_identity
            }
        }
    }
    pub(crate) fn mode(&self) -> &'static str {
        match self.receipt {
            PickerQualification::UnmodifiedHost(..) => "unmodified_host",
            PickerQualification::IncidentModifiedHost(..) => "incident_modified_host",
        }
    }
}

#[cfg(test)]
mod tests;
