# Architecture

The business libraries own MIME policy, collection, destinations, execution facts
and human outcome vocabulary. Frontends adapt inputs and render those facts.

| Crate | Responsibility |
|---|---|
| `eml-message` | Pure byte-based MIME parsing, body classification, plans, independent retained-span authorization and verification |
| `eml-core` | Native request admission, collection, destination preflight, filesystem execution, receipt qualification, settled-item facts, human display, diagnostics, product identity and vocabulary |
| `eml-protocol` | Wire-only protocol-1 request decoding, native path encoding, records and bounded JSON output |
| `emlattachmentremover` | Public CLI arguments, request transport, signals and human/JSON sinks |
| `eml-gui` | Pure batch transitions, Slint views and GUI wording, exact report export, native ingress/preferences and macOS adapters |
| `xtask` | Distribution archives, notices and the local merge gate |

`eml-message` has no filesystem or platform dependency. Fuzzing targets that crate;
its independent model and verifier check retained bodies and removed parts.
`eml_core::vocabulary` maps typed facts to human labels, explanations, simultaneous
publication/cleanup consequences, related-resource notices, attachment names/kinds,
size formatting and counts for both CLI and GUI. Both render those meanings;
frontend controls and transport diagnostics retain their interface-specific owners.
Core aggregation also counts represented pre-execution interface failures without
inventing core outcomes; the interface keeps the failure cause and lifecycle.
`eml_core::qualification` owns receipt predicates used by execution constructors,
outcome interpretation and report consumers. Complete namespace edge accounting,
method-specific native witnesses and cleanup facts must agree. Private success
constructors and actual native binding/readback checks remain separate safeguards.
The protocol crate structurally decodes records and uses core's phase and
removal-reason types through local serde bridges; it does not convert wire records
into domain outcomes. Core has no JSON dependency.
The GUI does not implement a second outcome converter or native-path decoder.
Independent reports can retain relative or foreign-host evidence without
turning it into native navigation. The GUI retains core’s native operational
paths directly instead of reconstructing them from records.
GUI details use current accepted observations, without reinterpreting a previous
preview as a current create result. Human diagnostics keep canonical phases and
separate cleanup resource/native causes from protocol-1 serialization.
`eml_core::product` owns the command/display names, purpose and source-identity formatting;
the workspace manifest owns the release version. `EML_SOURCE_REVISION` optionally
embeds a full commit ID for both frontends; About displays its first 12 characters.

## Execution

The caller captures an absolute working directory once. Validation checks native
resource limits before I/O. Collection observes selected files and directory roots,
coalesces proven identities, sorts native names and retains failures as known work.
Explicit ordinary leaf names use a canonical spelling only when canonical parent
identity and complete file signature prove the same captured entry. Unproven or
long-path canonicalization retains conservative spelling without a new admission
error; final symlink entries and distinct hard-link names are not merged by inode.
POSIX `realpath` is not a portable stored-leaf spelling oracle: on Linux it resolves
traversal and symbolic links without proving stored case or Unicode normalization.
Differing unproven leaf spellings therefore remain separate even when filesystem
lookup reaches the same file. Identical repeated operands still coalesce; native
canonical-spelling proofs on macOS and Windows can coalesce leaf aliases.
Explicit files take precedence; the deepest selected root owns a walked file.
The reported total stays paired with collection completeness; incomplete checking
does not by itself establish a read failure or an empty selection.
Selected directory aliases can be followed; links encountered during a walk are
excluded. Destination links and reparse edges are refused.

A request admits 4,096 input paths and 4 MiB of native input path bytes. Collection
bounds work at 4,096 files/failures, 100,000 visited entries, 64 directory levels
and 8 MiB of charged reported paths. A source is bounded by 256 MiB. MIME limits
also bound entity count, nesting, headers, parameters and metadata; see
[the source constants](../crates/eml-message/src/lib.rs) and [MIME policy](mime.md).

Destination preflight groups exact native names, existing object identities and
file/mirror-prefix conflicts. All known contenders are refused. Explicit files
are flat beneath an existing output root; walked files retain their selected-root
and relative layout. Without an output root, copies stay beside sources. Names
use `.no-attachments.eml`, shortening stems to native component limits without
retry counters or hashes. Existing generated names and source aliases participate
in conflict checks.

Sources are opened read-only. A bounded read checks identity, size and modification
time before and after. Preparation produces expected bytes without rewriting
retained payloads. Independent source-span authorization and verification must
accept the plan. Check performs the read/prepare/destination work without writes;
Create repeats it and may publish.

Filesystem code lives in `eml-core/src/fs`, with common held-directory/file types,
identities, signatures and receipts. Unix operations use rustix descriptor-relative
opens, exclusive creation, synchronization and no-replace publication. Linux's
`EINVAL` rename fallback uses link/unlink. Windows uses Win32 reparse-aware opens,
CREATE_NEW, file IDs, FlushFileBuffers and MoveFileExW without replacement or
copy fallback. Held Windows directory handles are checked against their requested
address before path-based child operations.

