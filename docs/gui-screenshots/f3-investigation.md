# F3: list squeezing and clipped verdicts

The confirmed F3 defect is **expanding Technical details squeezes the list and
clips the last verdict**. Literal verdict text painting over the inspector was
not reproduced. The redesign's independent inspector addresses the confirmed
layout coupling; no speculative renderer workaround is warranted.

## Evidence

The baseline was `456cc39204487b24f1e9088ed14b0dd0d945a8db`, with Slint 1.18.1
and Fluent widgets. Both versions used the same captured 60-message report:
57 created and three failures. Investigation used real software rasterization
on macOS 27.2 arm64, plus an isolated native baseline app with the original UI
and controller, captured protocol records and private test preferences.

In the supplied JPEG, strong red verdict ink ends at y=412 and the Selected
message heading's dark ink starts at y=425. There is a 13-pixel gap. These are
thresholded JPEG measurements, corroborated by the rendered and native checks.

The earlier WP0 geometry check capped row bounds at the viewport edge, so it
was not independent paint evidence. Fresh snapshots also force a full repaint.
The follow-up instead compared fresh, reused and swapped framebuffers, and
masked only the last verdict's color without changing layout. A deliberately
injected text leak proved the detector could identify actual overlap.

| Check | Result |
|---|---|
| Baseline measured frames, including filter-before-expand history | 1,136 |
| Last-verdict pixels outside its Flickable or inside the inspector | 0 |
| Deliberately injected overlap detected, per main matrix control | 4,330 pixels |
| Redesigned measured frames | 192 |
| Redesigned table/inspector intersections | 0 |

The baseline matrix covered 19 configurations: widths 780–1100, heights 600–720,
font sizes 14/20 and scale factors 1/2. Transitions were sampled at 0, 1, 16 and
180 ms of virtual toolkit time. Transient framebuffer differences were confined
to scrollbar rails; settled frames agreed. They are not asserted to be another
paint defect because separate windows can initialize lazy animations differently.

## Cause and resolution

The old stretchable list and details pane shared a vertical layout. Its 56 px
rows competed with the details pane's changing preferred height. At the native
screenshot's content budget, expansion reduced the list from **168 to 144 px**,
about 2.57 rows. A shorter toolkit frame reduced it to 136 px, leaving only a
3 px strip of the final verdict. Native title bars and toolkit menu bars have
different heights, so nominal screenshot sizes are not interchangeable.

In the redesign, expanding details left the list at **198.16 px** at 780×600 and
880×600, and **501 px** at 1080×700. The inspector scrolls within its independently
sized area. The existing S18 test also asserts that disclosure expansion leaves
the table rectangle unchanged, guarding the confirmed squeezing defect.

These findings do not exclude every native compositor or platform condition.
S18's original negative control establishes responsive placement, not proof of
the original painted-overlap allegation. The one-off raster probes remain outside
the merge gate; normal toolkit tests guard the product's layout behavior.
