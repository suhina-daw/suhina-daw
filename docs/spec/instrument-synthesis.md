---
description: The planned comprehensive instrument, its shared Rust synthesis boundary, fixed-point FM blocks and reusable time and linear envelopes
read_when: comprehensive instrument, sample to instrument, shared synthesis, FM operator, fixed point, instrument modelling, MPE instrument
type: spec
status: planned
last_verified: 2026-11-20
covers:
  - crates/dsp/**
  - devices/fm-synth/**
  - crates/device-sdk/src/sample_envelope_tests.rs
---

# Instrument synthesis

The comprehensive instrument is planned in [#1579](https://github.com/suhina-daw/suhina-daw/issues/1579).
It combines realistic factory instruments and creative editing, instruments made from user audio,
and MPE performance, using one Rust core for a native Suhina device and VST3/CLAP exports. The native
device has an SDK panel and an expanded editor. The complete requirements and implementation
milestones remain in the issue; this specification defines the contracts being implemented.

## Shared building blocks

General synthesis blocks belong in `crates/dsp`; devices own their instrument-specific topology,
parameters, presets and gestures. A device never depends on another device to obtain its DSP.
This follows [ADR 0004](../decisions/0004-built-in-devices.md) and the shared synthesis proposal
[#1481](https://github.com/suhina-daw/suhina-daw/issues/1481).

The first shared blocks are the FM instrument's fixed-point sine operators and lookup tables.
The existing FM instrument remains their first production consumer. Its algorithm routing, voice
allocation, legacy rates/levels envelopes, patch interpretation, parameter IDs and defaults remain
with that device. Reusable time and linear envelopes have the separate contracts below.
The extraction preserves its reference renders and its 64-frame control grid; it introduces no
new device, preset, parameter or claim of MPE support.

## Integration boundaries for later phases

Per-note expression follows [#921](https://github.com/suhina-daw/suhina-daw/issues/921) and its
event/model decision [#924](https://github.com/suhina-daw/suhina-daw/issues/924), rather than a
second instrument-specific event system. The current `Device::event` contract delivers an event
between processing spans: the host splits at its frame (ADR 0004). Any change to that rule belongs
in the shared decision first. [ADR 0025](../decisions/0025-object-identity.md) keeps note IDs out of
saved files and places future expression curves on their notes; persistent curve ownership and
runtime voice identity are different contracts.

The existing MIDI parser drops polyphonic key pressure and combines All Sound Off with All Notes
Off. [Recording](recording.md) currently stores neither the channel nor pedal events as such.
These gaps must be resolved through the shared MPE work before this instrument claims complete
expression recording and replay.

Exports and reusable panel drawing build on
[#931](https://github.com/suhina-daw/suhina-daw/issues/931). Framework, FFI/`unsafe` boundary,
parameter identity, state and editor choices need that wrapper's decision before implementation;
this extraction selects no framework and adds no FFI exception.

The content-size target concerns installed libraries, not the executable's embedded starter pack.
[ADR 0036](../decisions/0036-library-and-shipped-content.md) caps that pack at 16 MiB and leaves
larger downloads to separate content-delivery work. Calibration recordings require an external
dataset/provenance plan. [ADR 0016](../decisions/0016-scope.md) permits MPE when requested; any
broader scope that conflicts with its exclusions needs an explicit decision before implementation.

## Fixed-point FM contract

The shared `dsp::fm` module has no device, project, UI or file-system dependency. Its operator
kernels process exactly 64 frames. The caller owns output, modulation and feedback buffers; no
kernel allocates, frees, locks, logs or performs I/O.

- Phase is an integer whose low 24 bits represent one cycle. Frequency is a phase increment per
  sample; phase addition wraps. The caller advances its persistent phase once per processed block.
- Gains and sine output are linear Q24: `1 << 24` represents unity. Logarithmic lookup inputs use
  Q24 octaves. Preserve the original integer interpolation and rounding, including the gain step
  `(gain2 - gain1 + 32) >> 6` evaluated with wrapping subtraction/addition.
- An unmodulated operator renders a sine. A phase-modulated operator reads an independent buffer,
  or reads each output slot before replacing/adding to that same slot for in-place modulation.
- A feedback operator retains its previous two output samples between blocks. The feedback shift
  is increased by one using saturating arithmetic and clamped to 0 through 31. Disabling feedback
  is the caller's policy: it calls the unmodulated kernel instead.
- Kernels either replace output or add with wrapping integer arithmetic. The arithmetic helpers
  used by the FM instrument's extended-waveform kernel are shared too.

Lookup preparation runs outside audio processing. Rates below 1 kHz or non-finite rates use 48 kHz,
matching the DSP crate's safe-rate convention; supported instrument rates retain their previous
table values. Table interpolation accepts every integer argument without overflowing or panicking.
Prepared lookup arrays are immutable; legacy envelope, LFO and portamento coefficients retain
their existing representation for the FM instrument.

## Shared envelope contracts

`dsp::envelope` contains two distinct models. Both keep their state inline, accept bounded
numeric input and perform no allocation, locking or I/O in their lifecycle or stepping methods.
Device parameter IDs, graph node ports, project formats and preset defaults do not change.

### Time envelope

The FM time envelope moves into DSP with its initial/peak/sustain/end levels, slopes, time by
velocity/key and Off/Loop/Beat/Sync/Trigger modes. It advances at most one stage per call and
discards excess stage time. Starting uses the configured initial level; repeats and release use
the current level. A Sync call advances its repeat position once, even after a large transport
jump. Stopped transport uses Beat timing. Sustain changes glide using unscaled elapsed seconds
and the existing 15 ms time constant. Amplitude attack interpolates linear gain and returns dB;
decay/release interpolate dB. A silent amplitude end becomes -80 dB. Other uses interpolate
their own units. Legacy rates-and-levels envelopes remain device-specific.

At the public boundary, finite stage/loop times are clamped to 0.0005..60 seconds, repeats to
0.001..1024 beats, slopes and time-by-velocity to -1..1, and time-by-key to 0..1; non-finite
values use the corresponding setting defaults. Linear levels are limited to +/-1e6 (non-finite
becomes zero), amplitude levels to -160..120 dB (non-finite becomes -80 dB). Velocity is 0..1
(fallback zero), key 0..127 (fallback 60). Elapsed seconds are 0..60 (fallback zero), time scale
0..1000 (fallback one), tempo 1..1000 BPM (invalid or non-positive becomes 120), and beat position
+/-1e12 (fallback zero, preserving negative beats). Accumulated stage and loop time is capped at
1e9 seconds. These bounds include every existing supported FM parameter value.

The single-step f32 control glide is shared with the FM filter controls in `dsp::smooth`.
It keeps the original arithmetic and 1e-4 snap threshold for normal input. Its share is clamped
to 0..1 (non-finite becomes zero); invalid current values become zero and invalid targets retain
the current value. Interpolation between finite extremes stays finite. It is distinct from
the existing two-stage `Glide` and attack/release follower.

### Linear envelope

The SDK AD/ADSR nodes and sampler amplitude envelope use the same per-sample f64
`LinearEnvelope`, with private lifecycle state and named attack/decay/sustain/release settings.
It starts idle; `start` resets to zero, and each `next` advances at most one stage. Attack and
decay have a minimum duration of one sample. Sustain holds its reached level, including when
the sustain setting subsequently changes. Each `release` captures the current level, including
repeated release requests during release. Completion occurs when release reaches zero. AD's
caller ends at sustain instead. Times are 0..60 seconds and sustain 0..1; non-finite settings
become zero. Positive finite rates are retained, others use 48 kHz.

The sampler retains its separate filter and pitch envelope algorithms and existing note reuse
policy. A release request captures both amplitude and filter levels every time, and amplitude
completion stops the voice before the rest of that frame is processed. Extraction must preserve
this coupling, including repeated note-off and All Notes Off.

## Validation

The existing FM golden scenarios continue to check pitch, feedback, envelopes, controllers, LFO,
portamento, glissando and stealing. The extraction does not regenerate their expected audio.
Shared-block tests also cover pure/modulated/in-place/feedback operation, add/replace, gain ramps,
phase wrapping, feedback continuation and extreme arguments. A test using only the DSP crate
demonstrates that rendering no longer requires the FM device.

Frozen pre-extraction time-envelope trajectories cover all five modes in linear and dB units,
slopes, scaled timing, sustain automation, stopped transport and release. Consumer renders and
linear-envelope tests pin gate/retrigger, repeated release and sampler filter coupling. Public
DSP tests cover non-finite and extreme numeric input and recovery with ordinary settings.

All tests advance fixed numbers of samples or blocks. CPU benchmarks, when needed, are separate
from deterministic correctness gates.

## Not yet

The comprehensive instrument itself, shared voice/modulation engines, additional envelope families,
new SDK synthesis nodes, sample-to-instrument analysis, MPE event/persistence/editor changes, acoustic
model families, factory data, portable instrument packages, expanded UI and export adapters remain
planned. This first DSP boundary does not mark those milestones complete. In particular, a
block-stepped FM envelope is not substituted for the SDK's per-sample ADSR: their semantics differ.

## Sources

- Existing Rust FM implementation and its [behaviour specification](fm-synth.md), under
  [ADR 0034](../decisions/0034-fm-synth-grows-in-place.md).
- Existing SDK [timed envelope nodes](sdk-nodes.md#timed-envelopes) and sampler implementation.
- MSFA's `fm_op_kernel.cc`, `sin.cc`, `exp2.cc`, `freqlut.cc` and associated coefficient code,
  preserved in the existing Apache-2.0 Rust port. Original copyright and licence notices remain
  with extracted files; [ATTRIBUTION.md](../../ATTRIBUTION.md) identifies the upstream source.
