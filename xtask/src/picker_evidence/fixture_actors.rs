//! Closed, data-only observations of the native fixture owner's finite actors.
use super::{FixtureFilesystem, Result};
use base64::Engine as _;
use xtask::archive::ByteIdentity;

pub(crate) const MAX_OWNER_BYTES: usize = 1024 * 1024;
const MAX_ACK_BYTES: usize = 16 * 1024;
record!(ActorIdentity {
    device: i64,
    inode: u64,
    mode: u32,
    owner_uid: u32
});
record!(ActorBytes {
    byte_length: u64,
    sha256: String
});
record!(ActorError {domain:String,code:i64,native_errno:Option<i32>,message:String});
record!(ActorOutput {relative_path:String,kind:String,identity:ActorIdentity,bytes:Option<ActorBytes>,symlink_target:Option<String>});
record!(ActorFile {
    identity: ActorIdentity,
    bytes: ActorBytes
});
record!(ActorSource {
    relative_path: String,
    before: ActorFile,
    after: ActorFile
});
record!(ActorWorker {pid:i32,operation_id:u64,operation:String,phase:String,control_identity:ActorIdentity,pid_absent:Option<bool>,absence_errno:Option<i32>});
record!(ActorPending {state:String,relative_path:Option<String>,identity:Option<ActorIdentity>,error:Option<ActorError>});
record!(ActorAck {schema:u32,sequence:u32,actor:String,scenario:String,request_sha256:String,generation_before:u32,generation_after:u32,state:String,phase:String,effect:String,root:ActorIdentity,root_address_verified:bool,outputs:Vec<ActorOutput>,source:Option<ActorSource>,worker:Option<ActorWorker>,pending_cleanup:ActorPending,error:Option<ActorError>});
record!(ActorRecord {document:String,identity:ActorBytes,ack_published:bool,ack_pending_cleanup:ActorPending,ack_error:Option<ActorError>});
record!(UnexecutedActor {
    sequence: u32,
    actor: String,
    request_sha256: String,
    reason: String
});
record!(FixtureActors {state:String,scenario:String,actor_uid:u32,root:Option<ActorIdentity>,root_address_verified:Option<bool>,descriptor_closed:Option<bool>,generation:u32,records:Vec<ActorRecord>,unexecuted:Vec<UnexecutedActor>,failure:Option<ActorError>});

