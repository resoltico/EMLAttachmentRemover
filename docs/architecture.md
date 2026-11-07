# Architecture

The business libraries own MIME policy, collection, destinations, execution facts
and human outcome vocabulary. Frontends adapt inputs and render those facts.

| Crate | Responsibility |
|---|---|
| `eml-message` | Pure byte-based MIME parsing, body classification, plans, independent retained-span authorization and verification |
| `eml-core` | Native request admission, collection, destination preflight, filesystem execution, diagnostics, product identity and vocabulary |
| `eml-protocol` | Strict protocol-1 request decoding, native path encoding, typed JSONL events and bounded checked output |
| `emlattachmentremover` | Public CLI arguments, request transport, signals and human/JSON sinks |
| `eml-app` | Immutable request snapshots, bounded report reducer and std child-process supervision |
| `eml-gui` | Pure batch transitions, Slint views and GUI wording, exact report export, native ingress/preferences and macOS adapters |
| `xtask` | Distribution archives, notices and the local merge gate |

`eml-message` has no filesystem or platform dependency. Fuzzing targets that crate;
its independent model and verifier check retained bodies and removed parts.
`eml_core::vocabulary` maps typed facts to human labels for both CLI and GUI.
`eml_core::product` owns the command/display names and source-identity formatting;
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
Explicit files take precedence; the deepest selected root owns a walked file.
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

The desktop runs the public CLI from `Contents/Helpers`, never PATH. `RequestSnapshot`
captures absolute native inputs, destination and mode without filesystem I/O. Adding
inputs or changing destination schedules a `dry_run` automatically. Nothing is
written until Remove Attachments. Create repeats discovery and preparation, so its
observations supersede the preview rather than trusting stale source bytes.

`state.rs` owns one batch: inputs, destination, check/create slots, generation,
pending work, native-path selection and the problems-only filter. State is derived
as Empty, Checking, Ready, Removing, Stopped or Done. Changes collapse into one
pending check. Remove Attachments during a check replaces the pending operation
with Create. New inputs during Removing are refused; after Done or Stopped they
replace the batch. There is no run history, waiting queue, session report storage,
eviction or lazy reload. Continue creates a fresh request with the same operands.

`WorkerRun` owns the child and all three pipe threads. Stop sends SIGINT once.
An obsolete read-only check is force-killed after two seconds. The controller waits
for `poll()` to reap the child and join its readers/writer before starting pending
work. Generation checks prevent discarded results from returning to the batch.
Create can be force-killed only through Quit Now, offered five seconds after
Stop and Quit; earlier Stop time does not shorten that interval. The app waits for
child settlement and owned report exports before exiting. Checks can be killed
and reaped without a Quit dialog. Pending decisions invalidate native-panel epochs
and refuse drops; committed shutdown cannot admit more work.

Accepted check and create reports remain in memory. Each exact stream is bounded
below 256 MiB by protocol framing; parsed observations have independent resource
bounds. Only the current check and create slots remain attached to the batch.
One outstanding export or native-save capture can retain a prior batch's stream
in addition to those two streams. Starting another batch cannot invalidate a
physical report write already in progress. There are no automatic backing files.

Save Report captures exact immutable bytes from the latest settled create stream,
or the settled check if no create stream exists. A lifetime token rejects stale
panel cancellation/submission, while the native panel owns replacement approval.
The writer stages complete bytes privately in the approved target folder,
file-synchronizes, and atomically renames over the approved target. Write failure
preserves a prior target and attempts cleanup. Directory-entry survival across
power loss is not promised; a crash can leave a private staging file.

`wording.rs` owns the main window's product language, while core qualification
and CLI vocabulary remain authoritative for processing facts. `view_model.rs`
adapts accepted receipts to core meaning. `present.rs` unites check/create rows
by lossless native source path, retaining create precedence and distinguishing
Waiting, interrupted Not processed and completed No longer found. Natural order
uses display folder/name with native-path tie-breaking; selection remains native.
Report lock contention preserves the previous projection instead of emptying it.

The list uses fixed-height virtual rows, with row height derived from text size.
New rows insert in natural order during streaming. Selection stays visible when
insertion or the scrolling detail pane changes the viewport. Band backgrounds
span the window; content stays in a centered column at most 980 px wide. Below
900 px the Size column and naming hint are hidden. The detail pane is capped at
40% of the window, scrolls internally, and keeps its actions visible. At minimum
size two list rows remain visible, including with enlarged text and long details.
Buttons, menus and controller boundaries share predicates in `actions.rs`.

Slint uses Cupertino on macOS and Fluent on Linux. Native open/reveal requests run
off the UI thread, bounded to one per initiating window. Acceptance is advisory,
not proof of viewer contents or current file identity. About and Licenses retain
their focus/reset behavior. Preferences retain their folder keys; unreadable,
invalid or old content gives defaults silently and is replaced on the next save.

On macOS, the verified Winit application-delegate class receives About/Quit routing
and an `application:openURLs:` adapter. Only absent selectors with matching ABI
encodings are added. File URLs become absolute native paths, queue under a mutex,
and drain in one ingress batch from the controller tick. Non-file URLs are ignored.
The plist declares email messages and folders as Viewer/Alternate; Services are
absent. Installation precedes the event loop. Launch-by-document, Dock, Finder,
native panels and appearance are manual checks, outside the merge gate.

Headless checks drive production views and CLI stubs through Slint's testing
backend and virtual time. The software renderer is the default; Skia remains an
explicit measurement feature. Builds pin upstream crates. The small
[spin_on source patch](../crates/spin-on/README.md) uses standard Rust pinning and
a no-op waker to remove archived `pin-utils`; the audit policy is unchanged.
