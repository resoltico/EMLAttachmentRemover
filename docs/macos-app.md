# macOS application implementation

The fresh SwiftPM package implements the Foundation RunModel target and Swift
Testing. The SwiftUI executable now includes selection/run/detail/artwork, native
helper supervision, confirmed-quit admission/escalation and the narrow system
bridge. Actual signed-bundle GUI qualification remains unfinished. No old AppKit
screen is part of these components;
existing UI sources remain only until their required knowledge/artwork is ported
and the full purge is complete. See macos-process-ownership.md for the reviewed
native process mechanism exception and its actual qualification scope.

RunModel decodes exact protocol1 wire keys, including required nullable fields,
and ignores additive fields. Error codes remain open failure guidance; phases,
statuses, removal reasons and receipt enums are closed. The ordered reducer
retains earlier items after a later failure, permits increasing gapped indices,
checks counts and actual exit, and distinguishes typed publication/existing-copy
evidence. It never performs MIME or filesystem business operations. Stop UI
state cannot rewrite a valid summary. A partial last frame is discarded; only LF
bytes delimit records. Frame size includes LF, at most64MiB, and total incoming
bytes remain strictly below256MiB.

Report storage additionally bounds4096 items,20000 removals,8MiB reported native
paths and24MiB UTF8 display paths. The last limit allows three-byte U+FFFD
expansion of invalid native bytes; lossy display cannot reconstruct Rust's
original path charge. Queue admission freezes native destinations, allows eight
waiting requests, serializes one active reservation and pauses before a third
full report until an explicit completed-report dismissal. These are retained
data bounds, not exact heap-size guarantees.

The URL adapter accepts only local credential/query/fragment-free file URLs and
uses Foundation's native filesystem representation. For native-to-URL navigation
it constructs NSURL then bridges to URL: on the installed Foundation, the direct
Swift URL native initializer replaced invalid UTF8 bytes. Synthetic FF/LF/ESC
round-trip tests preserve the actual bytes through the NSURL bridge. Actual
Finder/drop/import ingress still requires native qualification. Relative CLI
protocol paths remain valid data but cannot become app navigation addresses.
Display strings never supply operational paths.

The fresh destination decoder accepts the same String form from argument or
saved defaults. Presence of copyDestinationBytes in the volatile argument domain
makes choices session-only, including an explicit empty beside-originals value;
the UI identifies that policy. Normal launches save only real destination
choices. Invalid initial values require an explicit folder/Beside originals
choice before admission and never cause an initialization write or silent output
redirection. There is no old-key reader or migration. Actual picker/argument and
saved-domain integration still requires owned native GUI qualification.

Local model checks use the installed Xcode explicitly, without changing
xcode-select:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun swift format lint --strict --recursive macos-app
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun swift test --package-path macos-app --scratch-path target/swift-model -Xswiftc -warnings-as-errors
```

Native Mac CI selects the exact Xcode version/build from xtask/tools.toml through
`cargo xtask distribution`, then checks its actual version before model tests on
each CPU. Model execution is separate from signed-app/bundled-helper, Services,
Finder, preferences, lifecycle, screenshots and accessibility qualification.

One SwiftUI Window owns the visible workflow; the shared Store survives close.
Apple documents automatic termination for a primary single Window, so the narrow
termination bridge returns false after last-window close. SwiftUI's macOS15
defaultLaunchBehavior(.presented) handles reopening, with scene restoration
disabled and no NSWindow construction. The actual Dock/closed-origin action
lifetime still needs native GUI verification.
Sources: [Window](https://developer.apple.com/documentation/swiftui/window),
[default launch behavior](https://developer.apple.com/documentation/swiftui/scene/defaultlaunchbehavior(_:)).

Swift Testing now also imports the actual executable module for Store/token
tests. Those cases serialize Foundation's shared volatile argument domain, use
fresh UUID persistent suites and owned synthetic files, and restore/remove only
their owned context after observed settlement. Timeout paths stop later admission
and retain evidence/context when settlement cannot be confirmed. These25functions
and22 shared cases cover state/defaults and pre-helper gates; they do not prove
native timers, actual picker delivery, signed app lifecycle or GUI accessibility.

`cargo xtask macos-process-tests` builds and runs owned native process fixtures
using the pinned Xcode and native Cargo host. It checks actual helper/driver
Mach-O CPU and deployment minimum, runtime translation, golden bytes, transport
and owned teardown. It is not signed GUI or shipping-archive qualification. The
required local command currently fails its prerequisite when Xcode26.3 is absent;
newer installed Xcode fixture runs are additional evidence only.

`cargo xtask build-macos-app --out DIR` requires a fresh directory and the same
shared native build context. It builds release CLI/SwiftUI, compiles the retained
icon, generates and reads back bundle metadata, signs the CLI once before copying
it, signs the outer bundle, verifies signatures/CPU/minimum/helper equality and
checks both final executables' linked dependencies. Complete notices, extracted
archives and real GUI execution are separate required gates; a build.json record
does not mark this internal integration build as shipping-qualified.
