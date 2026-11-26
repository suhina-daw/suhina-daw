---
description: The mixer's crossfader - an equal-power A/B blend of the tracks assigned to each side, the per-track A/B/off assign, the commands, the Main strip's slider, each strip's assign control and the APC40 MkII's CC 15 and per-track A/B buttons
read_when: crossfader, crossfade, A/B, A side, B side, assign A, assign B, mixer.setCrossfader, track.setCrossfadeAssign, APC40 crossfader, CC 15, note 66
type: spec
status: draft
last_verified: 2026-11-26
covers:
  - crates/model/src/lib.rs
  - crates/project-file/src/lib.rs
  - crates/engine/src/buses.rs
  - crates/engine/src/chain.rs
  - crates/session/src/commands/mixer.rs
  - crates/session/src/commands/track.rs
  - crates/ui/src/session/mixer_strip.rs
  - crates/ui/src/session/session_view.rs
  - crates/ui/src/interaction/grid.rs
  - packs/apc40-mkii/script.lua
---

# Crossfader

**Suhina:** one fader blends the tracks assigned to side **A** with those assigned to side
**B**, the way a live set's two halves crossfade under one hand. A controller's crossfader and
its per-track A/B buttons drive the same two commands.

## The value and the curve

- The project holds one crossfader value, `0` at the **A** end, `1` at the **B** end, centered
  at `0.5` in a new project.
- The blend is **equal power**: an A-assigned track's gain is `cos(value * π / 2)`, a B-assigned
  track's is `sin(value * π / 2)`. At the center both are `√½` (about −3 dB), so a sound moving
  from one side to the other keeps its loudness through the middle. At `0` only A is heard; at
  `1` only B.
- A track assigned **off** keeps a gain of `1` at every value: it is heard on both sides.
- The gain is applied where the track's own signal leaves its strip for its output, after its
  volume, pan and devices. The main channel is not assigned and its own output is not scaled.
  A track's cue send and its post-fader sends are tapped before this gain: the crossfader
  blends the main paths, not the headphones or the effect returns. A return track's own output
  is a main path and its assign applies there.

## The assign

- Every track has one assign: **A**, **B** or **off** (the default). It is saved with the
  project and shown on the track's mixer strip.
- A **group** track's assign applies to the group's whole summed output. The **outermost
  assigned group** in a track's chain governs it: an assign inside an assigned group adds no
  second fade. With no assigned group above it, a track's own assign applies at its output into
  its group or the main channel, so a track inside off groups can still be assigned on its own.
- Assigning a group and then a track inside it therefore assigns the group's side to everything
  it holds; assigning the group off lets the inner tracks' own assigns work again. A nested
  group follows the same rule.
- A return track has a strip and can be assigned; a presentation folder plays nothing and its
  assign has no effect. Main has no assign.

## Commands

- `mixer.setCrossfader { value, gesture }` sets the value, `0` to `1`; a controller or the
  window sends every frame of a drag with one `gesture`, so the drag is one undo step. A playing
  engine takes the move as a smoothed message, without a new graph. The answer carries `value`.
- `track.setCrossfadeAssign { tracks, assign }` sets each named track's assign, `a`, `b` or
  `off`, as one undo step. The answer carries the track ids and the assign. The command rebuilds
  the playing graph when the assign changes a chain's gain.
- Both commands are listed by `commands` and run from the window, the CLI and MCP.

## The window

- **Main's strip** holds a horizontal **crossfader** above its metronome, labeled **A** at the
  left and **B** at the right, its handle at the value. It is dragged left or right (double-click
  centers it); every frame is heard, and one drag is one undo step.
- **Each track, group and return strip** holds one compact **A/B/off** button under its pan:
  its face reads `A`, `B` or `Off`, lit while it is assigned. A click cycles A, B, off, running
  `track.setCrossfadeAssign`. Main has no assign button.
- The controls appear as the strip has room for them, in the same height steps as the pan, and
  follow the theme like the strip's other controls.

## The APC40 MkII

- **CC 15** (the crossfader) sends `0` to `127` as `0` to `1` through `mixer.setCrossfader` with
  one gesture, so a slide is one undo step and is heard while it moves.
- **Note 66** on a track's own MIDI channel is that track's **A/B** button: A becomes B, B
  becomes A, and off becomes A, through `track.setCrossfadeAssign`. Its LED is yellow for A and
  orange for B, and off when the track is off; the button has no third LED, so off is chosen in
  the window or with the command.
- The track reading the script follows (`project.tracks`) carries each track's
  `crossfade_assign`, so the button and its LED start on the side the project has.

## The project file

Version 17 adds:

- `project.crossfader`: a number from `0` to `1`, left out when it is the centered `0.5`.
- `track.crossfade_assign`: `"a"` or `"b"`, left out when it is `off`.

Every earlier file reads as centered with every track off.

## Tests

- The engine renders two tracks, one A and one B, at the A end, the center and the B end: only
  A, both at `√½`, only B; an off track is at full level at all three. A movement across the
  value fades one out and the other in without a jump.
- A group assigned A governs its children, and with the group off an inner track's own assign
  works.
- The commands: set, undo and redo; malformed values (`NaN`, out of range) and an unknown
  assign refused; a gesture's frames are one undo step; the project file round trip writes and
  reads version 17.
- The window drags the crossfader and cycles a strip's A/B/off, with the project ending at the
  value and assign.
- The APC40 stand-in drives CC 15 and note 66, and the LED follows the assign.