fn require(ok: bool, message: &'static str) -> Result<()> {
    if ok { Ok(()) } else { Err(message.into()) }
}
fn hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl ActorBytes {
    fn matches(&self, bytes: &[u8]) -> bool {
        let actual = ByteIdentity::of(bytes);
        self.byte_length == actual.byte_length as u64 && self.sha256 == actual.sha256
    }
    fn valid(&self) -> bool {
        hash(&self.sha256)
    }
}
impl ActorIdentity {
    fn same_object(&self, other: &Self) -> bool {
        self.device == other.device && self.inode == other.inode
    }
    fn kind(&self, root: &Self, kind: u32) -> bool {
        self.device == root.device
            && self.inode > 0
            && self.owner_uid == root.owner_uid
            && self.mode & 0o170000 == kind
            && (kind == 0o040000 || !self.same_object(root))
    }
}
const PUBLICATION: &[&str] = &[
    "retain-independent-report",
    "occupy-export",
    "create-export-directory",
    "create-export-symlink",
    "arm-operation",
    "race-export-winner",
    "release-operation",
    "archive-control",
    "arm-operation",
    "release-operation",
    "archive-control",
    "arm-operation",
    "release-operation",
    "archive-control",
    "arm-operation",
    "retain-child-observation",
];
fn sequence(scenario: &str) -> Result<&'static [&'static str]> {
    match scenario {
        "workflow" => Ok(&["occupy-checked-destination"]),
        "export-publication" => Ok(PUBLICATION),
        "export-writer-block"
        | "blocked-processing-preflight"
        | "blocked-processing-readback"
        | "blocked-processing-stage-write"
        | "blocked-processing-file-sync"
        | "blocked-processing-publication"
        | "blocked-processing-namespace-sync"
        | "blocked-export-preflight"
        | "blocked-export-readback"
        | "blocked-export-stage-write"
        | "blocked-export-file-sync"
        | "blocked-export-publication"
        | "blocked-export-namespace-sync" => Ok(&["arm-operation", "retain-child-observation"]),
        "picker" | "pagination" | "appearance" | "stop" | "force-stop" | "active-quit"
        | "published-error" | "owner-control" => Ok(&[]),
        _ => Err("Unknown fixture actor scenario".into()),
    }
}
fn pending_name(path: &str) -> bool {
    let Some(uuid) = path.strip_prefix("fixture-pending-") else {
        return false;
    };
    uuid.len() == 36
        && uuid.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'A'..=b'F').contains(&b)
            }
        })
}
fn recipe(scenario: &str, generation: u32) -> Result<String> {
    if scenario == "export-publication" {
        return [
            "export:publication",
            "export:publication",
            "export:namespace-sync",
            "export:after-publication",
        ]
        .get(generation as usize)
        .map(|s| (*s).to_owned())
        .ok_or_else(|| "Unknown fixture generation".into());
    }
    if scenario == "blocked-export-preflight" {
        return Ok("dual-quit".into());
    }
    if scenario == "export-writer-block" {
        return Ok("export:request-body".into());
    }
    if let Some(phase) = scenario.strip_prefix("blocked-processing-") {
        return Ok(format!("processing:{phase}:0"));
    }
    if let Some(phase) = scenario.strip_prefix("blocked-export-") {
        return Ok(format!("export:{phase}"));
    }
    Err("Fixture recipe belongs to another scenario".into())
}
fn check_request(ack: &ActorAck, payload: Option<&[u8]>) -> Result<()> {
    let operand = match ack.actor.as_str() {
        "occupy-checked-destination"
        | "occupy-export"
        | "create-export-directory"
        | "create-export-symlink" => "-".to_owned(),
        "arm-operation" => recipe(&ack.scenario, ack.generation_before)?,
        "release-operation" | "race-export-winner" => ack
            .worker
            .as_ref()
            .ok_or("Missing fixture operation")?
            .operation_id
            .to_string(),
        "archive-control" => format!(
            "{}:{}",
            ["racing-winner", "pre-edge-stop", "post-edge-stop"]
                .get(ack.generation_before as usize)
                .ok_or("Unknown fixture archive")?,
            ack.worker
                .as_ref()
                .ok_or("Missing archive operation")?
                .operation_id
        ),
        "retain-child-observation" | "retain-independent-report" => {
            let Some(bytes) = payload else {
                return Ok(());
            };
            require(
                !bytes.is_empty() && bytes.len() <= 65536,
                "Fixture request payload exceeds its bound",
            )?;
            base64::engine::general_purpose::STANDARD.encode(bytes)
        }
        _ => return Err("Unknown fixture request actor".into()),
    };
    let line = format!(
        "EML_FIXTURE_ACTOR {} {} {operand}\n",
        ack.sequence, ack.actor
    );
    require(
        ack.request_sha256 == ByteIdentity::of(line.as_bytes()).sha256,
        "Fixture request hash differs from its exact finite frame",
    )
}
#[derive(Default)]
struct ObservedGeneration {
    worker: Option<(u64, i32)>,
    control: Option<ActorIdentity>,
    previous_operations: std::collections::BTreeSet<u64>,
}

