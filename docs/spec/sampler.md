---
description: Suhina's sample and granular instrument, sample loading and SDK panel
read_when: sampler, granular instrument, sample recording, sample drop
type: spec
status: draft
last_verified: 2026-10-17
covers:
  - devices/sampler/**
  - crates/device-sdk/src/sampler.rs
  - crates/device-sdk/src/sampler_warp.rs
  - crates/ui/src/detail/device_canvas.rs
  - crates/ui/src/detail/detail_view.rs
---

# Sampler

Sampler is a single-source polyphonic instrument. Simple reads a sample at a rate
determined by MIDI key (root 60), transpose and cents. Grain schedules bounded,
Hann-windowed grains from the same source. Both use the same amplitude envelope,
filter and output controls. All voices and grains are allocated before processing.
Sample decoding, replacement and disposal occur outside the audio callback.

The SDK panel uses a 900 by 168 logical-pixel canvas (200 pixels including the
host header and padding). Sample and Grain buttons at the upper left select the
synthesis mode. A single full-width waveform stays visible at identical bounds
in both modes and while editing envelopes. Its compact marker rows stay inside the waveform; Input and Record sit adjacent
in the top row immediately before LFO and Envelopes. The filename appears once in the device
header, appended to the device name only when that name does not already contain
the source stem. There is no filename caption inside the waveform or grain cloud.

LFO and Envelopes at the upper right are mutually exclusive tabs. Clicking the
active tab again returns to normal controls. Both keep the full-width waveform
visible. Envelopes shows simultaneous amplitude/filter/pitch envelope editors;
each ADSR stage has its own draggable point, with decay also adjusting sustain. The
pitch peak adjusts attack and signed amount; its envelope remains attack/decay.
LFO has its own large waveform preview and shared rate selectable as Hz, Time (milliseconds), Beat, Triplet, Dotted or 16th, with
independent pitch, filter, volume and pan depths. Dragging the preview adjusts
rate horizontally; destination fields edit their own depth. The preview shows
the oscillator independently of destination depths. Switching tabs does not change sound parameters.

Below the mode-specific source/granular row, the normal controls have their own
backdrop and five named mini-sections: Tuning (transpose, cents, root and note
range), Filter (type, 12/24 slope buttons and compact cutoff/resonance dials), Voices (mode, polyphony/glide
and velocity range), Output (gain, pan and velocity amount), and Playback (mode,
Reverse and Retrigger). Section surfaces, headings and borders use the active
host theme and do not intercept pointer or keyboard input. The Grain row retains
position, scan, size, density, jitter, spread and Freeze without a cloud preview.

Definition revision 12 preserves existing sound parameters and migrations from
revisions 1 through 11. New LFO destinations and source fades default to zero;
LFO shape defaults to Sine. `panel_view` persists Controls (0), Envelopes (1) or LFO (2); new devices
default to Controls. Legacy `grain_env` and `envelope_tab` remain readable but no
longer select an editor. Existing embedded definitions update explicitly through
device.updateSd. Native wrappers without the appended page selector still prepare
with its default; audio parameter order and ranges remain strictly validated.
Numeric values drag vertically, Shift gives precision, and double-click resets.
All edits use the shared device parameter command, persistence and undo.

Start/end and loop markers are fractions of the source. Empty or crossed ranges
produce silence. Forward loops crossfade at the boundary; ping-pong reflects the
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
Glide is shown in Mono and controls the existing pitch transition time. Pitch env
adds a signed semitone offset with its own attack and decay back to zero, in both
Simple and Grain modes. These parameters use the standard device parameter command
and project persistence. Defaults preserve existing projects.

The waveform displays every active voice read head from the audio engine's
atomic meter telemetry, including live MIDI. A visible waveform requests frames
every 16 ms even while idle, so a new note does not wait for mouse activity. Grain mode shows the scan position.
Record and the input selector occupy the top row outside the waveform; Stop
is visible only while sample recording is active.

## Panel and voice controls

Sample/Grain buttons sit at the upper left. Envelopes sits at the far right.
The waveform and recording controls remain visible when the editor is open.
Amplitude includes velocity-to-volume; filter includes envelope, velocity and
key amounts. Tuning, note range, gain and pan are in the corresponding named mini-sections.
Glide replaces the polyphony count in Mono mode.
Playback uses Gate/One shot/Loop buttons plus Reverse and Retrigger. Retrigger is
on by default: a reused voice starts from the sample region's beginning. With it
off, an active reused voice keeps its position and envelopes while changing key
and velocity; unused voices still start normally. Replaced voices interpolate
from their last output to the new voice over 5 ms, without extra audible voices.

Filter buttons show Off, low-pass, high-pass, band-pass and notch response icons.
12/24 selects one or two cascaded biquad sections (12/24 dB/oct for low/high-pass).
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
The historical `crossfade` parameter retains loop crossfading and is labelled
Loop fade ms. Loop start, Loop end and Loop fade fields appear only for Sample
with Playback = Loop; Start, End, Fade in ms and Fade out ms remain available for
other Sample playback modes. Hidden controls cannot intercept input.

The waveform occupies y=24..82. The source/grain row at y=86 and five normal
sections at y=108..168 retain gaps and internal padding within the 200-pixel device.
Cutoff and Res have twice the ordinary drag sensitivity (200 logical pixels per
range). Shift remains available. All parameter inputs, including choice menus,
segmented buttons, switches and waveform markers, reset to their parameter's
saved definition default on double-click; recording actions remain actions.

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

Playback Slices replaces loop fields with Sensitivity and Slice by (Transient,
Beat, Region, Manual). Notes from Root upward trigger successive slices at the
original pitch, with transpose/cents still applied; notes outside the slice map
are silent. Slices play to their boundary, ignoring note-off. Reverse is supported.
Transient detects rising amplitude in 5 ms windows; Sensitivity lowers its threshold.
Beat uses quarter-note subdivisions at the transport tempo (120 BPM before playback);
Sensitivity selects 4, 2, 1, 1/2, 1/4 or 1/8 beats. Region selects 1..32 equal
regions with Sensitivity. Manual uses a dynamic list of normalized source positions, with at most 4096 points. Double-click empty waveform to add a point; double-click a line to
remove it. Drag a line horizontally to move it; its hover cursor is horizontal
resize. A drag is one undo step, even when crossing another line. Start/End crop
the map without deleting points outside it. All points persist in the device's
`suhina/sample_slices` extension as little-endian f64 values; an absent extension
reads legacy slice_1..slice_16 parameters. Empty lists are explicit. The shared
`device.setSampleSlices` command validates, sorts and deduplicates positions; its
4096-entry input budget applies both to stored maps and command input.
The runtime installs immutable lists off the audio thread and selects note ranges
by binary search without allocating. MIDI keys address successive slices from Root;
all points remain editable even beyond the 128 available MIDI keys.
Automatic maps hold at most 128 slices. Grain synthesis retains its existing behavior.
Definition revision 8 appends slice controls and migrates revisions 1..7.

## Waveform navigation

Areas outside the displayed S/E markers are gray, including during loop editing.
The waveform's default cursor is a magnifying glass. Drag empty waveform upward
to zoom in and downward to zoom out, anchored at the press position. Horizontal
drag pans the view; both axes work in the same gesture. Zoom stops at one source
frame and the full sample; panning stays within the source. Navigation is local
view state and never edits the sample or adds undo history. Replacing the source
resets the view. Wave peaks are recomputed for the visible source range so zoom
retains detail. Source/loop markers, manual lines and voice heads share the same
coordinate transform and clipping. Manual lines take precedence over navigation.

Sampler receives block tempo before note events, so a Beat slice triggered at frame zero uses the current tempo. Decoded PCM and transient analysis are shared across graph preparations. Every panel control is a typed SDK widget, including the LFO WaveformPreview; the sampler definition contains no custom drawing primitives.

## Tempo-synchronised sample playback

In Sample mode, the source row keeps all controls on one line. Warp, the warp-mode
selector and Source BPM sit to the right of Fade out; loop and slice controls use the
space to their left. The three warp controls appear after a nonempty source is loaded
and are hidden in Grain, Envelopes and LFO views.

`warp` defaults off and `warp_mode` defaults to Rhythmic. Source BPM is the
original tempo of the file. Loading a source initializes it from metadata or its
filename, then the existing loop-tempo detector. If none is available, the current
project/playback tempo is stored, so enabling Warp at that tempo keeps the original
speed. Source replacement recalculates its tempo; source and tempo undo together
and persist through save/reopen. With Warp on, speed is project BPM / Source BPM.
For example, a 120 BPM loop plays at its original speed in a 120 BPM project and
half speed in a 60 BPM project, regardless of its length or Start/End trimming.

The editable Source BPM field replaces Fit beats in the same row. Set it to the
loop's actual BPM to correct detection; double-click requests fresh detection.
The tooltip shows both BPM values and their speed ratio. Warp on/off remains
explicit in the button label. Source BPM has no timing effect with Warp off.
Tempo detection can choose a half/double tempo; the visible original BPM makes
that correction explicit. The SDK Tempo text format shows BPM, or Auto at zero.

`source_bpm` is appended (range 0..999, default 0). Zero retains the historical
`warp_beats` timing, preserving saved revisions 1..11. That legacy parameter is
kept for automation and commands, but hidden in the new panel. A legacy sample
without an explicit original tempo initializes it when Warp is enabled; explicit
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
Loop fade remains the direct-read loop crossfade. Changing project or source tempo re-primes
the voice at its current source position. Changing Warp or its mode keeps active notes, source positions and envelopes.
Switches between Repitch/direct reading and the stretcher use a 5 ms transition
from the voice's last output. The five modes sharing Signalsmith keep the existing
stretcher state unchanged when switched, with sample-identical output. Parameters persist and undo through the
existing device parameter commands. Revision 8 wrappers remain playable with Warp
off; updating their definition adds the new controls.

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
