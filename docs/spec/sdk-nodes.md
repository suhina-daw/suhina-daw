---
description: Every executable SDK node, ports, defaults, formulas, state and real-time limits
read_when: SDK node reference, node ports, DSP primitives, graph authoring, AI device development
type: spec
status: current
last_verified: 2026-11-02
covers:
  - crates/device-sdk/src/sample_resource.rs
  - crates/device-sdk/src/nodes/audio.rs
  - crates/device-sdk/src/nodes/native.rs
  - crates/device-sdk/src/nodes/factory.rs
  - crates/device-sdk/src/nodes/events.rs
  - crates/device-sdk/src/nodes/component.rs
  - crates/device-sdk/src/nodes/templates.rs
  - crates/device-sdk/src/nodes/voice.rs
  - crates/device-sdk/src/nodes/envelopes.rs
  - crates/device-sdk/src/nodes/resources.rs
  - crates/device-sdk/src/nodes/sample.rs
  - crates/device-sdk/src/nodes/playback.rs
  - crates/device-sdk/src/nodes/processor.rs
  - crates/device-sdk/src/nodes/modulation.rs
  - crates/device-sdk/src/nodes/transport.rs
  - crates/device-sdk/src/nodes/random.rs
  - crates/device-sdk/src/nodes/slicing.rs
  - crates/device-sdk/src/nodes/stretch.rs
  - crates/device-sdk/src/nodes/granular.rs
  - crates/device-sdk/src/nodes.rs
  - crates/device-sdk/src/midi.rs
  - crates/device-sdk/src/runtime.rs
  - crates/device-sdk/examples/catalog.rs
  - crates/device-sdk/tests/resource_nodes.rs
  - crates/device-sdk/tests/voice_nodes.rs
  - crates/device-sdk/tests/composed_templates.rs
  - crates/device-sdk/tests/prepared_players.rs
  - crates/device-sdk/examples/sampler_composition.rs
  - crates/device-sdk/tests/graphs.rs
  - crates/device-sdk/tests/golden/feedback.f32
---

# SDK node reference

Audio and control ports carry one stereo pair per sample frame, in f64 internally. Audio and
control signals share this representation: a constant or parameter broadcasts its value to both channels.
Frequency, Q, filter gain, delay time, oscillator rate and balance read the left control channel.
Arithmetic, comparisons, smoothing and envelope controls operate independently per channel.
Signal nodes have `out` (stereo), `left` (left broadcast) and `right` (right broadcast) outputs.
Resource and event nodes expose their typed catalogue outputs instead.
Connections select one of these source ports and one of the inputs below. Each input accepts
at most one connection; use add/mix to sum signals. Required inputs have no default.
Values are finite and limited to the f32 range after each node; non-finite results become zero.
MIDI event ports carry timestamped `NoteEvent` values in MIDI-effect chains and instrument event routes.
An odd final sample in a stereo buffer is cleared. No node allocates or performs I/O while processing.
The runtime schedules strongly connected feedback components frame by frame, committing their
delays after all component nodes have evaluated. Acyclic nodes use bounded 64-frame chunks;
feed-forward delays read and commit per frame within their chunk.

The Rust API is `Node::new(id, NodeKind::Variant { configuration })` and
`Connection::ports(source, source_port, destination, destination_port)`.
`Connection::new(source, destination)` means `out` to `in`, preserving old gain graphs.
Parameter ids are stable; range/unit changes must preserve the meaning of saved values.
Use `parameter` nodes and arithmetic for modulation, and explicit `smooth` nodes for ramps.

## Configuration

- `constant`: finite `value` within f32 range.
- `parameter` and legacy `gain`: `parameter`, an existing declared parameter id.
- `oscillator` and `synced_lfo`: `shape`: sine, triangle, saw_up, saw_down, square, random.
- `noise`: `seed`, a u32 (repeatable; not cryptographic).
- `envelope`: `rms`, a boolean.
- `filter`: `mode`: lowpass, highpass, bandpass, peak, low_shelf, high_shelf.
- `delay`: `max_seconds`, fixed allocation bound greater than zero and at most 60 seconds.
- Other nodes have no configuration. Unconnected optional controls use the defaults below.

Filter formulas are the project's existing DSP implementation. Arbitrary waveshapers can be
composed from arithmetic/transcendental nodes; crossover sections can be composed from filters.
Compression/gating can be composed from envelope, comparisons, arithmetic and smoothing.
Delay history across all nodes is capped at 2,000,000 stereo frames (32 MB at f64 precision).
Validation budgets history at 384 kHz so accepted files fit at every supported sample rate (8..384 kHz).
The combined delay capacity is therefore about 5.2 seconds; reduce `max_seconds` if the validator reports a budget error.
The delay cap is a memory bound, not a CPU deadline guarantee; graph cost depends on nodes used.
Intentional effect delays are not reported as host compensation latency.
FFT, external sample assets and oversampled nonlinear processing are not supported by this version.

