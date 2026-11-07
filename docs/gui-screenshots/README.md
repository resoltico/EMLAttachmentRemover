# Desktop review captures

These captures show the production one-batch controller and Slint views using
pinned Cupertino widgets and the software testing renderer. They are toolkit
captures, not evidence of native macOS panels or Finder/Dock integration. Protocol
fixtures provide deterministic inputs, progress and outcomes; they do not prove
MIME classification. Native app observations are recorded in the redesign PR.

| State | Light | Dark |
|---|---|---|
| Empty | [PNG](empty-light.png) | [PNG](empty-dark.png) |
| Ready | [PNG](ready-light.png) | [PNG](ready-dark.png) |
| Removing | [PNG](removing-light.png) | [PNG](removing-dark.png) |
| Done | [PNG](done-light.png) | [PNG](done-dark.png) |

The eight main images are 1080×700. [Ready at 760×520](ready-light-narrow.png)
shows the minimum-width layout with Size hidden and the detail actions visible.
All nine images were visually reviewed. Geometry tests separately cover 760×520
and 2560×1440, text sizes 13 and 18, and a 40-entry detail fixture.

To regenerate outside the merge gate:

```sh
GUI_CAPTURE_DIR=/absolute/output/folder cargo test --locked -p eml-gui --test desktop \
  acceptance::s20_capture_one_list_states -- --ignored --nocapture
```

The capture writes lossless PPMs. Convert each to PNG with, for example,
`magick input.ppm output.png`; no scaling is used for the committed images.

The [historical F3 investigation](f3-investigation.md) explains an earlier clipping
bug. Its sidebar/inspector descriptions concern the superseded desktop design.

## Native app observations — 7 November 2026

The fresh ad hoc signed universal app was exercised on Apple Silicon macOS 27.2.
These raw CUA JPEGs show the final product code; window chrome is included in the
normal captures. Both appearances were visually reviewed.

| Native window | Light | Dark |
|---|---|---|
| Full screen, 1920×1080 | [JPEG](manual/full-light.jpg) | [JPEG](manual/full-dark.jpg) |
| Normal, 1080×732 frame / 1080×700 client | [JPEG](manual/normal-light.jpg) | [JPEG](manual/normal-dark.jpg) |
| Minimum, 760×552 frame / 760×520 client | [JPEG](manual/minimum-light.jpg) | [JPEG](manual/minimum-dark.jpg) |

The requested native minimum-size captures are now complete on product commit
`86108db`: the installed Moom utility's temporary Resize action set the actual
frame to 760×552, including the 32-pixel title bar. The resulting 760×520 client
area shows three complete rows, the selected email's attachment details and both
detail actions. Both appearances were visually reviewed; Size is hidden as
required. These are raw native JPEG captures, without cropping or scaling. The
temporary resize action was removed afterward. Earlier CUA edge-drag attempts
had failed; the screenshots above supersede that capture gap.

Observed: native file and folder panels; automatic preview; destination changes;
seven saved small-batch copies (one as is, one malformed input refused); native
report saving; output-roots selection; Show Copy and Show in Finder. The latter
visibly opened the folder containing all seven copies. The copies matched
independent golden/original bytes. Eight small and 96 large exercised source
emails remained byte-identical; the unused Dock fixture also remained unchanged.

A native Stop left the large batch stopped; Continue then qualified all 96
existing copies without new writes. Quit while Removing showed the confirmation
at 89/96; Cancel kept the app open, and Stop and Quit exited cleanly. Forced Quit
Now timing and reaping are covered by the headless process tests.

Finder Open With on an email and cold launch-by-document reached the preview
without a system alert; Mail remained the default. Folder delivery through
`open -a` did the same. Finder on this host has no Open With context item for a
folder. **An actual Dock drop is still unverified**: CUA could not acquire its
surface. Finder/Dock gestures were not manually exercised on macOS 15; the
native adapter's minimum-OS runtime evidence is recorded below.
Both binary slices and the archive signature were checked separately. The later
minimum-OS archive smoke described below adds runtime evidence on both CPUs.

The original app preferences and light appearance were restored after testing;
task-created native windows and synthetic source/copy fixtures were cleaned up.

## Cross-platform follow-up

