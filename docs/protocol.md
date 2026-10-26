# Protocol 1

The public `emlattachmentremover` CLI and the desktop's bundled CLI use the same
protocol. Invoke `--format json` for LF-delimited UTF-8 start/item/summary events.
The GUI invokes its bundle-relative `Contents/Helpers/emlattachmentremover` with
`--request - --format json`, writes one request, closes stdin, and reads the stream.

## Requests

`--request FILE|-` accepts one complete EOF-terminated JSON document, at most 8 MiB.
It cannot be combined with positional paths, `--output-dir` or `--dry-run`.
All four top-level fields are required; unknown/duplicate fields are rejected:

```json
{"protocol":1,"paths":[{"encoding":"unix-bytes","base64":"bWVzc2FnZS5lbWw="}],"output_directory":null,"dry_run":true}
```

This checks `message.eml` relative to the CLI's captured working directory.
`output_directory:null` selects beside-source output. Otherwise supply a native
path object like those in `paths`. Path objects contain exactly `encoding` and
`base64`. Base64 must be canonical and padded. Unix uses `unix-bytes`; Windows
uses `windows-utf16le` over the original little-endian UTF-16 units. Foreign-host
encodings are refused. Display strings never supply operational paths.

Core validates native path bounds before I/O; the CLI also checks prerequisites
before starting a stream. At most 4,096 explicit paths and 4 MiB of native input
path bytes are admitted. Each expression is bounded by 32,768 native units and
256 components. See [architecture](architecture.md) for collection limits.

## Events

| Type | Required fields after `type` |
|---|---|
| `start` | `protocol`, `program`, `version`, `dry_run`, `total`, `collection_complete` |
| `item` | `index`, `source`, `status`, `destination`, `source_bytes`, `copy_bytes`, `removed`, `stripped_fields`, `plan`, `publication`, `existing_observation`, `error` |
| `summary` | `counts`, `interrupted`, `collection_complete`, `run_error`, `exit_code` |

`program` is `emlattachmentremover`. `version` is the workspace release version;
human headers and `--version` also include the shared product build number.
Counts contain `created`, `up_to_date`, `planned`, `failed`, `not_processed`.
Unobserved nullable fields are present as explicit null. Nested records and enums
are defined in [records.rs](../crates/eml-protocol/src/records.rs), with independent
[contract fixtures](../tests/contract/protocol). Consumers can ignore additive
object fields, but must reject unknown event types, invalid shapes, missing fields
and inconsistent state/count/receipt/exit combinations.

Item indices identify positions in the known work list, rather than argument or
emission ordinals. They increase strictly within `start.total`; gaps are valid
when admission prevents known inputs from starting. A collection-limit refusal
or collection stop can have no item events. Unknown folder contents are never
invented as work items. `collection_complete` separately records discovery state.

Each JSON line includes one LF and is bounded by 64 MiB; total framed run storage
stays below 256 MiB. Only byte 0x0A delimits records. U+0085/U+2028/U+2029 can occur
inside JSON strings. Native output paths carry both lossy display text and
lossless native encoding. Retain the native value for operations. Human output
escapes terminal controls and bidi controls; it supplies readable text rather
than shell quoting.

A missing summary, partial final line or malformed record leaves an incomplete
stream. Retain accepted earlier records and publication observations. A failure
can accompany a published copy. Existing-byte comparison, requested address,
publication, file sync, namespace sync and cleanup are independent facts. Null
comparison means unknown; unattempted synchronization means no attempt occurred.
Native numeric codes retain their `errno` or Win32 domain, without parsing OS prose.

## Exit and termination

Exit 0 means successful completion, 1 run/delivery failure, 2 pre-stream usage,
request or prerequisite failure, and 130 interruption. A valid final summary fixes
the normal process exit class. Request-reader/handler setup failures exit 1.
Pre-stream failures have diagnostics on stderr and no event stream. Writes and
flushes are checked; delivery failure stops further events.

The first interrupt requests cooperative stop; a second forces process exit 130.
OS calls may outlast cooperative stop, and signals can coalesce. Forced exit can
leave a truncated stream and staging residue. Windows console close/logoff/shutdown
can end the process under the OS deadline before graceful completion.

The desktop sends SIGINT once for Stop, uses `Child::kill` for confirmed Force
Stop, and reaps its owned child. Its bounded report reducer validates identity,
record ordering, counts and receipts. It drains stderr with a 64 KiB retention cap.
Technical export saves the exact received JSONL bytes, including an incomplete
stream; it never substitutes a different app report schema.

## Error vocabulary

The following table is checked against the core code/phase pairs. It defines
allowed values; individual records carry the observed result and phase.

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
