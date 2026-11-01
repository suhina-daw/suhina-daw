---
description: Tempo and time-signature changes over the song (the tempo map): steps and ramps, bars through signature changes, how the engine converts beats to time, the commands
read_when: tempo map, tempo change, tempo ramp, time-signature change, meter, bars, beats to samples, beat to frame, tempo.addPoint, timeSignature.add, bar ruler
type: spec
status: current
last_verified: 2026-10-18
covers:
  - crates/model/src/tempo.rs
  - crates/engine/src/tempo.rs
  - crates/session/src/tempo.rs
  - crates/session/tests/tempo.rs
  - crates/engine/src/tempo_tests.rs
---

# Tempo map

- **Sources:** general music practice (a song's tempo and metre can change; a tempo can jump or
  glide), the public Live 12 manual's description of tempo automation in the Arrangement
  (breakpoints joined by straight lines), and the Live Set fields
  in [`als-format.md`](als-format.md) (*Tempo and time-signature changes*).
- **Observed with:** nothing yet. The reading of a Live Set's tempo and time-signature envelopes is
  written from the envelope structure the corpus showed for their default events; corpus file
  [`311-tempo.als`](als-corpus.md#arrangement-automation-300314) is the check for the tempo, and
  a time-signature change has no corpus file yet.
- **Status:** first version, implemented in `crates/model/src/tempo.rs` (the conversion), the
  engine, the project file, the commands and the Arrangement's bar ruler (issue #402).

This document describes the behaviour Suhina implements, in Suhina's terms. Where Suhina decides
something the sources do not settle, the text says so (**Suhina:**).

## Terms

- **Beat:** a quarter note. Every musical position in a project (notes, clips, the play position,
  launch quantisation) is in beats from the song's start, beat 0.
- **Tempo:** beats per minute (bpm), from 1 to 999 in a project (the commands take 20 to 999,
  as `transport.setTempo` does).
- **Start tempo:** `transport.tempo_bpm`, the tempo from beat 0.
- **Tempo change:** a point `{ beat, bpm, ramp }` after beat 0.
  - **Step** (`ramp` false): the tempo is `bpm` from `beat` on.
  - **Ramp** (`ramp` true): the tempo glides from the tempo before it (the previous change's, or
    the start tempo) to `bpm`, which it reaches at `beat`. The glide is linear in beats: halfway
    between the two points in beats, the tempo is halfway between the two tempos.
  - **A glide and a jump on one beat:** a beat can hold a ramp and a step. The tempo glides to
    the ramp's tempo, reaching it at the beat, and jumps there to the step's: drawn as a line, a
    slope that ends in a vertical drop or rise. A beat holds at most one of each.
  - After the last change the tempo stays where it is.
- **Bar:** counted from 0 inside Suhina; shown from 1 (bar 0 is "bar 1"). A bar is
  `numerator × 4 / denominator` beats: 4 in 4/4, 3 in 3/4 and 6/8, 3.5 in 7/8.
- **Start signature:** `transport.time_signature`, from bar 0.
- **Time-signature change:** `{ bar, time_signature }`: from the start of `bar` (after bar 0)
  the bars have the new signature. A change always falls on a bar line, so a bar is never cut.

**Suhina:** a ramp belongs to the point it arrives at, not the one it leaves, so the start tempo
needs no flag of its own and deleting a point never leaves a ramp pointing nowhere.

## The conversion

One conversion, `TempoMap` (`suhina_model::Transport::tempo_map`), turns beats into time and back
for everything that plays: the engine, the play position, the UI. It counts in any unit: seconds,
or frames at the sample rate. Nothing converts with the start tempo alone.

- A segment at a constant tempo `T` takes `60 / T` seconds a beat.
- A ramp from `T0` to `T1` over `L` beats has the slope `k = (T1 − T0) / L` bpm a beat, and `b`
  beats into it takes `60 / k × ln(1 + k b / T0)` seconds; the beat at `t` seconds is
  `T0 / k × (exp(k t / 60) − 1)`. These are exact, so positions are worked out from the map and
  never added up frame by frame: a ramp minutes long does not drift.
- At a constant tempo the arithmetic is the one the engine has always used (`beats × 60 / T ×
  rate`), bit for bit, so a project without changes renders exactly as before.
- Malformed changes are ignored, not refused: a beat that is not finite, not after beat 0 or past
  beat 120 000, a tempo outside 1 to 999 bpm, more than 10 000 changes. Of two changes on one beat
  the later in the list wins, except a ramp followed by a step: the ramp arrives at its tempo,
  then the tempo jumps (a Live Set's glide that ends in a jump reads this way). The same for signature changes at bar 0, with an invalid signature
  (a numerator outside 1 to 99, a denominator that is not 1, 2, 4, 8 or 16) or more than
  10 000. (The project file refuses these, so they come only from an importer or a hand-edited
  model.)

`Meter` (`Transport::meter`) turns beats into bars the same way: the bar that holds a beat, the
beat a bar starts on, and the signature there.

## What follows the map

- **Arrangement notes** start and end at the time of their beats.
- **Arrangement audio clips** start and end at the time of their beats. A **warped** clip reads its
  file at the clip beat the map gives for each frame, so it stays on the grid through a change:
  time-stretched (ADR 0003) in pieces of constant speed cut at every tempo change, and during a
  ramp in pieces of at most a sixteenth note (**Suhina:** the stretcher takes one speed a piece;
  a sixteenth is short enough that the steps in speed are not heard, and every piece starts at
  its exact time, so they do not add up). A clip read directly (re-pitched) follows the map frame
  by frame. An **unwarped** clip plays at its file's own speed whatever the tempo, from the time
  of its start beat.
- **Fades** are in beats, so their length in time follows the tempo where they are.
- **The play position:** the playhead's beat is the map's beat of the arrangement time; a jump
  to a beat starts at that beat's time.
- **The launcher clock** (launched clips, launch quantisation) and **the metronome** run on the
  same beat as the arrangement while the transport follows the map: the beat is read from the map
  each frame, so it does not drift from the arrangement through a ramp. Launched clips advance by
  the beats the map moves each frame. Launched clips are not on the arrangement's timeline: their
  stretchers are prepared for the start tempo, and while the tempo is elsewhere they are
  re-pitched, as with a scene tempo. A scene's tempo or time signature takes the launcher off the
  map ([Scenes and the tempo map](#scenes-and-the-tempo-map)).
- **Launch quantisation to bars** waits for the next bar line of the meter, so a launch in a 7/8
  bar waits 3.5 beats from the bar's start, not 4. Quantisation to a note value counts from the
  bar's start.
- **The metronome** clicks high on the first beat of every bar of the meter.
- **Devices** (synced LFOs, delays) get the tempo at the start of each block and the beat, as
  before; during a ramp the tempo they see changes every block.
- **Edits while playing:** an edit to the tempo map keeps the playhead on its beat; the new map
  decides when that beat is (ADR 0005).

- **The loop region** ([arrangement editing](arrangement-editing.md#loop-region)) is in beats:
  playback wraps at the loop end's beat, and both ends are turned into frames through the map on
  every pass, so a loop with a tempo change inside it keeps its length in beats, and its length
  in time is what the map gives for those beats.

## Scenes and the tempo map

A scene's tempo or time signature ([clip launching](clip-launching.md) › Scenes) overrides the
tempo map, as a hand-made change overrides an automated control: the map stays in the project but
stops driving the launcher clock.

- **Taking over.** At the scene's launch beat the launcher clock leaves the map: it runs at the
  scene's tempo (or, for a scene with only a time signature, the tempo the map had there), and
  its bars are counted in the scene's time signature (or, for a scene with only a tempo, the
  signature of the bar it was in) from that beat on. A scene's time signature still starts at the
  next bar line, as launches quantised to bars do; the bar number keeps rising across the change.
- **What the override covers:** the launcher clock and everything on it: launched clips, launch
  quantisation, the metronome, and the tempo, signature and position the control bar and
  `launcher.state` show. Arrangement clips are not retimed to a scene's tempo: they are on the
  timeline, and their notes and audio stay where the map puts them while a scene's tempo plays.
  Retiming the arrangement to a scene's tempo would be a separate feature.
- **Back on the map.** The map drives the clock again
  - with `transport.followTempoMap` (a transport command, not an edit: nothing is undone),
  - when the transport starts again after a stop (a scene launched to start it takes the clock
    off again at its launch beat), and
  - when the tempo map or the time-signature changes are edited.

  From then on the tempo is the map's at the clock's beat and the bars are the meter's; a
  scene's tempo or time signature still waiting for its launch beat is dropped.
- `launcher.state` reports `follows_tempo_map`, false while a scene's tempo or time signature
  drives the clock. An output that is opened again (another device, buffer or sample rate) keeps
  the clock where it was: on the map, or at the scene's tempo.

**Suhina:** stopping alone does not put the clock back, so a stopped project still shows the
scene's tempo and signature until it is started again.

## Commands

All are edits with one undo step. Tempo points are numbered from 1 in beat order, as
`tempo.points` lists them; bars are numbered from 1, as shown.

| Command | Params | Does |
|---|---|---|
| `tempo.points` | | lists the start tempo and every change: `{ point, beat, bpm, ramp }` |
| `tempo.addPoint` | `beat` (> 0), `bpm` (20–999), `ramp?` (default false) | adds a change; one of the same kind (step or ramp) already on that beat is replaced |
| `tempo.movePoint` | `point`, `beat?`, `bpm?`, `ramp?` | changes a point; moved onto a beat with a point of the same kind, it replaces that one |
| `tempo.deletePoint` | `point` | removes a point |
| `timeSignature.add` | `bar` (≥ 2), `numerator` (1–99), `denominator` (1, 2, 4, 8, 16) | adds a change at the start of the bar; one already there is replaced |
| `timeSignature.move` | `bar`, `to?`, `numerator?`, `denominator?` | moves a change to bar `to` or changes its signature |
| `timeSignature.delete` | `bar` | removes the change at that bar |
| `transport.followTempoMap` | | puts the launcher clock back on the map after a scene took it off (not an edit) |

Each edit returns the list it changed. A beat past 120 000, a point or bar that does not exist, a
signature outside the ranges, a missing or mistyped param is an error and changes nothing.

`launcher.state` reports the tempo at the playhead and its bar and beat through the meter.

## Ranges

The commands set a narrower range than a project file may hold, so a file written by an import or
by hand opens; a command never writes a value the file reader refuses.

| Value | The commands set | A project file holds |
|---|---|---|
| Tempo | 20 to 999 bpm (`transport.setTempo`, `tempo.addPoint`, `scene.setTempo`) | 1 to 999 bpm, for the start tempo and every change |
| Time signature change | numerator 1 to 99, denominator 1, 2, 4 … 64 (`timeSignature.add`) | the same |
| Start time signature | as a change | each part 1 to 255 ([project file](suhina-format.md)) |
| A scene's time signature | numerator 1 to 99, denominator 1, 2, 4, 8 or 16 (`scene.setTimeSignature`) | each part from 1 ([project file](suhina-format.md)) |

Open: whether the start and scene time signatures in a file should follow the rule of a change
(a denominator that is a power of two up to 64), as the commands already do; the file reader
accepts any whole number from 1 there today.

## The Arrangement

The bar ruler numbers bars through the meter and marks each change: a tempo change with its bpm
(with `→` before it for a ramp), a signature change with the new signature. **Suhina:** editing the changes
in the window comes later; the commands edit them now (MCP and `suhina-cli`).

## Live Sets

See [`als-format.md`](als-format.md) › *Tempo and time-signature changes*: the tempo envelope's
breakpoints become tempo changes (a breakpoint on the same time as the one before is a step, any
other a ramp from it), and the time-signature envelope's become signature changes where they fall
on a bar line.

## To observe

- Corpus file `311-tempo.als` (tempo 120 at bar 1, 140 at bar 3): the events the envelope holds,
  and whether the bounce glides from bar 1 to bar 3 as the import plays it.
- A time-signature change in the Arrangement: where the set stores it, and what happens to a
  change placed inside a bar.
