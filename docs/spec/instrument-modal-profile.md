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
  - crates/dsp/src/modal/model.rs
  - crates/dsp/src/modal/model_tests.rs
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
Accepted profiles use renderer revision 2. Fitted recordings and sets of them, with phases,
two-part decays, beating partners, attack residuals and parameter-space fusion across
recordings, use revision 4 ([Fitted model sets](#fitted-model-sets-revision-4)). Room
separation, morphing controls and calibrated families remain separate work.

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

## Fitted model sets (revision 4)

Schema 1 profiles describe one decaying sine per mode. A fitted recording needs more to sound
like its source, and several recordings of an instrument need to become one playable
instrument. `suhina_dsp::modal::model` owns `ModelSet`, a bounded set of fitted recordings that
the native Instrument renders as **revision 4**. Revision 2 profiles keep their contract.

**One recording** (`ModelRecording`) has a key (the mapping anchor; its equal-tempered frequency
is the reference), a velocity in (0, 1] it stands for, a `source` 0..15 (which instrument it was
recorded from, for morphing: ten pianos are ten sources), an absolute `level` (the recording's
amplitude in source full-scale units that the normalized magnitudes multiply, 0 < level <= 16),
one to 64 modes and an optional attack. A mode has:

- `ratio` (1/256..256, strictly increasing), `magnitude` (0..1; the magnitudes of a recording sum
  to at most 1), `decay_seconds` (T60 of the slow part, 0.001..60) and `phase` (radians at the
  strike, -pi..pi);
- an optional fast part: `fast_decay_seconds` (0 for none, else 0.001..decay_seconds) and
  `fast_share` (0..1 of the magnitude), so a mode can fall quickly first and ring on;
- an optional beating partner: `beat_ratio` (1 for none, else within 0.97..1.03 of the ratio, not
  1) and `beat_share` (0..0.5 of the slow part), a second sine close in frequency with the slow
  T60, for strings detuned against each other.

The **attack** is mono PCM at `attack_rate` (1000..48000 Hz, or no attack with length 0), at most
0.25 s, in the same normalized units as the magnitudes. It is what the modes do not explain at
the start of the recording (strike noise, hammer and body knock), faded out over its second half.

**Set and codec.** A set holds one to 32 recordings, sorted by key, then velocity, then source,
with no two at the same key, velocity and source. `encode()` writes magic `SHMSET2` and NUL,
little-endian u16 schema 2, u16 revision 4, u8 count and three zero bytes; then per recording:
u8 key, u8 source, two zero bytes, f32 velocity, f64 level, u8 mode count, three zero bytes, u32 attack rate, u32 attack
length, the modes as eight little-endian f64 each (ratio, magnitude, decay, fast decay, fast
share, beat ratio, beat share, phase) and the attack as little-endian f32. `decode` accepts only
canonical bytes within these bounds (at most 32 recordings, 64 modes and 12000 attack samples
each, every number finite) and fails before returning anything otherwise. The model layer's
opaque state limit is 1 MiB, which holds 32 fitted recordings with their 22050 Hz attacks.
Decoding precomputes, off the audio thread, which mode of every other recording each mode
matches (the nearest ratio within 3 %, `|ln(r2 / r1)| <= 0.03`, each mode used at most once) and
each recording's **decay law**: the least-squares slope of `ln T60` against `ln frequency` over
its non-sustained modes, weighted by magnitude and limited to -2..0 (0 with fewer than two such
modes). Higher partials and higher notes of real instruments ring shorter; the law says how much
for this recording.

**A played note** (`ModelSet::voice`, allocation-free, at note-on) fuses the recordings in
parameter space, never by mixing their audio:

1. Each recording gets weight `1 / (d^2 + 1e-6)` with distance
   `d = |key - recording key| / 12 + 2 * |velocity - recording velocity|`, times its source's
   morph factor; all recordings take part and the weights are normalized. Recordings of one note
   at one velocity from several sources (ten pianos playing one note) therefore average with
   equal weight at the neutral morph; between recorded keys, the nearer recordings dominate.
   **Source morph** `m` (0..1, neutral 0.5) tilts the weights across the sources present, in
   source-number order: source rank `k` of `S` gets `x = k / (S - 1)` and factor
   `(1 - x) * (1 - m) + x * m` (all equal at 0.5; with two sources, `m = 0.3` is 70 % the first
   and 30 % the second). With one source the factor is 1.
2. The heaviest recording is the anchor. For each of its modes, the matched modes of the other
   participating recordings are combined with their weights: ratio, both T60s and the beat ratio
   as weighted geometric means, shares as weighted arithmetic means over the matching
   recordings, and magnitude as a weighted mean in which a recording without a match counts
   zero, so a resonance only one source has fades in proportion. Before combining, each
   recording's T60s follow its decay law to the played key: times `2^((key - recording key) / 12)`
   raised to the law's slope (sustained 60 s modes stay). Level is a weighted geometric mean.
   Phases are the anchor's.
3. Frequencies are the played key's equal-tempered frequency times the fused ratio. Each mode
   becomes up to three sines with its phase: the slow part
   `magnitude * (1 - fast_share) * (1 - beat_share)` at T60 `decay_seconds`, the fast part
   `magnitude * fast_share` at `fast_decay_seconds`, and the partner
   `magnitude * (1 - fast_share) * beat_share` at `ratio * beat_ratio` with the slow T60. At most
   128 sines sound per voice (64 with Modes per voice 8); the weakest by `amplitude * sqrt(T60)`
   are left out.
4. The anchor's attack plays from the strike, read at
   `attack_rate / sample_rate * 2^((key - recording key) / 12)` with linear interpolation and the
   same pitch wheel offset as the modes.

The voice level is `velocity * level`, so at the recording's own key a Modal note is as loud as
the Sample note; magnitudes are normalized by their sum when it exceeds 1. The model controls
transform each fused mode exactly as revision 2 transforms a profile mode (Decay scales both
T60s, Brightness tilts by ratio, Structure disperses ratios, Strike colour weights by mode
index); the attack is not changed by them.

**Hammer and felt.** A harder strike on a felt hammer or mallet is brighter and knocks more, not
only louder. Recordings at several velocities supply that directly: the nearest layer's attack
plays and the modes interpolate. **Velocity to brightness** `v` (0..1, default 0.35) adds the rest
relative to the anchor recording's velocity `v0`: the tilt exponent gains `1.5 * v * (velocity - v0)`
and the attack is multiplied by `(velocity / v0)^v`, so a softer note than the recording is darker
with less knock. At `v = 0` velocity changes only the level.

**Envelope.** A revision-4 voice ramps its gate in over 0.25 ms instead of 2 ms, since the attack
reproduces the recorded onset. The amplitude envelope controls, shared with the Sample layer, then
shape it: Attack (linear rise, 0..10 s), Decay (linear fall to Sustain, 0..10 s) and Sustain
(0..1) multiply the voice on top of the fitted decays; at Attack 0 and Sustain 1 the recording
plays as fitted. **Release** is a damper, not a fade: on note-off (or pedal-up for a released key)
every sine's T60 is capped at Release, keeping its phase and frequency, as felt dampers stop
strings; the attack runs to its end. Pedal, voice stealing, pitch wheel and source fades behave as
for revision 2. A missing, malformed or unsupported set is silent.

**Piano pedals and dampers** (approximations of the mechanism, not fitted from recordings, which
rarely contain them):

- **Damper noise** (0..1, default 0.2): when a damper lands (note-off, or pedal-up for a released
  key), the voice adds a 40 ms burst of low-passed noise (one-pole low-pass at 600 Hz, linear
  fade) at `damper noise * 0.05` of the voice level at that moment. Deterministic per voice
  (a fixed linear-congruential sequence), allocation-free.
- **Pedal resonance** (0..1, default 0.3): a note struck while the sustain pedal is down also
  sets undamped strings ringing at its partials. Each mode gains a fourth sine at its frequency
  with `pedal resonance * 0.15` of its slow part, twice its slow T60 and a quarter-cycle phase
  offset; the sine budget still keeps the strongest 64. A coupling model between strings is not
  attempted.
- **Soft pedal** (controller 67 at or above half, or the Play page's **Soft pedal** switch, the
  `soft_pedal` parameter, for playing without a pedal or automating it): fewer strings and a
  softer felt region are struck, so a note is quieter (level times 0.7), darker (tilt exponent -0.45) and has no beating
  partner. It applies to notes struck while it is down.

The fitted attack is the recorded hammer; these settings shape what a single recording cannot
show. Sympathetic coupling between particular strings, the damper's recorded sound and
half-pedalling remain open in #1624.

**Budget.** A revision-4 voice holds 128 resonators, an attack read position and its envelope
in place of the 16 resonators of revisions 1 and 2 or a bar body: under 8 KiB per voice, under
128 KiB for 16 voices, within the runtime's 256 KiB inline target. The set itself (at most 1 MiB)
is shared and prepared off the audio thread. Note-on fuses at most 32 recordings of 64 modes
without allocating.

**Commands.** `instrument.acceptFit` adopts a set with the fitted recording as its only member
(revision 4, Source Modal, full modes, neutral model controls, Attack 0, Sustain 1, Source morph
0.5, one undo step). `instrument.addFit` adds a ready fit's recording to the device's existing
revision-4 set as one undo step, with an optional `velocity` (default 0.8) and `source` (default
0), or `newSource` instead of `source`: one more than the highest source in the set, so another
instrument's recordings morph against the ones already there (a set that already uses source 15
refuses it). A recording at the same key, velocity and source as an existing one is an error, as
are a thirty-third recording or a device without a revision-4 set. The Sample page offers both
**Add to model** (source 0) and **Add as new source**. It selects Source Modal and leaves
the model controls alone. The proposal invalidation rules of fitting apply.

## Evidence and limits

Deterministic tests cover canonical round trips and hostile bytes, reference tuning, neutral
and changed model controls, revision-1 render preservation, block partitions, allocation-free
rendering, missing/unknown models, graph identity, atomic automation/undo, and saved-project
render equality. Fixtures are self-authored. CI performs compilation and tests under the
maintainer's CI-only instruction. No new calibrated-family or musician acceptance is claimed.

## Sources

Public research that informs revision 4's choices; no implementation is copied from any of them.

- Gabriel Weinreich, *Coupled piano strings*, J. Acoust. Soc. Am. 62(6), 1474-1484 (1977): unison
  strings slightly out of tune produce a prompt sound followed by a longer aftersound, and beating.
  Revision 4 approximates this with a fast and a slow part per mode and a beating partner, fitted
  from the recording rather than from a coupling model.
- Anatoli Stulov, *Hysteretic model of the grand piano hammer felt*, J. Acoust. Soc. Am. 97(4),
  2577-2585 (1995), and Donald E. Hall, *Piano string excitation* (series), J. Acoust. Soc. Am.
  (1986-1987): felt stiffens under harder blows, so higher partials grow faster than the
  fundamental with hammer velocity. Velocity to brightness and the attack gain are a bounded
  stand-in when only one velocity was recorded; recordings at several velocities supply it.
- Balázs Bank, Stefano Zambon, Federico Fontana, *A modal-based real-time piano synthesizer*,
  IEEE Trans. Audio, Speech, Lang. Process. 18(4), 809-821 (2010): a piano rendered as banks of
  decaying modes, with dampers as added losses and sympathetic resonance of undamped strings
  under the pedal. Revision 4's damper caps T60 the same way; its pedal resonance is a simpler
  per-partial approximation without string coupling.
- Neville H. Fletcher and Thomas D. Rossing, *The Physics of Musical Instruments*, 2nd ed.
  (Springer, 1998): partials and higher notes of struck and plucked strings decay faster, the
  basis of each recording's decay law.
- Anders Askenfelt and Erik V. Jansson, *From touch to string vibrations*, J. Acoust. Soc. Am.
  88(1) (1990) and later parts: the hammer and action noise precede and accompany the string
  tone, which the fitted attack keeps from the recording.
