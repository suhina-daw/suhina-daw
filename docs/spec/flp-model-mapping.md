---
description: Initial FLP project import, shared patterns and sampler channels, and explicit playback limits
read_when: FLP import, FL Studio patterns, FLP model mapping, imported sampler
type: spec
status: draft
last_verified: 2026-11-12
covers:
  - crates/flp/src/import.rs
  - crates/flp/src/appearance.rs
  - crates/flp/src/records.rs
  - crates/flp/src/effects.rs
  - crates/flp/src/automation.rs
  - crates/flp/src/normalize.rs
  - crates/flp/src/stretch.rs
  - crates/flp/src/wrapper.rs
  - crates/flp/src/patcher.rs
  - crates/flp/src/mixer.rs
  - crates/flp/tests/import.rs
  - crates/session/src/lib.rs
---

# FLP project import

## Playlist appearance evidence

The maintainer's self-authored `empty.flp`, `red-on-channel-rack.flp`,
`red-and-blue-on-channel-rack.flp`, `red-and-blue-on-arragement.flp` and
`grouped-on-arragement.flp` establish the following fields. Channel event 128
stores RGB in the low three bytes, enabled by event 41. Playlist event 238
stores the one-based row at offset 0 and RGB at offset 4, enabled by the
following event 43; event 239 is its name. Red is (96,34,32), blue (73,130,185).
Grouping the second row changes byte 46 of its 238 record from 0 to 1:
it belongs to the nearest preceding ungrouped row. This is presentation,
not a mixer routing instruction. Pattern event 150 uses an FF high byte
for a custom colour (public Lemon Studio reader at the revision cited below).
An explicit playlist row colour takes precedence for both its placements and the
source track on its first placement. Pattern, sample and source colours are used
only when the represented playlist row has no custom colour.

Playlist groups become channel-less presentation folders, marked by the neutral
`suhina/folder` extension `[1]`. Existing mixer families remain intact inside a
folder when all their placements belong there; a family shared across folders
stays outside and produces a warning. No channel output or send is changed.

Audio placement records of at least 60 bytes store fade-in milliseconds/tension
at offsets 36/40 and fade-out at 44/48, and linear gain at 52. Public Lemon Studio
`os-model` reads positive stored durations regardless of the manual/automatic
flags at byte 56. Convert milliseconds to project beats, retaining editable native
fade handles. Its raised cosine `s=(1-cos(pi*x))/2` bends with `q=1+abs(tension)`:
positive tension gives `(q-q^(1-s))/(q-1)`, negative `(q^s-1)/(q-1)`.
The neutral native cosine shape uses `curve=sign(tension)*ln(q)/(32*ln(2))`;
out-of-range tension is rejected rather than converted to non-finite model data.
Zero duration has no fade; malformed active fade values are import errors.
Overlapping fades, or fades longer than the placed clip, currently fit the native
non-overlap constraint and produce an explicit warning; these are not claimed
to reproduce the original envelope. Their source durations remain retained.


The initial importer extends the container reader. It reads channels, pattern notes and
playlist placements into the existing project model (ADR 0001). It is a partial import,
not a promise of identical playback. Import warnings name the unsupported behaviour.
Unavailable instruments carry the neutral missing-instrument marker, preventing fallback tones even when soloed.
The existing project.open command opens the result through the same session path in every frontend.

## Mapping

Imported arrangement tracks are the source instrument/sample tracks, so clips, devices,
meters and mixer controls address the same channel. Patterns with several source channels
split into per-source contents cached by original content and source; repeated placements
still share their per-source notes. All placements of one source merge into one arrangement
lane, even when the original uses several playlist rows. Original row labels stay in clip
extension metadata. Instruments and shared mixer buses are never duplicated.
Mixed-row groups survive only when each member belongs exclusively to that group and all
sounding members have the same output: the group is inserted before that output, preserving
the shared mixer chain. Otherwise the original grouping is reported as unsupported rather
than creating group controls with no audio path. Automation placements become lanes of the addressed device/channel when supported;
unsupported links remain in source metadata and warnings without extra arrangement tracks.
Audio clip offsets are f32 milliseconds, converted to beats at the project tempo;
automation offsets are f32 beats. A negative offset means the source start. These units
are recorded in the pinned os-engine/src/engine.rs audio-clip scheduling code. Per-source trimming and advanced envelope processing remain explicitly unsupported.

