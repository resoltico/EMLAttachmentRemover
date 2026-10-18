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

The GUI uses the core through a dedicated minimal same-CPU worker as a
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
startup cannot silently weaken those requirements. The shared icon producer and
separate native app archives have passed on the pinned hosted toolchain; that
does not establish the screen interaction requirements.

Native file paths must never be reconstructed from displayed strings. Destination
changes affect later admissions only. A full report reservation pauses the next
request without discarding prior outcomes. A malformed frame, summary/count/exit
disagreement or incomplete EOF tail must remain explicit while retaining earlier
accepted outcomes. A successful process or checksum cannot replace those checks.

Both actual native macOS CPUs still need ad hoc app/worker signature identity, archive
readback and extracted execution, file/Services/drop/picker ingress, queue/report
limits, Stop/Force Stop/Quit, reveal/export failures and accessibility/appearance
checks. Remove needless or superseded code before delivering v6.

## Native panel scheduling

An Intel native test reached Command-O but failed in NSOpenPanel construction:
AppKit's panel configuration semaphore timed out after60 seconds. The panel
service started and checked in; the retained diagnostics do not establish why
configuration failed. Its call stack entered AppKit inside Slint's timer callback
from a CFRunLoop observer. The same scenario passed on Apple Silicon.

A default-mode main-run-loop experiment moved panel creation out of the observer
and controller call. The actual Intel stack then entered through
`__CFRUNLOOP_IS_CALLING_OUT_TO_A_BLOCK__`, yet AppKit retained the same60-second
configuration assertion. This did not establish a fix. The deferred states and
their extra feature declarations were removed; owned synchronous panels retain
their weak completion references, parent checks, cancellation and native paths.

An XCTest-process AppKit observation proved unsuitable: its sandbox rejected
the panel for lack of a user-selected-file entitlement, unlike the product.
That observation has been removed. A separate, ad hoc signed Swift AppKit test
app now records construction, presentation and cancellation through an ordinary
application lifecycle. Its executable runs as a directly owned controller child
on fresh hosted runners only, with actual empty entitlements checked. A settled
diagnostic failure is retained while product interaction still runs; uncertain
cleanup closes admission. Either outcome only narrows the investigation and
does not qualify the product picker or establish an operating-system defect.

## Real publication synchronization fixtures

The nonshipping worker feature forwards the existing Core fixture feature. The
normal engine has one implementation; its shipping entry supplies no observation
hooks. Fixture-only hooks surround one selected item's real publication boundary.
They provide no replacement message processor, event, receipt or summary. Unused
hooks are cleared before the next item, including after a protected-message refusal.

A separately copied app receives the native fixture worker and new ad hoc
signatures. Cargo feature admission excludes this worker from shipping builds.
Before/after bundle trees bind both the changed test app and the unchanged
shipping app. The test environment is supplied only to the exact owned launch;
shipping worker builds ignore it, and version probes never consume it. Private,
canonical, same-owner control directories hold bounded recipes and atomic
no-replace markers. Their ten-minute maximum hold accommodates hosted XCTest
interaction delays; it does not change product Stop/Quit deadlines.

The external Rust actor, not XCTest, releases a cooperative Stop only after the
real Stop flag and private acknowledgment write. For Force Stop/active Quit it
retains the candidate across the first interrupt, and records both actual handler
observations. Active Quit's second-minus-first observation must satisfy its
reviewed timing bound. For published-with-error, the actor first checks actual
published golden bytes, moves only the owned output directory and recreates its
old address, then releases Core's independent qualification. Actual
Published/AddressChanged events and retained golden bytes are required.

Each GUI scenario has its own owner launch, one explicitly selected XCTest,
unchanged original checks, strict whole-domain preference comparison and actual
supervisor settlement. A read-only worker-PID absence observation supplements
settlement; it grants no signal authority and is not sufficient by itself.
Uncertain ownership stops further cases. These observations establish fixture
lifecycle/presentation boundaries only when executed; the baseline workflow and
unmodified shipping archive checks remain mandatory. Native execution is pending.
