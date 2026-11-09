# Desktop captures

Toolkit captures use the production controller, pinned Slint widgets and software
testing renderer. Real temporary MIME inputs drive core; an injected runner holds progress for
deterministic captures. Native operating-system UI is checked separately.

| State | Light | Dark |
|---|---|---|
| Empty | [PNG](empty-light.png) | [PNG](empty-dark.png) |
| Ready | [PNG](ready-light.png) | [PNG](ready-dark.png) |
| Removing | [PNG](removing-light.png) | [PNG](removing-dark.png) |
| Done | [PNG](done-light.png) | [PNG](done-dark.png) |
| Processing details | [PNG](processing-details-light.png) | [PNG](processing-details-dark.png) |

The main images are 1080×700. [Ready at 760×520](ready-light-narrow.png) shows the
minimum layout with Size hidden. Geometry tests separately check three visible rows at minimum size,
stable headers and details, aligned cells, command roles and anchored popups.

Regenerate outside the merge gate:

```sh
GUI_CAPTURE_DIR=/absolute/output/folder cargo test --locked -p eml-gui --test desktop \
  acceptance::s20_capture_one_list_states -- --ignored --nocapture
```

The capture writes lossless PPMs. Convert each to PNG with, for example,
`magick input.ppm output.png`; committed images are not scaled.

## Native app gallery

These native captures show the ad hoc signed macOS debug app built from the branch, with
its production controller and core facts. Images retain the complete client area
at PR 75's sizes. OS title bars are excluded;
application content is not retouched or scaled.

| Window | Light | Dark |
|---|---|---|
| Full-size client, 1474×938 | [JPEG](manual/full-light.jpg) | [JPEG](manual/full-dark.jpg) |
| Normal, 1080×700 client | [JPEG](manual/normal-light.jpg) | [JPEG](manual/normal-dark.jpg) |
| Minimum, 760×520 client | [JPEG](manual/minimum-light.jpg) | [JPEG](manual/minimum-dark.jpg) |

The selected inline-image detail includes the core warning about omitted related
resources. Normal/minimum captures keep actions beside the email name above the scrolling
details. Capture appearance and application preferences were restored afterward.

The in-process follow-up adds native [light](manual/processing-details-light.png)
and [dark](manual/processing-details-dark.png) Processing details captures at
1080×700 client size. They show core’s Code, Phase and Detail fields. The full/normal/minimum gallery and toolkit views were recaptured for the field fixes.

## Native field replay

These complete 1080×732 window captures use the fresh overflow-checked branch
debug app and the field tester’s original gui_probe.

| State | Capture |
|---|---|
| Checked problems filter and whole-batch scope | [PNG](manual/field-filter.png) |
| Finished mixed batch with truthful failure count | [PNG](manual/field-done.png) |
| Existing copies plus refusals | [PNG](manual/field-rerun.png) |
| Full-disk cause and completed cleanup | [PNG](manual/disk-full.png) |
| Unavailable folder retains checked emails | [PNG](manual/unavailable-folder.png) |
| Unreadable settings note | [PNG](manual/settings-unreadable.png) |
| Selection limit without a partial count | [PNG](manual/selection-limit.png) |

The mixed field batch now has 16 eligible emails and seven refusals: the formerly
accepted mailbox is correctly refused. Initial ScreenCaptureKit frames using a
stale post-resize size were retained outside the repository; the gallery uses
separate resize and capture calls. Only the OS title bar is cropped in the size
gallery, and no application content is scaled or retouched. Original preferences,
appearance, CLI and debug-app binaries were restored and hashes compared.