A fresh `.emlattachmentremover-<random>.tmp` file is written and verified in the
destination directory, synchronized, then published without replacement. Mirror
directories are created only during Create. Cleanup removes owned staging entries
on handled failures and reports uncertainty or residue when it cannot finish.
No startup sweep deletes abandoned staging files. Identical existing destinations
are compared and synchronized; different bytes are refused. Publication, address,
file sync, namespace sync and cleanup remain separate reported observations.
See the [threat model](threat-model.md) for the supported environment.

## Desktop boundary

The CLI and the desktop are independent renderers of core. The desktop calls
`eml_core::run` on a worker thread. `RunRequest::from_absolute` owns pure native
request admission and a stable working base; the CLI retains its captured working
directory. Adding inputs or changing destination schedules a read-only check.
Nothing is written until Remove Attachments. Create repeats discovery and
preparation instead of trusting preview bytes.

`state.rs` owns one batch: inputs, destination, check/create slots, generation and revision,
pending work, native-path selection and the problems-only filter. State is derived
as Empty, Checking, Ready, Removing, Stopped or Done. Changes collapse into one
pending check. Remove Attachments during a check replaces the pending operation
with Create. New inputs during Removing are refused; after Done or Stopped they
replace the batch. There is no run history, waiting queue, session report storage,
eviction or lazy reload. Continue creates a fresh request with the same operands, including when the
previous attempt stopped before any observations. Core owns the advisory preview
copy offer and complete-results interpretation; GUI busy/quit/lifecycle gates do
not depend on display severity.
Core owns the settled batch headline, nonzero supporting facts and originals promise;
the CLI and GUI render the same verdict and parts. Detail notices follow the part list.
A settled stopped summary uses core totals from current create records and
unprocessed work, including sources without preview rows. Retained preview rows
are not added a second time.
Until that create settles, the prior create report supplies row results that the
current report has not yet replaced. A new batch drops it. Before discovery
reports a total, create progress reads “Starting…”.

Every mutating batch transition advances its revision. At the end of each tick,
the controller refreshes if that revision differs from the last rendered one,
work or export is active, or presentation outside the batch is dirty. This also
renders workers started and settled inside the same native-document tick.
Exported Slint `BatchState` and `Tone` enums carry state and row tone across the
UI boundary; the Rust state mapping is exhaustive.

`worker.rs` owns one thread per run and the runner seam; production passes
`eml_core::run`. A sink sends owned core `SettledItem`, start and summary values
through a channel and writes protocol-1 with `JsonSink` into a buffer strictly
below 256 MiB. The UI drains typed facts in its tick; it never decodes report bytes.
`DisplayPath` and `DisplayText` belong to core. Earlier receipts, failures and
native paths stay typed through rendering. A report-write failure cannot erase a
typed publication observation already received.

Stop requests `RunControl` once. An obsolete check receives cooperative stop and
is detached from the current batch; generation guards discard its results. Its
physical check slot remains occupied until it settles, so pending checks coalesce
without accumulating threads. A create may proceed beside that retiring read-only
check. No UI call joins an unfinished thread, and no thread is force-killed.
Stop and Quit waits for the create and report exports. Quit Now is offered five
seconds after that decision and lets the event loop return without awaiting the
create, ending its thread when main exits. Report exports already in progress
still settle first. Read-only checks do not delay quit. Decisions invalidate
native-panel epochs and refuse drops; committed shutdown admits no more work.

The worker catches unwinding panics, retains earlier typed observations and report
bytes, and settles a distinct panic state without inventing a core failure. Its
unexpected-stop banner has no Processing details because no core diagnostic exists.
Real report delivery failures keep core’s explanation and diagnostic fields.
Abort and out-of-memory end the app; this
is accepted. As with a crash, Quit Now may leave a private staging file; abandoned
stages are never swept. Final copies are complete or absent.

Settled check and create facts and exact report bytes remain in memory. Core's
collection/metadata limits bound retained observations; `MAX_RUN_BYTES` bounds
report storage. Continue retains the previous create report until settlement.
One outstanding export or native-save capture can retain a prior batch's stream
in addition to those two streams. Starting another batch cannot invalidate an
already captured save. Its panel retains the single save lifetime until submission
or cancellation, preventing overlapping captures. Quit invalidates unsubmitted
panels through the native-panel epoch. Physical writes already in progress settle
before exit. There are no automatic backing files.

Save Report captures exact immutable bytes from the latest settled create stream,
or the settled check if no create stream exists. A lifetime token rejects stale
panel cancellation/submission, while the native panel owns replacement approval.
The writer stages complete bytes privately in the approved target folder,
file-synchronizes, and atomically renames over the approved target. Write failure
preserves a prior target and attempts cleanup. Directory-entry survival across
power loss is not promised; a crash can leave a private staging file.

