---
description: The Live Set (.als) format as observed: container, tracks, clips, notes, a clip's scale, audio, warp markers, devices and MIDI effects, locators
read_when: reading .als, importer, XML element, MIDI effects, MidiChord, MidiArpeggiator, clip scale, ScaleInformation, warp markers, locators, arrangement markers, time signature, tempo envelope, tempo changes, devices, plug-in state, Utility, StereoGain, device parameters, muted note, deactivated note, IsEnabled
type: spec
status: current
last_verified: 2026-10-23
covers:
  - crates/als/src/lib.rs
  - crates/als/src/devices.rs
  - crates/als/src/scale.rs
  - crates/plugins/src/vst2.rs
  - crates/als/tests/synthetic.rs
  - crates/als/tests/corpus.rs
---

# Live Set (`.als`) format

What we have learned about the Live Set format, from Live Sets we authored ourselves (see
[`als-corpus.md`](als-corpus.md)). Written in our own words; no Ableton code or documentation text.

- **Observed with:** Live 12.4.6 (`Creator="Ableton Live 12.4.6"`, `MinorVersion="12.0_12402"`,
  `MajorVersion="5"`), Windows.
- **Corpus:** series 1, files `00`–`20` (MIDI clips, scenes, mixer); series 2, files `30`–`35` (audio clips); series 3, files `40`–`44` (devices, plug-ins); series 4, files `50`–`62` (time signatures, clip markers, deactivation, groups, unwarped audio); series 6, files `110`–`135` (Utility).
- Every statement marked **(hypothesis)** still needs a corpus file that proves it.

## Container

A `.als` file is gzip-compressed UTF-8 XML. The root element is `<Ableton>` with the version
attributes above; its only relevant child is `<LiveSet>`.

Most scalar values are stored as an element with a `Value` attribute, for example
`<Name Value="Test" />`. Automatable parameters are an element with a `<Manual Value="…"/>` child
(the current value when there is no automation) plus automation and MIDI-mapping data.

Many elements carry an `Id` attribute. Ids are unique only among siblings of the same kind and
are **not stable**: Live renumbers some of them on every save (see *Noise*), and moving an
arrangement clip gives it a new id. Never use them to identify an object across saves.

## Time

All musical times are in **beats** (quarter notes), as decimal numbers. Note `Time="1"` is beat 2
of bar 1; `Duration="1"` is a quarter note. Bars are not stored; at 4/4, bar 3 starts at beat 8.

Default automation values use the time `-63072000` as a "before everything" marker.

## Transport

| What | Where (under `LiveSet/MainTrack/DeviceChain/Mixer`) | Encoding |
|---|---|---|
| Tempo | `Tempo/Manual@Value` | BPM as a decimal number |
| Time signature | the default event of the time-signature automation envelope (below) | An integer code: `99 × log2(denominator) + numerator − 1`. Observed: 3/4 = `200`, 4/4 = `201`, 6/8 = `302`, 7/8 = `303`. |

Each parameter's current value also lives in the default event (`Time="-63072000"`) of its
automation envelope. The envelope is found through ids: the parameter's `AutomationTarget@Id`
equals `MainTrack/AutomationEnvelopes/Envelopes/AutomationEnvelope/EnvelopeTarget/PointeeId@Value`.
For the tempo, `Manual` and the default event change together (a `FloatEvent`). For the time
signature only the default event changes (an `EnumEvent`); `TimeSignature/Manual` stayed `201`
through files `50`–`53`. **Read the envelope first and fall back to `Manual`.**

### Tempo and time-signature changes

