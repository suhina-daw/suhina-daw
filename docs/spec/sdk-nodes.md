---
description: Every executable SDK node, ports, defaults, formulas, state and real-time limits
read_when: SDK node reference, node ports, DSP primitives, graph authoring, AI device development
type: spec
status: current
last_verified: 2026-10-15
covers:
  - crates/device-sdk/src/nodes.rs
  - crates/device-sdk/src/midi.rs
  - crates/device-sdk/src/runtime.rs
  - crates/device-sdk/examples/catalog.rs
  - crates/device-sdk/tests/graphs.rs
  - crates/device-sdk/tests/golden/feedback.f32
---

# SDK node reference

Audio and control ports carry one stereo pair per sample frame, in f64 internally. Audio and
control signals share this representation: a constant or parameter broadcasts its value to both channels.
Frequency, Q, filter gain, delay time, oscillator rate and balance read the left control channel.
Arithmetic, comparisons, smoothing and envelope controls operate independently per channel.
Every node has `out` (stereo), `left` (left broadcast) and `right` (right broadcast) outputs.
Connections select one of these source ports and one of the inputs below. Each input accepts
at most one connection; use add/mix to sum signals. Required inputs have no default.
Values are finite and limited to the f32 range after each node; non-finite results become zero.
MIDI event ports carry timestamped `NoteEvent` values and are only used in MIDI-effect graphs.
An odd final sample in a stereo buffer is cleared. No node allocates or performs I/O while processing.

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
| `native_device` / `NativeDevice` | `in` (0, signal) | Whole-device native wrapper. Effects wire audio_input -> native_device -> audio_output; instruments wire native_device -> audio_output. Requires the host's registered factory via prepare_native_at; no arbitrary code or feedback. Parameters preserve the native contract. Processing/reset/events use Device's real-time-safe API. |
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
| `midi_input` / `MidiInput` | None | Incoming MIDI events for a MIDI-effect graph. |
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

Native Sampler wrappers from revisions 1-8 may retain their original parameter
prefix without the appended LFO destinations, shape, source fades and warp controls; the oldest may also omit
`panel_view`. New depths and fades default to zero; shape defaults to Sine. All declared audio parameter contracts
still match exactly; other omissions or changed native parameters remain errors.
