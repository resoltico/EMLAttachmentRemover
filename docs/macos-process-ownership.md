# Desktop worker ownership

The Slint product uses one concrete Darwin child transport for exactly two operations:
processing and report export. The application owns at most one child in each
lane; export has no queue. No command-line product is involved. `eml-macos` contains the native
spawn/signal/reap boundary; eml-app owns request, control and completion policy.

The positive PID comes directly from successful posix_spawn. Polling and signals
share one tiny mutex. A terminal waitpid result revokes authority before unlock;
traced stops retain it, and unexpected ECHILD revokes it and fails explicitly.
There is no background reaper, delayed PID notification, process-name lookup or
negative/group-zero signal. Native operations are named Interrupt/Kill; the
supervisor owns cooperative/second-interrupt and bounded-Quit escalation policy.

Spawn requires a stable default SIGCHLD disposition and exclusive reaping. It
checks without changing the parent's handler. Child defaults and an empty signal
mask are explicit; Rust Command's inherited mask is insufficient. Standard
streams use stable CLOEXEC copies above2 and Darwin's close-default spawn policy.
The owned request pipe suppresses SIGPIPE on its open file description. Parent child-only ends
are closed once with errors retained, without retrying numeric descriptors.

Both desktop operations add a private child-writer pipe at descriptor3; all stable
source copies are above3. A bounded `R` byte acknowledges that the handler and
request reader are ready before the parent may feed its immutable operation. The
first interrupt records Stop before writing `F`; only that actual observation
permits a second interrupt. These bytes are private lifecycle transport, separate
from the shared JSON outcome protocol. A completed normal worker may close after
`R` without receiving an interrupt. Invalid control bytes, live premature EOF and
missing readiness/interrupt acknowledgments fail explicitly.

An Ok SpawnObservation can retain both a real child and cleanup issues. The
supervisor must immediately retain that child and settle it before returning a
startup failure. Earlier pipe/dup failures use ordinary RAII and do not supply
checked-close receipts. Dropping OwnedChild does not terminate or reap it.

Stop/closing intent must remain monotonic through an in-flight launch. Apply it
before feeding a late successful child. No pipe I/O, blocking wait, reader join
or UI acknowledgement belongs under the native authority lock. The no-grandchild
worker contract is an ownership requirement, not a reason to retain a process
group identifier after its leader is reaped.

Every operation has a checked positive monotonically increasing OperationId,
independent of the processing report RunId. Captured callbacks and controls address
the exact operation, never a filename, row or recycled PID. Export reserves its
terminal source report until the actual child, request writer, stdout/stderr/control
readers and process owner settle. Selecting another report does not retarget it.

The private worker accepts only `--desktop-operation` and `--version`. Its identity
query binds product version/build, compiled CPU/OS, private protocol 2 and both
operations before request delivery. There is no previous private mode or unframed
stdin fallback. The strict `EML-DESKTOP/2\n` envelope carries a bounded object header
(64 KiB inclusive), checked operation ID and operation kind, plus an absolute
lossless destination for export. Body chunks are at most 1 MiB; a zero terminator
and immediate EOF are mandatory. Processing retains at most 8 MiB, report export
at most 256 MiB. Truncation, overflow, unknown/duplicate fields and trailing bytes
refuse before target mutation. Public processing JSONL remains protocol 1.

Completion requires terminal reap, request-writer settlement/explicit input
closure, both output EOF/reader settlements and final model application. Report
backpressure cannot block Stop or the event loop. Unknown lifetime closes later
admission. First Stop preserves completed outcomes; Force Stop sends the second
interrupt after confirmation. Confirmed Quit records one immutable monotonic instant for both lanes; second
interrupt is due at 10 seconds and Kill at 12 seconds, retaining handler-readiness
and first-interrupt-observation gating. Repeated confirmation does not move deadlines.
Confirmed Quit additionally has its bounded final Kill path and retains explicit uncertainty if settlement cannot be proved.

