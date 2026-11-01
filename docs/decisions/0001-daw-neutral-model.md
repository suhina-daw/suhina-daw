---
description: Why tracks and mixer channels are separate and clips place shared contents, so several DAWs' projects fit
read_when: changing the model, track, channel, clip content, lane, pattern, importing another format, clip scale, default scale
type: decision
status: accepted
built: yes
last_verified: 2026-10-30
covers:
  - crates/model/src/lib.rs
  - crates/model/src/scale.rs
---

# 0001: A DAW-neutral project model

- **Date:** 2026-10-11

## Context

Suhina should open projects from more than one DAW: Ableton Live (`.als`) first, later
DAWproject and FL Studio (`.flp`), and possibly others. These programs organise a song in
different ways:

| Concept | Ableton Live | FL Studio | DAWproject |
|---|---|---|---|
| Where notes and audio live | A clip belongs to one track | A pattern holds notes for many channels at once | `Clip` with inline `content`, or a `reference` to shared content |
| Reuse of the same content | Clips are copied; each copy is independent | One pattern is placed many times in the playlist; editing it changes every instance | `Clip.reference` points at one shared timeline |
| Timeline rows | Each arrangement row is a track with its own mixer strip | Playlist tracks are free rows, not tied to a channel or mixer insert | `Lanes`, each optionally bound to a `Track` |
| Instrument vs mixer | Track = instrument/effects chain + mixer strip | Channel rack (generators) routes to separate mixer inserts | `Track` (structure) and `Channel` (mixer strip) are separate |
| Clip launching | Launcher view: grid of slots × scenes | None (performance mode aside) | `Scene` + `ClipSlot` |

The FL Studio column is from general knowledge and will be verified and documented in
`docs/spec/` before any `.flp` work starts (sources: observation, self-authored files and the
GPL-3.0 PyFLP project).

If the model were shaped like `.als`, an FL pattern that drives eight channels would have to be
split into eight unrelated clips on import, and the link between its instances would be lost.

