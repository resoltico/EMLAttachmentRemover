# Protocol 1

The public `emlattachmentremover` CLI emits LF-delimited UTF-8 start/item/summary
events with `--format json`. `--request` supplies the same public CLI contract.
The desktop independently calls core in process; Save Report encodes those typed
events with the same `JsonSink`, without invoking or parsing the CLI.

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
human headers and CLI/GUI `--version` also include the full source revision when supplied.
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

Raw `failed` counts started terminal failures, including an item with typed
`INTERRUPTED`; `not_processed` counts known positions never started. Human CLI
summaries and GUI categories group explicitly interrupted failed items as Not
processed instead. This presentation does not imply that cleanup completed or
that no existing copy was observed. Exit selection and JSON records stay raw.
Only an accepted summary establishes never-started positions; missing results in
an incomplete stream remain unknown.

Each JSON line includes one LF and is bounded by 64 MiB; total framed run storage
stays below 256 MiB. Only byte 0x0A delimits records. U+0085/U+2028/U+2029 can occur
inside JSON strings. Native output paths carry both lossy display text and
lossless native encoding. Retain the native value for operations. Human output
escapes terminal controls and bidi controls. Shared human path rendering exposes
invalid Unix bytes as `<0xFF>` and unpaired UTF-16 units as `<U+D800>`; literal
`<` is doubled. This supplies readable native-unit distinctions rather than
shell quoting. The JSON display field and native encoding are unchanged.

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

Desktop Stop uses the run's cooperative control directly. Its report export saves
the exact bytes written by `JsonSink`, including an incomplete prefix after a
failure. It does not substitute a GUI-owned report schema.

## Error vocabulary

The following table is checked against the core code/phase pairs. It defines
allowed values; individual records carry the observed result and phase.

<!-- error-vocabulary:start -->
| Code | Allowed phases |
|---|---|
| READ_FAILED | admission/collection/read |
| SOURCE_NOT_REGULAR | admission/collection/read |
| SOURCE_CHANGED | admission/collection/read/publish |
| MAILBOX_FILE | prepare |
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

`MAILBOX_FILE` refuses an mbox envelope containing several messages before writing.
A single message with a From envelope remains supported. Removal reason
`after_message_end` reports non-whitespace multipart epilogue bytes, with their
actual byte count and section (empty for the outer root). These bytes have no
MIME part type; their report type is `application/octet-stream`. Whitespace-only
epilogues are omitted without a removal entry. Preambles are preserved.
