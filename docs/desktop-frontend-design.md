# macOS desktop frontend

The v6 macOS frontend uses Slint over reusable Rust libraries. Windows and Linux
GUI implementations remain outside scope; six native CLI targets and two native
macOS app targets remain required.

The reusable Rust engine, CLI protocol, immutable MIME edits and filesystem
publication contracts remain authoritative. The frontend replacement must carry
forward queued requests with captured destinations, bounded retained reports,
native path authority, exact reveal/export behavior, cooperative Stop, confirmed
Force Stop, active-run Quit settlement and file/Services/drop ingress.

## Chosen boundaries

An application model owns immutable queued request snapshots, admission and
report reservations, LF framing, strict protocol-1 reduction and presentation
facts. It is ordinary Rust without Slint or native window dependencies. The
existing shared hand-authored protocol scenarios remain independent inputs;
port their acceptance, rejection and every-byte-boundary checks to this model.
Reuse the business library's request/path limits instead of copying policy.

The Slint frontend owns controls, selection and display, not message processing,
publication decisions or output naming. Native macOS bridges are limited to
actual system needs: native file ingress/choosers, Services, exact Finder reveal,
saved destination preferences and ordinary Quit/reopen behavior.

The owner's subsequent boundary clarification requires the GUI to use the core
rather than the user-facing CLI. Use a dedicated minimal same-CPU worker as a
separately owned child, calling the same reusable core. The CLI remains a
separate thin command-line adapter; the app does not need its executable.
Shared native-path/protocol transport belongs in eml-protocol, independently of
both executable adapters. There is one business engine and one current protocol.
An in-process worker
cannot preserve Force Stop for a stalled native operation without terminating
the app or violating Rust thread safety. One supervisor owns each child through
terminal reaping; signals are revoked before its numeric PID can be reused.
Bounded stdout/stderr handling and backpressure must retain completed observations
and avoid UI-thread blocking. An unknown child or reader lifetime stops later
admission; neither an early EOF nor a helper exit alone establishes settlement.

Slint and its UI compiler must resolve at one pinned version. Select only needed
backend/rendering features and review the actual dependency and legal inputs.
AuditoryLab is a source reference, not an implementation template: its sound
workers, Python tooling, Windows import preparer and Linux font patch do not
belong in this macOS frontend. Its ordinary macOS Quit patch is a candidate for
specific source review, not an assumed drop-in guarantee.

## Challenge and qualification

Choosing Slint does not establish that a Metal failure is resolved. Its GPU path
can also use Metal. Renderer selection must account for filenames and diagnostic
text, accessibility, owner artwork and actual native frames; a software-only
startup cannot silently weaken those requirements. The current hosted icon
compilation failure is independent of the screen framework.

Native file paths must never be reconstructed from displayed strings. Destination
changes affect later admissions only. A full report reservation pauses the next
request without discarding prior outcomes. A malformed frame, summary/count/exit
disagreement or incomplete EOF tail must remain explicit while retaining earlier
accepted outcomes. A successful process or checksum cannot replace those checks.

Both actual native macOS CPUs still need ad hoc app/worker signature identity, archive
readback and extracted execution, file/Services/drop/picker ingress, queue/report
limits, Stop/Force Stop/Quit, reveal/export failures and accessibility/appearance
checks. Remove needless or superseded code before delivering v6.
