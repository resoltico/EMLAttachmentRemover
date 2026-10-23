# Protocol1

Public CLI requests and processing events use protocol 1 through eml-protocol.
The private desktop operation envelope is protocol 2; technical reports use the
app-owned schema described in [worker ownership](macos-process-ownership.md). Native execution
and release acceptance require the [verification gates](verification.md).

Cleanup residual names use platform-native component validation. POSIX rejects
slash and NUL; backslash is a valid filename byte. Windows rejects both separators
and NUL, preserving its UTF-16 native units. Do not suppress a legitimate POSIX
residual component to satisfy the archive's blanket backslash restriction.
Mapper tests cover both preservation and rejection.

An internal Windows native file-operation error retains exact NTSTATUS as primary
and translated Win32 error as secondary. Its bounded authored Error.detail may explain
the lossless eight-digit hexadecimal status; Error.code/phase remain typed stable
values. No publication.native_code field is added. Neither core nor app parses
detail or NT hex for decisions, and diagnostics never replace Published/Unknown
evidence. Existing synchronization/cleanup native_code fields retain their defined
meanings. Pending native I/O retains buffers/handles through completion; an
unprovable memory lifetime must not fabricate an unpublished receipt.

## Request decoding

The `emlattachmentremover` executable delegates shared transport to
`eml-protocol`. Its `decode_request` accepts a complete byte buffer plus the captured
absolute working directory. It checks the 8 MiB limit, strict protocol-1 JSON,
required fields (including explicit null for absent output directory), duplicate
and unknown keys, native encoding and canonical padded Base64. Native bytes are
converted directly to Unix bytes or Windows UTF-16LE, without Unicode conversion,
then core resource admission runs. Error values do not retain or echo JSON/path
content; malformed-document diagnostics expose only parser line/column.

Decoder tests alone do not qualify process input, signals, execution or output
delivery. Core and filesystem crates have no JSON
dependency. Request shape and native-byte tests live in the frontend package;
business path limits remain in core. Their pure error type is RequestPathError;
the read-only collector's outer InvalidRequest additionally retains genuine
working/output prerequisite I/O causes. Decoder validation still performs no I/O.

`PathValue` serializes core-supplied native and display-expression facts into
the exact output object, with lossy display text and a lossless native value. Encoding
names have one shared string authority with strict request decoding. Unix bytes
and Windows UTF-16LE units are encoded directly, including non-Unicode units.
No display string is used to reconstruct the native value. Spelling, including
symlink/dot-dot expressions, is retained. JSON escaping keeps embedded LF inside
the string; raw U+0085/U+2028/U+2029 remain valid string content. This primitive
does not itself perform human terminal escaping, LF event framing or
whole-event/run size admission. Those are separate checked output adapters.
Its tests perform no filesystem I/O.

### Core-supplied display and native path facts

The reviewed Windows rule uses an admitted extended SELECTED expression plus a
literal walked suffix for reported native values. Preserve aliases; a canonical
target query is a separate binding fact. Human display retains the original
request-derived expression. The path serializer must accept those two core facts
without reconstructing one from the other or adding frontend normalization.
An unadmitted failed root may report its failed requested native input when no
usable operand exists; this is intended input, not a valid I/O capability.
Successful items must use their admitted native expression. The borrowed adapter
requires both facts explicitly; same-path callers pass the same path twice. The collector supplies both facts. A hand-authored split-fact
golden rejects using display text for native encoding.

For each source/known destination let d be complete display-expression native
bytes and n complete reported-native bytes; charge b=max(d,n) once to the8MiB
budget, including failure items and repeated logical prefixes. Windows counts
two bytes per UTF-16 unit. Since6d+4n/3 <= (6+4/3)b, the existing protocol envelope
still bounds both representations; the fixed margin covers Base64 rounding.
Expanded reported cwd counts, while transient unreported operands are not charged
a second time. This is a report-retention bound, not an8MiB peak-allocation
promise for two independent buffers. Maximal event/run and exact/one-over tests
use independent display/native facts.

`TerminalPath` separately formats a borrowed path for human terminal text. It
escapes C0/C1 controls, Unicode line/paragraph separators and Unicode bidi-control
marks using visible Rust-style escapes. Printable Unicode, ordinary backslashes
and joiners remain readable. It is lossy display, not shell quoting, an
operational expression or a guarantee against every Unicode visual ambiguity.
Formatting errors propagate to the caller. HumanSink and the executable use this
formatter for event paths and diagnostics, with checked writes and flushes.

