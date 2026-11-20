---
description: Results of the F11 bar prototype (slice 2) - experiments E1-E4, E6 and E7 on Instrument's Marimba bar and Vibraphone bar bodies, comparing the nonlinear Contact mallet with the linear Pulse mallet
read_when: F11 bar prototype results, marimba, vibraphone, mallet contact, Hunt-Crossley, half-sine pulse, E1 E2 E3 E4 E6 E7, modal bar benchmark, Instrument bar bodies
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
half-sine force whose duration follows velocity and hardness). Their behaviour is specified in
[instrument.md](../spec/instrument.md) › Bar bodies; this report records what the experiments of
the research's section 5 measured. It makes no calibration claim: no reference recordings were
used (#1621), and no listening session took place (E8, #1631).

The deterministic experiments are tests in `devices/instrument/tests/bar_experiments.rs` that
assert each pass criterion and print their measurements:

```sh
cargo test -p suhina-device-instrument --test bar_experiments -- --nocapture
```

All renders are offline at fixed frame counts, key 60 (middle C) and default controls unless
stated. Measurements come from the rendered output, not from the model's internal values.

## Summary

| # | Contact | Pulse |
|---|---|---|
| E1 tuned modes | Pass (worst 0.03 cents) | Pass (worst 0.01 cents) |
| E2 brightness | Pass on every cell | **Fails** for velocity at soft and medium hardness; passes for hardness |
| E3 repeated strokes | Pass: differs from superposition by 0.8–8.6 % RMS; energy ≤ 0.95 of the strokes' | Superposes (differences ~1e-8, as designed); energy ≤ 0.60 |
| E4 strike position | Pass (mode 2 ≥ 129 dB weaker at the centre) | Pass (same) |
| E6 budget | Not validly measured; an unreliable run suggests the every-block worst case is over budget | Not validly measured |
| E7 stability | Pass at 8 and 22.05 kHz | Pass |

Contact is the default mallet model because it is the only candidate that passes every
experiment that could be measured; its cost (E6) is still open. Pulse stays selectable for comparison and as the cheaper alternative.

## E1: tuned modes

Single strokes at velocity 0.8 on every fourth key of each profile's range (Marimba bar keys
36–96, Vibraphone bar keys 53–89); each tuned mode's peak was found in a Hann-windowed 0.5 s DFT
by a 1-cent search with parabolic refinement and compared with the profile's ratio at that key.
Modes within 5 % of the anti-alias margin were skipped.

| Profile | Mallet | Tuned modes measured | Worst error |
|---|---|---|---|
| Marimba bar | Contact | 48 | −0.030 cents (key 96, mode 3) |
| Marimba bar | Pulse | 48 | −0.013 cents (key 96, mode 3) |
| Vibraphone bar | Contact | 20 | 0.000 cents |
| Vibraphone bar | Pulse | 20 | 0.000 cents |

The contact's brief stiffening does not shift the free modes measurably. E1 verifies the
renderer against the profile, not the profile against real instruments: the ratios are the
literature's targets (1 : 4 : 10 and 1 : 4), not measurements.

## E2: brightness against velocity and hardness

Magnitude-weighted spectral centroid of the first 50 ms (Hann window, every DFT bin), velocities
0.1–1.0 and hardness 0–1. Centroids in Hz, velocity increasing to the right:

| Profile, mallet | Hardness 0 | Hardness 0.5 | Hardness 1 |
|---|---|---|---|
| Marimba bar, Contact | 262 → 472 | 477 → 734 | 762 → 1075 |
| Marimba bar, Pulse | 274, 291, 321, **304, 278**, 312, 352 | 419 → 652 | 682 → 1023 |
| Vibraphone bar, Contact | 263 → 574 | 548 → 1451 | 1500 → 3601 |
| Vibraphone bar, Pulse | 310, 324, 401, **386, 368**, 376, 452 | 543, 710, 933, **905**, 960, 1100, 1183 | 1440 → 3160 |

