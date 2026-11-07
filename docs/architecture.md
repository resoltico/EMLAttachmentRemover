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
| `eml-gui` | Pure session state, Slint views and GUI wording, bounded report I/O, native ingress/preferences and macOS About/Quit adapters |
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

The desktop runs the public CLI from `Contents/Helpers`, never PATH. Native ingress
stages absolute paths without starting a worker. Explicit Start captures inputs,
mode and destination in an immutable protocol request. The pure `state.rs` owns
navigation, pending decisions and run-local filter, sort and selection; the runtime
owns workers, preferences and report I/O. One worker runs at a time. Admission is
bounded to 20 session runs, eight waiting requests and 2.5 GiB of stored stream bytes.

Every settled report is automatically backed by a private temporary file whose
name is unlinked before diagnostic bytes are written. One background writer/loader
replays the existing protocol reducer and compares accepted observations before
eviction. Loaded settled reports are bounded to three, including failed-storage
fallbacks; the active and visible report remain loaded when possible. Storage
failure retains the original completion and blocks further admission at the memory
limit. Hidden backed reports unload; selected reports load lazily. Private handles
are released on result close or exit, providing no durable history or crash recovery.
An immutable stream may also be pinned by one pending save panel or owned export.
Replay temporarily allocates one additional bounded parsed report.

`WorkerRun` owns `std::process::Child`; background threads write/close stdin and
drain stdout/stderr. The UI polls without blocking. Stop sends SIGINT once; Force
Stop becomes available after five seconds and requires a targeted confirmation.
The busy Quit decision can force the active worker immediately, but still waits
for reaping, reader settlement and owned report exports. Accepted records survive
malformed tails and worker failures. Pending decisions and committed shutdown
reject drops and late native-panel submissions. Confirmation targets cannot
silently change when the next queued worker starts.

Save Report captures exact immutable bytes and a run identity before opening the
parented native save panel. A lifetime token rejects stale cancellation/submission.
The target result cannot close while its panel or physical export is outstanding.
The native panel owns replacement approval. A background writer writes and
file-synchronizes a private same-folder staging file, then atomically renames it
over the approved target. Write failure leaves the prior target intact, or no
new target, and attempts cleanup. Directory-entry survival across power loss is
not promised; a crash can leave a private staging file. Transient saved-report
feedback retains the captured native path independently of later navigation.

GUI wording belongs to `wording.rs`; core meaning/qualification and CLI vocabulary
remain authoritative for processing facts. Interrupted items are not attention
failures; missing results become Not processed only when an accepted summary
establishes them. Unqualified publication remains attention-worthy. Natural sorting
and filters affect presentation only; rows retain original report offsets, and
exports retain original bytes. Native paths are never reparsed from display text.

Slint views are split by responsibility, with shared theme/control tokens and
Cupertino widgets on macOS, Fluent on Linux. A persistent session sidebar navigates
runs and the draft without changing captured requests. Fixed-height virtual rows
and a separately scrolling inspector adapt between side-by-side and stacked layouts.
Buttons and menus share action predicates, rechecked at controller/native boundaries.
Native open/reveal requests run off the UI thread, bounded to one per initiating
view. Acceptance is advisory, not proof of viewer contents or file identity.

Headless checks drive production views through Slint's testing backend and virtual
time; OS panels and services are manual checks. Only the application-menu About
and Quit adapters use macOS-specific unsafe Objective-C calls. About routes to
the application-owned Slint window. The software renderer is the default; Skia is
an explicit measurement feature. Builds pin upstream crates. A small [spin_on source patch](../crates/spin-on/README.md)
uses standard Rust pinning and a no-op waker to remove archived `pin-utils` from
Slint’s dependency graph; the audit policy is unchanged.
The new preferences schema records three recent destinations, the last destination
and last report folder; unavailable/old files are preserved with session-only defaults.