impl FixtureActors {
    pub(crate) fn check(&self, filesystem: &FixtureFilesystem, scenario: &str) -> Result<()> {
        let expected = sequence(scenario)?;
        let root = self
            .root
            .as_ref()
            .ok_or("Missing fixture actor root identity")?;
        let observed = filesystem
            .root
            .as_ref()
            .ok_or("Missing prelaunch fixture root")?;
        require(
            self.scenario == scenario
                && self.failure.is_none()
                && self.unexecuted.is_empty()
                && self.records.len() == expected.len()
                && self.root_address_verified == Some(true)
                && self.descriptor_closed == Some(true)
                && root.owner_uid == self.actor_uid
                && root.device == i64::from(observed.device)
                && root.inode == observed.inode
                && root.mode == u32::from(observed.mode)
                && root.mode & 0o777 == 0o700
                && root.kind(root, 0o040000),
            "Fixture actor root, admission or settlement is unverified",
        )?;
        require(
            self.state
                == if expected.is_empty() {
                    "unused"
                } else {
                    "complete"
                },
            "Fixture actors did not complete the required case sequence",
        )?;
        let mut generation = 0;
        let mut binding = ObservedGeneration::default();
        let mut seen_requests = std::collections::BTreeSet::new();
        for (index, record) in self.records.iter().enumerate() {
            require(
                !record.document.is_empty()
                    && record.document.len() <= MAX_ACK_BYTES
                    && record.identity.matches(record.document.as_bytes())
                    && record.ack_published
                    && record.ack_error.is_none(),
                "Fixture actor acknowledgement identity or publication failed",
            )?;
            let ack: ActorAck = serde_json::from_str(&record.document)?;
            require(
                record.ack_pending_cleanup.state == "complete"
                    && record.ack_pending_cleanup.error.is_none()
                    && record
                        .ack_pending_cleanup
                        .identity
                        .as_ref()
                        .is_some_and(|id| id.kind(root, 0o100000) && id.mode & 0o777 == 0o600)
                    && record
                        .ack_pending_cleanup
                        .relative_path
                        .as_deref()
                        .is_some_and(pending_name),
                "Fixture ACK temporary cleanup is unverified",
            )?;
            check_request(&ack, None)?;
            require(
                ack.schema == 1
                    && ack.sequence as usize == index + 1
                    && ack.actor == expected[index]
                    && ack.scenario == scenario
                    && hash(&ack.request_sha256)
                    && ack.generation_before == generation
                    && ack.generation_after <= 3
                    && ack.state == "succeeded"
                    && ack.phase == "complete"
                    && ack.error.is_none()
                    && ack.root == *root
                    && ack.root_address_verified
                    && ack.outputs.len() <= 4,
                "Fixture actor acknowledgement contradicts its case or native root",
            )?;
            require(
                seen_requests.insert(ack.request_sha256.clone()),
                "Fixture actor request was replayed",
            )?;
            let archive = ack.actor == "archive-control";
            require(
                ack.generation_after == generation + u32::from(archive),
                "Fixture actor generation was changed by another action",
            )?;
            check_effect(&ack, filesystem, root, generation, &mut binding)?;
            generation = ack.generation_after;
        }
        require(
            self.generation == generation,
            "Fixture actor final generation is stale",
        )
    }
}

