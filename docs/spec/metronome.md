---
description: How the metronome sounds and where its click is heard - its sounds (synthesized, recorded and the library's own) by family, its level, the bar's own sound, and where the click joins the output against the main channel (pre-fader, post-fader, the cue output only, through Main), with its latency
read_when: metronome, click, click track, click sound, click route, pre-fader click, post-fader click, click on the cue, click through Main, transport.setMetronome, transport.metronomeSounds, ClickSet, ClickRoute, own click sounds, Sounds/Metronome, bar sound, assets/metronome
type: spec
status: current
last_verified: 2026-11-19
covers:
  - crates/engine/src/metronome.rs
  - crates/engine/src/metronome_tests.rs
  - crates/session/src/metronome_sounds.rs
  - assets/metronome/**
---

# The metronome

A click on every beat while the transport runs, higher on the first beat of a bar
(`transport.setMetronome`, off by default). It follows the launcher's beat clock and the meter,
so it keeps time through tempo edits, scene tempos and time-signature changes
([tempo map](tempo-map.md)); the count-in clicks with the same sound and level
([recording](recording.md)).

## Sounds

The sounds come in families of sounds that are alike. `transport.metronomeSounds` lists them:
the built-in families, then the library's. `transport.setMetronome {sound}` picks the sound
every beat plays by its id, and `{accent}` the one a bar's first beat plays: `higher` (the
first) plays the beat's sound higher there, a sound's id plays that sound at its own pitch. Each
is kept until another is picked. The metronome button's right-click menu, in the control bar and
in the main strip, picks them under **Sound** and **Bar sound**, a submenu for each family; it
ends with where the library's own sounds go.

| Family | Id | What it is |
|---|---|---|
| Tones | `beep` (the first) | a sine at 1 kHz, 1.6 kHz on the bar, cut after 50 ms |
| Tones | `wood` | a sine at 1.8 kHz, 2.5 kHz on the bar, decaying much faster |
| Soft | `soft` | a recorded soft hit |
| Soft | `muted` | the soft hit low passed (two one-pole low passes at 1.4 kHz) |
| Soft | `deep` | the soft hit seven semitones lower, low passed at 2.5 kHz |
| Soft | `tick` | only the soft hit's top (high passed at 2.5 kHz), cut to 30 ms |
| Soft | `knock` | the soft hit with itself an octave lower under it |

The recorded clicks are the files in `assets/metronome/`, carried inside the executable and
decoded the first time one is picked, off the audio thread. A file that does not decode is
silence, never a crash. Their provenance is in `assets/metronome/README.md`;
`make_variations.py` there makes the four variations from `soft.wav`.

### The library's own sounds

Each folder in the library's `Sounds/Metronome/` (`Documents/Suhina/Sounds/Metronome/<family>/`,
[ADR 0036](../decisions/0036-library-and-shipped-content.md)) is a family named after the
folder, and each sound file in it (WAV, AIFF, FLAC, MP3, Ogg) one of its sounds, named after the
file without its extension. A sound's id is `library/<folder>/<file>`.

- Families come after the built-in ones, folders and files in name order without regard to
  case; a hidden folder or file (its name starting with a dot), a file that is no sound and a
  folder without one are left out. At most 64 families and 64 sounds in each are listed.
- The folder is looked at again whenever the metronome's menu opens and
  `transport.metronomeSounds` runs, so a folder added while Suhina runs is there the next time.
- A sound is read when it is picked: its channels mixed to one and cut to two seconds. A file
  over 16 MB, one that does not decode, and an id that names anything outside its family's
  folder (a separator, `..`, a hidden name) are refused, and the metronome keeps the sounds it
  had.

### Playing

A recorded click is read with linear interpolation at the output's sample rate; on a bar's first
beat with `accent` `higher` it plays a fifth higher, and so shorter. A synthesized one plays at
its bar pitch there. The count-in clicks with the same sounds. A new pair of sounds reaches the
audio thread by a queue of its own, and the pair it replaces goes back the same way to be freed,
so the audio thread never allocates or frees one.

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

`transport.setMetronome {on, db?, sound?, accent?, route?}` answers with `metronome`, `db`,
`sound`, `accent` and `route`. It makes no undo step and is not saved in the project: the
session keeps it while the app runs.

`transport.metronomeSounds {}` answers with `families` (each with its `name`, its library
`folder` or `null` for a built-in one, and its `sounds`, each an `id` and a `name`) and
`library`, the folder the library's families are looked for in (`null` without a library).