`eml_core::vocabulary` owns `Outcome`, severity, explanations, run failures,
part names, decimal byte formatting and `BatchTotals`. The CLI and GUI render
these values; `wording.rs` holds only desktop interaction and layout wording.
`eml_protocol` owns only protocol-1 wire representation; `RunReport` projects
settled core facts through core aggregation. `present.rs` unites check/create rows by lossless native source
path. A current create item takes precedence; before it arrives, the previous
create result or a known check failure remains visible. Otherwise the row waits.
Interrupted work is Not processed. Completed missing sources are No longer found;
worker or discovery failures do not establish that a source disappeared.
Report facts are owned by the UI thread; workers send observations without
sharing the rendering state.

Rows follow the worker's item indices. Core discovery owns folder traversal order;
the GUI does not apply a second name sort. Check, create and Continue retain the
same native input snapshot and destination layout. Selection remains native.

`theme.slint` owns colours, the fixed 13 px body type scale, spacing, radii and tone
mapping. No text-size input is exposed. The list uses fixed 44 px virtual rows,
centred cells and separators. The header reserves a 32 px summary line even when
there is no problems chip. Band backgrounds span the window; content stays in a
centred column at most 980 px wide. Below 900 px the Size column and naming hint
are hidden. The visible detail pane has a fixed preferred height of 200 px, capped
at 30% of the window, keeps its actions beside the email name and scrolls its body
internally. Selecting another email does not resize the list. At minimum size three list rows remain
visible, including with long details. Popups anchor to their command controls.
Buttons, menus and controller boundaries share predicates in `actions.rs`.

Slint uses Cupertino on macOS and Fluent on Linux. Native open/reveal requests run
off the UI thread, bounded to one per initiating window. Acceptance is advisory,
not proof of viewer contents or current file identity. About and Licenses retain
their focus/reset behavior. The native GUI uses Winit on macOS only, with builtin
Cupertino widgets and native menus. Linux uses the testing backend for headless development; Windows supports core
and the CLI only. Human path formatting is shared in `eml-core`, separate from
wire display strings. Preferences keep native paths in memory and use
their own Unix-byte/base64 encoding at the settings boundary, validated by
core's path policy. Invalid or unreadable settings use defaults, expose a
destination note and are replaced on the next successful save.

On macOS, the verified Winit application-delegate class receives About/Quit routing
and an `application:openURLs:` adapter. Only absent selectors with matching ABI
encodings are added. File URLs become absolute native paths, queue under a mutex,
and drain in one ingress batch from the controller tick. Non-file URLs are ignored.
The plist declares email messages and folders as Viewer/Alternate; Services are
absent. Installation precedes the event loop. Launch-by-document, Dock, Finder,
native panels and appearance are manual checks, outside the merge gate.

Headless checks drive production views with real core fixtures through Slint's
testing backend. Scripted runners hold typed event callbacks for lifecycle timing;
rare receipt projections use typed core facts rather than wire replay. The software renderer is the default; Skia remains an
explicit measurement feature. Builds pin upstream crates. The small
[spin_on source patch](../crates/spin-on/README.md) uses standard Rust pinning and
a no-op waker without depending on archived `pin-utils`.

Processing details shows the whole-run core diagnostic's Code, Phase and Detail,
plus its native cause. Item-only failures stay in selected-item details; without a
whole-run core failure, including a panic, the expandable control is absent. Core owns request admission
wording. There is no stderr capture or subprocess protocol reducer in the GUI.

Mailbox admission and multipart preamble/epilogue framing belong to
[the MIME policy](mime.md#mailbox-and-multipart-framing).

Read-only runs may collect and prepare sources while the save folder is
unavailable. Core reports that fact before item observations and on settlement.
Destination paths are spelling-only predictions with an assumed 255-unit name
limit, no root witness and no write capability. They still pass the same path
budget and lexical collision checks. Create always binds the real output folder
and refuses an unavailable one before processing or writing. Reconnecting a
folder requires a fresh check; choosing it again or choosing Next to originals
starts one. Typed core failures own shared explanations, including native write
causes and damaged-versus-headerless messages. Counts deliberately use no grouping.

Request-document diagnostics name short fields only; oversized or control-bearing
field names are omitted, and JSON values are never included. This preserves the
bounded diagnostic assurance while providing the requested schema-error reasons.

The desktop renders unavailable-folder guidance beside Save copies and keeps the
checked source rows. A lost folder during create admission returns to the editable
batch without pretending that processing began. Both commands and buttons use
the same core availability fact; changing the destination starts a fresh check.
The action label shows core’s eligible email count even while filtering problems.
The filter’s filled/checked state and Showing problems text make its scope clear.
Unreadable settings use defaults with a single note until the user chooses a
folder or saves new preferences; this setting-specific note belongs to the GUI.