Audio stretch settings are the four u32 fields at event 215 offsets 96..112. The first
is f32 time in 1/192 beats, the second signed pitch cents, the third a signed log2
length multiplier in 1/10000, and the fourth the method (0 repitch; -1/-2 realtime;
1..9 offline/auto methods). File import probes sample duration without decoding audio
and maps the fitted beat length to that duration with two warp markers. Source-time
clip offsets are converted through this map; timeline automation keeps its elapsed-beat
positions. The pinned os-model ChannelSound and os-engine channel setup define these
laws. Mode equivalents use Suhina stretchers, not identical algorithms. Missing duration
or unsupported ratios leave an explicit warning instead of inventing the sample tempo.

Channel mixer assignment is event 104 (or signed byte 22 in legacy files). It selects one
shared mixer bus, never a copied effect chain. Mixer blocks start with event 236 and its
preceding name/color header. Event 204 names a bus and 235 lists enabled destinations.
Event 149 carries its BGR colour when event 42 enables the custom colour (the pinned
os-flp mixer reader). Retained buses and returns keep that colour unless their represented
playlist row has a custom colour: playlist colour takes precedence over source and mixer
colours, including a bus family's first represented row. A placement's playlist colour also
overrides its pattern or sample colour. Mixer, source and pattern colours are fallbacks.
201/213 followed by slot marker 98 describe ordered effects. Parameter event 225 contains
12-byte records: parameter id at 4, kind at 5, location u16 at 6, signed value at 8.
Location is 0x7000 + bus*64 + slot (0x2000 in legacy files). Kind 0x1f parameter
0xc0 is the volume knob, 0xc1 pan /6400; slot parameters 0 and 1 are enable and wet /12800.
The old mixer location range is detected from the records, independently of the playlist
version boundary; the modern range began at 24.2.
Send kind 0x20 uses parameter id as destination; older 0x1f ids 64..190 subtract 64.
The final stored bus is the special current bus (parameter index 501), not an insert.
Single routes become primary outputs with a final route-gain stage. Multiple routes
become post-fader sends; a silent output sink prevents an unintended additional route to Main. Bus zero becomes Main. Unknown effects retain their native
state and slot order as placeholders. Missing destinations and cyclic routes fail validation.

Mixer faders and send levels use the source gain curve `(11^(stored/12800)-1)/10`,
not their normalized knob position. Source-channel volume and pan become a Utility after
the generator and before the shared mixer; its gain and balance jointly represent the
source's equal-power pan. Source faders stay at unity. Audio source levels are relative to
the original default level 10000. The source default sample envelope is one-shot.

The mixer equaliser is inserted before effect slots, as requested for this import. Its
three bands are low shelf, bell and high shelf (os-dsp/src/mixer.rs), with gains /100 dB,
frequency `(101^(stored/65536)-1)*159.9+10`, and independently stored widths. Suhina's
DJ EQ uses crossover bands and has no independent middle frequency/width; the ordinary
EQ preserves these controls more faithfully. Only three bands are enabled. Widths are
converted to Q from the published coefficients; values beyond Suhina's range are clamped
and reported. Flat mixer EQs add no device.

