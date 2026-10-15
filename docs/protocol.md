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
