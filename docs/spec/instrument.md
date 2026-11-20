---
description: Experimental Instrument modal core, local single-source import and reusable native runtime
read_when: Instrument, modal synthesis, sample-to-instrument, comprehensive instrument
type: spec
status: planned
last_verified: 2026-11-20
covers:
  - devices/instrument/**
  - crates/dsp/src/modal.rs
  - crates/dsp/src/bar.rs
---

# Instrument

Instrument is an experimental native instrument. Its public Rust runtime is shared by stock
and future export adapters. The stock panel uses the device SDK. It has no network operation,
training service, proprietary library dependency or background fitting requirement.
The [compact and expanded editor](instrument-editor.md) presents the same parameters on
Play, Model and Sample pages; navigation is local view state, not an audio control.

## Sound and controls

Source selects Modal, Sample or Hybrid. Body selects what Modal plays: **Struck** (the struck
modal body of revision 1, described in the rest of this section) or one of the two experimental
bar bodies of revision 3, **Marimba bar** and **Vibraphone bar** ([Bar bodies](#bar-bodies-revision-3)).
Struck is an original mathematical
prototype, not a calibrated piano, string or orchestral instrument. Sixteen damped modes per
voice describe a struck resonant body. Sixteen voices bound its work and memory. A key sets the
fundamental in equal temperament; velocity sets excitation amplitude. Brightness shapes the
mode amplitudes, Structure stretches their frequency ratios, Strike position changes the
excitation pattern and Decay sets the fundamental's time to lose 60 dB. High modes decay faster.
Quality selects 8 or 16 modes. Modes at or above 0.45 times the sample rate are omitted, not
folded into the audible band.

The original remains revision 1. [Accepted modal profiles](instrument-modal-profile.md) use
revision 2: a bounded stored list of ratios, magnitudes and T60 values replaces the heuristic
modal data. Default model controls preserve that profile; Decay, Brightness, Structure and
creative Strike colour transform it on the next strike. This runtime accepts data and does not
fit, calibrate or infer a physical instrument from a recording.
Release damps a released key; the sustain pedal holds it. Gain and the Hybrid sample blend are
smoothed. Changing the body controls or Source affects the next strike, so a held body does
not jump to another body's state. Modal mode starts no sample voices; Sample mode starts no
modal voices. Hybrid starts and advances both layers even at Blend 0% or 100%, so moving Blend
during a held note reveals its current source position and decay rather than a stale onset.
Released hidden layers still advance to silence. Switching to a pure Source retires the other
layer after its fade; switching back requires another note.
The ordinary pitch wheel bends both sources by two semitones in either direction. Retuning a
modal voice retains phase; a mode pushed past the anti-alias margin is stopped. Sustain delays
the sample gate's note-off as well as the modal release. One-shot samples still run to their end.
The shared sample runtime keeps pending pedal releases per voice. Repeating a key while the
pedal is down leaves the old released voices sustained; lifting the pedal releases those voices
while the newest held note continues. Retiring an inaudible sample source preserves the current
pedal and pitch-wheel state for the next note.

Sample uses the SDK's existing sample runtime. Root, key range, source trim, loop bounds and
Gate / One shot / Loop playback describe a single-source key map. Hybrid mixes this source
with the modal body; it does not infer a physical model from the source. Missing sample files
produce silence in Sample mode, and leave the modal body audible in Hybrid mode.

### Revision 1 reference

For mode number `n` starting at one, Structure `s`, Brightness `b` and Strike position `p`:

- Frequency is `fundamental * n * sqrt((1 + s*n*n)/(1 + s))`.
- Initial signed amplitude is `sin(pi*n*p) / n^(2 - 1.5*b)`.
- Decay time is `body_decay / (1 + 0.12*(n - 1))`.
- Usable amplitudes are divided by `max(1, sum(abs(amplitude)))`. The voice starts with a
  2 ms linear fade and a `0.25 * velocity` level. A stolen voice's last output fades for 2 ms
  while the new strike fades in; there is no unbounded extra tail voice.

The independent analytical render anchors in
`devices/instrument/tests/data/modal-v1-reference.txt` pin revision 1 at its default controls,
48 kHz, key 60 and velocity 0.8. They are self-authored CC0 data, not recorded reference audio.
Acoustic quality is not established by matching this mathematical reference.

### Bar bodies (revision 3)

Marimba bar and Vibraphone bar are experimental physical models of one struck bar per key: a
free-free bar's modes, excited by a mallet whose contact time depends on how hard and how fast it
strikes. They are not calibrated against recordings and claim no likeness to a particular
instrument. Each bar has a tube resonator under it; the vibraphone motor, the damper pedal and
dead strokes are not modelled yet. Struck, Sample and Hybrid behave as described above whatever
these controls say.

**Tuning.** A key sets the fundamental `f1` in equal temperament (A4 = 440 Hz). Mode `n` sounds at
`f1 * ratio_n(key)`. `U_n` are the uniform free-free bar's ratios `(beta_n / beta_1)^2`, where
`beta_n` is the `n`-th positive root of `cos(beta) * cosh(beta) = 1` (1, 2.7565, 5.4039, 8.9330 …).
A tuned mode blends the profile's target `T_n` with `U_n` geometrically (in cents) by a register
weight `w_n(key)` that is 1 at and below the first key, 0 at and above the second and linear in
between: `ratio_n = T_n^w_n * U_n^(1 - w_n)`. Modes above the last tuned mode keep the uniform
bar's spacing from it: `ratio_n = ratio_last * U_n / U_last`.

| Profile | Tuned targets | Register weights | Last tuned mode |
|---|---|---|---|
| Marimba bar | `T_2 = 4`, `T_3 = 10` | `w_2`: keys 84 → 96; `w_3`: keys 60 → 79 | 3 |
| Vibraphone bar | `T_2 = 4` | `w_2 = 1` at every key | 2 |

**Decay.** Each mode's time to lose 60 dB depends on its frequency `f` by the profile's material
law, scaled by Body decay `D` relative to its 2-second default:
`T60(f) = T_ref * (1 + (261.63 / f_c)^q) / (1 + (f / f_c)^q) * D / 2`.
Marimba bar (wood) uses `T_ref = 1.2 s`, `f_c = 300 Hz`, `q = 1.4`; Vibraphone bar (aluminium)
uses `T_ref = 8 s`, `f_c = 1500 Hz`, `q = 1`. `T_ref` is the fundamental's time at middle C.

**Bar and strike position.** The bar's mass is `M(key) = M_ref * 2^(-(key - 60) / 24)`, with
`M_ref` 0.4 kg (Marimba bar) or 0.5 kg (Vibraphone bar); every mode's modal mass is `M / 4`.
Mode `n`'s shape is the uniform free-free shape
`phi_n(x) = (cosh(beta_n x) + cos(beta_n x) - s_n (sinh(beta_n x) + sin(beta_n x))) / 2` with
`s_n = (cosh(beta_n) - cos(beta_n)) / (sinh(beta_n) - sin(beta_n))`, which is 1 or -1 at the ends;
the second mode has a node at the centre. Strike position `p` measures from the centre to the
end: the contact point is `x = 0.5 * (0.98 - p) / 0.96` of the bar's length, so the lowest
setting strikes the centre and the highest the end.

**Mallet.** Velocity sets the impact speed `v = 4 m/s * velocity`. The mallet has mass
`m = 0.025 kg` and a contact force `F = K * d^2.5 * (1 + lambda * d')` while it compresses
the bar's surface by `d > 0` (never negative, so it does not stick). Mallet hardness `h` sets the
contact time the mallet would have against a rigid surface at 1 m/s,
`tau_ref = 2.5 ms * 0.1^h` (2.5 ms soft to 0.25 ms hard), from which
`K = 3.5 * m / (2 * d_ref^3.5)` with `d_ref = tau_ref / (2 * I)` and
`I = sqrt(pi) * Gamma(1 + 1/3.5) / Gamma(1/2 + 1/3.5)`; and the dissipation
`lambda = 0.05 + 0.45 * (1 - h)` s/m. Harder and faster strokes therefore touch the bar for a
shorter time and excite higher modes: brightness follows velocity and hardness without a filter.

Mallet model selects how a stroke reaches the modes. Both drive each mode, as a damped
oscillator, with `phi_n(x) * F(t) / (M / 4)`, integrated exactly over each sample for the force
the model gives.

- **Contact** (the default) simulates the mallet against the bar. The mallet starts at the bar's
  surface at the stroke's sample, moving at `v`; the compression is the mallet's position less
  the bar's displacement at `x`, the sum of `phi_n(x)` times each mode's displacement. Each
  sample is divided into `S` equal steps, `S = clamp(ceil(4 * omega_c / sample_rate), 1, 32)`,
  where `omega_c` is the angular frequency of the contact's highest stiffness against the
  reduced mass of mallet and bar (see Pulse), at the largest compression a rigid surface would
  allow. Within a step the force is held constant, each mode advances exactly, and the mallet
  moves by semi-implicit Euler. A stroke on a ringing bar meets it where it is and as it moves.
  The contact ends when the mallet leaves the surface, or after 20 ms; a mallet that has not
  reached a receding surface after 10 ms gives up.
- **Pulse** applies a half-sine force `F(t) = (pi * J / (2 * tau)) * sin(pi * t / tau)` for
  `0 <= t <= tau`. The bar is treated as a point mass `m_b = 1 / sum(phi_n(x)^2 / (M / 4))` over
  its sounding modes, with the reduced mass `mu = m * m_b / (m + m_b)`. The impulse is
  `J = (1 + e) * mu * v` with restitution `e = max(0, 1 - 2 * lambda * v / 3)`, and the duration
  `tau = 2 * I * d_max / v` with `d_max = (3.5 * mu * v^2 / (2 * K))^(1 / 3.5)`. Strokes add
  linearly to what the bar already does.

A new stroke on a bar whose previous contact or pulse has not finished replaces it; the rest of
the earlier stroke is dropped.

**Sound.** The output is each mode's velocity weighted by Brightness `b` as a tilt,
`(f_n / f1)^(b - 0.5)`, summed and scaled by `0.12 * sqrt(M(key) / M_ref)`, which keeps the level of
light, high bars comparable with heavy, low ones. Quality selects 8 or 16 modes; modes at or above
0.45 times the sample rate are left out of the bar, its point mass and its output. Structure has
no effect on bar bodies.

**Resonator.** The tube under each bar is one damped resonance tuned to the bar's fundamental
`f1`, with quality factor 40 (decay rate `pi * f1 / 40`). The first mode's velocity drives it, and
its own velocity joins the output with the first mode's weight: once it has built up, it adds
`2 * r` times the first mode's output, in phase with it, where `r` is the Resonator control (0 to
100 %, default 100 %). A low tube builds up and dies away more slowly than a high one. The tube
draws its energy from the bar: the first mode's decay rate grows by `r * ln(1000) / T_rad`, with
`T_rad` 1.2 s for Marimba bar and 4 s for Vibraphone bar, so the fundamental sounds stronger and
shorter. The tube follows the pitch wheel with its bar; at 0 % there is no tube. A tube above the
anti-alias margin is left out with its mode.

**Coefficient changes.** When a re-strike, damping or the pitch wheel changes a ringing bar's
frequencies, decays or weights, the step this would make in its output fades out linearly over
2 ms instead of sounding as a click.

**One body per bar.** A key is one bar. A note on a key whose bar still sounds strikes that same
bar again, whether the key is held, released or damping, so a roll keeps one voice and a stroke
excites the bar's current motion. Otherwise the bar takes a silent voice, or else the voice
struck longest ago; a stolen voice's output fades for 2 ms, as in Struck. A re-strike applies the
current controls to the ringing bar, keeping each mode's motion: a mode that leaves the
sounding set stops. A key whose sounding voice belongs to another body takes a new voice. This
policy is the device's own until the shared voice lifecycle work (#1632) can express a
per-key body; it adds no allocator to the shared crates.

**Release and pedal.** Marimba bar rings freely: note-off and the sustain pedal do not damp it.
Vibraphone bar is damped on note-off, unless the sustain pedal holds it, and when the pedal lifts
for released keys: damping caps every mode's time to lose 60 dB at Release, keeping its phase; a
new stroke restores the profile's decay. All notes off damps both bodies the same way. Pitch
wheel retunes a ringing bar as it retunes Struck.

A bar whose state stops being finite falls silent instead of producing invalid output.

## Import and project state

`device.loadSample` accepts Instrument and loads an ordinary local audio file. The command is
one undoable project edit and selects Sample, so the next note plays the imported source.
Like manually selecting Source, import overrides an enabled Source automation lane while keeping
its points. Undo restores the previous source reference, Source value and lane state together;
redo restores the complete import.
Root defaults to MIDI 60. The explicit [local analysis workflow](instrument-analysis.md) can
propose a root from the loaded audio; the person checks it and accepts it in one undo step,
including the normal root-automation override. Completion itself changes nothing. Automatic
segmentation and physical-model fitting are not part of that pitch proposal. Source bytes use the
existing decoded-audio and sample-reference lifecycle: project save rebases the path, keeps
an absolute fallback, and the existing collect/missing-file/relink workflow applies.

All sound controls are stored as stable native parameter IDs. `model_revision` is metadata in
the existing native parameter state, hidden from the stock panel; a newly added Instrument stores
3. Revision 3 adds Body, Mallet hardness, Mallet model and Resonator, and its Struck body is revision 1's
renderer unchanged. A stored 1 is therefore read as revision 3: a revision-1 project has no Body
value, so it opens on Struck with the new controls at their defaults and renders exactly as
before. A project without a stored revision is read as revision 1. Revisions 2 and 4 render an
accepted profile or a [fitted model set](instrument-modal-fitting.md) and ignore Body, Mallet
hardness, Mallet model and Resonator; adopting a profile stores 2, adopting a set stores 4.
The device never rewrites the stored value, so an older build that knows only revisions 1 and 2
silences a revision-3 project's modal body, but plays Struck for a revision-1 project in which a
bar body was chosen later. Unknown
revisions retain their value and silence the modal body rather than running another revision.
This temporary metadata parameter is not a sound control; export adapters must preserve it in
state without exposing it as an automatable control. Future revisions must preserve the rendering
contract of revisions 1 to 4 or provide a migration and retained renderer. The sample reference is `suhina-sample-v1`,
independent of the model revision. Opening a project decodes its local source and restores
controls; it never refits. Portable plug-in assets are not implemented by this source-reference
contract, which currently belongs to the stock project's model layer.
Revision-2 profile bytes have their own bounded `instrument_modal_profile` extension; changing
or moving a sample reference cannot overwrite them. Graph identity includes their content, and
project save/reopen restores the accepted modes without a recording, fitting run or cache.
Missing, invalid or incompatible profiles silence the modal layer, while sample playback stays
independent. The canonical format and atomic adoption command are in the profile specification.

## Bounds and quality gates

The modal core has at most 256 active modes and no dynamic storage. A bar's contact runs at most
32 steps per sample for at most 30 ms per stroke. The sampler is configured
for at most 16 voices; Hybrid can therefore run 16 modal and 16 sample voices together. Sample
rendering reuses the shared runtime and its documented source limits. Preparation, decoding and graph
replacement happen away from the audio callback. Rendering, note events, parameter changes
and reset perform no allocation, deallocation, locking, logging or I/O.
The final mix saturates only at the finite `f32` limits, including when positive output gain
would otherwise overflow an extreme imported float sample. This is not an audio limiter.

Deterministic tests cover tuning/decay, bounded invalid inputs, note release/pedal/reset,
identical output across block partitions, sample rendering and project reopen. For the bar
bodies they also cover mode placement, brightness rising with velocity and hardness, strike
position against the mode shapes, repeated strokes on one bar never gaining more energy than the
strokes brought, low sample rates with the hardest mallet, and render anchors of both profiles
and both mallet models. A release-mode
benchmark reports fixed rendered-frame workloads rather than making CI pass on wall-clock
thresholds. The initial development reference machine is a Ryzen 5 5600X; this is a measurement
context, not a claim of a measured real-time polyphony budget. The provisional acceptance target
is at most 20% of one core on average at 48 kHz / 64 frames with all 16 voices, including the
dense-retrigger workload, and a 99th-percentile block shorter than one block deadline. Modal
voice state is bounded below 4 KiB per voice (a voice holds one body at a time: 16 struck modes,
a 16-mode bar with its contact, or revision 4's 64 fitted resonators); the whole inline runtime target is 256 KiB, excluding
shared source PCM and the SDK's prepared stretch buffers. Report those exclusions with the
measurement rather than calling them free. `cargo run --release -p suhina-device-instrument
--example benchmark` measures Modal, Sample and Hybrid at 64/256 frames, with pitch movement
every block and a separate 16-retriggers-per-block workload, and the bar bodies with rolls on
sixteen bars. Listening and calibrated acoustic
quality remain separate release gates.

## Not yet

The complete plan remains in #1579. No family F01-F18 is declared complete by this prototype.
Calibrated piano, bowed/plucked strings, winds, brass, reeds, voices, percussion and the other
planned families still need their own model, independently licensed references, articulation
coverage and listening validation. Folder/multisample import builds on the shared zone work
in #739; model candidates, automatic fitting, provenance and portable preset bundles remain
future work. The broader resonator request #394 also includes excitation and coupling models
that this first struck body does not supply. The bar bodies still lack the vibraphone motor
and damper pedal, dead strokes, mallet noise, resonances of the tube above its first, and
calibration (#1609). MPE (#921) and CLAP/VST3 exports (#931) are parallel
implementation workstreams that must use this same core.

## Sources

- Self-authored modal model: each mode is a complex oscillator multiplied by
  `exp(-ln(1000)/(sample_rate*decay_seconds))` per sample; a finite normalized sum is its output.
- Existing shared sample contract: [Sampler](sampler.md).
- Native device architecture: [ADR 0004](../decisions/0004-built-in-devices.md).
- Bar modes, mode shapes and mallet contact: [F11 research](../research/instrument-f11-mallets.md)
  and its sources (Fletcher & Rossing; Chaigne & Doutaut 1997; Hunt & Crossley 1975; Avanzini &
  Rocchesso 2001). The contact time against a rigid surface is the closed-form duration of a mass
  striking a spring `K * d^alpha`.
