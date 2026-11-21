---
description: Results of the F11 bar prototype (slice 2) - experiments E1-E7 on Instrument's Marimba bar and Vibraphone bar bodies with tube resonators, mallet noise, torsion and literature values, comparing the nonlinear Contact mallet with the linear Pulse mallet, and the first listening and trace of the test build
read_when: F11 bar prototype results, marimba, vibraphone, mallet contact, Hunt-Crossley, half-sine pulse, tube resonator, torsional modes, mallet noise, E1 E2 E3 E4 E5 E6 E7, modal bar benchmark, Instrument bar bodies
type: plan
status: draft
last_verified: 2026-11-20
covers:
  - devices/instrument/tests/bar_experiments.rs
  - devices/instrument/examples/benchmark.rs
---

# Instrument F11: bar prototype results

Slice 2 of the [F11 research](instrument-f11-mallets.md) › 7 built two experimental bar bodies
into Instrument, Marimba bar and Vibraphone bar, with two mallet models: **Contact** (candidate A,
a Hunt–Crossley contact simulated against the bar's motion) and **Pulse** (candidate B, a linear
half-sine force whose duration follows velocity and hardness), a tube resonator under each bar
(candidate D), mallet noise shaped by the contact force, torsional modes for the marimba, a stereo
spread by key and graded mallets. Their behavior and the sources of their values are specified in
[instrument.md](../spec/instrument.md) › Bar bodies; this report records what the experiments of
the research's section 5 measured. It makes no calibration claim: no reference recordings were
used (#1621), and no formal listening session took place (E8, #1631).

The deterministic experiments are tests in `devices/instrument/tests/bar_experiments.rs` that
assert each pass criterion and print their measurements:

```sh
cargo test -p suhina-device-instrument --test bar_experiments -- --nocapture
```

All renders are offline at fixed frame counts, key 60 (middle C) and default controls unless
stated (Resonator 100 %, mallet noise on, Stereo width 50 %). Measurements come from the rendered
output, not from the model's internal values. E1–E5 and E7 were measured at commit `3de33c61`;
E6 at `7baadffc`, before the torsional modes, the stereo spread and the literature values, which
add at most two modes and a pan per voice.

## Summary

| # | Contact | Pulse |
|---|---|---|
| E1 tuned modes | Pass (worst 0.14 cents) | Pass (worst 0.15 cents) |
| E2 brightness | Pass on every cell | **Fails** for velocity at soft and medium hardness, and for hardness at one velocity |
| E3 repeated strokes | Pass: differs from superposition by 0.4–11 % RMS; energy ≤ 0.92 of the strokes' | Superposes (differences ~1e-7, as designed); energy ≤ 0.48 |
| E4 strike position | Pass (mode 2 ≥ 139 dB weaker at the center) | Pass (same) |
| E5 resonator | The tube makes the fundamental 3–13 dB stronger and shorter; matching references needs #1621 | Same tube |
| E6 budget | Pass: worst case 11 % of a core, p99 16 % of the deadline (64 frames) | Pass: 11 % / 16 % |
| E7 stability | Pass at 8 and 22.05 kHz | Pass |

Contact is the default mallet model because it is the only candidate that passes every
experiment that could be measured, at a cost close to Pulse's. Pulse stays selectable for
comparison.

## E1: tuned modes

Single strokes at velocity 0.8 on every fourth key of each profile's range (Marimba bar keys
36–96, Vibraphone bar keys 53–89); each tuned mode's peak was found in a Hann-windowed 0.5 s DFT
by a 1-cent search with parabolic refinement and compared with the profile's ratio at that key.
A mode counts as tuned at a key when it is the fundamental or a target whose register weight is
above zero; modes within 5 % of the anti-alias margin, or ringing less than 20 ms (a high
marimba mode under the rosewood loss law), have no resolvable peak and were skipped.

| Profile | Mallet | Tuned modes measured | Worst error |
|---|---|---|---|
| Marimba bar | Contact | 40 | −0.141 cents (key 68, mode 3) |
| Marimba bar | Pulse | 40 | −0.152 cents (key 68, mode 3) |
| Vibraphone bar | Contact | 20 | 0.017 cents |
| Vibraphone bar | Pulse | 20 | 0.016 cents |

The contact, the tube (tuned to the fundamental) and the torsional modes do not shift the free
modes measurably. E1 verifies the renderer against the profile, not the profile against real
instruments: the ratios are the literature's targets (1 : 4 : 10 and 1 : 4).

