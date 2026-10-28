---
description: Suhina's sample and granular instrument, sample loading and SDK panel
read_when: sampler, granular instrument, sample recording, sample drop
type: spec
status: draft
last_verified: 2026-10-22
covers:
  - devices/sampler/**
  - crates/device-sdk/src/sampler.rs
  - crates/device-sdk/src/sample_axis.rs
  - crates/device-sdk/src/nodes/slicing.rs
  - crates/device-api/src/sample_edits.rs
  - crates/device-sdk/src/nodes/stretch.rs
  - crates/ui/src/detail/device_canvas.rs
  - crates/ui/src/detail/detail_view.rs
---

# Sampler

## Waveform editing revision (#1026, in progress)

This revision is being implemented on #916. The following contract is the target;
implementation below is local and awaits the authorized release build and runtime validation.

- Sample, Slice and Grain are a vertical mode selector to the left of the waveform.
  They use the SDK and Suhina's theme, with a stable waveform rectangle across modes.
- The source Start/End region stays bright and the unused audio is dimmed. Selection,
  sounding slices, read heads and loop bounds have separate marks. Stereo lanes are
  drawn separately. Automatic and pinned boundaries differ by shape as well as colour.
- Ctrl+wheel zooms around the pointer. Horizontal wheel input and Shift+wheel pan;
  middle-button dragging pans without editing a boundary. Fit source and Fit selection
  change only the view. Zoom is bounded by one source frame and the whole file.
- A source axis maps file positions to seconds and quarter-note beats, using Source BPM
  and a first-beat position in the file. The ruler uses the source's time signature.
  Project tempo never moves file boundaries. Source BPM must be valid before a beat
  grid is shown; unknown tempo is not silently replaced with a musical assertion.
  Unknown tempo still shows a source-anchored seconds grid, Grid on or off, since
  the times sit on its lines; it is a visual time reference, not musical snap. Selected mode buttons have an
  explicit selection mark even when a theme replaces their face.
  The active mode uses an accent-filled button with contrasting text. The ruler has
  its own strip and shows one unit at a time: source bars/beats when the beat grid
  is available, otherwise seconds. Grid lines are subdued; slice boundaries use
  text-colour lines and caps, distinct from the accent-coloured waveform. The
  selected slice has a stronger background and bottom edge; unused audio is neutral
  and dim. Output controls precede the adjacent voice and note-release sections.
- Grid division and the Beat slicing division are independent. Straight and triplet
  grids have an explicit snap switch; Alt bypasses snap for a gesture. Grid snapping
  is distinct from zero-crossing snapping, which is deferred in this revision.
- Transient uses Sensitivity, Beat uses Division and Region uses Regions. Every mode's
  boundaries can be moved, inserted or deleted. One drag is one undo step. Numerical
  editing and waveform dragging use the same command and validation.
- Manual corrections are source positions. Automatic analysis generates a candidate
  map; pinned positions and suppression records modify it. A moved automatic boundary
  records suppression of its original candidate and pins the destination. Transient
  candidates have source-frame identities; beat/region candidates use method and
  ordinal identities. Suppression belongs to its method; pinned positions survive
  method changes. Trimming hides points without deleting them. Regenerate clears
  corrections for the active method and pinned points in one undoable edit.
- Inserting/removing a boundary changes the ordinal MIDI mapping, so the panel always
  shows the selected slice's MIDI note. Notes before Root or after the last slice are
  silent. Source replacement clears source-specific corrections in the same undo step.
- Slicing method, voice overlap (one, polyphonic, through to the source-region end)
  and note release (play to boundary or release on note-off) are independent choices.
  Short fades apply at the actual played region's edges, including reverse and warp.
- Saved legacy parameters retain their meaning. Old fixed and dynamic manual maps
  remain readable. New state has bounded input, explicit versioning and migration.
  Analysis and map preparation happen off the audio thread, and processing consumes
  immutable bounded data through the existing graph/resource handoff.

The interaction reference is the public instrument manual's slicing description
(https://www.ableton.com/en/manual/live-instrument-reference/#slicing-playback-mode),
checked 2026-10-27, and the user's workflow observations. Grid snapping, correction
identity and navigation above are Suhina design decisions, not claims of equivalence.
No proprietary screenshots, transcripts or assets are included in the repository.

Validation must cover pointer anchoring, source/beat conversion, snap and free edits,
automatic corrections, undo/save/reopen, empty and short sources, reverse/stereo/warp,
malformed maps and legacy renders. Compiling checks and desktop verification are
pending the user's explicit permission to build; no passing result is implied here.

Sampler is a single-source polyphonic instrument. Sample reads a sample at a rate
determined by MIDI key (root 60), transpose and cents. Grain schedules bounded,
Hann-windowed grains from the same source. Both use the same amplitude envelope,
filter and output controls. All voices and grains are allocated before processing.
Sample decoding, replacement and disposal occur outside the audio callback.
The graph source prepares the shared kernel once per processing span (split at
timestamped note events), retaining the native block grain budget when voices
finish during the span.

The SDK panel uses a fixed 900 by 168 logical-pixel canvas, leaving the host's
header and margins inside the 200-pixel device row. Its height never depends on
the selected page. The layout also gives a 400 by 20 strip the host draws in the device's
header row, left of its maker, the device's name giving way to it: Input and Record, and the
pages Sample, Controls and More, the open one outlined. A built-in layout's header strip is at
most 480 by 20 pixels (`builtin::HEADER_WIDTH`, `HEADER_HEIGHT`).

Sample, Slice and Grain are buttons in a column left of the waveform, each a drawn glyph over
its caption (a wave between two bars, a strip cut in three, scattered grains); the chosen one has
a lighter face and its glyph and caption in the accent, the others are in a dimmer text colour.
On the Sample page the waveform fills the space right of the mode buttons, 96 pixels high, with
identical bounds in every mode; its times (`0:00`, `0:00.480`, or bar, beat and sixteenth from
the source tempo: `5`, `5.3`, `5.3.2`) sit on the grid lines along its bottom. Stereo channels
have separate lanes; Start–End stays bright and the source outside it is dim. Slices alternate
between the plain background and a lighter band, so neighbours stand apart; their lines take
the accent turned to the opposite hue and lightened, so they differ from the waveform in any
theme. A slice past the last key (127, counted from Root) is dimmed and has no line: no note
plays it. The selected slice has a background highlight, pinned boundaries a thick line and a
dot, and voice heads a separate text-colour line. Clicking a slice selects it and plays its note
for a moment; its number and note are in the waveform's context menu. Dragging the waveform away
from a boundary zooms the view, in as it goes down and out as it goes up, around where the drag
began, and moves it sideways with the pointer. Marker colours come from the theme.

The Controls page puts in the waveform's place the LFO (its drawn shape, and its pitch, filter,
volume and pan depths) and the Amplitude and Filter envelopes (A, D, S, R) and the Pitch
envelope (A, D, Amt), each in a titled section with its values two by two over its graph. Each
ADSR stage has its own draggable point, with decay also adjusting sustain; the pitch peak
adjusts attack and signed amount. Dragging the LFO's drawing sets its rate. Clicking the open
page again returns to Sample. `panel_view` 2, the earlier LFO page, shows the Controls page.
Switching pages does not change sound parameters.

The source row, under the waveform, has captioned controls for the mode: Slice has Note end
(Play to end or Release on key up), Slice by and its Sensitivity, Division or Regions; Sample has
Playback, Start and End; Grain has Position, Scan and Size. A line divides them from Voices
(Mono, Poly or, for slices, Through) and their Max number, and another from Warp, its mode and
the source tempo. The sound row under it has three sections on their own backgrounds: Filter
(type and slope) with Cutoff and Res knobs; the LFO's shape, rate unit and rate; and knobs for
Fade in, Fade out, Pitch, Vel, Volume and Pan; then Reverse and Retrigger. The LFO's bordered
Hz/time value accepts typing as well as dragging, and beat modes offer the musical division
menu. Both rows stay on the Controls page. More replaces both rows with cents, root, key and
velocity ranges, glide, loop bounds and fade, grain density, jitter and spread, the filter
envelope's amount and velocity and key tracking, First beat, the editing grid and its Snap,
and Freeze.

All controls retain SDK bindings and dispatch device.setParam. The panel has no
fixed colour stylesheet: typed controls inherit the active theme and user CSS.
Control identities use parameter names (with suffixes for repeated controls),
bounded by the existing device-panel scope. Knobs use the theme's artwork;
mode buttons share the scoped button foreground/accent, and waveform ink, grid and
background use the waveform scope. Changing the page never changes sound values.

The Sampler is a built-in device: a project stores it by its id with its parameter values by id,
and its panel is its layout, loaded by id from the running Suhina
([ADR 0021](../decisions/0021-device-graph-files.md) › Amendment: device panels). A parameter a
project does not store takes its default, so the LFO destinations and source fades added later
start at zero and LFO shape at Sine. `panel_view` persists Sample (0), Controls (1), the earlier LFO page (2, shown as Controls)
or More (3); new devices default to Sample. Legacy `grain_env` and `envelope_tab` remain readable but no
longer select an editor.
Numeric values drag vertically, Shift gives precision, and double-click resets.
All edits use the shared device parameter command, persistence and undo.

Start/end and loop markers are fractions of the source. Empty or crossed ranges
produce silence. Forward loops wrap at the boundary; ping-pong reflects the
read head. Ping-pong remains readable in saved projects but has no panel control.
Changing playback mode immediately silences existing voices; selecting the current
mode leaves them playing. Gate releases on note-off; one-shot ignores note-off. Reset silences all
voices and restores the deterministic grain random sequence. Playback is linearly
interpolated, mono is duplicated, and stereo retains its channels.

Grain position is a fraction of the selected region, scan is source seconds per
second, size is milliseconds and density is grains per second per voice. Jitter
scatters grain positions, spread scatters stereo pan, and Freeze stops scan while
grains continue. Active grains finish when controls change. Limits are 32 voices
and 32 simultaneous grains per voice; a full grain pool skips the next grain.

Sample files dropped from the browser or operating system replace the source
through device.loadSample, an undoable command shared by UI, CLI and MCP. Loading
must validate and decode the new file before changing the project. Missing sources
produce silence and appear in the project's missing-file workflow.

The source reference in `Device.state` is UTF-8 `suhina-sample-v1\n` followed by
the file path, a NUL separator and its absolute fallback path. Legacy references
without a separator remain readable. Saving rebases the file path relative to the
project folder, as for clips. Audio samples are not embedded in that state.
Recorded files live in the project's existing Recorded directory; undo restores
the previous source without deleting the recording. Record/Stop operate immediately
on the arrangement clock, independently of launch quantization. The project must
be saved, an audio output open, the track armed, and an audio input selected.

The Voices selector switches between Poly and Mono (last note replaces the voice).
Glide is available under More and controls the existing mono pitch transition time. Pitch env
adds a signed semitone offset with its own attack and decay back to zero, in both
Simple and Grain modes. These parameters use the standard device parameter command
and project persistence. Defaults preserve existing projects.

The waveform displays every active voice read head from the audio engine's
atomic meter telemetry, including live MIDI. A visible waveform requests frames
every 16 ms even while idle, so a new note does not wait for mouse activity. Grain mode shows the scan position.
Record and the input selector occupy the top row outside the waveform; Stop
is visible only while sample recording is active.

## Panel and voice controls

Sample/Slice/Grain buttons and their glyphs sit at the left. Editor tabs sit at the upper right.
The waveform and recording controls remain visible when the editor is open.
Velocity-to-volume is the Vel knob; the filter envelope's amount and its velocity and key
amounts are under More. Tuning and note range are under More; gain and pan are knobs on the
normal page.
Glide is under More and affects Mono playback.
Playback uses a Gate/One shot/Loop menu plus Reverse and Retrigger. Retrigger is
on by default: a reused voice starts from the sample region's beginning. With it
off, an active reused voice keeps its position and envelopes while changing key
and velocity; unused voices still start normally. Replaced voices interpolate
from their last output to the new voice over 5 ms, without extra audible voices.

The filter menu offers Off, low-pass, high-pass, band-pass and notch.
The slope menu selects one or two cascaded biquad sections (12/24 dB/oct for low/high-pass).
A dedicated filter ADSR drives Env amount in octaves independently of AMP.
Cutoff < Vel adds octaves proportional to note velocity. Cutoff < Key adds the
signed amount in octaves per octave above the root key. All modulation amounts
default to zero. Existing parameter ids and their order retain their meanings;
new controls are appended. The filter's previous envelope amount now uses its
own ADSR, whose defaults match the amplitude envelope defaults.

Each active voice publishes its own read head through a fixed 32-slot meter
array, without allocation or locking on the audio thread. The waveform draws a
line per active voice, including release tails, up to Poly's limit or one in
Mono. Grain reports each voice's scan position. Inactive voices have no line.

## LFO destinations and waveform handles

One bipolar LFO drives independent depths: pitch (semitones), filter cutoff
(octaves, 0..8), volume (attenuation depth, 0..100%) and pan (0..100%). New depths
default to zero. Filter depth multiplies cutoff by 2^(wave * depth), then applies
the existing safe cutoff clamp; it has no effect while the filter is Off. Volume
uses 1 - depth * (wave + 1) / 2, so it never boosts above the base gain. Pan adds
wave * depth to the base pan and clamps to -1..1. All destinations use the same
per-sample phase in both Sample and Grain, with no allocation or locking.

There are no separate arrow handles for loops. In Sample/Loop playback, S/E edit
loop_start/loop_end; otherwise S/E edit start/end. Grain always uses source
boundaries, since its scan reads the source region rather than a sample loop. Hover text explains the current
role. Source and loop ranges remain independently editable through numeric fields.
All SDK drag-value controls traverse their full normalized range in 400 logical
pixels, independent of control height; Shift retains the precision multiplier.
Filter cutoff/resonance use compact dials and slope uses adjacent 12/24 buttons.

## Source fades and LFO shape

`lfo_shape` selects Sine, Triangle, Saw up, Saw down, Square or Random. The menu
shows the selected wave icon and an icon beside every option. The large preview
uses the same waveform function as DSP. Random holds one deterministic value per
oscillator cycle, independently of audio block size and the grain random sequence;
reset restarts its sequence.

`fade_in` and `fade_out` independently attenuate the selected source region's
edges (0..1000 ms, default 0). They are linear functions of source position and
multiply when overlapping; reverse playback follows the same source edges.
Grain applies them to each grain's source reads before its own window.
With Sample Playback = Loop, Fade in ms and Fade out ms attenuate the Loop start
and Loop end edges on every pass, including reverse and ping-pong playback.
Outside the loop region, the source edges still apply. Zero fades preserve the
full loop length without overlapping or skipping source frames. The historical
`crossfade` parameter is retained for saved-definition compatibility but has no
sound effect or panel control. Loop start and Loop end fields are on More and
apply in Sample with Playback = Loop. Source Start/End markers stay available in
the waveform. Hidden controls cannot intercept input.

The waveform occupies y=22..100, with its own ruler and slice/audition footer.
The source row occupies y=102..120 and the lower strip y=122..168. None of the
pages changes the panel height. Numeric values support Shift precision; regular
parameter widgets reset on double-click. Bordered rate fields use click-to-type
and dragging, preserving the shared parameter command and undo path.

## Envelope graph geometry

Each envelope graph alone has a darker theme-derived surface. Parameter readouts
remain outside that surface. Editable points have 9-pixel diameter and an 11-pixel
capture radius; 8-pixel inner padding prevents clipping at minimum/maximum levels.
A/D/R use cumulative stage times on one fixed logarithmic time axis (10 ms base,
up to the sum of the stage maxima). Zero Decay puts D at A's x coordinate; zero
Release puts R at S. The ADSR hold segment occupies one quarter of the plot width
and represents a held note, not an additional time parameter. Pitch uses the same
cumulative Attack/Decay geometry around a bipolar centre line.

Only actual point hit targets edit a stage. A click does not change sound values;
a drag retains the press-time values and applies pointer displacement without
snapping to the edge of an arbitrary region. A edits Attack, D edits Decay and
Sustain, S edits Sustain, R edits Release. Pitch A edits Attack and signed Amount;
Pitch D edits Decay. Closest-point selection resolves overlapping hit areas.
Double-click resets the point's parameters. Keyboard arrows and Shift precision
remain available; commands, grouping, undo and persistence remain shared.
Definition revision 7 updates the panel only and preserves all sound parameters.

## Slice playback

Slice is a visible synthesis mode (value 2); the previous Sample (0) and Grain (1)
values keep their meanings. Old Sample + Playback Slices remains playable with its
original tempo-based mapping. New Slice uses separate controls: Transient with
Sensitivity, Beat with Division, Region with Regions (1..128), and Manual.
Beat uses Source BPM and First beat rather than transport tempo; its whole-bar
choice uses the source time signature. Region boundaries divide the whole source,
so trimming hides boundaries without changing their identities.

Transient finds hits by level in four bands (under 200 Hz, 200 to 1200 Hz, 1200 to 5000 Hz and
above), so a new chord or a hat over a held bass counts as well as a louder hit. A band's level
is its peak in each 5 ms; its rise is in decibels over its loudest level of the 50 ms before, so
a low note's cycles (down to 20 Hz) and a decaying tail never count, and a hit's strength is the
sum of its bands' rises. Only the strongest hit within 50 ms is kept, none closer than 50 ms to
Start or End, and none more than 45 dB under the source's loudest level. A boundary sits one
reading before the rise begins. Sensitivity sets the strength needed: 40 dB at 0, 13.75 dB at
0.5 and 5 dB at 1; at 1, a dense 93-second loop gave about as many boundaries as another
sampler's analysis at full sensitivity, three quarters of them within 20 ms of its own.
A single held or decaying note is one slice at any sensitivity.

Notes from Root upward trigger successive slices at original pitch, with tuning
still applied. Notes outside the map are silent. Slice voices offers Mono, Poly
and Through; Through continues from the chosen boundary to End. Slice release
independently chooses Play to end or Release on key up. Fades use the region
actually played. Changing synthesis or playback mode stops existing voices.

In Slice mode double-click empty waveform to add a pinned boundary, double-click
a boundary to delete it, or drag a boundary to move it. The drag previews locally
and dispatches one `device.editSampleSlice` command on release. The context menu
also offers numerical editing, deletion and Regenerate slices. Regeneration clears
pins and suppressions for the current method in one undo step. Pins survive method
changes, sensitivity changes and trimming. Transient suppressions identify source
frames; Beat and Region suppressions identify their method and grid ordinal.
Clicking a slice selects it and auditions it through the shared preview-note commands; its
number and MIDI note are in the context menu; note release is scheduled by the host even when
the panel is hidden. Keyboard playing uses the common typing-keyboard note route.

Ctrl+wheel zooms around the pointer; horizontal wheel, Shift+wheel and middle-button
drag pan. Dragging the waveform away from a boundary zooms (down in, up out) and pans. Show whole sample, Zoom to source region and Zoom to
selected slice change local view state only. Zoom is bounded by a source frame
and the whole file. Replacing a source resets the view and source-specific edits.
Grid and Beat Division are separate. Grid offers straight and triplet subdivisions;
Snap can be disabled, and Alt bypasses it. The ruler shows source bars/beats and
seconds; its lines thin at wide zoom. Source time signature is edited in its context
menu. With unknown BPM the ruler shows seconds and beat snapping is unavailable.

### Source correction storage

Project version 5 introduces the `suhina/sample_edits` extension. Its payload is
`SSE1`, a little-endian u32 pin count, that many normalized f64 positions, and zero
or more nine-byte suppression records (u8 method and signed little-endian i64
identity). Both lists are capped at 4096. Pins must be finite and strictly inside
0..1; methods are 0..2. Preparation rejects unsupported headers, truncated data,
invalid values and oversized lists; commands return errors without changing state.
Pins and suppression records are sorted and deduplicated off the audio thread.
The immutable `SampleEdits` resource is shared through the Device API and exchanged
during graph adoption; its old owner remains on the retiring graph. Note selection
merges the bounded candidate array and sorted pins without allocation or locks.
Held voices retain their bounds when corrections change.

Legacy `suhina/sample_slices` f64 lists and slice_1..slice_16 remain readable. The
first correction in Manual seeds pins from the legacy map. `device.setSampleSlices`
continues to replace manual pins; `device.editSampleSlice` adds/moves/deletes a
boundary or regenerates candidates, shared by UI, CLI and MCP. Source replacement
clears corrections and legacy boundaries in the same undo step as the new source.

Sampler receives block tempo before note events for legacy Beat slicing. Decoded
PCM and transient analysis are shared across graph preparations. Every panel
control uses the SDK; the sampler definition contains no custom drawing primitives.

## Tempo-synchronised sample playback

In Sample mode, the source row keeps all controls on one line. Warp, the warp-mode
selector and Source BPM sit to the right of the voice controls. The three warp
controls appear after a nonempty source is loaded, on the normal and LFO pages;
Envelopes and More replace this source row.

`warp` defaults off and `warp_mode` defaults to Rhythmic. Source BPM is the
original tempo of the file. Loading a source initializes it from metadata or its
filename, then the existing loop-tempo detector. If none is available, Source BPM remains zero; the legacy warp length is initialized
from the current tempo so enabling Warp retains the original speed without asserting
a detected musical grid. Source replacement recalculates its tempo; source and tempo undo together
and persist through save/reopen. With Warp on, speed is project BPM / Source BPM.
For example, a 120 BPM loop plays at its original speed in a 120 BPM project and
half speed in a 60 BPM project, regardless of its length or Start/End trimming.

The editable Source BPM field replaces Fit beats in the same row. Set it to the
loop's actual BPM to correct detection; double-click requests fresh detection.
The tooltip shows both BPM values and their speed ratio. Warp on/off remains
explicit in the button label. Source BPM determines the grid and new Beat slices even with Warp off.
Tempo detection can choose a half/double tempo; the visible original BPM makes
that correction explicit. The SDK Tempo text format shows BPM, or Auto at zero.

`source_bpm` is appended (range 0..999, default 0). Zero retains the historical
`warp_beats` timing, preserving saved revisions 1..11. That legacy parameter is
kept for automation and commands, but hidden in the new panel. A legacy sample
without a stored original-tempo parameter initializes it when Warp is enabled; explicit
saved beat lengths are preserved. Editing legacy `warp_beats` clears Source BPM
back to zero, so old command callers remain effective. Double-clicking Source BPM
explicitly re-detects the tempo regardless of legacy beat-length settings.
Repitch changes pitch together with speed and ignores tuning. Rhythmic, Tonal,
Texture, Smooth and HighQuality all use the shared Signalsmith stretcher today;
they preserve the tuning from MIDI key, transpose, bend, envelopes and LFO. Speed
ratios outside 0.25..4 use Repitch, as arrangement clips do. Grain retains its
existing independent scan and grain tuning; Warp applies only to Sample mode.

The SDK allocates 32 stereo stretch voices and input/output buffers off the audio
thread when loading a source or preparing a loaded instrument. A note resets its
existing stretch state, primes it using bounded source lookahead, and streams in
64-frame blocks without allocation. Source sample rates are converted to the output
rate during lookahead. Forward, reverse, sliced and looping reads respect their
source bounds. Stretched loops use the stretcher's continuous spectral overlap;
Fade in/out apply to the loop edges in direct and stretched playback. Changing project or source tempo re-primes
the voice at its current source position. Changing Warp or its mode keeps active notes, source positions and envelopes.
Switches between Repitch/direct reading and the stretcher use a 5 ms transition
from the voice's last output. The five modes sharing Signalsmith keep the existing
stretcher state unchanged when switched, with sample-identical output. Parameters persist and undo through the
existing device parameter commands. A project saved before Warp existed plays with Warp off, its
default.

LFO rhythmic modes use the same rate conversion as the SDK LfoRate node. Beat,
Triplet and Dotted select a note divisor from 1 to 64; 16th selects a count of
sixteenth notes. Tempo changes apply on the next audio block without resetting
phase. Hz remains the default for older projects.

A single audio file dropped onto an empty MIDI track (header, slot, arrangement lane or empty device area)
adds Sampler and loads its source without creating an audio clip. Tracks with devices
are left intact. A pending file drop is cancelled when another project is active.

Sample loading accepts 1000..384000 Hz and 1..32 channels before decoding.
Decoded PCM is limited to 1 GiB per file (all streams together for Stems).
Grain windows use a 2049-point Hann table prepared off the audio thread, with
linear interpolation and an absolute error below 0.000001 against the cosine formula.

Grain mode shares a budget of 256 active grains across sounding voices, up to 32
per voice. Increasing polyphony reduces the per-voice grain limit; discarded grains
are ended rather than resumed later. Normalization follows the active grain limit.

Grain normalization follows a separate 5 ms ramp when density, grain size or the
voice budget changes; it does not share the master gain ramp.

## Composable sample source (first stage)

An instrument graph may contain `{"kind":"sampler"}` sources. Each owns the
shared polyphonic Sampler kernel and exposes `out`, `left` and `right` audio
outputs, so filtering, gain, mixing and feedback can use ordinary SDK nodes.
Sources receive routed timestamped note events, or global events when unconnected.
Signal inputs expose pitch/cutoff/pan/position modulation. Effect and MIDI-effect graphs reject them.

Compile with `compile_instrument_at`, then bind PCM and configure each source
through `PreparedEffect::sampler_mut(node_id)` and the existing `Device` API.
Do this off the audio thread: replacement may allocate and release resources.
Missing ids return `None`. Unbound sources are silent; reset clears voices while
retaining PCM and controls. Processing uses fixed stack stereo buffers and
preallocated voices, and passes the advancing transport beat to each source.

The stock Sampler definition retains its implementation. Prepared SampleAsset resources,
private voice graphs and smaller playback/envelope/granular nodes are specified in the
[SDK node reference](sdk-nodes.md); they do not imply an equivalent expansion of the stock device.

## SDK graphs in projects

`device.addSd` accepts an optional `sample` audio path for the stable resource key `sample`.
`track.add` forwards the same option when `sd` is given. The host decodes off-thread and
prepares immutable shared PCM before insertion. The graph JSON stays in device state; an SDK
device's sample reference uses the `suhina/sample_reference` extension with the existing
`suhina-sample-v1` reference encoding. Built-in sampler state and parameter ids are unchanged.
Save/open, relative-path rebasing, missing-file warnings and relinking use the same reference
accessors for both device kinds. A prepared graph keeps its source alive when a replacement
is prepared; the host swaps and retires whole graphs off-thread. The project command binds
one source named `sample`; Rust hosts may bind several named sources.
