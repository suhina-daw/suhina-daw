---
description: Canonical bounded modal profiles, native Instrument rendering and atomic project adoption
read_when: modal profile, fitted modes, Instrument model state, modal adoption, F11
type: spec
status: current
last_verified: 2026-11-20
covers:
  - crates/dsp/src/modal.rs
  - crates/dsp/src/modal/profile.rs
  - crates/dsp/src/modal/profile_tests.rs
  - devices/instrument/src/lib.rs
  - devices/instrument/src/profile_tests.rs
  - devices/instrument/src/panel.rs
  - devices/instrument/src/editor.rs
  - crates/device-api/src/lib.rs
  - crates/model/src/instrument_profile.rs
  - crates/engine/src/slot_factory.rs
  - crates/engine/src/chain.rs
  - crates/engine/src/chain_tests.rs
  - crates/session/src/instrument_profile.rs
  - crates/session/src/commands/instrument_profile.rs
  - crates/session/tests/instrument_profile.rs
  - crates/device-registry/tests/reset.rs
---

# Modal profiles

This is the native runtime/state prerequisite for fitting in #1624 and the F11 family in
#1609, delivered by #1644. It renders supplied modal data; it does not estimate it, identify a
physical object or certify acoustic realism. The original heuristic model remains revision 1.
Accepted profiles use renderer revision 2. Phase, attack residuals, room separation, coherent
multi-recording surfaces and morphing remain separate work.

## Shared data contract

`suhina_dsp::modal::profile` owns immutable `ModalProfile` and `ProfileMode`. Analysis, native
rendering and packages use this one codec. It depends on no project, GUI, file or audio runtime.
`ModalProfile::new(reference_key, reference_hz, modes)` validates before constructing fixed
storage. Getters expose the reference key/Hz and the active mode slice. Each mode has a frequency
`ratio`, nonnegative `magnitude` and `decay_seconds` (time to decay by 60 dB).

Bounds are part of schema 1: reference key 0..127, reference Hz 8..20000, one to sixteen modes,
strictly increasing ratios in 1/256..256, magnitudes 0..1 with at least one positive magnitude,
and T60 0.001..60 seconds. Every number is finite. Frequencies beyond the renderer's existing
anti-alias margin are silent at that sample rate; they are not folded into the audible band.

The reference key is a mapping anchor, not a claim about a detected fundamental. At that key,
mode frequency is exactly `reference_hz * ratio`. A fitter can choose the nominal frequency of
the user's root key and encode measured resonances as ratios; inharmonic material need not
have a unique fundamental. Other keys transpose this frequency by equal-tempered semitones.
Pitch wheel offsets remain relative to these measured strike frequencies.

`encode()` returns a fixed 408-byte array; `decode(&[u8])` accepts exactly 408 bytes. Header:
eight-byte magic `SHMODAL` followed by NUL, little-endian u16 schema (1), u16 renderer (2),
u8 reference key, u8 active count, two zero reserved bytes, and little-endian f64 reference Hz.
Sixteen 24-byte records follow, each ratio/magnitude/T60 as little-endian f64. Unused records
are all zero. Unknown revisions, noncanonical reserved/padding bytes and invalid values fail
before a profile is returned. Prepared profiles and their codec never allocate.

## Rendering and controls

Revision 1 preserves its existing formula and render reference. Revision 2 requires a valid
profile: absence or malformed data never silently selects revision 1. Existing sample playback
remains independent, so a Hybrid with an unavailable modal profile can still play its sample.
The existing 16-voice/16-mode ceiling, source fades, pedal behavior and shared ModalBank remain.
Loading a profile happens off the audio thread and clears previous signal state. Graph swaps
retire old device ownership outside the callback through the existing engine mechanism.

At default model controls, revision 2 reproduces its supplied ratios, magnitudes and T60.
Decay multiplies T60 by `body_decay / 2`. Brightness applies the spectral tilt
`ratio.powf(1.5 * (brightness - 0.5))`. Structure applies the ratio between the existing
dispersion factor at the current value and at the neutral value 0.002. Strike multiplies
magnitudes by a bounded modal-index excitation weight relative to its neutral value 0.25:
`(0.25 + 0.75*abs(sin(pi*n*strike))) / (0.25 + 0.75*abs(sin(pi*n*0.25)))` for n=1..16.
This is a creative excitation-colour control, not a measured impact position. Magnitudes are
normalized before ModalBank excitation; T60 and frequencies retain the bank's safety limits.
Velocity excites a new strike and never rewrites a previous strike. No new automation IDs appear.

## Project state and commands

Profile bytes live in the device's `suhina` extension `instrument_modal_profile`. The existing
sample-reference envelope in `Device.state` is unchanged; loading, relinking or collecting a
sample cannot overwrite the profile. The model layer stores bounded opaque bytes without a
dependency on DSP. Native construction receives them through an off-thread capability, and
graph identity includes their exact content as well as the current sample identity.

`instrument.adoptModalProfile` is one ordinary project EDIT command. It takes track and full
device path, a bounded profile object (`referenceKey`, `referenceHz`, `modes` containing
`ratio`, `magnitude`, `decaySeconds`) and optional `source` (`Modal`, default, or `Hybrid`).
It validates the complete profile and Instrument target before editing. Adoption writes the
profile, model revision 2, Source, the full 16-mode quality budget and the four neutral model controls above, with the same
manual automation override contract as `device.setParam`. Lane points remain intact. One undo
restores the old profile, revision, controls, source and lane enabled/override state together.

The compact and expanded SDK captions follow the stored revision. Revision 2 labels Strike as
creative colour rather than physical position. A revision label identifies the requested model;
it does not certify valid/correct calibration data. Unsupported or missing modal data is silent.

The session's typed `apply_profile` helper performs those same mutations inside an existing
EDIT command; future fit acceptance uses it after validating its job and device identity,
without nested command execution or another undo step. It reads no file and grants no new
filesystem authority. Saved projects reopen their accepted model without fitting, a source
recording or disposable cache. Portable preset/asset packages remain their own coordinated
delivery; the existing parameter-only preset path does not establish portability.

## Evidence and limits

Deterministic tests cover canonical round trips and hostile bytes, reference tuning, neutral
and changed model controls, revision-1 render preservation, block partitions, allocation-free
rendering, missing/unknown models, graph identity, atomic automation/undo, and saved-project
render equality. Fixtures are self-authored. CI performs compilation and tests under the
maintainer's CI-only instruction. No new calibrated-family or musician acceptance is claimed.
