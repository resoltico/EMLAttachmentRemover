# Desktop review captures

These eight PNGs show the production controller and Slint views at 1080 × 700,
using the pinned Cupertino widgets and software testing backend. The palette is
explicitly light or dark. They are toolkit captures, not screenshots of native
macOS panels. Captured on 5 November 2026 from the production controller after the control alignment, destination-name and recovery-action updates. The A/B/C/D
states come from `capture_redesign_states_in_light_and_dark`; the capture uses
controlled related-resource metadata in C so the ordinary notice is visible.

| State | Content | Light | Dark |
|---|---|---|---|
| A | Empty draft | [PNG](A-light.png) | [PNG](A-dark.png) |
| B | Two inputs, ready to create | [PNG](B-light.png) | [PNG](B-dark.png) |
| C | Active run, 6 of 15 processed | [PNG](C-light.png) | [PNG](C-dark.png) |
| D | Settled run, 12 created and 3 needing attention | [PNG](D-light.png) | [PNG](D-dark.png) |

C and D use controlled CLI protocol records so progress and all attention states
can be reviewed consistently. They do not establish native-panel behavior or MIME
classification. All eight images were visually inspected. The separate local app
check uses the built bundle and real processor.

To reproduce outside the merge gate:

```sh
GUI_CAPTURE_DIR=/absolute/output/folder cargo test --locked -p eml-gui --test desktop \
  captures::capture_redesign_states_in_light_and_dark -- --ignored
```

The separate manual `captures::capture_policy_layout_at_supported_sizes_and_scales`
test checks disclosure/footer and inspector-action reachability at 760 × 520 and
1080 × 700, 14/20-logical-pixel text, 1×/2× display scales and both palettes, with and
without a prior session sidebar. It writes its additional review images to the
supplied external output folder; they are not another committed gallery. These
checks do not replace native minimum-window legibility verification.

The capture writes lossless PPMs; the checked-in PNGs are lossless conversions.

See the [F3 investigation](f3-investigation.md) for the confirmed clipping cause
and the distinction between clipped verdicts and painted overlap.