Native effect substitutes use the pinned os-engine effects modules as their state-layout
source. Balance (two i32 values) becomes Utility; Parametric EQ 2 version 8 becomes EQ
with its seven gains, types and exponential frequencies; Reeverb 2 versions 10000/10001
becomes Reverb with predelay, decay, size and wet/dry; Delay 3 version 1 becomes Delay
with time, feedback, ping-pong and wet/dry. These are approximate substitutions, named in
the import report: the algorithms, EQ widths and advanced modulation differ. Original
states are retained in extensions. Unsupported versions remain placeholders. Fast
distortion has no verified Overdrive equivalence and remains a placeholder.
Reverb uses its source's exponential level curve (dry /128, wet /256); its equal-power
mix is atan2(wet,dry)/(pi/2), with a following Utility at hypot(dry,wet). Early-reflection
level and stereo matrix are not translated. Delay uses a linear mix with a separate
output correction. Neither substitution claims identical wet-signal loudness.

- Channel event 64 starts a channel; 51 ends a modern channel. Older channels end at the
  next channel or playlist/mixer section. A channel becomes a track and mixer channel.
- Channel type 21 identifies a sampler (0), plug-in (2), layer (3), audio (4) or automation (5).
  Display name 203, sample path 196, plugin name 201, plugin state 213 and levels 219 belong
  to that channel. Levels contain i32 pan, volume and pitch at offsets 0, 4 and 8:
  pan / 6400 - 1, volume / 12800, pitch in cents. Root key is event 135.
- A simple sampler becomes Suhina's Sampler with its source file and root key. An empty
  sampler stays a Sampler without a source file; the importer does not invent a tone. Advanced
  sample processing and envelopes are not translated; the import report says so.
  Wrapped VST2/VST3 instruments use the host mapping below. Other instruments are silent
  Foreign placeholders, with their state retained. Channel enabled event 0 maps to the
  main instrument device's enabled state, including sampler and instrument groups, not
  the source channel's mute. Audio-only sources retain their source channel mute. Mixer
  enabled (event 236 byte 4 bit 0x08) independently maps to the mixer track or bus mute;
  muting a bus does not overwrite its sources' device states. Automation and
  advanced channel modulation are not played by this initial slice.

Mixer inserts with a single primary output and no incoming sends are Submix channels, so
the routing controls expose them as group destinations. Inserts receiving sends retain
the Effect role; parallel-send sources stay Regular to preserve send panning. The UI names
the actual output channel even when it is not a selectable group destination.

The final mapping keeps only mixer channels reachable from imported sources, including
their primary outputs and sends. A mixer insert fed exclusively by one source is folded
into that source: instrument, source Utility, then mixer effects, with mixer volume, pan,
mute and automation on the resulting track. Instrument enabled remains independent.
An audio source's own mute moves to its source Utility before this fold. Shared primary
destinations are parent groups; additional routes use sends. A group may also receive
sends, without duplicating its effects. Unused mixer strips remain in retained source
data but do not become project tracks.
- Pattern event 65 selects a pattern, 193 names it, and 224 contains 24-byte note records.
  Each pattern becomes one shared content, with a targeted notes lane per channel.
  Positions and lengths are ticks divided by the header PPQ. A note record holds u32
  position at 0, u16 channel at 6, u32 length at 8, u16 key at 12, and velocity at 21
  (0..128). Byte 19 bit 0x20 mutes a note. Note expression is retained but not
  interpreted. A note whose length is all ones (a step-sequencer step, which has no
  note-off) or zero is left out of the pattern, stays in the retained source and is counted in
  one warning; it never stops the import.
- Playlist event 233 contains records of 32, 60, 80 or 88 bytes, selected only when all
  records have the 0x5000 marker at offset 4. Records hold position at 0, source at 6,
  length at 8, reversed row at 12, mute flag 0x20 at 19 and pattern offsets at 24/28.
  A source at least 0x5000 names pattern source - 0x5000. Pattern placements reference
  shared content, preserving their start, length, trim offset and mute. Rows count down
  from 198 before 12.9 and 499 afterwards. Audio placements reference their source channel;
  automation placements stay disabled. Only the selected arrangement (event 100, numbered
  by 99) is played.