## Request and event shapes

A Unix request for the relative path `message.eml` is:

```json
{"protocol":1,"paths":[{"encoding":"unix-bytes","base64":"bWVzc2FnZS5lbWw="}],"output_directory":null,"dry_run":true}
```

All four top-level fields are required. `output_directory:null` selects beside-source
output; a native-path object selects an existing output root. Windows uses
`windows-utf16le` with canonical Base64 of the actual little-endian UTF-16 units.
A request path object has exactly `encoding` and `base64`; display text is output
only. Request objects reject unknown and duplicate fields.

Each event has a `type` discriminator and the following fields:

| Type | Fields |
|---|---|
| `start` | `protocol`, `program`, `version`, `dry_run`, `total`, `collection_complete` |
| `item` | `index`, `source`, `status`, `destination`, `source_bytes`, `copy_bytes`, `removed`, `stripped_fields`, `plan`, `publication`, `existing_observation`, `error` |
| `summary` | `counts`, `interrupted`, `collection_complete`, `run_error`, `exit_code` |

`start.program` is `emlattachmentremover`; version 7 changes this product-name
value along with the public command, without changing protocol 1 record fields.
`start.version` remains the release version, independent of the product build.

Counts contain `created`, `up_to_date`, `planned`, `failed` and `not_processed`.
Nullable fields are present with explicit JSON null when unobserved. Nested path,
publication, synchronization, existing-copy and error records are defined by the
[typed wire records](../crates/eml-protocol/src/records.rs) and independent
[contract scenarios](../tests/contract). Event consumers may ignore additive object
fields, but reject unknown types, wrong value shapes, missing required fields and
inconsistent state/count/receipt/exit observations. There is no older decoder.

## Canonical terminal report indices

Core freezes ordinary rows by complete raw logical destination route, absolute
scope/source, selected-origin and semantic/native diagnostic tie breaks. Failed
rows without a logical route follow in raw native source/origin order. Terminal
item indices are contiguous zero-based report ordinals in that order; they are
never raw argument indices. For a known file, a missing explicit root and another
known file, the admission stream has total3/incomplete collection, one failed item
at index0 with no publication, then failed1/not_processed2 and
INPUT_ADMISSION_FAILED/exit1. Unexpanded folders never become fabricated items.
This intentionally changes the former gapped-inventory-index meaning while
retaining the protocol-1 field set.

## Core diagnostic vocabulary

The core enum owns these stable codes and permitted phases. A source test checks
this contract table against every code/phase pair. This is a vocabulary and pure
preparation classification, not independent runtime evidence for every scenario.
Pre-stream `InvalidRequest` remains separate. An allowed INTERRUPTED/publish pair
does not establish that interruption happened before the edge; the runner
enforces that condition and preserves all observed publication effects.

<!-- error-vocabulary:start -->
| Code | Allowed phases |
|---|---|
| READ_FAILED | admission/collection/read |
| SOURCE_NOT_REGULAR | admission/collection/read |
| SOURCE_CHANGED | admission/collection/read/publish |
| MALFORMED | prepare |
| LIMIT_EXCEEDED | prepare |
| PROTECTED | prepare |
| UNSUPPORTED_MIME | prepare |
| NO_BODY | prepare |
| INTERNAL | prepare |
| INPUT_ADMISSION_FAILED | admission/collection |
| INPUT_LIMIT_EXCEEDED | admission/collection |
| NO_INPUTS | admission/collection |
| OUTPUT_OVERLAP | admission |
| DESTINATION_COLLISION | admission/existing/publish |
| DESTINATION_EXISTS | admission/existing/publish |
| SOURCE_DESTINATION_ALIAS | admission/existing/publish |
| DESTINATION_UNSUPPORTED | admission/stage/publish |
| BIND_FAILED | admission/read/stage/address |
| STAGE_FAILED | stage |
| WRITE_FAILED | write/publish |
| FILE_SYNC_FAILED | sync |
| NAMESPACE_SYNC_FAILED | sync |
| ADDRESS_CHANGED | address |
| ADDRESS_UNVERIFIED | address |
| CLEANUP_FAILED | cleanup |
| EXISTING_SYNC_FAILED | existing |
| INTERRUPTED | read/prepare/existing/stage/write/sync/address/publish |
| OUTPUT_FAILED | delivery |
<!-- error-vocabulary:end -->