Contact rises monotonically in both velocity and hardness on all 35 cells of both profiles.
Pulse is monotonic in hardness everywhere but not in velocity: Marimba bar fails at hardness 0
(1 of 5 hardnesses), Vibraphone bar at hardness 0, 0.25 and 0.5 (3 of 5). The cause is the
half-sine pulse's spectrum, whose zeros (the first at 1.5 / duration) sweep across the bar's
second and third modes as the contact shortens; a soft, slow stroke's contact is long enough
for the first zero to fall below mode 2. The nonlinear contact's force is asymmetric and coupled
to the bar's motion, which fills those zeros. A pulse shape without spectral zeros (a one-sided
gamma pulse, for example) would remove this failure while keeping B linear; it was not tried in
this slice. With no references, the slope criterion of E2 cannot be checked yet (#1621).

## E3: repeated strokes on one bar

Two strokes at velocity 0.8 on one bar, 2–96 ms apart, compared with the sum of the two strokes
rendered on separate, fresh bars (relative RMS difference over 0.5 s):

| Profile, mallet | 2 ms | 5 ms | 11 ms | 23 ms | 47 ms | 96 ms |
|---|---|---|---|---|---|---|
| Marimba bar, Contact | 4.6e-2 | 6.8e-2 | 8.0e-3 | 6.1e-2 | 2.7e-2 | 1.7e-2 |
| Marimba bar, Pulse | 7.9e-8 | 5.6e-8 | 3.9e-8 | 3.3e-8 | 3.7e-8 | 3.0e-8 |
| Vibraphone bar, Contact | 2.9e-2 | 7.0e-2 | 3.0e-2 | 8.6e-2 | 7.6e-2 | 2.7e-2 |
| Vibraphone bar, Pulse | 3.7e-8 | 3.7e-8 | 3.5e-8 | 3.0e-8 | 3.5e-8 | 3.2e-8 |

Contact's second stroke depends on where the bar is and how it moves when the mallet arrives;
the difference varies with the interval as the bar's phase at contact does. Pulse equals
superposition to rounding, as candidate B predicts.

Energy, measured on the bar's modes directly during one-second rolls (alternating 4 and 2.5 m/s
strokes every 61 ms, soft and hard mallets, centre, default and end positions): the bar never
held more than 0.94 (Marimba bar) or 0.95 (Vibraphone bar) of the mallet energy brought by the
strokes so far with Contact, and 0.35 / 0.60 with Pulse. The shared DSP's tests check the same
bound for single strokes and rolls at 8, 22.05, 48 and 192 kHz.

## E4: strike position

Mode 2's level at the centre (the lowest Strike position) against a quarter of the bar's length,
from a 0.2 s Hann-windowed DFT at mode 2's frequency: 129 dB weaker at the centre on Marimba
bar and 148 dB on Vibraphone bar, for both mallets. The free-free shape's second mode has its
node exactly at the centre, so the centre stroke leaves it unexcited and the measurement shows
only the analysis floor. Real, undercut bars have shapes that differ from the uniform bar's,
so the depth of this notch is a property of the model, not a prediction for an instrument.

## E6: rolls within budget

`cargo run --release -p suhina-device-instrument --example benchmark` on the development
reference machine (Ryzen 5 5600X, Windows 11, release build), 48 kHz, 16 voices, 16 modes,
hardest mallet, rolls on sixteen bars with a pitch-wheel move every block. "Roll" strikes each
bar every 30 ms, staggered; "every block" strikes all sixteen bars in every block, so contacts
run almost continuously. There is no vibraphone motor in this slice, so the research's "motor
on" condition could not be measured. #1619 has set no budget yet; the measurements are compared
with the spec's provisional target (20 % of one core on average at 64 frames, p99 under one
block deadline).

**Not validly measured yet.** One local run was made, but another build was compiling on the
same machine at the time, and its results are not self-consistent (Marimba bar with Contact took 45 % of
a core for the 30 ms roll at 256 frames but 9.5 % at 64 frames), so they are not recorded as a
measurement. Read as an upper bound only, they show what to look at when
the benchmark is repeated on an idle machine:

- The 30 ms roll on sixteen bars averaged roughly 10–15 % of one core at 64 frames for both
  mallets, inside the provisional 20 % target, but its p99 did not stay under the block deadline.
- Striking all sixteen bars in every block is the worst case: Contact, which runs up to 32 steps
  per sample during each contact, averaged far above the target; Pulse was above it too, by less.

E6 therefore does not pass yet. Before slice 3, the benchmark has to be rerun on an idle machine,
and Contact's cost per contact step reduced if the rerun confirms it (fewer steps for soft
mallets, whose contact is long and smooth, or a step count chosen per stroke from the actual
compression rather than the rigid bound).

## E7: low sample rates and extreme settings

At 8 and 22.05 kHz, both profiles and both mallets, hardest mallet, velocity 1, all 128 keys
struck at once and then a dense roll on every fifth key with pitch-wheel swings, Decay 20 s,
Brightness 100 %, Strike at the end, and hostile parameter values (NaN, infinite and
out-of-range Body, hardness and mallet model): output stayed finite, the bars decayed after the
roll, and the largest sample was 0.61. Contact runs at most 32 steps per sample and stops after
at most 30 ms per stroke; the shared DSP's tests check the step bound with a 100 m/s stroke at
22.05 kHz and hostile modes, rates, masses and speeds.

## What this slice leaves open

- E6 on an idle machine, and Contact's cost if the rerun confirms the worst case.
- E5 (resonators) and E8 (listening) belong to slice 3 and #1631.
- Pulse's E2 failure: try a pulse shape without spectral zeros before deciding whether B has a
  place beside A.
- Strike position measures from the centre with uniform-bar shapes; undercut bars' shapes and
  centre-versus-edge (across the bar's width) strokes need references.
- Bar masses, the mallet's mass and exponent, and the decay laws are literature-scale
  assumptions, not fits; #1624 will fit them from samples using this body.
- Mallet noise, dead strokes, the damper pedal and the motor are not modelled.

## Sources

- [F11 research](instrument-f11-mallets.md) and its sources.
- [Instrument spec](../spec/instrument.md) › Bar bodies.