- The unmodified input file is retained once in project Extensions as flp/source, so
  event ordering, unknown data and associations survive saving to .suhina. This includes
  the original license metadata: it is never printed in diagnostics. No FLP writer exists.

Playlist track records (event 238) have a one-based row number u32 at 0 and enabled byte at 12. A disabled row disables all its placements, including automation.

## Audio volume automation

Event 234 holds a channel automation curve: point count u32 at 17, followed at 21 by
24-byte records (f64 delta beats, f64 value, f32 tension, mode byte at 20). A segment's
mode belongs to its end point. Event 227 links its channel u16 at 2 to target u32 at 8; links with the controller flag 0x8000 are not automation-clip links.
For channel targets the upper word is the target channel, low word 0 is volume and
0x11 is the volume multiplier (os-model target constants); pan is deferred.
Automation range comes from event 219's first two integers, both divided by 12800.
Volume uses `(11^value-1)/10`; multiplier uses `(11^(2*value)-1)/10`.

The first supported case is an enabled automation placement covering an entire audio
placement, with one unambiguous volume link and no overlapping controller. Its curve
becomes that audio clip's gain envelope, at content-relative beats including the clip's
trim. Single and double exponential segments (modes 0 and 1), straight segments and holds are supported, with gain interpolation
sampled into dB. The source Utility remains the static level; volume automation is
relative to that level, while multiplier automation multiplies it. A silent static volume,
partial overlap, multiple controllers, formulas, LFOs or other curve shapes stay unsupported
and are reported; no approximate link by name is attempted. The disabled automation
placement remains visible as source information. Converted envelopes are capped at 10000
points per clip and one million points per import.

## Limits and validation

File import first keeps a stored absolute sample path that is on a local disk (a drive letter on
Windows) without parent steps and names an existing file; a network share or a device path the
project names is decided from the path alone and never touched
([missing files](missing-files.md) › Project file boundaries). Otherwise it checks the
stored relative path (or an absolute path already inside the project directory), then the
filename beside the project and in its Samples, Recorded and Rendered subdirectories,
then the caller's library candidates as described below. Only existing regular
files are used; no parent traversal or symlinks are followed. A local match becomes a relative
reference while the original absolute reference is retained. Stretch timing reads a sample's
duration only from a file resolved this way. Missing media is reported,
not a failure to open the project. Nothing is created in, moved from or removed from an
imported project directory.

Imports are capped at 64 MiB, 4096 channels, 4096 patterns, one million notes and
one million placements. PPQ must be positive. Missing channel/pattern references,
truncated records and invalid numeric values are errors with offsets, not invented music.
Versions 10 through 26 can be attempted; older step sequencing and non-ASCII legacy text
are not supported yet. Other versions give an actionable error. Every successful result
is validated by the model before a session sees it.

Tests use small authored byte fixtures for mapping, malformed records, shared content and
preservation through .suhina. Real projects are tried locally without committing them or
requiring a large corpus as a prerequisite.

## Wrapped third-party instruments and effects

Public GPL source at the pinned revision above, `os-flp/src/wrapper.rs`,
`os-vst2/src/state.rs`, `os-vst3/src/fl_state.rs` and `os-vst3/src/lib.rs`, defines
this conversion. Event 201 `Fruity Wrapper` identifies event 213 as a u32 type followed
by records (u32 id, u64 byte length, payload), all little-endian. Records 51/52
hold the VST2 unique id / Windows GUID; 54 names the actual plugin and 55 its path.
A `.vst3` path selects VST3; otherwise a VST2 id takes precedence over a companion
GUID. Windows GUID fields are reordered to canonical FUID text independently of
the importing platform. Other wrapper formats remain preserved placeholders.