## Nodes

The table below comes from `node_reference_markdown()` in the SDK. Regenerate it with
`cargo run -p suhina-device-sdk --example catalog -- --markdown`; its test rejects stale tables.
The JSON catalog uses the same records and includes a minimal configuration example for each node.


<!-- sdk-nodes:begin -->
| JSON kind / Rust variant | Inputs (default, unit) | Behaviour |
|---|---|---|
| `native_effect` / `NativeEffect` | `in` (0, signal), `control1` (0, signal), `control2` (0, signal), `control3` (0, signal), `control4` (0, signal) | Trusted native graph kernel, resolved off-thread by the host. Supports graph wiring and delayed feedback; never loads third-party code. |
| `sampler` / `Sampler` | `pitch` (0, semitones), `cutoff` (20000, Hz), `pan` (0, balance), `position` (0.5, normalized), `events` (0, midi) | Polyphonic sample source using the shared Sampler kernel. Receives timestamped instrument events; bind sample data and parameters by node id before processing. Audio can feed any signal node. |
| `sampler_asset` / `SamplerAsset` | `pitch` (0, semitones), `cutoff` (20000, Hz), `pan` (0, balance), `position` (0.5, normalized), `events` (0, midi) | Polyphonic sample source using the shared Sampler kernel. Receives timestamped instrument events; bind sample data and parameters by node id before processing. Audio can feed any signal node. |
| `key_range` / `KeyRange` | `in` (required, midi) | Filter note keys in an inclusive range. |
| `velocity_range` / `VelocityRange` | `in` (required, midi) | Filter note-on velocities while preserving accepted releases. |
| `sustain` / `Sustain` | `in` (required, midi) | Defer note releases while controller 64 is held. |
| `voice_group` / `VoiceGroup` | `events` (0, midi), `control1` (0, signal), `control2` (0, signal), `control3` (0, signal), `control4` (0, signal) | Bounded private voice graphs with sustain, retrigger, stealing and explicit completion. |
| `component` / `Component` | `in` (0, signal), `control1` (0, signal), `control2` (0, signal), `control3` (0, signal), `events` (0, midi) | Versioned embedded graph with private state and five named public inputs. |
| `component_input` / `ComponentInput` | None | One public component or voice-group input by stable index. |
| `voice_input` / `VoiceInput` | `events` (0, midi) | Per-voice key, gate, event pulses and channel controls. |
| `voice_output` / `VoiceOutput` | `in` (required, signal), `finished` (required, signal) | Voice audio output with explicit completion and tail silence. |
| `midi_controller` / `MidiController` | `events` (0, midi) | Selected normalized MIDI controller. |
| `pitch_bend` / `PitchBend` | `events` (0, midi) | Normalized channel pitch bend. |
| `pressure` / `Pressure` | `events` (0, midi) | Normalized channel pressure. |
| `adsr` / `Adsr` | `gate` (0, signal), `attack` (0.005, seconds), `decay` (0.2, seconds), `sustain` (0.8, signal), `release` (0.2, seconds) | Linear ADSR with timed stages and explicit completion. |
| `ad` / `Ad` | `trigger` (0, signal), `attack` (0.005, seconds), `decay` (0.2, seconds) | Linear AD with timed stages and explicit completion. |
| `sample_asset` / `SampleAsset` | None | Prepared immutable sample binding. |
| `sample_info` / `SampleInfo` | `asset` (required, sample) | Prepared sample metadata selected by field. |
| `sample_region` / `SampleRegion` | `asset` (required, sample), `start` (0, frames), `end` (-1, frames) | Checked half-open frame region, negative end means full sample. |
| `sample_read` / `SampleRead` | `asset` (required, sample), `position` (0, frames), `region` (required, region) | Linear sample read within a checked region. |
| `sample_tempo` / `SampleTempo` | `asset` (required, sample) | Source BPM or duration in beats; unknown tempo is zero. |
| `slice_map` / `SliceMap` | None | Prepared immutable manual slice boundaries. |
| `slice_select` / `SliceSelect` | `map` (required, slice_map), `index` (0, index) | Select a half-open slice region, or empty for invalid indices. |
| `playback_position` / `PlaybackPosition` | `rate` (1, signal), `start` (0, signal), `reset` (0, signal), `gate` (1, signal) | Signed frame position with rising-edge retrigger and gate. |
| `playback_bounds` / `PlaybackBounds` | `position` (0, signal), `start` (0, signal), `end` (1, signal), `gate` (1, signal) | Half-open position bounds; -1 means inactive. |
| `loop_position` / `LoopPosition` | `position` (0, signal), `start` (0, signal), `end` (1, signal), `ping_pong` (0, signal) | Forward or ping-pong position wrapping for signed positions. |
| `playback_rate` / `PlaybackRate` | `key` (60, signal), `root` (60, signal), `fine` (0, signal), `bend` (0, signal), `ratio` (1, signal) | Pitch and sample-rate ratio to signed frame advancement. |
| `region_fade` / `RegionFade` | `position` (0, signal), `length` (1, signal), `fade` (0, signal) | Linear source-region edge gain. |
| `range_map` / `RangeMap` | `value` (0, signal), `in_min` (0, signal), `in_max` (1, signal), `out_min` (0, signal), `out_max` (1, signal) | Affine range conversion with safe zero input span. |
| `curve` / `Curve` | `value` (0, signal), `exponent` (1, signal) | Normalized power curve for controls and velocity. |
| `beat_to_position` / `BeatToPosition` | `beat` (0, signal), `bpm` (120, signal), `sample_rate` (48000, signal) | Source beat position to sample frames. |
| `tempo_ratio` / `TempoRatio` | `source_bpm` (120, signal), `target_bpm` (120, signal) | Target tempo divided by positive source tempo. |
| `key_to_slice` / `KeyToSlice` | `key` (60, signal), `root` (60, signal), `count` (16, signal), `wrap` (0, signal) | Key-to-index mapping with optional wrap. |
| `slice_sequence` / `SliceSequence` | `count` (16, signal), `trigger` (0, signal), `step` (1, signal), `start` (0, signal), `events` (0, midi) | Signed slice sequence advanced by triggers or note-ons. |
| `random_slice` / `RandomSlice` | `count` (16, signal), `trigger` (0, signal), `events` (0, midi) | Seeded random slice index, bounded by count. |
| `random_hold` / `RandomHold` | `trigger` (0, signal), `min` (-1, signal), `max` (1, signal), `events` (0, midi) | Seeded random value held between triggers or note-ons. |
| `random_spread` / `RandomSpread` | `value` (0, signal), `spread` (1, signal), `trigger` (0, signal), `min` (-340282346638528860000000000000000000000, signal), `max` (340282346638528860000000000000000000000, signal) | Seeded held bipolar offset added to a live control. |
| `dc_block` / `DcBlock` | `in` (required, signal) | Stereo DC removal through the shared 5 Hz DSP blocker. |
| `lfo` / `Lfo` | `frequency` (1, Hz), `reset` (0, signal), `phase` (0, cycles), `events` (0, midi) | Free LFO with phase offset, rising reset and event retrigger using shared waveforms. |
| `scan_position` / `ScanPosition` | `rate` (1, signal), `start` (0, signal), `reset` (0, signal), `gate` (1, signal) | Signed frame position with rising-edge retrigger and gate. |
| `position_reset` / `PositionReset` | `position` (0, frames), `trigger` (0, signal), `target` (0, frames), `events` (0, midi) | Event-timed or rising-trigger position replacement. |
| `playback_finished` / `PlaybackFinished` | `position` (0, signal), `start` (0, signal), `end` (1, signal), `gate` (1, signal) | One-frame completion pulse and inactive level when leaving playback bounds. |
| `key_tracking` / `KeyTracking` | `key` (60, key), `root` (60, key), `amount` (1, signal/key), `offset` (0, signal) | Linear key-distance modulation relative to a root. |
| `velocity_curve` / `VelocityCurve` | `value` (0, signal), `exponent` (1, signal) | Normalized power curve for controls and velocity. |
| `jitter` / `Jitter` | `value` (0, signal), `spread` (1, signal), `trigger` (0, signal), `min` (-340282346638528860000000000000000000000, signal), `max` (340282346638528860000000000000000000000, signal) | Seeded held bipolar offset added to a live control. |
| `sample_rate` / `SampleRate` | None | Current graph processing sample rate in Hz. |
| `glide` / `Glide` | `target` (0, signal), `time` (0.1, seconds), `gate` (1, signal), `legato` (1, signal) | Exponential pitch glide; gate rise starts at target in legato mode. |
| `grain_scheduler` / `GrainScheduler` | `density` (8, Hz), `gate` (1, signal), `trigger` (0, signal), `events` (0, midi) | Bounded periodic grain starts with event-timed retrigger. |
| `grain_pool` / `GrainPool` | `trigger` (0, signal), `position` (0, frames), `rate` (1, frames/sample), `duration` (0.12, seconds), `pan` (0, balance) | Prepared private grain graphs started by a rising trigger, bounded lifetime and drop-new overflow. |
| `granular_player` / `GranularPlayer` | `position` (0.5, normalized), `size` (0.12, seconds), `density` (8, Hz), `pitch` (1, ratio), `events` (0, midi) | Granular instrument compiled from the same editable grain and voice graphs. |
| `grain_read` / `GrainRead` | `asset` (required, sample), `position` (0, frames), `rate` (1, frames/sample), `duration` (0.12, seconds), `events` (0, midi) | Triggered one-shot grain read with age and completion. |
| `slice_playback` / `SlicePlayback` | `region` (required, region), `rate` (1, frames/sample), `gate` (0, signal), `events` (0, midi) | Triggered selected-region playback in gate or one-shot mode. |
| `loop_crossfade` / `LoopCrossfade` | `asset` (required, sample), `position` (0, frames), `region` (required, region), `fade` (0, frames) | Periodic two-head loop reader with a linear overlap crossfade. |
| `time_stretch_player` / `TimeStretchPlayer` | `speed` (1, ratio), `pitch` (1, ratio), `start` (0, frames), `end` (-1, frames), `events` (0, midi) | Triggered resident-sample playback through the shared native stretch voice. |
| `grain_window` / `GrainWindow` | `position` (0, signal), `duration` (1, signal) | Hann grain lifetime window, zero outside duration. |
| `audio_input` / `AudioInput` | None | Stereo input of the track; no state. |
| `audio_output` / `AudioOutput` | `in` (required, signal) | Final stereo output; no state. |
| `gain` / `Gain` | `in` (required, signal) | Multiplies by the declared parameter in linear amplitude; legacy gain node. |
| `constant` / `Constant` | None | Broadcasts finite configuration value to both channels. |
| `parameter` / `Parameter` | None | Broadcasts the current declared parameter value; values use their declared unit. |
| `add` / `Add` | `a` (required, signal), `b` (required, signal) | a + b. Per channel, no state. |
| `subtract` / `Subtract` | `a` (required, signal), `b` (required, signal) | a - b. Per channel, no state. |
| `multiply` / `Multiply` | `a` (required, signal), `b` (required, signal) | a * b. Per channel, no state. |
| `divide` / `Divide` | `a` (required, signal), `b` (required, signal) | a / b; zero divisor gives zero. Per channel, no state. |
| `min` / `Min` | `a` (required, signal), `b` (required, signal) | minimum of a and b. Per channel, no state. |
| `max` / `Max` | `a` (required, signal), `b` (required, signal) | maximum of a and b. Per channel, no state. |
| `power` / `Power` | `a` (required, signal), `b` (required, signal) | a raised to b; invalid results give zero. Per channel, no state. |
| `less` / `Less` | `a` (required, signal), `b` (required, signal) | 1 when a < b, otherwise 0. Per channel, no state. |
| `greater` / `Greater` | `a` (required, signal), `b` (required, signal) | 1 when a > b, otherwise 0. Per channel, no state. |
| `abs` / `Abs` | `in` (required, signal) | absolute value. Per channel, no state. |
| `negate` / `Negate` | `in` (required, signal) | negative value. Per channel, no state. |
| `sqrt` / `Sqrt` | `in` (required, signal) | square root of max(in, 0). Per channel, no state. |
| `exp` / `Exp` | `in` (required, signal) | exponential. Per channel, no state. |
| `log` / `Log` | `in` (required, signal) | natural logarithm; nonpositive input gives zero. Per channel, no state. |
| `floor` / `Floor` | `in` (required, signal) | round down. Per channel, no state. |
| `fract` / `Fract` | `in` (required, signal) | in - floor(in). Per channel, no state. |
| `sin` / `Sin` | `in` (required, signal) | sine in radians. Per channel, no state. |
| `cos` / `Cos` | `in` (required, signal) | cosine in radians. Per channel, no state. |
| `tanh` / `Tanh` | `in` (required, signal) | hyperbolic tangent. Per channel, no state. |
| `atan` / `Atan` | `in` (required, signal) | arc tangent in radians. Per channel, no state. |
| `db_to_gain` / `DbToGain` | `in` (required, signal) | 10^(in / 20), input in dB limited to -120..120. Per channel, no state. |
| `gain_to_db` / `GainToDb` | `in` (required, signal) | 20*log10(abs(in)), floor -120 dB. Per channel, no state. |
| `clamp` / `Clamp` | `in` (required, signal), `min` (-1, signal), `max` (1, signal) | Limits input between min and max; reversed bounds are swapped. |
| `mix` / `Mix` | `a` (required, signal), `b` (required, signal), `amount` (0.5, fraction) | Linear a*(1-amount)+b*amount; amount limited to 0..1. |
| `select` / `Select` | `a` (required, signal), `b` (required, signal), `condition` (0, signal) | Selects b when condition > 0, otherwise a; per channel. |
| `stereo` / `Stereo` | `left` (required, signal), `right` (required, signal) | Left channel of left input and right channel of right input become a stereo pair. |
| `mono` / `Mono` | `in` (required, signal) | Broadcasts (left+right)/2 to both channels. |
| `mid_side_encode` / `MidSideEncode` | `in` (required, signal) | Outputs mid=(L+R)/2 in left, side=(L-R)/2 in right. |
| `mid_side_decode` / `MidSideDecode` | `in` (required, signal) | Outputs left=mid+side and right=mid-side. |
| `pan` / `Pan` | `in` (required, signal), `balance` (0, balance) | Balance -1..1: unity at centre, attenuates the opposite channel to zero at the extremes. |
| `oscillator` / `Oscillator` | `frequency` (1, Hz) | Free-running phase in cycles, frequency limited to +/-0.49*sample rate. Starts at phase zero on reset. Waveforms are not band-limited. |
| `synced_lfo` / `SyncedLfo` | `rate` (1, cycles/beat) | Waveform at transport beat * rate; follows song position, including stopped transport. No phase state. |
| `noise` / `Noise` | None | Deterministic xorshift32 white noise -1..1, broadcast to both channels; seed zero becomes one. Reset restores seed. |
| `smooth` / `Smooth` | `in` (required, signal), `time` (0.02, seconds) | One-pole smoothing per channel, time 0..60 seconds; zero is instantaneous. State resets to zero. |
| `envelope` / `Envelope` | `in` (required, signal), `attack` (0.01, seconds), `release` (0.1, seconds) | Per-channel peak follower, or exponential mean-square follower followed by sqrt when rms=true. Attack/release 0..60 seconds. State resets to zero. |
| `sample_hold` / `SampleHold` | `in` (required, signal), `trigger` (required, signal) | Captures input on each nonpositive-to-positive trigger transition per channel. Held value and previous trigger reset to zero. |
| `filter` / `Filter` | `in` (required, signal), `frequency` (1000, Hz), `q` (0.7071067811865476, Q), `gain` (0, dB) | Stereo biquad using suhina-dsp; frequency 1..0.49*sample rate, Q 0.025..100, gain +/-48 dB. Independent channel state resets to zero. Modulation updates coefficients only when controls change. |
| `sample_delay` / `SampleDelay` | `in` (required, signal) | One-frame stereo delay; reads previous input before current input is committed. Breaks feedback cycles. Reset clears history. |
| `delay` / `Delay` | `in` (required, signal), `time` (0, seconds), `mode` (2, 0 repitch, 1 fade, 2 jump) | Fractional stereo delay with independent channel times and linear interpolation, time limited to one frame..max_seconds. Mode 0 slews time with a 20 ms one-pole (repitch), 1 crossfades read heads over 20 ms, 2 jumps to the new time (the default for older graphs). max_seconds must be >0 and <=60. Breaks feedback only through in port; time remains an ordinary dependency. Reset clears history. |
| `tempo` / `Tempo` | None | Broadcasts current transport tempo in beats/minute, limited to 1..1000. |
| `beat` / `Beat` | None | Broadcasts transport beat, advanced each frame while playing. |
| `playing` / `Playing` | None | Broadcasts transport running flag as 0 or 1. |
| `midi_input` / `MidiInput` | None | Incoming MIDI events with their frame offsets. |
| `midi_output` / `MidiOutput` | `in` (required, midi) | Emits processed MIDI events from a MIDI-effect graph. |
| `midi_transpose` / `MidiTranspose` | `in` (required, midi) | Transposes note-on and note-off events by semitones; keys outside 0..127 are dropped. |
| `midi_transpose_parameter` / `MidiTransposeParameter` | `in` (required, midi) | Transposes note events by an integer parameter; keys outside 0..127 are dropped. |
| `midi_key` / `MidiKey` | None | Instrument signal containing the current MIDI key, or 0 when no key is held. |
| `midi_velocity` / `MidiVelocity` | None | Instrument signal containing the current note velocity from 0 to 1. |
| `midi_gate` / `MidiGate` | None | Instrument signal set to 1 while a note is held and 0 otherwise. |
| `lfo_rate` / `LfoRate` | `rate` (0.5, Hz), `time` (2000, ms), `division` (4, divisor or sixteenth count), `mode` (0, 0 Rate, 1 Time, 2 Synced, 3 Triplet, 4 Dotted, 5 16th), `tempo` (120, BPM) | Stateless Hz conversion. Clamps rate to 0.01..20, time to 1..20000 ms, rounded division to 1..64, rounded mode to 0..5 and tempo to 1..1000 BPM. Modes output rate, 1000/time, tempo*division/240, tempo*division/160, tempo*division/360 or tempo/(15*division). Broadcasts mono; no reset state. |
<!-- sdk-nodes:end -->