fn check_effect(
    ack: &ActorAck,
    filesystem: &FixtureFilesystem,
    root: &ActorIdentity,
    generation: u32,
    binding: &mut ObservedGeneration,
) -> Result<()> {
    require(
        ack.pending_cleanup.error.is_none(),
        "Fixture actor pending cleanup failed",
    )?;
    match ack.pending_cleanup.state.as_str() {
        "not_needed" => require(
            ack.pending_cleanup.relative_path.is_none() && ack.pending_cleanup.identity.is_none(),
            "Unattempted cleanup claims an owned entry",
        )?,
        "complete" => require(
            ack.pending_cleanup
                .relative_path
                .as_deref()
                .is_some_and(pending_name)
                && ack
                    .pending_cleanup
                    .identity
                    .as_ref()
                    .is_some_and(|id| id.kind(root, 0o100000)),
            "Completed fixture cleanup lacks its exact temporary name and identity",
        )?,
        _ => return Err("Fixture actor cleanup is unverified".into()),
    }
    for output in &ack.outputs {
        let kind = match output.kind.as_str() {
            "file" => 0o100000,
            "directory" => 0o040000,
            "symlink" => 0o120000,
            _ => return Err("Unknown fixture actor output kind".into()),
        };
        require(
            output.identity.kind(root, kind) && !output.identity.same_object(root),
            "Fixture actor output lost native ownership or type",
        )?;
        require(
            output.bytes.as_ref().is_none_or(ActorBytes::valid),
            "Malformed fixture byte identity",
        )?;
    }
    if let Some(source) = &ack.source {
        require(
            ack.actor == "occupy-checked-destination"
                && source.relative_path == "two.eml"
                && source.before == source.after
                && source.before.identity.kind(root, 0o100000)
                && ack
                    .outputs
                    .iter()
                    .all(|output| !output.identity.same_object(&source.before.identity))
                && source.before.bytes.matches(include_bytes!(
                    "../../../tests/contract/mime/inputs/01-gmail-attachment.eml"
                )),
            "Fixture source identity or independent bytes changed",
        )?;
    }
    require(
        (ack.actor == "occupy-checked-destination") == ack.source.is_some(),
        "Fixture actor omitted or invented a source observation",
    )?;
    if let Some(worker) = &ack.worker {
        require(
            worker.pid > 0
                && worker.operation_id > 0
                && worker.operation == "export"
                && worker.control_identity.kind(root, 0o040000)
                && !worker.control_identity.same_object(root)
                && worker.phase
                    == if generation < 2 {
                        "publication"
                    } else {
                        "namespace-sync"
                    },
            "Fixture actor worker does not match its armed generation",
        )?;
        if let Some((operation, pid)) = binding.worker {
            require(
                operation == worker.operation_id && pid == worker.pid,
                "Fixture actor worker identity changed within one generation",
            )?;
        } else {
            require(
                binding.previous_operations.insert(worker.operation_id),
                "Fixture actor reused an earlier operation ID",
            )?;
        }
        if let Some(previous) = binding.control.as_ref() {
            require(
                previous == &worker.control_identity,
                "Fixture actor control generation changed",
            )?;
        }
        binding.control = Some(worker.control_identity.clone());
        binding.worker = Some((worker.operation_id, worker.pid));
        if ack.actor == "archive-control" {
            require(
                worker.pid_absent == Some(true) && worker.absence_errno == Some(3),
                "Fixture control archive lacks actual read-only ESRCH absence",
            )?;
            binding.worker = None;
        } else {
            require(
                worker.pid_absent.is_none() && worker.absence_errno.is_none(),
                "Unqueried fixture worker absence was synthesized",
            )?;
        }
    }
    require(
        matches!(
            ack.actor.as_str(),
            "race-export-winner" | "release-operation" | "archive-control"
        ) == ack.worker.is_some(),
        "Fixture actor omitted or invented a worker observation",
    )?;
    check_outputs(ack, filesystem, generation, &mut binding.control)
}

fn check_outputs(
    ack: &ActorAck,
    filesystem: &FixtureFilesystem,
    generation: u32,
    control: &mut Option<ActorIdentity>,
) -> Result<()> {
    let (path, kind, bytes): (&str, &str, Option<Vec<u8>>) = match ack.actor.as_str() {
        "occupy-checked-destination" => (
            "chosen-destination/two.ctpjzw7urreuwuroqgjcbuwt3m.no-attachments.eml",
            "file",
            Some(b"Owned entry created after Check; never replace.\n".to_vec()),
        ),
        "occupy-export" => (
            "occupied-report.json",
            "file",
            Some(b"Independent report occupancy sentinel\n".to_vec()),
        ),
        "race-export-winner" => (
            "racing-report.json",
            "file",
            Some(b"Independent report occupancy sentinel\n".to_vec()),
        ),
        "create-export-directory" => ("directory-report.json", "directory", None),
        "create-export-symlink" => ("symlink-report.json", "symlink", None),
        "retain-independent-report" => ("independent-report.json", "file", None),
        "retain-child-observation" => ("child-ui-observation.json", "file", None),
        "release-operation" => (
            "operation-control/release",
            "file",
            Some(b"release\n".to_vec()),
        ),
        "arm-operation" => {
            let recipe = recipe(&ack.scenario, generation)?;
            (
                "operation-control/observation",
                "file",
                Some(format!("{recipe}\n").into_bytes()),
            )
        }
        "archive-control" => {
            require(
                generation < 3 && ack.effect == "archived" && ack.outputs.len() == 2,
                "Fixture archive generation or effect differs",
            )?;
            let old = &ack.outputs[0];
            let new = &ack.outputs[1];
            require(
                old.relative_path
                    == format!(
                        "control-history/{}",
                        ["racing-winner", "pre-edge-stop", "post-edge-stop"][generation as usize]
                    )
                    && new.relative_path == "operation-control"
                    && old.kind == "directory"
                    && new.kind == "directory"
                    && old.bytes.is_none()
                    && new.bytes.is_none()
                    && old.symlink_target.is_none()
                    && new.symlink_target.is_none()
                    && !old.identity.same_object(&new.identity)
                    && control.as_ref() == Some(&old.identity)
                    && old.identity.mode & 0o777 == 0o700
                    && new.identity.mode & 0o777 == 0o700
                    && ack.pending_cleanup.state == "not_needed",
                "Fixture archive lost its exact old/new directory binding",
            )?;
            *control = Some(new.identity.clone());
            return Ok(());
        }
        _ => return Err("Unknown fixture actor".into()),
    };
    require(
        ack.outputs.len() == 1,
        "Fixture actor output inventory differs",
    )?;
    let output = &ack.outputs[0];
    require(
        output.relative_path == path && output.kind == kind,
        "Fixture actor wrote another path or object kind",
    )?;
    require(
        ack.effect
            == if ack.actor == "release-operation" {
                "released"
            } else {
                "created"
            },
        "Fixture actor effect contradicts its action",
    )?;
    if kind == "file" {
        let actual = output
            .bytes
            .as_ref()
            .ok_or("Fixture file has no byte identity")?;
        require(
            output.identity.mode & 0o777 == 0o600
                && output.symlink_target.is_none()
                && actual.byte_length > 0
                && actual.byte_length <= 65536,
            "Fixture file modes, bytes or symlink facts contradict",
        )?;
        if let Some(bytes) = bytes {
            require(
                actual.matches(&bytes),
                "Fixture bytes differ from the independent literal oracle",
            )?;
        }
        require(
            ack.pending_cleanup.state == "complete"
                && ack.pending_cleanup.identity.as_ref() == Some(&output.identity),
            "Fixture publication lost its completed pending identity",
        )?;
    } else {
        require(
            output.bytes.is_none() && ack.pending_cleanup.state == "not_needed",
            "Non-file fixture invented byte or publication observations",
        )?;
        if kind == "symlink" {
            require(
                output.symlink_target.as_deref()
                    == Some(format!("{}/occupied-report.json", filesystem.fixture_path).as_str()),
                "Fixture symlink target differs from the fixed owned target",
            )?;
        } else {
            require(
                output.identity.mode & 0o777 == 0o700 && output.symlink_target.is_none(),
                "Fixture directory modes or target differ",
            )?;
        }
    }
    Ok(())
}