Record 53 contains saved state. VST2 uses i32 -9, a kind byte, i32 -2, u32 size,
two reserved u32s, then payload. Kinds 12/13 contain a bank chunk, wrapped into
Suhina's standard FBCh bank. Kinds 4/5 contain a u32 parameter count and normalized
little-endian f32 values; these become one FxBk program. The trailing source program
section is retained in the original wrapper, not interpreted. Counts are capped at
65536 and values must be finite and within 0..1. VST3 state starts with u32 1 and
uses the same record framing: 3 is component state, 2 controller state, 1 metadata,
4 a parameter-id count/list. The component and controller go to the existing host
state mechanism; the original wrapper is kept in a `flp/wrapper` extension.
Malformed or unknown saved states remain silent instrument placeholders (or bypassed
foreign effects) with a warning, rather than loading an unintended default preset.

Both channel instruments and mixer effects use this decoder. Supported instruments
retain their source mute state and are matched by the existing scanned-plugin catalog.
The importer does not load a binary directly from an untrusted project path. A stored
`suhina/instrument` extension marks hosted instruments so an unavailable installation
is silent even when soloed or bypassed. An installed host slot takes precedence.

## Audio stretch time encodings

Event 215 at byte 96 holds the fitted sample length in 1/192-beat ticks. Alongside
the floating-point encoding described by the upstream model, a user-authored project saved
by version 24.1.1.4239 stores the integer 43392 for a 226-beat sample. Interpreting those
bits as a float loses the fitted duration and moves the source trim away from beat boundaries.
The importer accepts positive integer ticks up to 192,000,000 (its one-million-beat limit)
and floating-point ticks outside that bit range. Zero means no fitted duration. Within the
integer range the same bits represent floats smaller than one tick, not valid fitted times.
Both encodings use the same duration ratio for source trims and warp markers.

## Library sample lookup

After checking a still-existing stored absolute file and then project-local candidates, the
importer offers unresolved paths in one batch to its caller. The session's project-open
path searches the existing browser index by filename (case-insensitive), without walking
the disk. A unique best candidate is accepted: compare the number of equal trailing path
components; equal-best matches stay unresolved, and a saturated 32-candidate result is
never auto-selected. Missing index entries also stay unresolved. Existing files are checked
before selection. The replacement is applied before reading source duration and warp
mapping, for both sampler devices and audio regions. Original references remain in source
metadata. No audio is copied, moved or deleted. The normal media trust boundary still
applies when audio is decoded. The importer cannot choose new library roots.

## Routing, automation and compound instruments

The mapping uses primary channel outputs for a mixer insert with exactly one
route. Its send gain is a final Utility after the insert effects, separate from the
mixer fader; it is not also sent to the same destination. Multiple destinations
remain explicit sends. This preserves shared bus processing and output-chain solo.

Automation sources are not playable tracks. Supported links produce channel or device
lanes on their actual owner; unsupported links remain in the retained source and import
warnings. VST2 targets use parameter indices; VST3 targets use the wrapper state's
parameter-id table (pinned Lemon os-vst3/src/fl_state.rs). Source slot identity is retained
independently of added EQ or gain devices. Curves are bounded sampled points, with jumps
at holds and placement boundaries and held values through gaps. Overlapping independent
sources for one target are not guessed. Audio clip volume retains its gain-envelope mapping.

Layer children are repeated event 94 channel ids; event 144 holds selection flags
(pinned Lemon os-flp/src/project.rs and os-engine layer scheduling). Neutral layers
fan their notes into the existing child sources, recursively with cycle and expansion
limits. Nonneutral gain/pan/pitch/modulation and random/sequential/crossfade selection
are unsupported rather than being incorrectly substituted with MIDI velocity.