## E2: brightness against velocity and hardness

Magnitude-weighted spectral centroid of the first 50 ms (Hann window, every DFT bin), velocities
0.1–1.0 and hardness 0–1. Centroids in Hz, velocity increasing to the right:

| Profile, mallet | Hardness 0 | Hardness 0.5 | Hardness 1 |
|---|---|---|---|
| Marimba bar, Contact | 277 → 437 | 385 → 497 | 468 → 539 |
| Marimba bar, Pulse | **289, 283**, 333 → 411 | 348 → 478 | 448 → 529 |
| Vibraphone bar, Contact | 282 → 669 | 430 → 1126 | 693 → 1917 |
| Vibraphone bar, Pulse | **322, 311**, 400, 476, **528, 510, 509** | 419, 528, **693, 659**, 727 → 881 | 727 → 1715 |

Contact rises monotonically in both velocity and hardness on all 35 cells of both profiles. Pulse
is not monotonic in velocity at 1 of 5 hardnesses (Marimba bar) and 2 of 5 (Vibraphone bar), and
not in hardness at one velocity on each. The cause is the half-sine pulse's spectrum, whose
zeros (the first at 1.5 / duration) sweep across the bar's upper modes as the contact shortens.
The nonlinear contact's force is asymmetric and coupled to the bar's motion, which fills those
zeros. Marimba centroids are low and close together because the rosewood loss law
(Chaigne & Doutaut, with Rucz et al.'s loss factor) damps the upper modes within milliseconds and
the tube strengthens the fundamental. With no references, the slope criterion of E2 cannot be
checked yet (#1621).

## E3: repeated strokes on one bar

Two strokes at velocity 0.8 on one bar, 2–96 ms apart, compared with the sum of the two strokes
rendered on separate, fresh bars (relative RMS difference over 0.5 s):

| Profile, mallet | 2 ms | 5 ms | 11 ms | 23 ms | 47 ms | 96 ms |
|---|---|---|---|---|---|---|
| Marimba bar, Contact | 1.1e-1 | 6.0e-2 | 4.2e-3 | 4.6e-2 | 2.0e-2 | 8.0e-3 |
| Marimba bar, Pulse | 1.1e-7 | 4.0e-8 | 3.0e-8 | 3.0e-8 | 3.6e-8 | 2.9e-8 |
| Vibraphone bar, Contact | 5.9e-2 | 5.6e-2 | 1.0e-2 | 5.2e-2 | 4.3e-2 | 9.3e-3 |
| Vibraphone bar, Pulse | 8.8e-8 | 4.1e-8 | 3.4e-8 | 3.2e-8 | 3.9e-8 | 3.3e-8 |

Contact's second stroke depends on where the bar is and how it moves when the mallet arrives.
Pulse equals superposition to rounding, as candidate B predicts; the tube is linear and each
stroke's noise starts from the stroke itself, so neither breaks it.

Energy, measured on the bar's modes during one-second rolls (alternating 4 and 2.5 m/s strokes
every 61 ms, soft and hard mallets, center, default and end positions): the bar never held more
than 0.91 (Marimba bar) or 0.92 (Vibraphone bar) of the mallet energy brought by the strokes so far
with Contact, and 0.31 / 0.48 with Pulse. The shared DSP's tests check the same bound for single
strokes and rolls at 8, 22.05, 48 and 192 kHz.

## E4: strike position

Mode 2's level at the center (the lowest Strike position) against a quarter of the bar's length,
from a 0.2 s Hann-windowed DFT at mode 2's frequency: 139 dB weaker at the center on Marimba bar
and 140 dB on Vibraphone bar, for both mallets. The free-free shape's second mode has its node
exactly at the center, so the center stroke leaves it unexcited and the measurement shows only the
analysis floor. Real, undercut bars have shapes that differ from the uniform bar's, so the depth
of this notch is a property of the model, not a prediction for an instrument.

## E5: the resonator

Each bar's tube is one damped resonance at the bar's fundamental, quality factor 60 (measured
tube damping gives 57–68), driven by the first mode's velocity; at full Resonator its velocity
settles at four times the first mode's, raising the fundamental's steady level by about 14 dB
(Rucz et al. report more than 10 dB for a tuned resonator). The tube draws its energy from the
bar's first mode, fitted for the marimba to a bare F4 bar's 3.03 s shrinking to 0.67 s over a tuned
resonator and to a low D sharp ringing 3–4 s over a detuned tube and 1–2 s over a tuned one. A low
tube builds up slowly (its time constant is `60 / (pi * f1)`, about 0.3 s at C2), so a low note's
fundamental swells after the stroke.

`e5_the_resonator_strengthens_and_shortens_the_fundamental` renders the lowest, middle and
highest key of each profile with Resonator at 0 and 100 % and compares the fundamental's level
and decay in two windows scaled by the bare fundamental's decay time `T` (capped at 2 s): from
0.05 `T` to 0.3 `T`, and from 0.5 `T` to 0.75 `T`.

| Profile, key | Fundamental with the tube | Decay of the fundamental, dB/s (bare → tube) |
|---|---|---|
| Marimba bar, 36 | +8.6 dB | 13.7 → 20.0 |
| Marimba bar, 60 | +2.6 dB | 24.1 → 76.9 |
| Marimba bar, 96 | +13.1 dB | (the bar outlasts the tube too little to tell) |
| Vibraphone bar, 53 | +12.8 dB | 7.1 → 11.4 |
| Vibraphone bar, 60 | +12.9 dB | 7.5 → 12.3 |
| Vibraphone bar, 89 | +12.5 dB | 12.3 → 17.3 |

The test asserts only the direction the physics gives (stronger, and shorter where the bar rings
well beyond the tube's own decay). At middle C the tube's strong damping of the marimba bar takes
back most of its gain within the measurement window. The research's criterion needs reference
recordings of the same register (#1621) and stays open; the vibraphone motor is not modelled.

## E6: rolls within budget

`cargo run --release -p suhina-device-instrument --example benchmark` on the development
reference machine (Ryzen 5 5600X, Windows 11, release build), 48 kHz, 16 voices, 16 modes,
hardest mallet, rolls on sixteen bars with a pitch-wheel move every block. "Roll" strikes each
bar every 30 ms, staggered; "every block" strikes all sixteen bars in every block, so contacts run
almost continuously. Measured at commit `7baadffc` (tubes and mallet noise included) on an
otherwise idle machine: no other compiler ran during the build or the run. #1619 has set no budget
yet; the provisional target is 20 % of one core on average at 64 frames with the 99th-percentile
block under the deadline.

| Workload | 64 frames: mean core / p99 of deadline | 256 frames: mean core / p99 of deadline |
|---|---|---|
| Marimba bar, Contact, roll every 30 ms | 3.8 % / 4.8 % | 3.5 % / 4.0 % |
| Marimba bar, Contact, every block | 11.2 % / 16.5 % | 5.0 % / 6.8 % |
| Marimba bar, Pulse, roll every 30 ms | 3.7 % / 4.8 % | 3.5 % / 4.3 % |
| Marimba bar, Pulse, every block | 10.8 % / 16.4 % | 5.0 % / 6.2 % |
| Vibraphone bar, Contact, roll every 30 ms | 3.8 % / 5.1 % | 3.5 % / 4.4 % |
| Vibraphone bar, Contact, every block | 11.4 % / 15.3 % | 5.3 % / 7.9 % |
| Vibraphone bar, Pulse, roll every 30 ms | 3.8 % / 4.6 % | 4.3 % / 5.5 % |
| Vibraphone bar, Pulse, every block | 10.6 % / 13.1 % | 5.0 % / 6.8 % |

For comparison, the revision-1 Struck body in the same run: 2.0 % / 3.5 % at 64 frames, and
2.8 % / 5.0 % with sixteen retriggers per block. The inline core is 70 984 bytes, inside the
256 KiB target. E6 passes against the provisional target. An earlier run reported several times
these figures; it overlapped another compiler on the same machine and is not a measurement.

## E7: low sample rates and extreme settings

At 8 and 22.05 kHz, both profiles and both mallets, hardest mallet, velocity 1, all 128 keys
struck at once and then a dense roll on every fifth key with pitch-wheel swings, Decay 20 s,
Brightness 100 %, Strike at the end, and hostile parameter values (NaN, infinite and out-of-range
Body, hardness, mallet model and Resonator): output stayed finite, the bars decayed after the
roll, and the largest sample was 0.61. Contact runs at most 32 steps per sample and stops after at
most 30 ms per stroke; the shared DSP's tests check the step bound with a 100 m/s stroke at
22.05 kHz, and hostile modes, rates, masses, speeds, tubes and noise gains.

## Levels

Peak of a velocity-1 stroke at default controls (Struck at middle C: 0.173): Marimba bar with
Contact 0.16 / 0.14 / 0.06 at keys 36 / 60 / 96, with Pulse 0.08 / 0.08 / 0.03; Vibraphone bar with
Contact 0.22 / 0.23 / 0.16 at keys 53 / 60 / 89, with Pulse 0.12 / 0.13 / 0.09.

The later bodies were measured at the bottom, middle and top of their ranges and given a level
factor to land beside the first two: Xylophone bar (0.6) with Contact 0.21 / 0.16 / 0.04 at keys
65 / 86 / 108, with Pulse 0.10 / 0.07 / 0.02; Glockenspiel bar (3) with Contact 0.24 / 0.14 / 0.07
at keys 79 / 93 / 108, with Pulse 0.16 / 0.08 / 0.03; Kalimba tine (0.6), plucked whichever mallet
model is chosen, 0.16 / 0.19 / 0.12 at keys 60 / 74 / 88.

## More bodies and shaping

After the first listening the maintainer asked for more bodies and for controls that edit a body
rather than only choose one. Revision 3 now also has Xylophone bar, Glockenspiel bar and Kalimba
tine, and four shaping controls, Tuning, Ring, Torsion and Mallet weight, neutral at their
defaults, so a project made before them sounds the same (`docs/spec/instrument.md` › More bodies,
The tine, Shaping controls). The kalimba needed two pieces of shared DSP in `crates/dsp/src/bar.rs`:
clamped-free mode ratios and shapes, and a pluck that starts the tine from a static deflection at
rest. Its box resonance is a fixed-frequency tube driven by the whole tine, which the pitch wheel
leaves in place. The Model page now shows the bodies as buttons, a drawing of the bar or tine whose
contact point moves with Strike, and the controls in Body, Mallet and Space groups.

That page was then redesigned with the maintainer in a mock-up, against commercial instrument
editors' layouts and general UI guidance (one main control, no boxes, few controls on the main
page, the rest on an Advanced page). The result: the sound drawn as light across the page
(`Widget::Strands`, a host-drawn SDK widget, drawn on the GPU with a lighter CPU version), moved
by seven live values the Instrument now hands its panel (the first five modes' energies, the time
since a note-on and its velocity), one large machined Hardness dial at its center, one even row of
dials, and Tuning, Ring, Torsion, the mallet model and modes per voice on the Advanced page
(`docs/spec/instrument-editor.md`, `docs/spec/sdk-custom-ui.md` › Reusable functional widgets).

## First listening and real use

The maintainer played the Windows test build of the head before the tubes (`d733d20`) on an ASIO
interface at 44.1 kHz with a 2048-frame buffer and found Marimba bar fairly convincing, with room
to improve over time. The trace of that session showed both bodies and both mallet models in use,
the engine load at 4–7 % for the whole project, and no underruns or late render blocks. A mild
crackle was reported; the trace rules out dropouts, and at the time the track ran the Instrument
at +1.6 dB into two Multiband devices with +5 dB inputs and up to +10 dB band gains, so clipping
in that chain is the likeliest cause, not yet confirmed. Changes since address what the session
showed: the bars' output is lower (scale 0.08), and coefficient changes on a ringing bar no
longer step the output.

## What this slice leaves open

- The budget of #1619, once set, for E6.
- E5 against reference recordings (#1621), the tube's higher resonances and the vibraphone
  motor; E8 (listening) belongs to #1631.
- The crackle reported in the first listening: confirm or rule out the gain chain.
- Pulse's E2 failure: try a pulse shape without spectral zeros before deciding whether B has a
  place beside A.
- Vibraphone values: no per-mode decay, resonator or torsion measurements were found.
- The marimba's base loss `a0` balances a marimba source (Stevens) against a xylophone bar
  (Rucz et al.); a measured marimba would settle it.
- Strike position measures from the center with uniform-bar shapes; undercut bars' shapes and
  center-versus-edge strokes need references.
- Dead strokes, the damper pedal and the motor are not modelled.
- The glockenspiel's decay law, the kalimba's decay law, box quality and box frequency are
  assumptions; recordings of either instrument would settle them. The kalimba's tines are thin
  plates whose second overtone (McNeil & Mitran: about 14 times the fundamental) varies from tine
  to tine.

## Sources

- [F11 research](instrument-f11-mallets.md) and its sources.
- [Instrument spec](../spec/instrument.md) › Bar bodies and its sources for every value used.