impl FixtureActors {
    pub(crate) fn require_output(&self, actor: &str, identity: &ByteIdentity) -> Result<()> {
        let mut found = None;
        for record in &self.records {
            let ack: ActorAck = serde_json::from_str(&record.document)?;
            if ack.actor == actor {
                require(found.is_none(), "Repeated fixture payload actor")?;
                found = ack
                    .outputs
                    .first()
                    .and_then(|output| output.bytes.as_ref())
                    .cloned();
            }
        }
        require(
            found.is_some_and(|observed| {
                observed.byte_length == identity.byte_length as u64
                    && observed.sha256 == identity.sha256
            }),
            "Fixture payload does not match the independently retained artifact",
        )
    }
}

impl FixtureActors {
    pub(crate) fn check_payloads(&self, actor: &serde_json::Value) -> Result<()> {
        let expected = sequence(&self.scenario)?;
        if expected.contains(&"retain-child-observation") {
            let ui = actor["ui_receipt"]["contents"]
                .as_str()
                .ok_or("Missing actual fixture UI bytes")?;
            self.require_payload("retain-child-observation", ui.as_bytes())?;
        }
        if expected.contains(&"retain-independent-report") {
            let contents = actor["matrix"]["expected_document_contents"]
                .as_str()
                .ok_or("Missing independent report fixture bytes")?;
            let observed: ActorBytes =
                serde_json::from_value(actor["matrix"]["expected_document"].clone())?;
            require(
                observed.matches(contents.as_bytes()),
                "Independent report bytes differ from their retained identity",
            )?;
            self.require_payload("retain-independent-report", contents.as_bytes())?;
            let controls = actor["matrix"]["controls"]
                .as_array()
                .ok_or("Missing actual archived worker controls")?;
            let archives = self
                .records
                .iter()
                .map(|record| serde_json::from_str::<ActorAck>(&record.document))
                .collect::<std::result::Result<Vec<_>, _>>()?
                .into_iter()
                .filter(|ack| ack.actor == "archive-control")
                .collect::<Vec<_>>();
            require(
                controls.len() == 3 && archives.len() == 3,
                "Fixture archive and actual worker control counts differ",
            )?;
            for (index, (ack, observed)) in archives.iter().zip(controls).enumerate() {
                let worker = ack
                    .worker
                    .as_ref()
                    .ok_or("Missing archived worker observation")?;
                require(
                    ack.generation_before as usize == index
                        && observed["case"]
                            == ["racing-winner", "pre-edge-stop", "post-edge-stop"][index]
                        && observed["operation_id"].as_u64() == Some(worker.operation_id)
                        && observed["worker_pid"].as_i64() == Some(i64::from(worker.pid))
                        && observed["phase"] == worker.phase
                        && observed["worker_absent"] == true
                        && worker.pid_absent == Some(true),
                    "Fixture archive contradicts the independently read worker matrix",
                )?;
            }
        }
        Ok(())
    }
    fn require_payload(&self, kind: &str, bytes: &[u8]) -> Result<()> {
        self.require_output(kind, &ByteIdentity::of(bytes))?;
        for record in &self.records {
            let ack: ActorAck = serde_json::from_str(&record.document)?;
            if ack.actor == kind {
                check_request(&ack, Some(bytes))?;
            }
        }
        Ok(())
    }
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn owner_document(bytes: &[u8]) -> Result<serde_json::Value> {
    require(
        !bytes.is_empty() && bytes.len() <= MAX_OWNER_BYTES,
        "Desktop lifetime observation exceeds its bound",
    )?;
    super::observation_document(bytes)
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn check_fixture_actors(
    value: &serde_json::Value,
    filesystem: &serde_json::Value,
    scenario: &str,
) -> Result<()> {
    let facts: FixtureActors = serde_json::from_value(value.clone())?;
    facts.check(&serde_json::from_value(filesystem.clone())?, scenario)
}

#[cfg(test)]
pub(crate) fn test_value(scenario: &str) -> serde_json::Value {
    use serde_json::{Value, json};
    let actors: Vec<&str> = match scenario {
        "workflow" => vec!["occupy-checked-destination"],
        "export-publication" => vec![
            "retain-independent-report",
            "occupy-export",
            "create-export-directory",
            "create-export-symlink",
            "arm-operation",
            "race-export-winner",
            "release-operation",
            "archive-control",
            "arm-operation",
            "release-operation",
            "archive-control",
            "arm-operation",
            "release-operation",
            "archive-control",
            "arm-operation",
            "retain-child-observation",
        ],
        name if name.starts_with("blocked-") || name == "export-writer-block" => {
            vec!["arm-operation", "retain-child-observation"]
        }
        _ => Vec::new(),
    };
    let native = |inode, mode| json!({"device":7,"inode":inode,"mode":mode,"owner_uid":501});
    let root = native(10, 0o40700);
    let mut records = Vec::new();
    let mut generation = 0;
    for (index, actor) in actors.iter().enumerate() {
        let seq = index + 1;
        let mut after = generation;
        let mut worker = Value::Null;
        let mut source = Value::Null;
        let mut pending =
            json!({"state":"not_needed","relative_path":null,"identity":null,"error":null});
        let mut outputs = Vec::new();
        let mut operand = "-".to_owned();
        let mut file: Option<(&str, Vec<u8>)> = None;
        let mut effect = "created";
        match *actor {
            "occupy-checked-destination" => {
                file = Some(("chosen-destination/two.ctpjzw7urreuwuroqgjcbuwt3m.no-attachments.eml", b"Owned entry created after Check; never replace.\n".to_vec()));
                let fact = json!({"identity":native(99,0o100644),"bytes":ByteIdentity::of(include_bytes!("../../../tests/contract/mime/inputs/01-gmail-attachment.eml"))});
                source = json!({"relative_path":"two.eml","before":fact,"after":fact});
            }
            "occupy-export" => file=Some(("occupied-report.json", b"Independent report occupancy sentinel\n".to_vec())),
            "race-export-winner" => file=Some(("racing-report.json", b"Independent report occupancy sentinel\n".to_vec())),
            "retain-independent-report" => file=Some(("independent-report.json", b"abc".to_vec())),
            "retain-child-observation" => file=Some(("child-ui-observation.json", b"independent observation bytes".to_vec())),
            "release-operation" => { operand=(90+generation).to_string(); effect="released"; file=Some(("operation-control/release",b"release\n".to_vec())); }
            "arm-operation" => {
                let recipe = if scenario=="export-publication" { ["export:publication","export:publication","export:namespace-sync","export:after-publication"][generation] .to_owned() }
                else if scenario=="blocked-export-preflight" {"dual-quit".into()}
                else if scenario=="export-writer-block" {"export:request-body".into()}
                else if let Some(phase)=scenario.strip_prefix("blocked-processing-"){format!("processing:{phase}:0")}
                else {format!("export:{}",scenario.strip_prefix("blocked-export-").unwrap())};
                file=Some(("operation-control/observation",format!("{recipe}\n").into_bytes()));
            }
            "create-export-directory" => outputs.push(json!({"relative_path":"directory-report.json","kind":"directory","identity":native(20+seq,0o40700),"bytes":null,"symlink_target":null})),
            "create-export-symlink" => outputs.push(json!({"relative_path":"symlink-report.json","kind":"symlink","identity":native(20+seq,0o120777),"bytes":null,"symlink_target":"/owned/fixture/occupied-report.json"})),
            "archive-control" => {
                operand=format!("{}:{}",["racing-winner","pre-edge-stop","post-edge-stop"][generation],90+generation);
                effect="archived";after+=1;
                outputs.push(json!({"relative_path":format!("control-history/{}",["racing-winner","pre-edge-stop","post-edge-stop"][generation]),"kind":"directory","identity":native(100+generation,0o40700),"bytes":null,"symlink_target":null}));
                outputs.push(json!({"relative_path":"operation-control","kind":"directory","identity":native(101+generation,0o40700),"bytes":null,"symlink_target":null}));
            }
            _ => unreachable!(),
        }
        if let Some((path, bytes)) = file {
            if *actor == "arm-operation" {
                operand = String::from_utf8(bytes.clone())
                    .unwrap()
                    .trim_end_matches('\n')
                    .to_owned();
            }
            if matches!(
                *actor,
                "retain-child-observation" | "retain-independent-report"
            ) {
                operand = base64::engine::general_purpose::STANDARD.encode(&bytes);
            }
            if *actor == "race-export-winner" {
                operand = (90 + generation).to_string();
            }
            let identity = native(20 + seq, 0o100600);
            outputs.push(json!({"relative_path":path,"kind":"file","identity":identity,"bytes":ByteIdentity::of(&bytes),"symlink_target":null}));
            pending = json!({"state":"complete","relative_path":"fixture-pending-00000000-0000-0000-0000-000000000000","identity":identity,"error":null});
        }
        if matches!(
            *actor,
            "race-export-winner" | "release-operation" | "archive-control"
        ) {
            worker = json!({"pid":140+generation,"operation_id":90+generation,"operation":"export","phase":if generation<2{"publication"}else{"namespace-sync"},"control_identity":native(100+generation,0o40700),"pid_absent":if *actor=="archive-control"{json!(true)}else{Value::Null},"absence_errno":if *actor=="archive-control"{json!(3)}else{Value::Null}});
        }
        let ack = json!({"schema":1,"sequence":seq,"actor":actor,"scenario":scenario,"request_sha256":ByteIdentity::of(format!("EML_FIXTURE_ACTOR {seq} {actor} {operand}\n").as_bytes()).sha256,"generation_before":generation,"generation_after":after,"state":"succeeded","phase":"complete","effect":effect,"root":root,"root_address_verified":true,"outputs":outputs,"source":source,"worker":worker,"pending_cleanup":pending,"error":null});
        let document = serde_json::to_string(&ack).unwrap();
        records.push(json!({"identity":ByteIdentity::of(document.as_bytes()),"document":document,"ack_published":true,"ack_pending_cleanup":{"state":"complete","relative_path":"fixture-pending-00000000-0000-0000-0000-000000000000","identity":native(300+seq,0o100600),"error":null},"ack_error":null}));
        generation = after;
    }
    json!({"state":if records.is_empty(){"unused"}else{"complete"},"scenario":scenario,"actor_uid":501,"root":root,"root_address_verified":true,"descriptor_closed":true,"generation":generation,"records":records,"unexecuted":[],"failure":null})
}
