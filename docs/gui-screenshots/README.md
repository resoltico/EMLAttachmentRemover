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