## LFO rate conversion

`lfo_rate` is a stateless control-rate converter with five mono inputs: `rate` in Hz
(0.01..20), `time` in ms (1..20000), integer `division` (1..64), `mode` (rounded 0..5)
and `tempo` (1..1000 BPM). It broadcasts a frequency in Hz. Modes are Rate, Time, Synced,
Triplet, Dotted and 16th respectively: rate; 1000/time; tempo*division/240;
tempo*division/160; tempo*division/360; tempo/(15*division).
The synchronized divisor describes 1/division of a whole note; the 16th mode counts
sixteenth notes per cycle. Inputs are clamped before conversion. It has no state to reset,
allocations, latency or signal-dependent processing.

## Composable sample source

An instrument graph may contain `{"kind":"sampler"}` sources. Each owns the
shared polyphonic Sampler kernel and exposes `out`, `left` and `right` audio
outputs, so filtering, gain, mixing and feedback can use ordinary SDK nodes.
Sources receive their routed timestamped events, or the graph events when unconnected.
Signal pitch/cutoff/pan/position ports support modulation. Effect and MIDI-effect graphs reject them.

Compile with `compile_instrument_at`, then bind PCM and configure each source
through `PreparedEffect::sampler_mut(node_id)` and the existing `Device` API.
Do this off the audio thread: replacement may allocate and release resources.
Missing ids return `None`. Unbound sources are silent; reset clears voices while
retaining PCM and controls. Processing uses fixed stack stereo buffers and
preallocated voices, and passes the advancing transport beat to each source.

