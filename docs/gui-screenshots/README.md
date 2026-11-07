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

The requested **native 760×520 capture is not complete**. Edge drags did not
reliably reach that width through CUA. An earlier app build reached a 920×520
client area; the final normal captures above are labelled with their actual size.
Exact 760×520 is covered by the toolkit image and geometry/constraint checks,
which is separate evidence from native OS resizing.

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
surface. macOS 15 and physical Intel Mac document opening also remain unverified.
Both binary slices and the archive signature were checked separately.

The original app preferences and light appearance were restored after testing;
task-created native windows and synthetic source/copy fixtures were cleaned up.
