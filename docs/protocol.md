# Protocol1 implementation contract corrections

The protocol mapper and CLI are still being implemented. These reviewed
corrections do not claim an implemented or qualified transport. The supplied
archive and checksum remain unchanged.

Cleanup residual names use platform-native component validation. POSIX rejects
slash and NUL; backslash is a valid filename byte. Windows rejects both separators
and NUL, preserving its UTF-16 native units. Do not suppress a legitimate POSIX
residual component to satisfy the archive's blanket backslash restriction.
The mapper must test both preservation and rejection when implemented.

An internal Windows native rename error retains exact NTSTATUS as primary and
translated Win32 error as secondary. Its bounded authored Error.detail may explain
the lossless eight-digit hexadecimal status; Error.code/phase remain typed stable
values. No publication.native_code field is added. Neither core nor app parses
detail or NT hex for decisions, and diagnostics never replace Published/Unknown
evidence. Existing synchronization/cleanup native_code fields retain their defined
meanings. Pending native I/O retains buffers/handles through completion; an
unprovable memory lifetime must not fabricate an unpublished receipt.

## Implemented request decoding

The `remove-eml-attachments` package currently provides a transport library, not
an executable. `decode_request` accepts a complete byte buffer plus the captured
absolute working directory. It checks the 8 MiB limit, strict protocol-1 JSON,
required fields (including explicit null for absent output directory), duplicate
and unknown keys, native encoding and canonical padded Base64. Native bytes are
converted directly to Unix bytes or Windows UTF-16LE, without Unicode conversion,
then core resource admission runs. Error values do not retain or echo JSON/path
content; malformed-document diagnostics expose only parser line/column.

This does not implement interruptible stdin reading, signals, execution, event
rendering, filesystem admission or output delivery. Decoder tests cannot qualify
those still-required capabilities. Core and filesystem crates have no JSON
dependency. Request shape and native-byte tests live in the frontend package;
business path limits remain in core. Their pure error type is RequestPathError;
the read-only collector's outer InvalidRequest additionally retains genuine
working/output prerequisite I/O causes. Decoder validation still performs no I/O.

`PathValue` now serializes core-supplied native and display-expression facts into
the exact output object, with lossy display text and a lossless native value. Encoding
names have one shared string authority with strict request decoding. Unix bytes
and Windows UTF-16LE units are encoded directly, including non-Unicode units.
No display string is used to reconstruct the native value. Spelling, including
symlink/dot-dot expressions, is retained. JSON escaping keeps embedded LF inside
the string; raw U+0085/U+2028/U+2029 remain valid string content. This primitive
does not implement human terminal escaping, LF event framing, whole-event/run
size admission or the CLI executable. Its tests perform no filesystem I/O.

### Core-supplied display and native path facts

The reviewed Windows rule uses an admitted extended SELECTED expression plus a
literal walked suffix for reported native values. Preserve aliases; a canonical
target query is a separate binding fact. Human display retains the original
request-derived expression. The path serializer must accept those two core facts
without reconstructing one from the other or adding frontend normalization.
An unadmitted failed root may report its failed requested native input when no
usable operand exists; this is intended input, not a valid I/O capability.
Successful items must use their admitted native expression. The borrowed adapter
now requires both facts explicitly; same-path callers pass the same path twice.
Its hand-authored split-fact golden rejects using display for native encoding.
The read-only collector now supplies these facts; source/native qualification
and actual split-path filesystem integration remain pending.

For each source/known destination let d be complete display-expression native
bytes and n complete reported-native bytes; charge b=max(d,n) once to the8MiB
budget, including failure items and repeated logical prefixes. Windows counts
two bytes per UTF-16 unit. Since6d+4n/3 <= (6+4/3)b, the existing protocol envelope
still bounds both representations; the fixed margin covers Base64 rounding.
Expanded reported cwd counts, while transient unreported operands are not charged
a second time. This is a report-retention bound, not an8MiB peak-allocation
promise for two independent buffers. Maximal event/run and exact/one-over tests
must use independent display/native facts when those capabilities are built.

`TerminalPath` separately formats a borrowed path for human terminal text. It
escapes C0/C1 controls, Unicode line/paragraph separators and Unicode bidi-control
marks using visible Rust-style escapes. Printable Unicode, ordinary backslashes
and joiners remain readable. It is lossy display, not shell quoting, an
operational expression or a guarantee against every Unicode visual ambiguity.
Formatting errors propagate to the caller. Complete human event/error rendering
and actual stdout delivery still need implementation and integration tests.

## Collection-admission event indices

The read-only collector now supplies bounded work positions/provenance. Event
emission and the runner/reducer remain unimplemented. Item indices identify
positions in the known work list.
They are neither emission ordinals nor raw argument indices. Strictly increasing
indices within `start.total` may have gaps when admission leaves known inputs
unstarted. For known file0, missing explicit root1 and known file2, the valid
admission stream has total3/incomplete collection, one failed item at index1
with no publication, then failed1/not_processed2 and INPUT_ADMISSION_FAILED/exit1.
Do not reindex that failure to0 or fabricate the unexpanded contents of folders.
Collection limit and collection stop remain the separately specified no-item
cases. Future shared protocol/reducer fixtures must accept this gap and reject
repeated, decreasing and out-of-range indices.

## Core diagnostic vocabulary

The core enum owns these stable codes and permitted phases. A source test checks
this contract table against every code/phase pair. This is a vocabulary and pure
preparation classification, not evidence that all runner scenarios are implemented.
Pre-stream `InvalidRequest` remains separate. An allowed INTERRUPTED/publish pair
does not establish that interruption happened before the edge; the runner must
later enforce that condition and preserve all observed publication effects.

<!-- error-vocabulary:start -->
| Code | Allowed phases |
|---|---|
| READ_FAILED | admission/collection/read |
| SOURCE_NOT_REGULAR | admission/collection/read |
| SOURCE_CHANGED | admission/collection/read/publish |
| MALFORMED | prepare |
| LIMIT_EXCEEDED | prepare |
| PROTECTED | prepare |
| NO_BODY | prepare |
| INTERNAL | prepare |
| INPUT_ADMISSION_FAILED | admission/collection |
| INPUT_LIMIT_EXCEEDED | admission/collection |
| NO_INPUTS | admission/collection |
| OUTPUT_OVERLAP | admission |
| DESTINATION_COLLISION | admission/existing/publish |
| DESTINATION_EXISTS | admission/existing/publish |
| SOURCE_DESTINATION_ALIAS | admission/existing/publish |
| DESTINATION_UNSUPPORTED | stage/publish |
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
