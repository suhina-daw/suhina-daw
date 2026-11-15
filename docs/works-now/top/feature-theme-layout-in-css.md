- Themes lay out the track headers, the mixer strips and the transport in CSS, as the window and
  the bars: `mixer meter { order: -1 }` puts the meter before the fader,
  `mixer-toggles { flex-direction: column }` stacks solo, cue and arm, `display: none` takes a
  part away from painting and the pointer, and a fader laid out across the strip drags sideways.
  A theme with no rule lays out as before. Control placements (`.control-*` rules with offsets
  and anchors) are gone.