DAWproject ([bitwig/dawproject](https://github.com/bitwig/dawproject), MIT, version 1.0) was
designed as a vendor-neutral interchange format by Bitwig and PreSonus and already solves this.
Its key ideas:

- **Track and Channel are separate.** A track is a node in the project structure; a channel is a
  mixer strip with volume, pan, mute, sends, devices and an output destination.
- **Content is separate from placement.** A `Clip` places a timeline at a time, with play range,
  loop range and fades. Its content is either inline or a `reference` to a shared timeline.
- **A timeline can target a track.** `Lanes` contain timelines, each optionally bound to a track,
  so one piece of content can carry notes for several tracks.
- **Time units are explicit.** Every timeline says whether it counts in beats or seconds; audio
  is mapped to musical time with warp markers (`time` ↔ `contentTime`).
- **Plug-in state is opaque.** Devices store their state as a blob that is handed back to the
  plug-in unchanged.

## Decision

`suhina-model` is a DAW-neutral model built on DAWproject's ideas, in three layers:

1. **A common core** of concepts every DAW has:
   - a transport (tempo, time signature, and their changes over the song: tempo steps and ramps,
     time-signature changes at bars, [tempo map](../spec/tempo-map.md); the arrangement's loop
     region and whether it is on)
   - **tracks** (structure) and **channels** (mixer strips: volume, pan, mute, solo, sends,
     output, device chain) as separate objects
   - a pool of **clip contents**, each made of **content lanes**; a lane holds notes or audio and
     may target a specific track
   - **clips** that place a content from the pool on the arrangement or in a launcher slot, with
     their own start, length, offset and loop range, and (audio on the arrangement) a fade-in and
     a fade-out in beats ([arrangement editing](../spec/arrangement-editing.md) › Fades)
   - an **arrangement** of lanes, each optionally bound to a track, its **markers** (named
     beats on the timeline, kept in beat order) and its **take lanes** (recorded passes kept
     under their tracks). *Amended 2026-10-24, see Amendment: take lanes.*
   - a **launcher**: scenes and per-track slot columns
2. **Per-format extensions.** Every object carries an `Extensions` list where an importer keeps
   data the core does not model, keyed by format, verbatim, so that a future exporter for the
   same format can write it back and lose nothing the core does not understand yet. No such
   exporter exists yet (there is no `.als` writer); Suhina's own project file keeps the
   extensions when a project is saved.
3. **Formats live in their own crates** (`crates/als`, later `crates/flp`, `crates/dawproject`) and
   convert to and from the core. Adding a format never requires changing another format.

Objects refer to each other by typed ids (`TrackId`, `ChannelId`, `ContentId`), not by pointers.
`Project::validate` checks that every id resolves, that every arrangement clip's fades fit it and
that every marker is at a beat from 0 on, so a malformed import becomes an error instead of a
crash. A track's `parent` is the group it is
in, and groups nest: validation also refuses a track that is inside itself through its groups,
and one inside more than `MAX_GROUP_DEPTH` (16) groups, so walking up a track's groups always
ends (added 2026-10-15, [#209](https://github.com/suhina-daw/suhina-daw/issues/209)). A group's
routing is its channel's `output`; the commands that move tracks between groups keep the two
in step.

How each DAW maps onto the core:

- **Ableton clip** → a clip content with one lane that has no target (it plays on the track it
  is placed on), placed by one clip. Copies of a clip get their own content.
- **FL pattern** → one clip content whose lanes each target a channel's track; every playlist
  instance is a clip pointing at the same content.
- **FL playlist track** → an arrangement lane with no track binding.
- **Ableton Launcher view** → launcher columns (one per track) × scenes.

## Amendment: built-in effect or instrument parameters (2026-10-11)

A `Device` also holds `params`: the parameter values of a built-in effect or instrument
(`DeviceKind::Builtin`), as a list of a stable parameter id and a value in the parameter's own
unit (decibels, hertz, a fraction, an option index). Parameters that are not listed keep their
defaults, and the device limits each value to its range when it loads it, so a project written
by an older or newer Suhina still opens. Plug-ins keep using `state`; placeholders have
neither. Importers fill `params` when they map another DAW's device onto a Suhina device
(first: Live's Utility, `docs/spec/als-format.md` › Devices).

## Amendment: plug-in parameters and automation lanes (2026-10-15)

`params` also holds values set on a plug-in device: by the plug-in's parameter id written in
decimal, in the plug-in's own range (a VST3 plug-in's normalized, a CLAP plug-in's plain), set
over the `state` it loads with. A `Device` also holds `automation`: one `AutomationLane` per
parameter (by the same id as `params`), points of an arrangement beat and a value in beat order,
the value moving in a straight line from one to the next and holding before the first and after
the last; two points at one beat make a jump. Lanes play on plug-in devices first (#109); built-in
effects and instruments follow with LOOP-39. A lane belongs to its device, so it moves, copies and
goes with it.

## Amendment: envelopes for every parameter (2026-10-15)

Lanes play on built-in devices too, and a `Channel` holds `automation` of its own: lanes of its
mixer parameters `volume`, `pan` and `mute`, in the units of the channel's fields (#395). An
`AutomationPoint` has a `curve` (-1 to 1, 0 a straight line) for the segment to the next point,
and an `AutomationLane` an `enabled` flag: a manual change to the parameter turns it off, and the
value set by hand holds until it is on again. The shapes, steps and how the engine plays them are
in [the automation spec](../spec/automation.md); the pure parts (a lane's value at a beat, the
curve) are in `model::automation`.

## Amendment: fade shapes and curves (2026-10-13)

Each fade of a `Clip` also has a `FadeCurve` (`fade_in_curve`, `fade_out_curve`): a
`FadeShape` (linear, equal power, fast, slow, S-curve) and a curve from −1 to +1 that bends it.
Linear with curve 0 is the default, so a project without them sounds as before. A curve outside
−1 … +1 does not validate (`ValidationError::BadFades`). The formulas are in
[arrangement editing](../spec/arrangement-editing.md) › Fades.

## Amendment: copy on edit for duplicated clips (2026-10-15)

`clip.duplicate` keeps placing the same content: a duplicate costs one `Clip`, and a later
linked copy (a clip that stays shared on purpose) builds on that. Independence comes when the
notes are edited instead: the first `note.*` edit that changes the notes of a clip whose
content other clips also play gives that clip its own copy of the content (a new `ContentId`,
the same name), inside the same command and so the same undo step; the other clips keep the
original ([clip editor spec](../spec/ui-clip-editor.md) › Coordinates,
[#352](https://github.com/suhina-daw/suhina-daw/issues/352)). This is the "make unique" the
consequences below ask for, done without asking. A content with a lane that targets a track (a
shared pattern) is not copied: its placements are one pattern on purpose. Copying on duplicate
instead was rejected because it would leave nothing for a linked copy to share and doubles the
contents of a set full of repeats that are never edited. `clip.paste` already gives the pasted
clip its own content. Audio content edits (`clip.setGain`) still act on the shared content.

## Amendment: muted notes (2026-10-15)

A `Note` also has a `muted` flag (`false` when nothing says otherwise, so a project without muted
notes is unchanged). A muted note stays in the clip, keeps its time, length, key and velocity, and
is shown by the editor as a note that does not play; the engine emits no voice for it at all, the
same as if it were not there
([clip editor spec](../spec/ui-clip-editor.md) › Muting, [#460](https://github.com/suhina-daw/suhina-daw/issues/460)).

It is a flag on the note rather than a delete, because the note is still the user's material: mute
is how a part is taken out and put back while the arrangement around it stays, and it survives
saving. The other two ways a note can be taken out of a clip are not this: a length of zero is a
malformed note, and a velocity of zero means a note-off in MIDI, so neither can carry the meaning.

The flag rides on every command that copies notes (paste, duplicate, cut) and does not change what
they mean; a note written by a command that does not name mute is unmuted. A source format that
marks a note as not playing maps onto it instead of losing the note
([.als format](../spec/als-format.md) › Notes): a note that is there but silent in another program
is a muted note here, not a hole.

## Amendment: clip scales (2026-10-16)

A `ClipContent` also has a `scale`: `Option<Scale>`, `None` when nothing says otherwise, so a
project without scales is unchanged. A `Scale` is a root pitch class (0 is C, 11 is B) and a
`ScaleMode` (major, minor, harmonic minor, melodic minor, dorian, phrygian, lydian, mixolydian,
locrian, major pentatonic, minor pentatonic, blues); the root can only be made 0 to 11
(`Scale::new` refuses anything else), so a scale in the model is always valid and needs no
validation error. The `Project` has a `default_scale`, `Option<Scale>` as well, that a new MIDI clip
takes: one `clip.create` makes, a recorded take and a MIDI file placed on a track, all through the
same placing code. Changing it leaves the clips that exist as they are; an audio clip never takes
it ([clip editor spec](../spec/ui-clip-editor.md) › Scale and key,
[#347](https://github.com/suhina-daw/suhina-daw/issues/347)).

The scale is on the content, not on the `Clip` that places it, because it describes the notes:
two clips that play the same notes play them in the same key, and a content copied for a clip
(the copy on edit above, a paste) takes its scale with its notes. Setting a scale through a clip
whose content other clips also play is an edit like a note edit: that clip gets its own copy of the
content first, and a shared pattern stays one content. On the `Project` it would be one key for a
whole set, which a song that changes key does not have; on each `Note` it would repeat itself.

A scale is something the editor shows and the note tools use (`note.fitToScale` fits to it when
given none); the engine never reads it and it changes nothing that sounds. The modes are the
twelve `note.fitToScale` already knew, now one list in the model that the commands, the project
file and the piano roll share. A source format that keeps a clip's scale maps onto it: the `.als`
reader does not read one yet (a follow-up), and a format without scales leaves it `None`.

## Amendment: device groups (2026-10-18)

A `DeviceKind` can be a `Group(Box<DeviceGroup>)`: a device on a channel's chain that holds other
devices in one to 16 parallel `Branch`es, each a serial chain with its own volume, pan, mute and
solo, with eight `Macro`s whose `MacroMapping`s name parameters inside the group
([device groups spec](../spec/device-groups.md)). A kind rather than a field on every device,
because a group never has `state` and a plug-in never has branches, so the type rules out the
combinations that mean nothing. The group's own values (`mix`, `gain`, `macro.1` … `macro.8`) are
ordinary `Device::params`, so setting, saving and automating them needs nothing new.

A device inside groups is found by a *device path*: its index on the channel, then branch and index
for each group on the way (`device_at`, `walk_devices`). The limits count every depth: at most 64
devices on a channel (`MAX_DEVICES`, the groups themselves included) and groups at most four deep.
`Project::validate` reports a group without one to 16 branches or without eight macros, one too
deep, a channel over the device limit, a branch volume that is negative or not finite or a pan
outside -1 to 1, a mapping that does not resolve or has a range outside 0 to 1, and a parameter
mapped twice, each as its own `ValidationError`. Whether a group is an instrument group depends on
which devices are instruments, which only the device registry and the plug-in catalog know, so
those rules are checked by the commands that build groups, not by the model.

The project file writes a group as a device of `"type": "group"` and is version 2 only when a
project holds one ([`.suhina` format](../spec/suhina-format.md)), so a project without groups
still opens in a Suhina from before them.

## Amendment: take lanes (2026-10-24)

`Arrangement::takes` holds `TakeLane`s: a track and a name, and the clips of one recorded pass on
the arrangement's beats ([recording spec](../spec/recording.md#take-lanes), #909). Recording into
the arrangement keeps every pass over the same beats (the loop wrapping, the playhead moved back)
as a take lane and puts the latest on the track, so comping copies parts of a take onto the track
as new clips. On the arrangement rather than on each lane, because takes belong to a track and
not to the lane's own clips, they never play, and the engine, the importers and every lane stay as
they were. Take clips reference contents like any clip; they have no ids (`ClipId::NONE`), since no
command names one by id: a take is named by its track and its number. `Project::validate` checks
that a take's track and its clips' contents exist; deleting a track deletes its takes.

The project file writes them under `arrangement.takes`, left out when there are none; files with
them are version 4 ([`.suhina` format](../spec/suhina-format.md) › Versions).

## Consequences

- Importers do real conversion work instead of copying a file's structure, and every importer
  needs tests that show nothing is lost on a round trip.
- Shared content means editing a clip content can change several clips at once. Commands must
  offer "make unique" (copy-on-edit) for DAWs that expect independent copies.
- The engine plays the core model only; it never sees format-specific data.
- Plug-in state stays opaque, but in each plug-in format's standard container where a source
  format keeps its own: a VST2 plug-in's state is the standard VST2 bank (`.fxb`), which other
  hosts and a VST3 version of the plug-in read (ADR 0006, decision 8).
- Arrangement markers are a beat and a name: what a Live Set's locators hold, and what
  DAWproject's arrangement `Markers` (a time and a name, with an optional colour) hold.
- The core will grow (automation, tempo maps, comping). Each addition is checked
  against at least two formats before it lands, so it does not bake in one DAW's assumptions.
- Positions are `f64` beats in the core. If rounding errors appear against tick-based formats
  (FL uses PPQ ticks), we revisit this with a rational or tick-based time type.
