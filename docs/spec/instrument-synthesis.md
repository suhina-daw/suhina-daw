---
description: The planned comprehensive instrument, its shared Rust synthesis boundary, and the fixed-point FM blocks that establish the reusable DSP contract
read_when: comprehensive instrument, sample to instrument, shared synthesis, FM operator, fixed point, instrument modelling, MPE instrument
type: spec
status: planned
last_verified: 2026-11-19
covers:
  - crates/dsp/**
  - devices/fm-synth/**
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
allocation, envelopes, patch interpretation, parameter IDs and defaults remain with that device.
The extraction preserves its reference renders and its 64-frame control grid; it introduces no
new device, preset, parameter or claim of MPE support.

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

## Validation

The existing FM golden scenarios continue to check pitch, feedback, envelopes, controllers, LFO,
portamento, glissando and stealing. The extraction does not regenerate their expected audio.
Shared-block tests also cover pure/modulated/in-place/feedback operation, add/replace, gain ramps,
phase wrapping, feedback continuation and extreme arguments. A test using only the DSP crate
demonstrates that rendering no longer requires the FM device.

All tests advance fixed numbers of samples or blocks. CPU benchmarks, when needed, are separate
from deterministic correctness gates.

## Not yet

The comprehensive instrument itself, shared voice/modulation engines, reusable envelope families,
SDK synthesis nodes, sample-to-instrument analysis, MPE event/persistence/editor changes, acoustic
model families, factory data, portable instrument packages, expanded UI and export adapters remain
planned. This first DSP boundary does not mark those milestones complete. In particular, a
block-stepped FM envelope is not substituted for the SDK's per-sample ADSR: their semantics differ.

## Sources

- Existing Rust FM implementation and its [behaviour specification](fm-synth.md), under
  [ADR 0034](../decisions/0034-fm-synth-grows-in-place.md).
- MSFA's `fm_op_kernel.cc`, `sin.cc`, `exp2.cc`, `freqlut.cc` and associated coefficient code,
  preserved in the existing Apache-2.0 Rust port. Original copyright and licence notices remain
  with extracted files; [ATTRIBUTION.md](../../ATTRIBUTION.md) identifies the upstream source.
