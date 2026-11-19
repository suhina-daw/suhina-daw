---
description: How the metronome sounds and where its click is heard - its sounds (synthesized and recorded), its level, the bar's accent, and where the click joins the output against the main channel (pre-fader, post-fader, the cue output only, through Main), with its latency
read_when: metronome, click, click track, click sound, click route, pre-fader click, post-fader click, click on the cue, click through Main, transport.setMetronome, ClickSound, ClickRoute, metronome samples, assets/metronome
type: spec
status: current
last_verified: 2026-11-19
covers:
  - crates/engine/src/metronome.rs
  - crates/engine/src/metronome_tests.rs
  - assets/metronome/**
---

# The metronome

A click on every beat while the transport runs, higher on the first beat of a bar
(`transport.setMetronome`, off by default). It follows the launcher's beat clock and the meter,
so it keeps time through tempo edits, scene tempos and time-signature changes
([tempo map](tempo-map.md)); the count-in clicks with the same sound and level
([recording](recording.md)).

## Sounds

`transport.setMetronome {sound}` picks one; it is kept until another is picked.

| Name | What it is |
|---|---|
| `beep` (the first) | a sine at 1 kHz, 1.6 kHz on the bar, cut after 50 ms |
| `wood` | a sine at 1.8 kHz, 2.5 kHz on the bar, decaying much faster |
| `soft` | a recorded soft hit |
| `muted` | the soft hit low passed (two one-pole low passes at 1.4 kHz) |
| `deep` | the soft hit seven semitones lower, low passed at 2.5 kHz |
| `tick` | only the soft hit's top (high passed at 2.5 kHz), cut to 30 ms |
| `knock` | the soft hit with itself an octave lower under it |

The recorded clicks are the files in `assets/metronome/`, carried inside the executable and
decoded once when a renderer is made, off the audio thread. They play a fifth higher (and so
shorter) on a bar's first beat, read with linear interpolation at the output's sample rate. A
file that does not decode is silence, never a crash. Their provenance is in
`assets/metronome/README.md`; `make_variations.py` there makes the four variations from
`soft.wav`.

## Level

`transport.setMetronome {db}`, at most +6 dB, -6 dB at first. The metronome button, in the
control bar and in the main strip, is held and dragged for it
([Session view](ui-session-view.md)). The level applies wherever the click plays.

## Where the click plays

`transport.setMetronome {route}` says where the click joins the output, against the main
channel; it is kept until another is picked, and a new audio output gets it again. The
metronome button's right-click menu, in the control bar and in the main strip, picks it under
**Plays**.

| Route | Main's devices | Main's fader, mute and pan | Output |
|---|---|---|---|
| `pre-fader` (the first) | no | no | main |
| `post-fader` | no | yes | main |
| `cue` | no | no | the cue output only, when the cue has outputs of its own; otherwise main, as `pre-fader` |
| `through-main` | yes | yes | main |

- **Pre-fader** is the click a player hears whatever the mix does: a limiter on Main, the main
  fader pulled down or Main muted leave it as loud as its own level says.
- **Post-fader** follows the main volume, its lane, its mute and its pan, but no device on Main
  touches it. Without a chain on Main (no devices or lanes) the main volume, and a move of the
  main fader since the graph was built, scale it; a muted Main silences it.
- **Cue** puts the click only on the headphones' output, at the cue volume, so the main output
  (and what is recorded from it elsewhere) has none.
- **Through Main** mixes it into the main channel's input with every track, so Main's devices
  process it too; without a chain on Main it is as post-fader.

The main output's meter reads the click wherever it reaches the main output.

### Latency

The click lines up with what it plays against. Past the main channel it is delayed by as much
as a sound that goes straight to the output is when it leaves Main: the graph's latency
compensation, Main's devices' latency and Main's own compensation. On the cue output it is
delayed as much as the latest track cued there is where the cue takes it: where the track plays
into its group or the output, so a group's devices after it do not delay the click either. With
no track cued, the click on the cue is as late as the mix past the tracks. Through Main it
simply goes with the mix.

A change of route, or of whether the cue has outputs of its own, starts the click's new way
empty: a click still on its way along the old one is dropped there, never heard later.

## Commands

`transport.setMetronome {on, db?, sound?, route?}` answers with `metronome`, `db`, `sound` and
`route`. It makes no undo step and is not saved in the project: the session keeps it while the
app runs.
