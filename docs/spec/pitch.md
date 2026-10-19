---
description: The built-in pitch MIDI effect - its parameters, how a note is transposed, kept in a window of keys (Block, Fold, Limit) or moved along the clip's scale, how note-offs follow, how MIDI effects run before the instrument, how Live's Pitch imports into it and how close it comes to Live's recordings
read_when: pitch, MIDI effect, transpose notes, scale degrees, follow scale, clip scale in devices, block fold limit, key window, lowest and range, importing Live's Pitch, MidiPitcher elements, built-in MIDI effects
type: spec
status: current
last_verified: 2026-10-18
covers:
  - devices/pitch/**
---

# Pitch

Suhina's `pitch` (`devices/pitch`): the first built-in MIDI effect. It sits before a MIDI
track's instrument and changes the notes on their way to it: it transposes each note by a number
of semitones, or by a number of steps along the scale of the clip that plays it, and keeps notes
inside a window of keys, either by dropping the ones outside (Block), wrapping them into it (Fold)
or moving them to its nearest edge (Limit). It is designed so that Live's Pitch imports into it
parameter for parameter ([`device-import.md`](device-import.md)).

- **Observed with:** Live 12.4.6. The parameters (ranges, defaults, stored elements) come from the
  parameter dump and a saved set; the behaviour from recordings of the device's output: a second
  MIDI track recording what leaves the device, for clips of every key from 0 to 127 and of a
  chromatic octave under several scales (`live-12/midi-effects/30.05-pitch.md` and
  `measurements/30.05-pitch/` in the device-specs repository).
- **Sources:** Live 12 Reference Manual, 30.5 Pitch (what each control is for); the recordings
  above (what it does). Everything else is this device's own design and is marked as a decision
  below.

## Parameters

Values are plain values in their unit (ADR 0004). Ids are stable.

| # | Id | Name | Range | Default | Unit, kind |
|---|---|---|---|---|---|
| 0 | `pitch` | Pitch | −128 – +128 | 0 | semitones, integer |
| 1 | `pitchDegrees` | Pitch in Scale Degrees | −30 – +30 | 0 | scale degrees, integer |
| 2 | `followScale` | Follow Scale | off/on | off | toggle |
| 3 | `lowest` | Lowest | 0 – 127 (C-2 – G8) | 0 | MIDI key, integer |
| 4 | `range` | Range | 0 – 127 | 127 | semitones, integer |
| 5 | `mode` | Mode | Block, Fold, Limit | Block | choice |

Ranges and defaults are Live's. The semitone and the degree transposition are two parameters, so
switching Follow Scale never clips or changes the other's value. Two more ids are planned for the
step buttons (Decisions, 2): `stepWidth` (1 – 48 semitones, default 12) and `stepWidthDegrees`
(1 – 30 degrees, default 7), as indices 6 and 7.

## Behaviour

A note-on of key `p` goes through three steps; the window is the `range + 1` keys from `lowest`
to `lowest + range`, both ends included.

1. **Block** only: a `p` outside the window is dropped; nothing is sent for it, and its note-off
   is dropped too. The test is on the key as played, before the transposition: with Lowest 60,
   Range 12 and Pitch +24, keys 60 – 72 pass and sound 84 – 96. Range 0 passes Lowest alone.
2. **Transposition.** Follow Scale off: `p + pitch`. Follow Scale on and a scale on the clip:
   `pitchDegrees` steps along the scale's keys of every octave, counted from the scale's key
   nearest to `p` (a tie goes to the lower key). In C major +1 takes C to D, E to F and B to the C
   above, +7 is an octave, −1 takes C to the B below; C♯ at 0 plays C. A scale of five keys has
   five steps to the octave. Only the scale's set of keys matters, not its root (D Dorian and C
   major give the same notes). Follow Scale on and no scale on the clip: `p + pitchDegrees`, in
   semitones.
3. **Fold** or **Limit**, after the transposition: Fold wraps the key into the window with a
   period of `range + 1`, `lowest + ((p' − lowest) mod (range + 1))` (with Lowest 60 and Range 11
   every key lands on its own pitch class in 60 – 71; with Range 7, key 0 plays 64 and key 8 plays
   64); Limit moves a key below the window to `lowest` and one above to `lowest + range`. Range 0
   sends every note to Lowest in both. So Fold and Limit always send a key inside the window,
   while Block's window is on the input.

The result is then limited to 0 – 127: nothing is lost at the ends of the keyboard, and several
keys can land on one (Pitch +12 plays 116 – 127 all as 127).

- **Note-offs.** The device keeps, per key played, the key its note-on sent; the note-off ends
  that key, whatever the settings are by then. So a change of Pitch, the window or the scale
  under a held note never leaves it hanging; the note keeps its key and its length. When two keys
  sound the same key, each note-off ends one of them. A key played again while its note sounds
  ends that note first.
- **Everything else passes unchanged:** velocities, release velocities, the frame of every event,
  controllers, pitch bend, pressure and all-notes-off. A note-on of velocity 0 is a note-off. The
  device adds no latency and does nothing to the sound.
- **The scale.** A clip's scale ([ADR 0001](../decisions/0001-daw-neutral-model.md) › Amendment:
  clip scales) reaches the devices of its track as an event before the first of its notes whose
  scale differs from the last one the chain sent; a clip without a scale sends "no scale". Notes
  played on a controller (a MIDI input) follow the scale of the clip that played last on the track.

### How MIDI effects run (ADR 0004 › 14)

A built-in MIDI effect describes itself as one (`Role::MidiEffect`) and takes the events of each
span of a block through `Device::process_events`, at frames from the span's start, writing what it
sends on. The chain hands its notes to the first device as before; a MIDI effect's output becomes
the events the next device gets, so MIDI effects run in the order they sit before the instrument.
A span is split where one of the effect's parameters follows a lane, with the change before the
events at its frame. `device.add` puts a MIDI effect after the track's other MIDI effects, before
its instrument and audio effects, and only on a MIDI track; an instrument goes after the MIDI
effects. On a track with no instrument the changed notes play as the track's sine tones, as its
notes would without the effect. When a block is so crowded that the room for events runs short,
the device leaves out note-ons before note-offs, so a note it sent always ends. Offline renders
and playback run the same chain. The browser lists the built-in ones under **MIDI Effects**.

## Import

Live's Pitch is the element `MidiPitcher`; every value is `<Element>/Manual@Value`
([`als-format.md`](als-format.md) › Devices). `crates/als/src/devices.rs` maps it:

| Element | Stored | Suhina |
|---|---|---|
| `On` | true/false | the device's on/off |
| `Pitch` | −128 … 128 | `pitch` |
| `PitchScaleDegrees` | −30 … 30 | `pitchDegrees` |
| `UseCurrentScale` | true/false | `followScale` |
| `Lowest` | 0 … 127 | `lowest` |
| `Range` | 0 … 127 | `range` |
| `RangeMode` | 0 Block, 1 Fold, 2 Limit | `mode` |
| `StepWidth`, `StepWidthScaleDegrees` | 1 … 48, 1 … 30 | not mapped yet (Decisions, 2) |
| `…ScrollPos…` | view positions | not mapped: not part of the device |

A mode outside the list is Block, with a warning. The importer also reads the scale of each Live
MIDI clip ([`als-format.md`](als-format.md) › MIDI clips › Scale), so an imported Pitch with
Follow Scale on moves notes along the clip's scale as in Live. A Live scale that Suhina has no
mode for (Whole Tone, for one) comes in as no scale, with a warning, and there degrees count as
semitones until the clip is given a scale in Suhina.

## Decisions

Each entry: what was chosen, what it costs, why.

1. **Integer arithmetic on events, nothing per frame.** The device keeps one table of 128 keys and
   the scale; a note costs a few comparisons and at most 30 × 11 steps along a scale. There is no
   audio work at all, so the budget is not a concern.
2. **No step buttons yet, and so no step widths.** The buttons move Pitch (or Pitch in Scale
   Degrees) by the step width; they are not parameters, and built-in devices have no panel yet to
   put them on. A width that nothing uses would be a parameter that does nothing, so the two widths
   wait for the buttons with their ids reserved (Parameters). Cost: an imported set loses its step
   widths, which change no sound.
3. **The scale reaches the device as an event** (`NoteEvent::Scale`), sent by the chain before a
   clip's note when the clip's scale differs from the last one sent. The scale changes exactly
   where the notes of the next clip start, in any block size, and devices that ignore it (every
   instrument, every plug-in) need nothing. Notes from a MIDI input keep the last clip's scale;
   what Live does while no clip plays was not observed.
4. **Fold and Limit work in semitones with Follow Scale on.** Only Block with the scale was
   recorded (the window stays in MIDI keys, as the single Lowest parameter suggests); a folded key
   can land outside the scale.
5. **One note-off per note-on, also when several keys land on one key.** Live's recordings show
   overlapping notes of the same key; ending each separately is what they show and never leaves a
   note hanging in an instrument that counts its notes.
6. **The name Follow Scale** for the switch, in Suhina's own words.

## Known differences

Measured: the law above reproduces **3 761 of 3 761** recorded notes of the six runs
(`pitch-transpose`, `pitch-block`, `pitch-fold`, `pitch-limit`, `pitch-degrees`, `pitch-noteoff`):
every input key's output key under every recorded setting, starts, lengths and velocities
unchanged, no latency.

- **Switching the device off or on while notes sound.** In Live the held notes go on unchanged.
  In Suhina a device switched off leaves the track's chain, which is built again (ADR 0004 › 9),
  so the instrument starts afresh and the notes that sounded end.
- **Per-note pitch bend** (MPE) with Follow Scale on is re-mapped in Live so a bent note stays in
  the scale. Suhina's notes carry no per-note expression yet (ADR 0004, Not decided yet).
- Not observed, so not matched on purpose: the LED that flashes when Block drops a note, the step
  buttons, a note started while the device is off, and Lowest + Range above 127.

## Tests

`devices/pitch/src/tests.rs`: the device contract, with the MIDI-effect checks
(`device_api::conformance`: the same events in spans of every size at every parameter's ends,
`reset`, hostile events, no note left sounding); the measured maps for transposition and the
keyboard's ends, Block before the transposition, Fold's period and Limit after it, a window past
the top key, scale degrees in major, Dorian and a pentatonic scale with keys outside the scale,
degrees without a scale, Block with the scale; note-offs after a change, blocked notes, several
keys on one, a key played again, note-ons left out before note-offs when room runs short, and
everything else passing unchanged.

`crates/engine/src/midi_effect_tests.rs`: offline renders through the FM synth equal those of the
transposed notes without the device, in every block size; without an instrument the sine tones
play the transposed notes, before the audio effects; blocked notes are silent; the clip's
scale reaches the device from the arrangement and from launched clips; a Pitch lane that moves
under a held note leaves no note hanging. `crates/session/tests/devices.rs` checks where
`device.add` puts it, and `crates/als/src/devices.rs` the import. `crates/als/tests/synthetic.rs`
renders an imported Pitch with Follow Scale on over a clip imported in D Dorian, and checks that
it moves the notes by degrees of that scale.
