---
description: Functional spec of swing - the project's swing and a MIDI clip's own, how swing moves notes when a clip plays (every second sixteenth of each eighth), the Global switch and slider in the MIDI clip panel, the swing slider beside Resize all, the commands transport.setSwing and clip.setSwing, and the project file
read_when: swing, shuffle, groove, global swing, clip swing, use global swing, transport.setSwing, clip.setSwing, second sixteenth, triplet feel, swing slider, Resize all
type: spec
status: draft
last_verified: 2026-11-06
covers:
  - crates/model/src/swing.rs
  - crates/session/src/swing.rs
  - crates/engine/src/lib.rs
  - crates/engine/src/launcher.rs
  - crates/ui/src/detail/detail_view/midi_panel.rs
  - crates/ui/src/arrangement/arrangement_view.rs
---

# Swing

Swing delays every second sixteenth of each eighth when a MIDI clip plays, so straight sixteenths
lean towards a shuffle ([#1221](https://github.com/suhina-daw/suhina-daw/issues/1221)). It is
applied as the clip plays, live and in an offline render alike; the notes themselves do not move,
so turning swing down gives the straight notes back.

## How notes move

A swing amount goes from 0 % (straight) to 100 %. Within each eighth (half a beat) of the clip's
content, the second sixteenth moves from a quarter of a beat to the eighth-note triplet, a third of
a beat, by the amount: at 50 % a note on it plays a twenty-fourth of a beat late, at 100 % a
twelfth. The time between is stretched before that point and squeezed after it, so a note between
the sixteenths moves in proportion, the eighths themselves stay where they are, and no note passes
another. A note's end moves the same way as its start would at that place, so a note that filled
its sixteenth still fills it.

Swing works on the content's beats, so every pass of a looping clip swings the same, and a clip's
start marker does not change which sixteenths are delayed.

## The project's swing and a clip's own

- The project has one swing amount (0 % at first). Every MIDI clip follows it until it gets its
  own.
- A clip with its own amount plays that one, whatever the project's is.
- Following the project's again keeps the clip's own amount: turning its own back on brings that
  amount back.
- Audio clips do not swing.

## In the window

- **MIDI clip panel**, in the Clip part under **Scale** ([clip editor spec](ui-clip-editor.md) ›
  Clip column): the name says whose swing the clip plays, **Swing (global)** or **Swing (this
  clip)**, with the amount at its right; under it the **Global** switch (lit while the clip follows
  the project's swing) and a slider.
  - While **Global** is lit the slider shows the project's swing, and dragging it sets the
    project's: every clip that follows it, and the slider beside **Resize all**, follow at once.
  - A click on **Global** turns it off: the clip plays its own amount, and the slider sets that.
    A click turns it on again.
  - A click on the slider puts the handle there; a drag moves it and the swing follows while it
    moves, one undo step for the drag; a double-click puts it at 0 %.
- **Beside Resize all**, on the lower line of the Arrangement's track header corner, opposite the
  waveform zoom: **Swing**, a slider of the project's swing and its amount, which works as the
  panel's does. It is left out when the corner is too narrow for it beside the zoom.

## Commands

- `transport.setSwing {amount, continue?}`: the project's swing, `amount` 0 to 100 (percent).
- `clip.setSwing {clip | track + index | track + scene, amount?, global?, continue?}`: `amount`
  (0 to 100) is the clip's own and makes the clip play it; `global: true` makes it follow the
  project's again, keeping its amount; `global: false` makes it play its own amount again. Not
  `amount` with `global: true`. A clip without notes is refused. The result gives `amount`,
  `global` and `plays`, the amount it plays now.

Each is one undo step; with `continue` the calls of one drag make one step. A refused call changes
nothing.

## Project file

`transport.swing` (0 to 1, left out at 0) and a clip's `swing` `{ amount, own }` (amount 0 to 1,
left out when the clip follows the project's swing with an amount of 0), from version 9
([`.suhina` format](suhina-format.md) › Versions). A value outside 0 to 1 is an error naming where
it is.