## Typed core execution observations

Core dry_run emits borrowed typed start/item/summary values through EventSink.
The read-only
operation explicitly requires a DryRun request before I/O/events and never
substitutes for general Create execution. Core counts completed terminal results
once before delivery, including an item whose delivery fails. Transport failure
is independent of RunControl, preserves completed results and prevents later
emission. A successfully delivered summary fixes its exit class; subsequent stop
requests do not retroactively change that completed stream. Summary delivery
failure remains explicit in the returned outcome.

Existing bytes_equal is unknown (null on the eventual wire) when no complete
comparison occurred; it must not be manufactured as false or true from an I/O
failure. Actual comparison/stability, requested-address state, publication effect,
file sync, namespace sync and cleanup are independent facts. None synchronization
is explicitly unattempted, not a successful native receipt. Namespace aggregation
must qualify its represented method/outcome across the counted ordered edge set;
a final edge's stronger method cannot erase a prior limited/failed request.

The same typed event/outcome hierarchy serves Core run for Create and DryRun;
normal execution does not silently become a plan. Native receipts, source/output
reservations,
metadata budgeting and counts remain business-library decisions. The CLI package
has the checked event renderer below. Process/application qualification remains
distinct from library adapter qualification.

## Event adapters and framing bound

JsonSink projects borrowed typed Core events into every required protocol1 field,
including explicit null/unattempted states and native identity/error domains.
Native numeric context uses a bounded typed accessor rather than localized/raw
io::Error Display. HumanSink uses terminal-safe paths and typed publication
knowledge. Both serialize/format a whole event before checked write/flush and
permanently reject further delivery after a failure; neither owns filesystem
policy, status decisions or counters.

The shared line limit counts encoded UTF-8 JSON bytes plus its one LF delimiter:
at most64MiB including LF. The producer reserves that byte before serialization.
Run accounting also includes every LF and remains strictly below256MiB. A valid
Core item is bounded below32MiB, so this delimiter clarification excludes no valid
Core result. U+0085/U+2028 may remain raw inside JSON strings; only byte0x0A frames
records. The independent maximal constructors use contract path/metadata budgets,
full existing/publication receipt forms and2048bytes per-item fixed margin; their
combined dimension maxima are conservative supersets, not ordinary message output
recorded as an oracle or exact peak-memory promises.

Current tests include hand-authored native planned-create golden lines, actual
Created/UpToDate projection, raw Unicode separators, human escaping, exact/+1
line storage, run storage and actual partial-write/flush progress with permanent
failure. Shared scenarios, extracted shipping smoke and the Rust frontend reducer
exercise their separate boundaries. Adapter tests alone do not qualify the
process protocol.

## Executable transport and termination

The executable accepts native positional paths or one EOF-terminated request
document, never both. It captures the working directory once. Request open/read
and stdin locking run on a transport-only worker; the main thread checks its
per-run cancellation token while waiting. Reads are bounded to64KiB with an8MiB
document ceiling plus one rejection byte. An exact-limit document still requires
an actual EOF. Cancellation does not join a reader blocked in an OS call; process
exit disposes of that transport worker without starting business work.

Handlers are installed before arguments or request I/O. The first delivered
callback requests Core stop; the second exits130, accepting incomplete reporting
and the documented crash residue. Standard Unix signals can coalesce. Windows
console close/logoff/shutdown may terminate the process under the OS deadline
before its queued Rust callback runs; graceful completion is not promised for
those events. Both native Windows rows execute the Ctrl-C/Break lifecycle cases.

Usage/request/core-prerequisite errors have no event stream and exit2. Transport
thread/handler setup failure exits1. Whole untrusted diagnostic strings are
terminal-escaped before one authored LF, so path/argument newlines cannot forge
diagnostic structure. Authored help/version use checked stdout. A successfully
delivered Core summary fixes the executable exit class even if a first stop
arrives afterward. Closed stdout before request EOF is tested to reject the start
event and prevent any source-copy work.