The original final-tree `cargo xtask ci` pass was on macOS 27.2. GitHub Actions
run [42349434185](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/42349434185)
then failed S13 on both Linux x86_64 and ARM64: the stressed layout test required
an outer list height of 100 pixels. Reproduction with Slint 1.18.1 measured a
96-pixel Linux viewport containing two complete 48-pixel rows. Cupertino adds a
four-pixel frame around the same 96-pixel viewport. The fixed test checks actual
row bounds against the clipping viewport, verifies fixed row heights and rejects
overlap in both S13 geometry cases. Product layout did not change.

All three S13 tests pass locally on macOS and Linux ARM64. As a negative control,
reducing the list height by one pixel made the stressed-layout test fail on both:
the viewport became 95 pixels and only one complete 48-pixel row fit. The mutation
was restored. The existing toolkit and native captures still show the unchanged
product; they do not establish Linux native rendering or macOS 15 behavior.

Initial bounded native attempts could not complete Dock delivery or exact
minimum-size captures. The minimum-size gap was subsequently resolved with the
existing Moom utility as described above. Dock acquisition by path and bundle ID
still returned CUA `timeoutReached`, including after keyboard focus. A separate
investigation used Preview's native full-screen capture to locate the actual
Dock and attempted a Finder-file drag to its app icon, with both the original
left-side Dock and a temporarily bottom-positioned Dock (positive destination
coordinates). The app did not receive the fixture. This is **not a passing Dock
check**, and gesture failure cannot yet be distinguished from a product failure.
No assurance was removed or substituted. Desktop diagnostic captures remain
private outside this repository; they include unrelated windows. Original Light
appearance and Left Dock position were restored, the temporary Moom action was
removed, and task-created app, Finder, Preview and Settings windows were closed.
Three stale launch-service alerts created by CUA reads after earlier Quit actions
were dismissed; those cleanup alerts are separate from document-delivery evidence.

An independent control reproduced the drag limitation without involving this
app: within one Finder icon-view window, dragging an isolated email icon onto
an isolated folder icon left the file in place and the destination empty. This
also failed with the file explicitly selected. Source bytes remained unchanged.
The control's capture and observations are retained privately outside the repo.
A fresh CUA session with fresh fixtures and explicit source selection reproduced
the same failure; resetting the automation bindings did not resolve it.

The owner subsequently waived the manual Dock gesture requirement for this
redesign. Dock delivery remains unverified; the waiver permits merge and does
not convert the unsuccessful automation attempts into passing evidence.

A fresh local build directory resolved the reproducibility question around the
local gate: `CARGO_TARGET_DIR=/Users/erst/Tools/one-list-clean-verify-target cargo
xtask ci` passed with 346 tests, zero failures and one explicitly manual capture
ignored, including fresh universal archive signing and verification. An earlier
run using the existing target directory and one focused diagnostic had failed
the unchanged CLI FIFO/SIGINT test (signal termination instead of exit 130).
That failure evidence is retained; no test or product code changed to obtain the
clean-build result. Cache contamination is not established as the root cause.

The existing branch-dispatched [archive workflow 42385688624](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/42385688624)
passed for commit `86108db`, including exact-archive floor smoke on macOS 15.8.2
ARM64 and 15.7.9 x86_64, CLI golden checks, native app launch with operands, idle
Quit and source preservation. Its tag-only release drafting job was skipped;
no release was created. This establishes runtime evidence on both CPU types,
not just compilation. That first smoke supplied document paths as launch
arguments. The floor smoke now additionally runs
[a native document-event probe](../../scripts/smoke-macos-document-event.sh):
after startup, one `open -a` event must produce the real GUI's read-only helper
request containing both the initial and newly delivered native paths. This uses
a separate re-signed copy with an isolated bundle identifier and recording helper;
the original exact archive checks remain intact. Omitting native delivery was
shown to fail locally. [Floor run 42389677519](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/42389677519)
passed the native event probe on macOS 15.8.2 ARM64 and 15.7.9 x86_64 at commit
`052ee84`, in addition to the unchanged exact-archive checks. This checks our
native adapter without OS UI automation; it does not establish an actual Dock
gesture result. The available interactive local host is macOS 27.2.