**(hypothesis, until corpus file [`311-tempo.als`](als-corpus.md#arrangement-automation-300314)
is recorded.)** A tempo or time-signature change over the song is more events in the same
envelope, after the default event: `FloatEvent` (tempo) or `EnumEvent` (time-signature code), each
with `Time` in beats and `Value`. Breakpoints are joined by straight lines (the public manual's
description of automation); **Suhina:** two on one time are read as a jump. Suhina reads them into
the [tempo map](tempo-map.md):

| Events | Becomes |
|---|---|
| A tempo breakpoint at or before beat 0 | the start tempo |
| A later tempo breakpoint | a tempo change at its time: a ramp from the breakpoint before it, a step when there is none (only the default event) or it shares its time |
| A jump after a glide (two breakpoints on one time) | both: the ramp arrives, then the tempo steps |
| A time-signature breakpoint at or before beat 0 | the start signature |
| A later one on a bar line (counted through the signatures before it) | a signature change from that bar |
| One inside a bar, a tempo outside 1–999, a code that is not a signature, a time that is not a number | left out, with a warning |

At most 100 000 events are read from one envelope.

## Tracks

`LiveSet/Tracks` lists the tracks in display order. Element names give the kind: `MidiTrack`,
`AudioTrack`, `ReturnTrack` and `GroupTrack`. The main output is the
separate `LiveSet/MainTrack`.

Per track:

| What | Where | Notes |
|---|---|---|
| Name | `Name/EffectiveName@Value` | Default names look like `1-MIDI`, `3-Audio`, `A-Reverb`. |
| Colour | `Color@Value` | Index into Live's colour palette. |
| Group | `TrackGroupId@Value` | `-1` when not in a group; otherwise the group track's `Id`. Grouping two tracks (file `60`) inserted a `GroupTrack` before them and renumbered the default names (`3-Audio` became `4-Audio`). A `GroupTrack` has a `TrackGroupId` too, so a group inside a group names the outer group's `Id`; the reader takes any depth (the synthetic tests write such sets by hand from this; no corpus file holds one yet). A track whose groups contain each other, or that sits deeper than Suhina nests groups (16 levels), is moved to the top level and plays to the main output, with a warning. |
| Mixer | `DeviceChain/Mixer` | see below |
| Output | `DeviceChain/AudioOutputRouting/Target@Value` | `AudioOut/Main` routes to the main output; `AudioOut/GroupTrack` to the track's group. |
| MIDI input | not read yet | An imported track hears every MIDI port and channel when armed ([ADR 0017](../decisions/0017-inputs-and-latency.md)); where a Live Set keeps a track's input is still to be observed. |
| Session clips | `DeviceChain/MainSequencer/ClipSlotList/ClipSlot` | One slot per scene, in scene order. Return tracks have no slots. |
| Arrangement clips | `DeviceChain/MainSequencer/ClipTimeable/ArrangerAutomation/Events` | `MidiClip` (and `AudioClip`) elements. |

### Mixer

Under `DeviceChain/Mixer`, values in `…/Manual@Value`:

| What | Element | Encoding |
|---|---|---|
| Volume | `Volume` | Linear gain. `1` = 0 dB; `0.5011872053` = −6 dB. |
| Pan | `Pan` | `-1` … `1`; Live's "50L" is `-1` (full left). |
| Track on/off ("mute") | `Speaker` | `true` = playing, `false` = muted. |
| Sends | `Sends/TrackSendHolder[n]/Send` | Linear gain. One holder per return track, in return-track order. "−∞" in the UI is stored as `0.0003162277571` (−70 dB). |

## Scenes

`LiveSet/Scenes/Scene`, in display order. A new scene inserted in the middle gets the next free
`Id` but takes its place in the list, so **order comes from the list, not the ids**.

Per scene: `Name@Value`, `Tempo@Value` with `IsTempoEnabled@Value` (the tempo applies only when
enabled), and the same pair for time signature.

## MIDI clips

The same `MidiClip` element is used in session slots
(`ClipSlot/ClipSlot/Value/MidiClip`) and on the arrangement.

| What | Where | Notes |
|---|---|---|
| Arrangement position | `@Time` | Beats. Session clips have `Time="0"`. |
| Extent | `CurrentStart@Value`, `CurrentEnd@Value` | Arrangement clips: positions on the arrangement (`8` … `12` for a one-bar clip at bar 3). Session clips: positions in the clip's content (`0` … `4`). |
| Loop on/off | `Loop/LoopOn@Value` | |
| Loop region | `Loop/LoopStart@Value`, `Loop/LoopEnd@Value` | In content beats. Setting the clip length to 2 bars changed `LoopEnd` (and `CurrentEnd`) from 4 to 8. |
| Start marker | loop on: `LoopStart + StartRelative`; loop off: `LoopStart` | Beats. See *Markers* below. |
| End marker (loop off) | `Loop/LoopEnd@Value` | Not `OutMarker`, whose meaning is still unknown. |
| Name, colour | `Name@Value`, `Color@Value` | |
| Deactivated clip | `Disabled@Value` | `true` after selecting the clip and pressing `0` (file `58`). |

### Markers

Observed on a session clip with a 2-bar loop (files `54`–`57`), in beats:

| Edit | `LoopStart` | `LoopEnd` | `StartRelative` | `LoopOn` | `HiddenLoopStart`/`End` |
|---|---|---|---|---|---|
| before | 0 | 8 | 0 | true | 0 / 4 |
| Start → 1.2.1 | 0 | 8 | **1** | true | |
| Loop Position → 1.3.1 | **2** | **10** | **7** | true | |
| Loop off | **9** | 10 | **0** | **false** | **2 / 10** |
| End → 3.4.1 | 9 | **11** | 0 | false | 2 / 10 |

- With the loop on, the start marker is `LoopStart + StartRelative`. Moving the loop kept its
  length and moved the start marker by the same distance (to 3.2.1 = beat 9), so it stayed inside.
- Turning the loop off rewrote the fields: `LoopStart` became the start marker and `LoopEnd` the
  end marker, and the loop region moved to `HiddenLoopStart`/`HiddenLoopEnd` so it can return.
- For a session clip, `CurrentStart`/`CurrentEnd` follow: `CurrentEnd − CurrentStart` was the
  played length (8 with the loop on, then 1, then 2).

### Notes

Notes are grouped by key: `Notes/KeyTracks/KeyTrack`, each with `MidiKey@Value` (MIDI note
number) and its notes in `Notes/MidiNoteEvent`:

| Attribute | Meaning |
|---|---|
| `Time` | Start in content beats |
| `Duration` | Length in beats |
| `Velocity` | 0–127, **a decimal**: dragging gave `64.251297` |
| `OffVelocity` | Release velocity, 0–127 |
| `NoteId` | Per-clip note id |
| `IsEnabled` | Present as `false` on a deactivated note (file `59`); absent otherwise |

A note with `IsEnabled="false"` is read as a **muted** note (`Note::muted`): it is kept, with its
time, length, key and velocity, and the importer does not play it
([ADR 0001](../decisions/0001-daw-neutral-model.md) › Amendment: muted notes). Before this the
importer dropped such notes, so a deactivated note in a set came in as a hole in the clip. File
`59` is the corpus file for it; a Suhina project saved with muted notes writes `"muted": true` on
those notes ([project files](../decisions/0010-project-files.md)).

Live names MIDI note 60 "C3" (so 64 is E3, 67 is G3). Notes drawn without changing anything got
velocity 100 in one clip and 98 in another.

### Scale

A clip's scale, which MIDI effects with Use Current Scale on follow (see *MIDI effects* below,
and Pitch in [`pitch.md`](pitch.md)), is stored in the clip. The same elements also sit at the top of
`LiveSet`, for the set's own scale **(hypothesis: the one new clips take)**.

