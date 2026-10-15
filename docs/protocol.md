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
business path limits remain in core.

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