The supervisor retains its positive-PID owner and endpoint transfer slots outside
its panic boundary. Reader/writer setup failures and panics cannot discard the
endpoints; the owner joins completed roles and checks their closes. Unresolved
native authority remains owned with a bounded pending warning. Report locking and
pipe backpressure do not share that authority lock. A pending warning remains a
failure after late settlement, and terminal observations remain terminal on
repeated polls. Known item outcomes are retained independently of lifecycle
failure or cancellation intent.

Owned native tests cover real stdio/EOF/normal exit, Interrupt while waiting for
input, live-child Kill, spawning-thread mask reset, failed-spawn/no-authority and
post-reap signal rejection. Test cleanup failures abort later cases visibly and
preserve the original failure. These tests do not establish complete production
supervisor, inherited-descriptor or GUI qualification.


The parent performs only lexical native-path/resource admission. Binding arbitrary
source/output directories, destination occupancy, staging, synchronization,
publication, readback and cleanup occur inside children. The GUI selects an export
folder and renders a pure filename draft. Existing files are never replaced; a
collision is an ordinary child refusal, with no guessed alternate filename.
Finder receives the exact captured file URL as advisory navigation without an
existence-probe actor.

Report serialization belongs to `eml-app::report_document`, independent of Slint.
The writer streams compact schema `eml-technical-report`, schema_version 1,
product version/build, RunId, accepted start/items/summary/protocol-failure facts
and complete bounded worker lifecycle fields (including exact OperationId). The
required `terminal` kind is derived by the application-owned RunMeaning classifier
from accepted summary/protocol facts and actual completion; the UI and exported
report use that same view. Exit zero without a complete accepted summary is never
a successful terminal meaning. Later process/transport failure coexists with
earlier usable item facts. The report validator rejects a contradictory terminal
kind without reconstructing processing or publication authority. Optional observations are explicit
null. Empty launch-failure, interrupted and partial reports remain exportable.
Serialization occurs on the owned pipe-writer actor, enforces 256 MiB inclusive,
and emits the body terminator only after success. The child retains one bounded
body, validates a complete strict document in place using borrowed raw record
slices and at most one bounded decoded record at a time, then publishes exactly
those bytes. It never reconstructs MIME decisions or imports historical exports.

Export stdout is one strict receipt of at most 64 KiB including the exact operation
ID, independent publication/address/identity/readback/sync/cleanup facts and bounded
failures, followed by EOF. The worker derives its exit from that same actual-operation receipt qualification
predicate before encoding it; no separate producer success predicate exists. A
qualified receipt and clean matching process/transport completion are both
necessary for successful export. No receipt is manufactured
from an exit status. A published receipt followed by failed transport retains both
facts. A force-killed or lost child without an accepted terminal receipt means
**export outcome unknown; a complete report may exist**. Pre-execution launch
failure means **export not started**. The parent never removes guessed destination
or temporary paths after killing a child. Force termination can leave temporary
artifacts when owned cleanup could not be observed.

The `native-fixtures` worker can observe actual operation boundaries through
`EML_OPERATION_OBSERVATION`, an exclusively owned canonical private directory.
A missing `observation` recipe is inactive when that worker starts. Present
recipes are finite: `processing:<phase>:<0|1>` or `export:<phase>`, followed by a
newline. Storage phases are `preflight`, `readback`, `stage-write`, `file-sync`,
`publication`, `namespace-sync` and `after-publication`. Export `request-body`
pauses the request-reader actor after the same strict header decoder succeeds and
before it reads body bytes. Complete framing and document admission remain
mandatory before export. Export readback observes only the held published file;
occupied output never becomes a readback source.

The fixed `dual-quit` recipe, followed by a newline, requires private `processing`
and `export` subdirectories with exactly `processing:preflight:0` and
`export:preflight` recipes. It admits the two existing lanes without adding a
report slot or changing process ownership. Each worker selects one lane only from
its actual decoded header; identity probes and unrelated operations claim no
markers. The selected directory receives `worker-pid`, a phase/operation-bound
`ready`, and actual interrupt markers. Only the first selected phase blocks.
Release is exactly eight bytes, `release\n`, published complete and without
replacement from a pending file. A partial or malformed release is refused.
Stop does not release a storage barrier; native controls, later release or actual
process termination must settle the operation. None of these controls exist in
shipping workers, supply substitute receipts, or authorize destination deletion.