Patcher parsing uses only pinned Lemon os-flp/src/patcher.rs: version 4 and bounded
u32/u64 chunks for nodes, named host ports and links. Representable independent branches
become device-group branches; shared or reconvergent processing is unsupported rather than duplicated across branches.
Unlinked controllers do not affect playback. Unsupported active controls, sidechains,
cycles or ambiguous ports keep the whole patch as a placeholder, never a partial graph.
Nonunity input/internal edge gains are unsupported, while output-edge gains become branch gains.
The original state is retained. No LYNX code or format research is used, except that one
note of its analysis marked as taken from saved data (the channel switch of event 0, separate
from the mixer mute) was compared, with the maintainer's leave, after the same behaviour had
been observed in FL Studio itself and found in Lemon Studio's source; nothing was taken from
it into the code. Its own
REA_rodada14/141 provenance explicitly names Ghidra decompilation. Lemon's pinned README
states independent specification/public-documentation development and excludes code
derived from decompiled output; its Patcher source describes measured cycle behaviour.

Authored-project observation (Trance Idea, read-only, 2026-11-12): the three-generator
patch has `Notes` class 1 connected to each generator's class-1 port (bus 0, flags 3),
and each class-0 output (bus 0, flags 2) to `Audio output`. Link flags are 2 and
levels differ between branches. Generator header flags are 8. These exact encodings
are accepted; other flags, linked controllers, shared/reconvergent paths and nested
patches are retained as unsupported rather than inferred from excluded sources.

Mixer volume automation uses the full 16000-unit travel, giving unity at 0.8.
Balance gain uses its 320-unit travel, converted to Utility dB, and balance to bipolar values.
When the host substitutes a different plug-in identity, imported automation events are
suppressed with a notice; saved-state compatibility does not establish parameter identity.
An opaque plug-in's unknown initial value is explicitly reported: the first automation
value holds before the first placement. Known model initial values are preserved.

Authored on/off comparison, 2026-11-13: the user's #2, #2_2 and #2_3 copies
keep the existing Spire node header flags at 12. Its output cable flags change from
2 (on, user-confirmed) to 3 (off); the new Spire has flags 12 and an enabled output
cable at approximately 0.43. Thus flags 12 must not be treated as node bypass.
The importer accepts the observed node flags 8 and 12, skips disabled links (3), and
retains a MIDI-driven branch with no audio output as muted. Unknown flags remain unsupported.
These Spire instances store wrapper kind 15 with the same 21-byte header and exact
chunk-size boundary as 12/13. Its payload is treated as an opaque VST2 bank chunk;
acceptance is checked with the installed plug-in through the existing sandbox host.

## Sources

The existing [container spec](flp-format.md) and GPL-3.0-or-later source from
[Lemon Studio os-flp](https://github.com/ItsAlphaNeon/Lemon-Studio/tree/011a03f23d9db7f10f07cd7d165cc978a9dda263/crates/os-flp),
Copyright (C) 2026 ItsAlphaNeon and thacer13. Record decoding and channel mapping are
adapted from src/project.rs and the row-version rule from src/project/legacy.rs.
Gain laws and mixer EQ settings come from os-dsp/src/{level,mixer}.rs; native effect
state layouts from os-engine/src/effects/{balance,eq,reverb,delay}.rs; audio offset units
from os-engine/src/engine.rs. Automation shapes and links come from os-model/src/{lib,curve,clip_lfo}.rs. These retain the same repository revision and licence.
Changes: bounds-checked decoding, explicit errors, resource limits, no writer or os-model
dependency, and conversion to Suhina's shared-content model. This specification records
the source-derived layouts; it does not claim independent corpus confirmation.

When multiple mixer connections include Main at unity, Main remains the primary output and
other destinations become sends. A non-unity Main connection retains the independent route
level through the routing return, since applying it to the source fader would also change
its other sends.

Normalized plug-in automation is imported as editable cubic Bezier segments, not a fixed
64-point grid. Straight and constant segments need only their boundary points. Curved
segments are fitted in normalized parameter space, checked at 63 interior positions to
1e-4 absolute error and subdivided when needed (at most 64 pieces per source segment).
Source corners, placement boundaries and discontinuities remain explicit. Identical points
at the same beat are merged; distinct values at the same beat retain their jump.
