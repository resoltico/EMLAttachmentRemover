# macOS application implementation

The fresh SwiftPM package currently implements the Foundation RunModel target
and Swift Testing. The executable SwiftUI screens, process supervisor and narrow
system bridge remain to be implemented. No old AppKit screen is part of this
model; existing UI sources remain only until their required knowledge/artwork is
ported and the full purge is complete.

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

The fresh destination preference decoder accepts the String form from either
argument or saved defaults and performs no defaults I/O. The app must write
`copyDestinationBytes` only after an actual picker choice. There is no old-key
reader or migration. Model tests perform no owner-default reads/writes.

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