| What | Where | Encoding |
|---|---|---|
| Scale on | `IsInKey@Value` (in the clip); `InKey@Value` (in `LiveSet`) | `true`/`false`: the clip's Scale Mode switch |
| Root | `ScaleInformation/Root@Value` | 0 – 11, the semitones up from C |
| Scale | `ScaleInformation/Name@Value` | the scale's position in the clip's Scale Name list, from 0 |

Not from the corpus: from the set Live saved for the MIDI effect measurements
(`populate-midi-12.4.6.als`, a clip in C Major: `true`, `0`, `0`), and from sets generated with
other scales whose recordings Live played as those scales (Pitch, Chord, Random, Scale and
Arpeggiator with Use Current Scale on; every output note fell inside the scale written). Those
confirm Root `0` C, `2` D and `9` A, and Name `0` Major, `1` Minor, `2` Dorian and `11` Minor
Pentatonic. **(Hypothesis:)** the rest of the Name list is the Scale device's list of scales
(`MidiScale`'s `InternalScale`) without its first entry, the user's own grid, so Name n is
`InternalScale` n + 1. Not observed yet: whether a clip with Scale Mode off keeps its root and
scale, and the scale of an audio clip (possible since Live 12.1).

The importer reads a MIDI clip's scale into its content's scale
([ADR 0001](../decisions/0001-daw-neutral-model.md) › Amendment: clip scales;
`crates/als/src/scale.rs`), so MIDI effects that follow the scale, such as Pitch, follow it in
Suhina too. Twelve of Live's scales have a Suhina mode: Name `0` Major, `1` Minor, `2` Dorian,
`3` Mixolydian, `4` Lydian, `5` Phrygian, `6` Locrian, `10` Minor Blues (Suhina's Blues, the
same six notes), `11` Minor Pentatonic, `12` Major Pentatonic, `13` Harmonic Minor and `17`
Melodic Minor; those past `2` and `11` rest on the hypothesis above. A clip with Scale Mode off,
or without `IsInKey` (sets from before Live 12), has no scale. A clip with Scale Mode on comes in
without a scale, with a warning, when its scale has no Suhina mode (Whole Tone and the 22 others),
or when Root or Name is missing, not a whole number, or out of range (Root past 11, Name past
34). The set's own scale (`InKey`) is not read yet.

## Audio clips

Corpus: series 2, files `30`–`35` (one 4-second, 48 kHz WAV rendered by Suhina).

`AudioClip` shares `Time`, `CurrentStart`/`CurrentEnd`, `Loop`, `Name`, `Color` and `Disabled`
with `MidiClip`. Session audio clips sit in clip slots like MIDI clips, but **arrangement audio
clips are under `DeviceChain/MainSequencer/Sample/ArrangerAutomation/Events`**, not under
`ClipTimeable` as on MIDI tracks.

| What | Where | Notes |
|---|---|---|
| File | `SampleRef/FileRef/RelativePath@Value` | Relative to the Live Set's folder (`../loopwerk-demo.wav`). |
| File, absolute | `SampleRef/FileRef/Path@Value` | The full path on the machine that saved the set. **Personal data**: strip before sharing a corpus. |
| File size, checksum | `FileRef/OriginalFileSize`, `FileRef/OriginalCrc` | |
| Length | `SampleRef/DefaultDuration@Value` | In sample frames (`192000`). |
| Sample rate | `SampleRef/DefaultSampleRate@Value` | `48000` |
| Warp on/off | `IsWarped@Value` | |
| Warp mode | `WarpMode@Value` | `4` for this file by default, shown by Live as **Complex** (files `30`, `61`). Other codes **(needs corpus)**. |
| Warp markers | `WarpMarkers/WarpMarker@SecTime`, `@BeatTime` | Pairs of (seconds in the file, beats in the clip). |
| Clip gain | `SampleVolume@Value` | Linear, `1` = 0 dB; −6 dB is `0.5011872053` (file `36`). |
| Transpose | `PitchCoarse@Value`, `PitchFine@Value` | Semitones, and cents **(fine: hypothesis)**. |

**Warp markers.** A newly added file got markers at (0 s, beat 0) and (4 s, beat 8): Live assumed
120 bpm. Each list ends with one more marker 1/32 beat after the last real one
((4.015625 s, beat 8.03125)); it sets the playback rate after the last marker. Adding a marker on
beat 2 and dragging it right replaced the list with (0, 0), (0.500146 s, beat 1.125) and its
1/32-beat follower.

**Unwarped clips.** With warp off, the clip's loop fields are in **seconds**. Turning warp off
in file `31` changed `LoopEnd` from `8` (beats) to `4`; in file `61`, after a warp marker had been
moved, it became `3.9376458177`, which is beat 8 converted through the warp markers (0.500146 s at
beat 1.125, then 0.5 s per beat). Moving the start to 1 s (file `62`) set `LoopStart` to `1`. The
warp markers are kept in `SavedWarpMarkersForStretched` while warp is off. With warp off the loop
switch is unavailable in Live.
`OutMarker` and `CurrentEnd` did not change.

Adding the first clip to an empty track renamed the track after the clip (`4-Audio` →
`4-loopwerk-demo`), stored in `Name/EffectiveName` and `Name/MemorizedFirstClipName`.

## Locators

The named positions on the arrangement's timeline (locators) import as arrangement markers
([arrangement editing](arrangement-editing.md#markers)). Live's published Live Object Model
documentation describes a locator (`CuePoint`) as a name and a time in beats. Where the set
stores them is a **hypothesis** until [corpus series 11](als-corpus.md#series-11-locators)
proves it:

| What | Where | Notes |
|---|---|---|
| The locators | `LiveSet/Locators/Locators/Locator`, one per locator | **(hypothesis)** Order not relied on: the importer sorts by time. |
| Position | `Time@Value` | Beats from the start of the arrangement, as everywhere else **(hypothesis)**. |
| Name | `Name@Value` | Any text, may be empty **(hypothesis)**. |

The importer reads only these two values. A locator without a `Time`, or with a negative one, is
skipped with a warning. A set without `Locators` has no markers.

## Arrangement loop

The arrangement's loop region imports as the transport's loop region ([arrangement
editing](arrangement-editing.md#loop-region)). Live's published Live Object Model documentation
describes the song's loop as on or off, a start and a length, in beats. Where the set stores them
is a **hypothesis** until [corpus series 12](als-corpus.md#series-12-arrangement-loop) proves it:

| What | Where | Notes |
|---|---|---|
| On or off | `LiveSet/Transport/LoopOn@Value` | `true` or `false` **(hypothesis)**. Missing: off. |
| Start | `LiveSet/Transport/LoopStart@Value` | Beats from the start of the arrangement **(hypothesis)**. |
| Length | `LiveSet/Transport/LoopLength@Value` | Beats; the loop end is `LoopStart + LoopLength` **(hypothesis)**. |

The importer reads only these three values, and only when the set has a `Transport`. A region
with a start or length missing or not a number, a negative start or a length of 0 or less is
skipped with a warning; the set then has no loop region.

## Devices

Corpus: series 3, files `40`–`44` (Simpler, EQ Eight, and two third-party plug-ins, an effect and an
instrument, as VST2 and VST3; the plug-in files stay local, see below).

A track's device chain is `DeviceChain/DeviceChain/Devices`, devices in chain order. The main
track has the same structure. Adding an instrument to a track named after its first clip renamed
it after the device (`1-MIDI` → `1-Simpler`, later after the plug-in).

Every device has `On/Manual@Value` (the device on/off switch; turning EQ Eight off changed it
from `true` to `false`) and `UserName@Value` (empty unless renamed).

**Live's own devices** are elements named after the device type: `OriginalSimpler`, `Eq8`,
`Reverb`, `Delay`. Their parameters are nested elements with the usual `Manual` values; band 3
gain of EQ Eight is `Bands.2/ParameterA/Gain/Manual` (+3 dB stored as `3`). `LastPresetRef`
points at the default preset the device was loaded from. The importer keeps them as
placeholders, except the ones that have a Suhina counterpart: those become Suhina devices
with their parameters mapped (`crates/als/src/devices.rs`: the Utility, below, the Delay and
Reverb, whose element tables are in [`delay.md`](delay.md) › Import and
[`reverb.md`](reverb.md) › Import, EQ Eight, `Eq8`, in [`eq.md`](eq.md) › Import, and the MIDI effect Pitch, `MidiPitcher`, whose element
table is in [`pitch.md`](pitch.md) › Import). A MIDI track's MIDI effects come first in its
`Devices`, before the instrument.

**Plug-ins** are `PluginDevice` elements whose `PluginDesc` holds one of:

| | `VstPluginInfo` (VST2) | `Vst3PluginInfo` (VST3) |
|---|---|---|
| Name | `PlugName@Value` | `Name@Value` |
| Identity | `UniqueId@Value`: the 32-bit plug-in id as a decimal | `Uid/Fields.0`…`Fields.3`: the 16-byte class id as four signed 32-bit decimals |
| State | `Preset/VstPreset/Buffer`: hex text | `Preset/Vst3Preset/ProcessorState` and `ControllerState`: hex text |
| Location | `Path@Value`: absolute path of the `.dll` on the saving machine (**personal data**) | none |
| Other | `Version`, `NumberOfParameters`, `Category`, window position | `DeviceType` (`1` for the instrument, `2` for the effect **(hypothesis)**), window position |

The importer keeps a VST3 `ProcessorState` as the device's state and `ControllerState` in the
`Vst3ControllerState` extension. A VST2 `Path` is kept in the `VstPath` extension, so it is
saved with the project (`.suhina` writes extensions) and goes with it when the project is shared.
A plug-in of another kind (Audio Unit, CLAP) is skipped with a warning.

The VST3 `Uid` fields, each written as eight hex digits and joined, give the class id in the
canonical text form the VST3 scanner and `moduleinfo.json` use: checked against the scanned
catalog for 21 different VST3 plug-ins across 169 Live Sets, all equal.

Plug-in state is the plug-in vendor's own format. Suhina keeps it verbatim and hands it back
to the plug-in. For VST2, `VstPreset` also says how the `Buffer` is laid out, and the importer
wraps it in the standard VST2 bank (an `.fxb` file's contents, `crates/plugins/src/vst2.rs`), which
is what the model keeps and what a VST3 version of the plug-in reads. The bank's layout follows
the VST3 SDK's `vst2persistence.h` (MIT, linked from ADR 0006), not the VST2 SDK:

| `VstPreset` child | Meaning |
|---|---|
| `Type` | `1178747752` (`FBCh`): `Buffer` is the plug-in's own chunk. `1182286443` (`FxBk`): `Buffer` is `ProgramCount` programs, each a 28-byte name and `ParameterCount` 32-bit floats |
| `ProgramCount`, `ParameterCount`, `ProgramNumber` | programs, parameters, the current program |
| `PluginVersion` | the plug-in's version number |
| `ByteOrder` | `2`: the floats of an `FxBk` buffer are little-endian (the only value seen; `1` is read as big-endian, a **hypothesis**) |
| `UniqueId` | the plug-in id again |

Checked on a real project's 14 VST2 plug-ins: every `FxBk` buffer is exactly
`ProgramCount × (28 + 4 × ParameterCount)` bytes. State the importer cannot wrap (another `Type`, a buffer of
the wrong length, an unknown `ByteOrder`, missing counts, more than 65 536 programs or
parameters) is kept unread in the `VstPresetBuffer` extension with a warning; a missing
`ByteOrder` is read as `2`. **Never commit a corpus file that contains a commercial plug-in's state.**

### Utility (`StereoGain`)

Corpus: series 6, files `110`–`135`, and two extra files at 50M and 50S. Those files were saved as a chain, each from the previous
one, not from the default, so a neighbour diff shows the step; `128` has both phase switches on.

Live's Utility is the element `StereoGain`. Parameters that can be automated hold their value in
`<Name>/Manual@Value` next to an `AutomationTarget`; the two plain elements hold it in `@Value`.

| Element | Stored | Observed | Suhina `utility` |
|---|---|---|---|
| `Gain` | linear gain | -12 dB → `0.2511886358`, +6 dB → `1.99526238`, -inf → `0`, fully up → `56.2341309` (+35 dB) | `gain` = 20·log10 of it, dB; `0` → the minimum (silence) |
| `Balance` | -1 … 1 | the knob shows 50L … 50R; typing `50L` stored `-1`, typing `100R` stopped at `1` | `balance`, the same number |
| `StereoWidth` | fraction | 0 % → `0`, 200 % → `1.99999988`, fully up → `4` (400 %) | `width`, the same number |
| `MidSideBalanceOn` | `@Value` true/false | the Width knob's Mid/Side Mode (right-click menu, not on the panel). Turning it on reset `StereoWidth` to `1` | when true: `width` = 1 and `midSide` from `MidSideBalance` |
| `MidSideBalance` | 0 … 2, linear in what the knob shows | on switching the mode on, the knob shows `0` and stores `1`; 100M `0`, 50M `0.5`, 50S `1.5`, 100S `2` | `midSide` = value − 1 (−1 only mid, +1 only side), only when `MidSideBalanceOn` is true |
| `Mono` | true/false | | `mono` |
| `BassMono` | true/false | | `bassMono` |
| `BassMonoFrequency` | Hz | 50 … 500, default 120 | `bassMonoFrequency` |
| `BassMonoAudition` | `@Value` true/false | the headphones button by the frequency; not automatable | `bassMonoListen` |
| `PhaseInvertL`, `PhaseInvertR` | true/false | `Ø L`, `Ø R`, independent | `invertLeft`, `invertRight` |
| `ChannelMode` | 0 … 3 | `0` Left, `1` Stereo (default), `2` Right, `3` Swap | `channel`: Stereo 0, Left 1, Right 2, Swap 3 (a different order) |
| `DcFilter` | true/false | | `dcFilter` |
| `Mute` | true/false | | `mute` |
| `On` | true/false | the device switch, as on every device | the model's `Device::enabled` |
| `LegacyGain`, `LegacyMode` | `0`, `false` | unchanged in every file | not mapped **(hypothesis: older sets stored gain here)** |

Still open: how Mid/Side between the ends scales mid and side in the sound (the stored value is
linear in the display; Suhina keeps one at full and turns the other down linearly), and
whether Width and Mid/Side sound like Suhina's at the same settings. Both need bounces of
`118` and of the 50M and 50S files (`120-utility-midside-mid-50M.als`, `…-50S.als`).

### MIDI effects

Not from the corpus series but from three measurements of Live 12.4.6 kept in the device-specs
repository (`measurements/`):

- **A parameter dump.** Every MIDI effect was loaded fresh, and each parameter's name, range,
  default and choices were read through Live's API (`dump-midi-12.4.6.json`).
- **The set Live saved from it** (`populate-midi-12.4.6.als`): one MIDI track per device. It
  gives the element names and every stored element of a fresh device.
- **A calibration set.** A generated set holds one track per parameter element, with only that
  element's stored value changed from the default. Live opened it, and the API reported which
  parameter moved and to what (`als-map-12.4.6.json`). Every parameter element of the eight
  devices matched exactly one API parameter, with no element and no parameter left over.

The device is an element in the track's `Devices`, before the instrument, as on every device:
`On/Manual@Value` is the device switch and `UserName@Value` the name the user gave it.
Every parameter is stored in `<Element>/Manual@Value`. Unless the table says otherwise, the
stored number is the one the API shows: a choice (marked *index*) is stored as its position in
the list, from 0, and a switch as `true`/`false`. A stored number with a fraction is cut towards
zero when Live loads it, for every parameter in whole steps (−51.2 semitones reads as −51, a
Lowest of 88.9 as 88).

| Device | Element | Parameter elements and the ranges stored | Stored differently from the API |
|---|---|---|---|
| Arpeggiator | `MidiArpeggiator` | `Mode` (Style, index 0 – 17), `PatternOffset` (−8 – 8), `RepeatCount` (0 – 16, 0 = endless), `SyncState`, `SyncedRate` (index 0 – 13), `Groove` (index 0 – 3), `FreeRate`, `Gate` (% 1 – 200), `Retrigger` (index 0 – 2), `RetriggerInterval` (index 0 – 14), `Hold`, `TransposeMode` (index 0 – 35, 0 = Chromatic), `TransposeKey` (0 – 11 from C), `TransposeSteps` (0 – 8), `TransposeDistance` (semitones −24 – 24), `VelocitySwitch`, `RetriggerVelocity`, `VelocityDecay`, `VelocityTarget` (0 – 127), `UseCurrentScale` | `FreeRate` and `VelocityDecay` in milliseconds as displayed; the API takes 0 – 1 along a curve |
| CC Control | `MidiCcControl` | `ModWheel` (0 – 127), `PitchBend` (−64 – 64), `Pressure` (0 – 127), `CustomBoolValue`, `CustomFloatValues.0` … `.11` (0 – 127) | none. Also stored: each custom control's name (`CustomBoolName`, `CustomFloatNames.n`) and the controller it sends (`CustomBoolTarget`, `CustomFloatTargets.n`); whether a target is the CC number or a position in the list is not known yet |
| Chord | `MidiChord` | per note n = 1 … 6: `Shiftn` (semitones −36 – 36), `ShiftScaleDegreesn` (−36 – 36), `Velocityn` (a factor 0.01 – 2), `Chancen` (0 – 1); then `Strum`, `StrumTension` and `StrumCrescendo` (% −100 – 100), `UseCurrentScale` | `Strum` in milliseconds (−400 – 400) = sign(x)·x² of the API's −20 – 20. Also stored: the two right-click options, `PlayDuplicateNotesWhenStrumming` and `SendPerNoteEventsToGeneratedNotes` |
| Note Length | `MidiNoteLength` | `Mode` (Trigger Source, a switch), `Latch`, `SyncState`, `SyncedLength` (index 0 – 9), `TimeLength`, `Gate` (% 1 – 200), `Balance` (Release Velocity, % 0 – 100), `DecayTime`, `DecayKeyScale` (−1 – 1) | `TimeLength` and `DecayTime` in milliseconds (10 – 60000); the API takes 0 – 1 along a curve |
| Pitch | `MidiPitcher` | see [`pitch.md`](pitch.md) › Import | none |
| Random | `MidiRandom` | `Chance` (0 – 1), `Choices` (1 – 24), `Scale` (Interval, 1 – 24), `Sign` (index 0 – 2), `Alternate` (Mode: `false` random, `true` alternate), `UseCurrentScale` | none |
| Scale | `MidiScale` | `Base` (0 – 11 from C), `InternalScale` (index 0 – 35, 0 = the user's own grid), `UseCurrentScale`, `Transpose` (semitones −36 – 36), `Fold`, `Lowest` (0 – 127), `Range` (0 – 128), `Mapping.0` … `.11` (−1 – 12, the grid) | none |
| Velocity | `MidiVelocity` | `Drive` (−1 – 1), `Compand` (−1 – 1), `Random` (0 – 64), `Mode` (index 0 – 2), `Operation` (index 0 – 2), `MaxOut` (Out Hi, 0 – 127), `MinOut` (Out Low, 0 – 127), `Range` (0 – 127), `Lowest` (0 – 127) | none |

Several devices also store view positions that are not part of the device (`…ScrollPos…`,
`…RelativePosition`), and Arpeggiator stores four `…CompatibilityMode` switches, all `false` in
a fresh device **(hypothesis: `true` in sets from older versions, to keep their behaviour)**.
Sets saved before Live 12 are not observed: Arpeggiator's transposition and Random's mode were
stored differently then.

Only Pitch has a Suhina device; the importer maps it and keeps the other seven as placeholders
([device import](device-import.md)), which pass notes on unchanged. Not observed yet: the MIDI
Effect Rack, and a Max MIDI effect (series 8, file `406`).

## Noise: ignore when importing or diffing

These change without any musical edit:

- `LiveSet/NoteAlgorithms`: state of the MIDI tools; renumbered on every save.
- Everything named `ViewState*`, `SequencerNavigator`, `ScrollerTimePreserver`, `TimeSelection`,
  `ClientSize`, `ScrollerPos`, `CurrentZoom`, `HighlightedTrackIndex`, `Selected*`,
  `IsContentSelectedInDocument`, `LastSelected*`: view and selection state.
- `ColorSequenceIndex`, `AutoColorPicker*/NextColorIndex`: automatic colour assignment.
- `TakeId` on clips.
- `Transport/CurrentTime`: the play position.

## Open questions (next corpus files)

- Note probability and velocity deviation; MIDI CC and pitch-bend envelopes.
- Warp modes (`WarpMode` codes).
- Audio clip fades: where a set stores an arrangement clip's fade-in and fade-out, their unit
  and curve. The model has them (`Clip::fade_in`, `Clip::fade_out`, in beats, and their shapes
  and curves `Clip::fade_in_curve`, `Clip::fade_out_curve`); the importer leaves them at 0
  (linear, unbent) until [corpus series 9](als-corpus.md#series-9-audio-fades) shows the format.
- Return tracks added or removed; sends routed pre-fader; track output to another track.
- Tempo and time-signature changes over time: the reading above is a hypothesis until corpus
  file `311-tempo.als` and a time-signature change are recorded.
- CLAP plug-ins, Audio Units, racks.
- Max for Live devices: how the set refers to the `.amxd` file, what Collect All and Save does
  with it, where parameter values are stored ([corpus series 8](als-corpus.md#series-8-max-devices)).
- Locators: where they are stored and what else a locator carries ([corpus series 11](als-corpus.md#series-11-locators)).
- The arrangement loop: where the set stores it and in which unit ([corpus series 12](als-corpus.md#series-12-arrangement-loop)).
- Sets saved by older Live versions.
