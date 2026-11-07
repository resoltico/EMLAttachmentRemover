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

## Native polish acceptance

Native acceptance was completed on 7–8 November 2026 with ad hoc signed universal
apps (Slint 1.18.1, version unchanged). Apple Silicon observations used product
commit `fc952d1` on macOS 27.2. The additional Intel run used `ff31272` on macOS
15.7.9; its product sources are identical to `fc952d1`. Both universal slices,
archive signatures and actual floor launches passed. The final WP7 commit changes
only documentation and captures relative to that tested product tree.

**N1:** A1 was first reproduced on the unmodified `97bfb2a` archive: after 3,000
copies, Open With left an empty “Looking for emails…” window. See the
[baseline frame](polish/a1-baseline.jpg). On the final product, each of five
3,000-email creates reached Done before `open -a` delivered two emails. Both
rows appeared Ready within the required two seconds:

| Repetition | Observed within | Frame |
|---|---|---|
| 1 | 146 ms | [JPEG](polish/n1-final-1.jpg) |
| 2 | 187 ms | [JPEG](polish/n1-final-2.jpg) |
| 3 | 196 ms | [JPEG](polish/n1-final-3.jpg) |
| 4 | 196 ms | [JPEG](polish/n1-final-4.jpg) |
| 5 | 196 ms | [JPEG](polish/n1-final-5.jpg) |

A driver outside the repository timed the actual native `open -a` command;
CUA read the native accessibility tree and captured the window. In repetition
2, the first accessibility read at 138 ms still showed the prior batch; the
saved frame at 187 ms already shows both Ready rows. This is recorded as frame
evidence, not as a successful first accessibility read. These runs verified
existing copies created during baseline reproduction.

The [Intel floor run](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/42435560220)
then exercised the exact universal archive on the real `macos-15-intel` runner
(macOS 15.7.9, native `x86_64`). Each repetition checked 2,000 emails, pressed the
native Remove Attachments button, observed Done and delivered two paths through
`open -a`. All five met the two-second bound:

| Repetition | Ready rows observed within | Before delivery | After delivery |
|---|---|---|---|
| 1 | 194 ms | [Done](polish/intel/intel-n1-1-done.png) | [Ready](polish/intel/intel-n1-1.png) |
| 2 | 389 ms | [Done](polish/intel/intel-n1-2-done.png) | [Ready](polish/intel/intel-n1-2.png) |
| 3 | 327 ms | [Done](polish/intel/intel-n1-3-done.png) | [Ready](polish/intel/intel-n1-3.png) |
| 4 | 261 ms | [Done](polish/intel/intel-n1-4-done.png) | [Ready](polish/intel/intel-n1-4.png) |
| 5 | 518 ms | [Done](polish/intel/intel-n1-5-done.png) | [Ready](polish/intel/intel-n1-5.png) |

An omitted-delivery control remained at Done and was rejected. The
[native transcript](polish/intel/acceptance.txt) records the timings, native
geometry, appearance samples and source preservation. All 2,000 large-batch
originals and both delivered sources remained byte-identical.

The manual diagnostic used the app's native accessibility actions and observed
its rendered labels, then captured its own WindowServer window. It did not replace
the helper, modify the app bundle, inject controller hooks or use the Slint testing
backend. Native accessibility and capture permissions were already enabled; no
security setting was changed. A temporary extension to the existing floor script
ran the probe through the branch-dispatched archive workflow. It was removed
before delivery; no workflow, merge gate or permanent harness was added. The
workflow's tag-only release-creation job was skipped.

**N2:** Stop left 782 of 3,036 copies saved (25.8%). The
[frame 464 ms after Continue](polish/n2-mixed-continue.jpg) shows those rows still
reading “Copy saved” while the current worker begins. Continue subsequently saved
the other 2,254 copies. The fixture mixed 3,000 small emails with 36 larger
multipart emails, and the view was scrolled within the saved portion. Earlier
small-file attempts reached re-verified “Copy already exists” before capture;
they did not demonstrate the required retained-label interval. T2 separately
gates the stub before its first new record and verifies “Starting…”, saved-label
fallback, current-result precedence and Start Over clearing the batch.

**N3: complete.** All six native captures below are current, unscaled and free
of capture indicators. Full-screen frames were captured on Apple Silicon macOS
27.2; normal/minimum frames came from the Intel floor run. All were visually
reviewed. The latter were captured as native PNG window frames and converted to
JPEG without cropping, scaling or painting out any pixels.

| Native window | Light | Dark |
|---|---|---|
| Full screen, 1920×1080 | [JPEG](manual/full-light.jpg) | [JPEG](manual/full-dark.jpg) |
| Normal, 1080×728 frame / 1080×700 client | [JPEG](manual/normal-light.jpg) | [JPEG](manual/normal-dark.jpg) |
| Minimum, 760×548 frame / 760×520 client | [JPEG](manual/minimum-light.jpg) | [JPEG](manual/minimum-dark.jpg) |

The Intel probe measured the native content group's rectangle independently of
the outer window: macOS 15 has a 28-point title bar here, versus 32 points on the
previous macOS 27 captures. It derived the frame size from that measurement, then
asserted the requested client size. Appearance was checked against captured
background pixels as well as the requested native setting. The minimum frames
show complete rows, the selected email's detail and both detail actions.

Earlier local edge-drag and Moom-shortcut attempts did not establish a native
minimum size, and Preview display captures retained capture UI. Those failed
attempts remain recorded privately; the Intel window captures supersede the gap.
On resumption, Moom's settings were accessible through Finder's Open Finder item
action, but its synthetic shortcut still did not resize this controlled window.
The temporary action was removed. No failure was reclassified as a pass and no
capture indicator was edited out.

The nine toolkit captures above were regenerated from the final product tree and
visually reviewed. T3 checks the actual deepest-folder label's bounds at 760×520;
T5 checks the shared left edge and stable header geometry at text sizes 13 and 18.
The native evidence above independently establishes the requested native sizes.

The [negative-control transcript](polish/negative-controls.txt) records both WP0
failures and the executed, restored T1–T8 mutations. The final local
`cargo xtask ci` passed 355 tests, zero failures and one explicitly manual capture
ignored; that capture test was run separately and passed. Budget:
product 20,069 → 20,160 (+91); harness 10,778 → 11,083.
The initial full-gate failure exposed separate header/footer report reads and a
fast Continue fixture. WP2 now shares one progress snapshot; S7 uses an explicit
fixture release before new results, preserving its filtering assertions. The
failed run is retained outside the repository. No timing retry or longer deadline
was introduced. Light appearance and the original app preferences were restored;
task-created native windows were closed.

## Earlier one-list native observations (historical)

The following observations describe the preceding one-list implementation, not
a new validation of the polish changes. The earlier minimum frames at
[`97bfb2a`](https://github.com/resoltico/EMLAttachmentRemover/tree/97bfb2a/docs/gui-screenshots/manual)
had been set through a temporary Moom Resize action and reviewed at 760×520 client
size. The current table supersedes those images with the measured Intel captures.

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
