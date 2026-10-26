# Architecture

The core owns MIME policy, collection, destinations, execution facts and human
outcome vocabulary. Frontends adapt inputs and render those facts.

| Crate | Responsibility |
|---|---|
| `eml-message` | Pure byte-based MIME parsing, body classification, plans, independent retained-span authorization and verification |
| `eml-core` | Native request admission, collection, destination preflight, filesystem execution, diagnostics, product identity and vocabulary |
| `eml-protocol` | Strict protocol-1 request decoding, native path encoding, typed JSONL events and bounded checked output |
| `emlattachmentremover` | Public CLI arguments, request transport, signals and human/JSON sinks |
| `eml-app` | Immutable queued requests, bounded report reducer and std child-process supervision |
| `eml-gui` | Slint view/controller, desktop ingress, configuration and the small macOS Quit shim |
| `xtask` | Distribution archives, notices and the local merge gate |

`eml-message` has no filesystem or platform dependency. Fuzzing targets that crate;
its independent model and verifier check retained bodies and removed parts.
`eml_core::vocabulary` maps typed facts to human labels for both CLI and GUI.
`eml_core::product` owns the command/display names and build number; the workspace
manifest owns the release version.

## Execution

The caller captures an absolute working directory once. Validation checks native
resource limits before I/O. Collection observes selected files and directory roots,
coalesces proven identities, sorts native names and retains failures as known work.
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

The desktop runs the public CLI from `Contents/Helpers`, never PATH. Native ingress stages absolute paths without starting a worker. Explicit Start
captures the reviewed selection, current destination and mode in a protocol request. At most eight
requests wait, and two retained reports reserve display capacity. A serial worker
runs one request at a time; dismissing a completed report releases capacity.

`WorkerRun` owns `std::process::Child`; background threads write/close stdin and
drain stdout/stderr. The UI polls without blocking. Derived Slint rows update in place, preserving
controls and keyboard focus across selection and append-only result updates. Stop sends SIGINT once; Force
Stop kills the child; completion includes reaping and reader settlement. Partial
accepted records survive malformed tails and worker failures. Exports write the
exact retained stream to a new file in a background thread. Busy Quit waits for
worker/export settlement or follows the user's confirmed stop choice.

Slint view data comes from the shared vocabulary. Headless tests drive accessible
actions through Slint's testing backend and mock time. Native ingress delivers
launch arguments, batches window drops with a zero-delay timer, and parents rfd
asynchronous dialogs to the retained window. Only the small macOS application-menu About and Quit adapters use
unsafe Objective-C calls. The About adapter routes the focused window’s standard
application-menu item to the application-owned Slint About window; it does not
modify or fork the toolkit. Dock/Finder/Services integration and private worker
protocols are removed. The software renderer is the default; Skia remains an
explicit build feature for measurement. Builds use upstream crates without forks.
