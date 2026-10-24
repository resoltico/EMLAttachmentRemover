# macOS frontend boundaries

The macOS frontend uses Slint over reusable Rust libraries. Windows and Linux
GUIs are outside scope. The CLI and GUI use one business engine. Public requests/events remain protocol 1;
private desktop framing is protocol 2 and metadata reports use app-owned schema 1.

eml-app owns immutable native request/destination snapshots, serial queue and
report reservations, LF framing and checked protocol reduction. It has no native
window dependency. Shared hand-authored protocol scenarios test acceptance,
rejection and every-byte-boundary framing. Request/path limits come from the
business library.

eml-gui owns display, controls and native integration: file ingress/choosers,
Services, exact Finder reveal, destination persistence and Quit/reopen. It never
reconstructs an operational path from display text or implements MIME/publication
policy. Destination changes affect later admissions only. Full report reservations
pause the next request without discarding prior results. Malformed frames,
summary/count/exit disagreement and incomplete EOF remain explicit while earlier
accepted observations are retained.

A dedicated same-CPU worker calls eml-core directly. Its process boundary allows
Force Stop during a stalled native operation. One supervisor retains child
authority through terminal reaping; it revokes signals before PID reuse. Bounded
readers and backpressure preserve completed observations without blocking the UI.
Unknown child/reader lifetime stops later admission. EOF or process exit alone
does not establish settlement. See [worker ownership](macos-process-ownership.md).

Slint and its compiler use one pinned version with explicit Winit/software/font/
accessibility features. The vendored Winit patch provides source-defined lifecycle callbacks and synchronous borrowed
native URL/pasteboard batches; see its [source contract](../vendor/winit/EML-PATCH.md).
Native panels retain weak completion references, parent checks, cancellation and
native paths. Renderer selection still requires actual native frames, text,
artwork, accessibility and appearance checks.

The source-defined application subclass is initialized before Slint/Winit creates
the shared AppKit application. Its standard About action supplies the existing
packaged project icon as an `NSImage` option after decoding and materializing its
bitmap representations. This avoids the standard panel's default application-icon
lookup. Winit keeps its delegate and the existing lifecycle/URL callback path;
there is no alternate About window or application-event implementation.

Both native CPUs require app/worker signatures, archive readback and extracted
execution, file/Services/drop/picker ingress, queue/report limits,
Stop/Force Stop/Quit, reveal/export failures and accessibility/appearance checks.
See [verification](verification.md) for candidate, desktop and picker admission.

## Controlled native operation fixtures

The nonshipping worker feature enables finite Core, filesystem, export and private
request-boundary observations. The normal engines have one implementation; their
shipping entries supply no observation hooks. Selected processing/export children
can pause at preflight, held-file readback, stage write, file sync, publication or
namespace sync. A separate export-input pause exercises actual parent pipe
backpressure and the report serialization lock. These are actual child/UI lifetime
observations at defined boundaries, not claims that a kernel fsync was forced to
hang. Released operations execute their ordinary native calls and guards. Hooks
supply no replacement processor, bytes, events, receipts or summary; unused
selected-item hooks never transfer to a later item.

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
unchanged original checks and actual supervisor settlement. Scenarios without a
native picker require an unchanged preference domain; export folder pickers use
the same narrow, observed standard-picker preference comparison as shipping
picker scenarios. A read-only worker-PID absence observation supplements
settlement; it grants no signal authority and is not sufficient by itself.
Uncertain ownership stops further cases. These observations establish fixture
lifecycle/presentation boundaries only when executed; the baseline workflow and
unmodified shipping archive checks remain mandatory. Exact-source native outcomes are recorded in the CI run and its retained artifacts.

The existing XCTest stdout relay also carries ten closed parent fixture actors.
The parent owns the fixture root descriptor and performs the fixed post-Check
sentinel, recipe/release/history, export-occupancy and retained-oracle writes.
Requests contain a finite actor and operand, never an arbitrary destination path
or write command. The Runner waits for the exact bounded, sequence-bound
acknowledgment and keeps its independent byte and namespace assertions. Control
history rotation additionally requires the current export operation's displayed
worker settlement and a separate parent PID-absence observation; PID absence
grants no process ownership. Failed actors, acknowledgments, root identity or
cleanup leave the scenario unqualified. This fixture authority does not change
product Check/Create, filesystem or worker authorization.

## Capacity-consistent export and processing acceptance

Two full report reservations include the active processing report. The handoff's
literal three-distinct-report example (export A, view B, process C concurrently)
cannot coexist with that required bound. The resolved contract preserves two
slots: A can export while B is viewed and C waits for capacity, or A can export
while C processes and the view switches between A/C. Export remains bound to A
through real child/writer/reader settlement. A slot becomes available only after
settlement and explicit dismissal; stale controls cannot affect a later child.
The staged lane regression covers both admissible states without a third slot.

This corrects the concrete acceptance scenario rather than relaxing reservation,
responsiveness or process ownership. Native execution of these prepared scenarios
remains a separate frozen-archive qualification obligation.