The stock Sampler definition retains its existing DSP and parameter contract. Typed resources,
private voice graphs and smaller playback/envelope/granular nodes compose separate instruments.

Native kernels receive five bounded stereo input spans, in the same order as their declared ports. Unconnected ports carry their declared defaults. Source kernels ignore these spans. Block and feedback scheduling use the same input contract; kernels never retain borrowed input spans.

## Prepared sample resources

The Rust `sample_resource` API describes host-prepared immutable PCM, independently of any
instrument. Construction validates an integer rate of 8000..384000 Hz, 1..32 interleaved
channels and complete frames. PCM stays shared through `Arc`; no decoding, analysis or copying
occurs during reads. The host owns preparation, binding, replacement and retirement off-thread.
`SampleRegion` is a half-open frame interval within one asset; empty regions are valid and silent.
Reads use a region-relative fractional position, linear interpolation and a clamped last frame.
Negative, nonfinite or out-of-region positions are silent. Mono duplicates to stereo; for
multichannel data the first two channels are read. Nonfinite PCM values become zero.
This API supplies serialized resource ports without embedding PCM in `.sd`; it is not a replacement for existing Sampler playback, looping, grain or stretch algorithms.

## Playback and mapping primitives

Positions and lengths are sample frames; region ends are exclusive. PlaybackPosition emits
its current position and then advances by signed rate while gate is positive. A rising reset
or note-on restarts at the start input. LoopPosition maps arbitrary positions into a checked
forward or ping-pong interval; invalid/empty intervals return the start. PlaybackBounds returns
-1 outside a half-open interval or with a closed gate. PlaybackRate converts key/root, cents,
bend semitones and source/output rate ratio to frame advancement (pitch capped to 20 octaves).
RegionFade is a linear gain at both edges with fade length capped to half the region.
RangeMap is an unclamped affine conversion; zero input span returns output minimum.
Curve maps clamped 0..1 values through a bounded positive exponent and also serves velocity
curves. BeatToPosition uses source BPM and sample rate; TempoRatio is target/source BPM.
GrainWindow is a Hann lifetime window, silent outside its positive duration.
All nodes preserve stereo independence, clear private state on reset, and do no I/O or allocation.
These primitives do not yet implement voice graphs, serialized assets or grain allocation.

## Resource-port execution

SampleAsset emits a bounded prepared asset handle; SampleInfo converts it to frame count,
duration, rate or channels. SampleRegion emits checked half-open frame bounds; SampleRead
consumes an asset, relative position and region bounds with linear interpolation. SampleTempo
reads optional source BPM or duration in beats; unknown tempo is zero. SliceMap stores sorted
integer frame boundaries in .sd with a stable sample binding; SliceSelect emits the indexed
half-open region or an empty region for an invalid index. Sample, region and slice-map ports
are distinct from signal ports and cannot feed arithmetic or audio output directly.
`PreparedSamples` binds immutable assets by stable names off-thread. Compilation with resources
fails for missing names or invalid maps. Handles are private to a prepared graph, never paths,
and are not serialized. Replacement builds a new prepared graph; active graphs retain old Arcs.

Resource graphs cap assets at 128, exact frame indices at 16777216 per asset (349 s at 48 kHz,
380 s at 44.1 kHz; the error names the sample's length and the limit at its rate), referenced PCM
at 256 MiB and manual slice boundaries at 4096 per map. These limits are checked off-thread.

## Named signal outputs

In addition to out/left/right aliases, native kernels may expose up to eight named stereo
signal outputs. The interpreter snapshots them at every rendered frame, so trigger/release
pulses, envelope completion and per-voice controls keep exact timing across block boundaries.
`NativeNode::output` reads one auxiliary output from the most recent process call, whose length
is at most 64 frames. Storage is prepared off-thread; reads never allocate or mutate a kernel.

## Timed envelopes

ADSR and AD use linear stages with times in seconds, clamped to 0..60, and a minimum one
sample per stage. ADSR retriggers from zero on a rising gate, holds sustain, and releases
from the level present at the falling gate. AD triggers on a rising signal and completes
its attack/decay without requiring gate hold. Each exposes `finished`, one while idle or
complete and zero while active. Stereo gates have private state. The ADSR step is shared
with the existing sample instrument; this does not change its parameters or render behavior.

## Instrument event routing and voice controls

Instrument graphs may route MIDI Input through KeyRange, VelocityRange, Transpose and Sustain
into instruments or VoiceInput via midi-typed ports. Events retain input order at the same
frame; event edges are acyclic. Unconnected native event inputs retain the legacy global
stream. MIDI value nodes remain legacy global controls. VoiceInput emits key, velocity, gate,
trigger/release one-frame pulses, bend, pressure and selected controller values. Explicit
MidiController, PitchBend and Pressure nodes provide the same channel controls. Sustain defers
note-offs until controller 64 falls below 0.5; note-on zero means note-off. VelocityRange remembers
accepted keys so rejected notes cannot lose their corresponding releases. Fixed transpose reuses
the existing MIDI processor. Event batches are bounded at 256; overflow increments a counter,
never allocates or panics. Reset clears event/filter/controller and pedal state.
VoiceOutput passes audio until its finished control becomes positive, then emits silence and
reports completion to its owner. Event outputs cannot connect to signal/resource ports.

## Private voice graphs and components

VoiceGroup contains an embedded instrument graph and 1..32 private instances, sums their
outputs, and requires one VoiceOutput in that graph. Note identity is MIDI key: a repeated
note-on retriggers that key's voice; otherwise allocation uses a free voice, then the oldest
voice (ties use slot order). A stolen voice fades from its last output over 64 samples.
Controller 64 defers releases; retrigger cancels a deferred release. Controllers, pitch bend
and pressure are inherited by newly allocated voices. Completion frees a slot; no graph is
created or retired on the audio thread. Nested preparation caps depth at four, expanded nodes
at 4096, expanded voice capacity at 128 and total delay storage at the existing delay budget.
A coarse Sampler consumes 32 of that capacity; a TimeStretchPlayer consumes one. Private graphs cannot contain another VoiceGroup.

Component embeds a named, positive-version graph snapshot including its parameters and optional
panel. Its stable public ports are in/control1/control2/control3 signals, optional MIDI events and stereo out;
ComponentInput index 0..3 reads component signal ports; voice groups additionally supply index 4. Events forward with exact
span timing. Each component owns private state. No external component dependencies are looked up,
so a saved graph never silently changes behavior. Effect graphs cannot embed instrument components.
MIDI-effect components use their existing MIDI chain API rather than this audio Component.

## Slice and random controls

KeyToSlice subtracts a root key and either wraps into count or returns -1 outside it.
SliceSequence emits start at the first trigger then advances by signed integer step modulo
count. RandomSlice draws a deterministic index 0..count-1. Invalid/zero counts emit -1.
Sequence and random slice triggers are rising edges or timestamped note-ons. SliceMap also
exposes count as a signal output, distinct from its slice-map handle.
RandomHold uses a seeded LCG and updates on rising trigger or note-on; min/max bound its output.
RandomSpread holds a seeded bipolar offset, updated by rising trigger, and adds spread times
that offset to a live value with min/max clamping. It also supplies the Jitter operation;
use Curve for VelocityCurve and RangeMap/PlaybackRate for KeyTracking. Reset restores seeds.
The LCG and Hann window are shared with the existing sample instrument.

## Granular execution and stretch

GrainScheduler emits one start on gate rise or trigger, then periodic starts using the same
bounded countdown as the sample instrument. Density is capped at one grain per sample.
GrainPool owns 1..128 prepared grain graphs (256 total across an expanded graph, which a
GranularPlayer's voices × grains is checked against when the graph is validated), snapshots
position/rate/duration/pan on each sample where its trigger rises above zero (a trigger held
high starts one grain, not one per sample), and drops new starts when full.
Its private inputs are position, rate, duration seconds, pan and age seconds. Every grain body
has one VoiceOutput; it completes explicitly or at its duration limit. No grain allocation
occurs during processing. GrainRead is a triggered, signed-rate one-shot sample reader with
age/finished/position outputs; GrainWindow supplies its lifetime window. GranularPlayer is an
editable composition of these same nodes, private voices and seeded spread, not a second DSP.
ScanPosition reuses signed PlaybackPosition; Jitter reuses RandomSpread.

SlicePlayback reads a selected region in gate or one-shot mode, with signed speed and explicit
position/finished outputs. LoopCrossfade is a periodic two-head sample reader: with span L and
fade F<=L/2 its period is L-F, primary=start+F+phase and the secondary starts at region start
in the last F frames. A linear blend joins the heads continuously at wrap, including reverse
phase traversal. Zero fade is a normal loop; invalid/empty regions are silent.
TimeStretchPlayer uses the existing preallocated native stretch voice. Its speed is 0.25..4,
pitch is a ratio, start/end are frames; note-on restarts and note-off leaves the source tail
for an external envelope. AllNotesOff stops it. Prefetch reads immutable resident PCM, with no
worker, file I/O or allocation. Short one-frame regions use direct reads. Speed changes restart
the shared stretch state, as in the existing sampler. No coarse/composed audio equivalence is claimed.

PositionReset passes position except on a rising trigger/note-on, when it emits target.
PlaybackFinished emits a pulse on leaving a gated half-open region and exposes an inactive
finished level. ScanPosition shares signed position integration. VelocityCurve shares Curve,
KeyTracking is offset+(key-root)*amount, and Jitter shares RandomSpread. DCBlock uses the shared
5 Hz stereo blocker. Free LFO adds offset and event/rising-edge reset to shared DSP waveforms;
SyncedLFO retains absolute transport phase (relative reset can be authored with held beat offset).
Glide uses the shared smoothing coefficient; in legato mode gate rise starts at target and
subsequent held target changes glide with a seconds time constant. Gate-off holds the last value.

## Parameter bindings

A node may declare bindings from parent parameter ids to native kernel parameter ids. Binding
names are validated off-thread; unknown parameters and duplicate targets fail compilation.
Initial values are installed during preparation and updates use the same set-parameter path
as ordinary graphs. Components and voice/grain pools forward bound updates into their private
graphs; each private graph retains its own smoothing state and range checks. Binding a sampler
control already connected as a signal is rejected. Bindings are data, not executable callbacks.

## Editable sampler template

`sampler_template` returns a data-only private-voice graph using prepared sample reads,
signed position/rate, gated and one-shot bounds, forward/ping-pong loops, ADSR, velocity
tracking, per-voice filtering, gain and pan. Its panel uses standard parameter descriptions.
Parameter ids reuse the corresponding sampler names; the existing built-in sampler is not
replaced. `SamplerAsset` binds the ready-made sampler to an immutable resource by name.
The ready-made Sampler also exposes signal pitch/cutoff/pan/position and generic named parameter
bindings; unconnected controls retain their existing values. No expansion/equivalence claim is
made: the editable template deliberately has its own documented behavior and supported controls.

`PreparedSamples::slice_map` performs manual, transient, seconds-grid or beat-grid preparation
off-thread. It reuses the sample instrument's source analysis, rejects oversized grids and
stores only checked frame boundaries in SliceMap. Beat grids require explicit or source BPM.

The JSON node catalogue includes `output_types`, and `NodeInfo::output_type` exposes the same
contract in Rust. Event and resource connections must have matching types; numeric audio and
control signals share stereo signal wires with units specified at their destination.
RandomSpread/Jitter sample a new offset at every positive trigger sample, including consecutive
grain starts; RandomHold retains rising-edge semantics.

The `sampler_composition` example writes editable loop/reverse, slice, granular and tempo/stretch
.sd definitions and renders each from a deterministic resident source. Its WAV files demonstrate
execution without a file decoder in the SDK; a host supplies that same immutable source under the
stable `sample` binding. Moving the graphs does not change that binding; absent bindings are errors.

KeyRange, VelocityRange and Sustain also run in MIDI-effect chains, preserving their order relative
to transposes. A caller output too small for a pedal release receives AllNotesOff in its last slot
and the chain clears held state, so overflow cannot leave a newly sustained note stuck.
