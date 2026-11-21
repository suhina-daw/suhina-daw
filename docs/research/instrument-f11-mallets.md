---
description: Research gate for Instrument family F11 (mallets, bells and bowls) - instrument inventory, physics, candidate models, gaps in the current modal core, falsifiable experiments and the reference data they need
read_when: F11, mallet percussion, marimba, vibraphone, xylophone, glockenspiel, tubular bells, bells, singing bowls, modal synthesis, mallet contact, Instrument family research
type: plan
status: draft
last_verified: 2026-11-20
covers: []
---

# Instrument F11: mallets, bells and bowls — research

This is the research gate of the F11 family epic (#1609, part of #1579). It inventories the
instruments, summarises the physics the models must reproduce, lists candidate models, measures
the current modal core against them, and defines falsifiable experiments for the prototype gate.
It contains no implementation decision that bypasses `docs/spec/`: a prototype writes its
behavior into [instrument.md](../spec/instrument.md) before code, as golden rule 1 requires.

Nothing here declares F11 playable, calibrated or complete. Ranges, ratios and timings below are
targets and literature values for designing experiments, not measurements of Suhina.

## 1. Instrument inventory

Ranges are typical concert instruments; instances vary and every profile records its own.
"Written" pitch differs from sounding pitch for the transposing instruments, which matters when a
reference recording is labelled by written note.

| Instrument | Typical sounding range | Body | Tuned modes | Resonator | Characteristic gestures and transitions |
|---|---|---|---|---|---|
| Marimba | C2–C7 (5 oct.; 4.3 and 4.5 oct. common) | Wooden bars (rosewood or synthetic), arched undercut | 1 : 4, often 1 : 4 : 10 | Closed tube per bar, tuned to the fundamental | Single strokes, rolls (repeated alternating strokes on one bar), dead strokes, mallet hardness from yarn to rubber, strike position along the bar and center vs. edge |
| Vibraphone | F3–F6 (3 oct.) | Aluminium bars, undercut | 1 : 4 | Tubes with motor-driven rotating discs | Long sustain, damper pedal (half and full), motor tremolo on/off and speed, dead strokes, bowing the bar end, mallet dampening |
| Xylophone | F4–C8 (sounds an octave above written) | Hard wood or synthetic bars | 1 : 3 (twelfth) | Short tubes, sometimes none | Short bright decay, rolls, glissandi across bars |
| Glockenspiel / orchestra bells | G5–C8 (sounds two octaves above written) | Steel bars, mostly rectangular | Usually only the fundamental; higher modes follow the free bar | None in the orchestral form | Bright long ring, hard mallets, glissandi, damping by hand |
| Crotales | C6–C8 (sounds two octaves above written) | Small tuned bronze discs | Disc modes, fundamental tuned | None | Struck or bowed, long ring, hand damping |
| Tubular bells (chimes) | C4–F5 (1.5 oct.) | Brass tubes, struck at the cap | Upper modes; the strike note is a perceived (virtual) pitch | None | Damper pedal, hammer hardness, struck near the cap |
| Bells (church, hand) | Instance-specific | Bronze shell | Partials named hum, prime, tierce, quint, nominal | None | Strike, clapper damping, hum lasting far longer than the strike note, warble from split mode pairs |
| Singing bowls | Instance-specific | Bronze or crystal bowl | Bowl modes, often inharmonic | None | Struck, or rubbed around the rim (continuous friction excitation), beating from split mode pairs |

Regional and traditional instruments (gamelan metallophones, balafon, gongs with pitched bosses)
get named profiles inside F11 when a profile is researched; gongs and cymbals without a tuned
pitch belong to F13.

## 2. Physics the models must reproduce

**Bars.** A uniform free-free bar has inharmonic transverse modes in the ratios
1 : 2.756 : 5.404 : 8.933. Makers undercut the bar's middle to pull the upper modes onto
musical ratios: the second mode two octaves above the fundamental for marimba and vibraphone,
an octave and a fifth (3 : 1) for xylophone, and on concert marimbas the third mode near ten
times the fundamental as well (Fletcher & Rossing, ch. 19; Bork 1995, who reports that tuning the
lower overtones as well as the fundamental clearly improves the tone). Many modern xylophones are
not tuned to the twelfth, or only in their lowest bars, so the xylophone ratio is a profile
choice, not a constant. A profile therefore needs per-note mode ratios, not one stretch formula
for the whole range: the low marimba register is tuned to three modes, the top register often to
one.

**Damping.** Losses rise with frequency, and differently by material: wooden bars lose their
upper modes within a fraction of a second while aluminium vibraphone bars ring for many
seconds. Each mode needs its own decay, and the decay law is a property of the profile.

**Resonators.** A closed tube under a bar is a quarter-wave resonator tuned to the bar's
fundamental. It strengthens and shortens the fundamental by radiating energy faster (Doutaut,
Matignon & Chaigne 1998). On the vibraphone, motor-driven discs in the tube tops open and close
the tubes, modulating the resonator's contribution at the motor rate; the bar itself is not
modulated. Manufacturer documentation describes the result as a level change with a small pitch
fluctuation, deeper as the motor speeds up. Commonly quoted motor rates reach about 12 rotations
per second, but no peer-reviewed acoustics source for the range was found; the prototype measures
or records it rather than assuming it.

**Mallet contact.** The mallet touches the bar for roughly a millisecond, and the force is
nonlinear: a felt or yarn head stiffens as it compresses, commonly modelled as
`F = K * delta^alpha` with a dissipative term (Hunt & Crossley 1975; Chaigne & Doutaut 1997;
Avanzini & Rocchesso 2001). Contact time shortens as velocity and hardness rise, so a harder or
faster stroke excites higher modes: **brightness follows velocity physically**, which a level
scale alone cannot reproduce. Strike position weights each mode by its mode shape at the contact
point; a free-free bar's mode shapes are not the sines of a string, and the second mode has a
node at the bar's center.

**Repeated impacts.** A roll strikes a bar that is still vibrating. The new contact meets a moving
surface, so the result depends on the bar's phase and velocity at contact; it is not a second,
independent bar sounding on top of the first.

**Bells and bowls.** A bell's partials sit near ideal ratios relative to the prime: hum 0.5,
prime 1, tierce a minor third above (1.2 just; tuned bells measure nearer 1.18–1.19), quint 1.5
and nominal 2. The perceived strike note is a virtual pitch built from the upper partials, and
it shifts measurably with the partial tuning (Rossing & Perrin 1987; Hibbert 2008). Axisymmetric shells split each mode into two close
frequencies when the symmetry is imperfect, which is heard as beating or warble. A rubbed singing
bowl is a continuously excited friction system whose excitation travels around the rim (Inácio,
Henrique & Antunes 2006); it belongs to the continuous-excitation problem that #1637 explores for
reeds, not to impulsive modal synthesis.

## 3. The current modal core against F11

The shared `crates/dsp` `ModalBank` and the Instrument's revision 1 body (PR #1590) are the
starting point. Measured against section 2:

| Requirement | Current state | Gap |
|---|---|---|
| Per-note mode ratios | Revision 1 computes `n * sqrt((1 + s*n^2)/(1 + s))`, a stiff-string stretch | Bar profiles need tabulated or fitted ratios per note; the bank itself accepts arbitrary frequencies, so the gap is in the Instrument, not in `ModalBank` |
| Per-mode decay law | Revision 1 uses `decay / (1 + 0.12*(n - 1))` | Material-dependent laws per profile; `ModalBank` already takes a decay per mode |
| Mode count | 8 or 16 per voice | Enough for bars (a handful of audible modes); bells and bowls with split pairs need more; budget belongs in #1619 |
| Velocity → spectrum | Velocity scales level only | Needs a contact model or a velocity- and hardness-dependent excitation spectrum |
| Strike position | Weights `sin(pi*n*p)`, string mode shapes | Free-free bar mode shapes per profile |
| Attack | Every strike starts with a 2 ms linear fade-in | A 2 ms ramp is longer than a hard mallet's contact and removes the click a hard stroke makes; it must become part of the excitation model, not a fixed fade |
| Repeated impacts | `ModalBank::strike` replaces the bank's state, and a repeated key takes another voice | One sounding body per bar; a new contact excites the bar's current state. Rolls otherwise consume voices quickly and sound like layered bars |
| Resonator | None | Per-bar resonator coupling, and a shared, per-instance motor modulation for the vibraphone |
| Damping gestures | Release damps; sustain holds | Damper pedal semantics (vibraphone, chimes), dead strokes (damping by the mallet), half pedal |
| Split mode pairs | None | Bells and bowls need detuned pairs with their own decays |

The bank's numerical properties (bounded decaying rotation per mode, anti-alias margin, no
allocation) are kept. The gaps sit in excitation, per-body state and coupling.

## 4. Candidate models

Each candidate is a hypothesis for the prototype gate, compared under the experiments in
section 5; none is chosen here.

- **A. Modal bar with a nonlinear contact excitation.** A lumped mallet (mass and nonlinear
  spring with dissipation) interacts with the bar's displacement at the contact point, computed
  from the modal states and mode shapes, for the duration of contact; after contact the bar rings
  freely in the bank. Reproduces velocity-dependent brightness, hardness, position and repeated
  impacts on a moving bar from one mechanism. Costs a short per-sample contact solve while the
  mallet touches; needs a stability analysis at high stiffness and low sample rates.
- **B. Modal bar with a linear pulse excitation.** The contact force is a fixed pulse shape (for
  example a half-sine) whose duration depends on velocity and hardness, applied through the mode
  shapes. Cheaper and unconditionally stable; reproduces brightness against velocity but treats
  a repeated stroke as linear superposition.
- **C. Hybrid attack.** A modal body with a short recorded or residual attack layer for mallet
  noise. Depends on permitted recordings (#1621) and alignment rules from #1579 (never applying a
  recorded body response twice).
- **D. Resonator coupling.** Either a coupled two-oscillator model of the bar fundamental and the
  tube, or a modified fundamental (gain and decay) as the cheap alternative. The vibraphone
  motor modulates only the tube's contribution, shared per instance rather than per voice.
- **E. Bells.** Modal partial sets with split pairs and a virtual strike pitch; the same
  excitation as A or B with a hard clapper.
- **F. Rubbed bowls.** A continuous friction exciter on a modal bowl; scheduled after the
  continuous-excitation work in #1637, since it shares its stability problems.

## 5. Falsifiable experiments for the prototype gate

Each experiment has a hypothesis, a reproducible render or measurement, and a pass criterion.
Renders use the offline path (golden rule 4) at fixed frames; nothing depends on wall-clock time
except the separate benchmark.

| # | Hypothesis | Measurement | Passes when |
|---|---|---|---|
| E1 | A bar profile places its tuned modes where the profile says | Render single strokes per note; pick spectral peaks | Tuned modes within ±5 cents of the profile's ratios over the profile's range |
| E2 | Brightness follows velocity and hardness | Spectral centroid of the first 50 ms across velocities and hardnesses, for candidates A and B | Centroid rises monotonically with both; with references available (#1621), the slope lies within the references' spread |
| E3 | A repeated stroke interacts with the moving bar | Two strokes on one bar at varied intervals; compare with the sum of two isolated strokes | Candidate A differs from superposition and stays bounded; output energy never exceeds the injected energy |
| E4 | Strike position follows bar mode shapes | Center and quarter-length strokes | Mode 2 at least 20 dB weaker at the center than at a quarter of the length |
| E5 | The resonator shapes the fundamental | Fundamental level and decay with and without coupling, candidate D both ways | The difference matches reference recordings of the same register within their spread; until references exist the experiment only reports |
| E6 | Rolls stay within budget | Benchmark: 16 voices, dense rolls on every bar, motor on, 48 kHz at 64 and 256 frames, with the existing `devices/instrument` benchmark example extended | Mean and 99th percentile under the per-voice and per-instance budget set in #1619; until #1619 sets it, the provisional Instrument target in the spec (20% of one core, p99 under the block deadline) |
| E7 | Low sample rates and extreme settings stay stable | Candidate A at 22.05 kHz with the hardest mallet and highest velocity; hostile parameters | Finite output, no growth, bounded contact iterations |
| E8 | Musicians accept the result as playable | Blind listening and playing sessions per #1631 | Recorded in #1631's report format; an optimisation loss or an AI score never substitutes |

## 6. Reference data needed

Calibration and E2/E5 need permitted, dry recordings, owned by #1621. For each instrument
instance: single strokes per note at several measured dynamics, at least three mallet hardnesses,
center/off-center/edge positions, repeated strokes and rolls at known intervals, dead strokes,
pedal and motor states for the vibraphone, and the room, microphone position and noise floor.
Self-recorded or CC0 material comes first; every candidate source's terms are checked file by
file before use, and generated audio is never calibration evidence (#1621, #1629).

Candidates found so far, none adopted:

- **University of Iowa Electronic Music Studios, Musical Instrument Samples:** marimba,
  xylophone, vibraphone, crotales and an unlabelled "Bells" set (C5–E8, plastic and brass
  mallets, anechoic). The page says the recordings may be used "for any projects, without
  restrictions", but names no license and does not mention redistribution. Usable at most as
  local measurement input; shipping or redistributing any file needs #1621's decision first
  (golden rule 14).
- **Synthesis ToolKit (STK) `ModalBar`:** a permissively licensed (MIT-style, copyright Cook and
  Scavone) open-source struck-bar modal model with marimba and vibraphone presets. Its parameter
  values are a license question, not a clean-room one (golden rule 1); a port keeps the
  copyright and permission notice. They are a starting point to compare against, not calibration
  evidence.

## 7. Proposed delivery slices under #1609

1. **Research** (this document).
2. **Bar prototype:** candidates A and B behind one experimental marimba profile and one
   vibraphone profile without motor; one sounding body per bar; experiments E1–E4, E6, E7.
3. **Resonators and vibraphone gestures:** candidate D, motor, damper pedal and dead strokes; E5.
4. **Bells and tubular bells:** split pairs and virtual strike pitch.
5. **Rubbed bowls and bowed bars:** after #1637's continuous-excitation results.
6. **Calibration, factory content and release:** the remaining gates of #1609, needing #1621,
   #1619 and #1631.

## 8. Open questions

- Does one excitation model (A) serve marimba through glockenspiel, or does the stiff end need a
  different contact law?
- What mode count do bells and bowls need to be recognisable, and does that fit #1619's budget?
- Should a voice be a key or a bar? A bar-per-key body changes voice allocation and stealing for
  this family, and the shared voice work in #1632 should be consulted before the prototype.
- Which permitted recordings exist for mallet instruments with measured dynamics and positions?

## Sources

Bibliographic details were checked on 2026-11-20 against Crossref, DataCite, publisher and
author pages; the free-free bar ratios were also recomputed from the Euler–Bernoulli roots.

- N. H. Fletcher and T. D. Rossing, *The Physics of Musical Instruments*, 2nd ed., Springer, 1998,
  ch. 19 "Mallet Percussion Instruments" and ch. 21 "Bells".
  [doi:10.1007/978-0-387-21603-4](https://doi.org/10.1007/978-0-387-21603-4)
- T. D. Rossing, *Science of Percussion Instruments*, World Scientific, 2000.
- A. Chaigne and V. Doutaut, "Numerical simulations of xylophones. I. Time-domain modeling of the
  vibrating bars", *J. Acoust. Soc. Am.* 101(1):539–557, 1997.
  [doi:10.1121/1.418117](https://doi.org/10.1121/1.418117)
- V. Doutaut, D. Matignon and A. Chaigne, "Numerical simulations of xylophones. II. Time-domain
  modeling of the resonator and of the radiated sound pressure", *J. Acoust. Soc. Am.*
  104(3):1633–1647, 1998. [doi:10.1121/1.424376](https://doi.org/10.1121/1.424376)
- I. Bork, "Practical tuning of xylophone bars and resonators", *Applied Acoustics*
  46(1):103–127, 1995. [doi:10.1016/0003-682X(95)93953-F](https://doi.org/10.1016/0003-682X(95)93953-F)
- K. H. Hunt and F. R. E. Crossley, "Coefficient of restitution interpreted as damping in
  vibroimpact", *J. Applied Mechanics* 42(2):440–445, 1975.
  [doi:10.1115/1.3423596](https://doi.org/10.1115/1.3423596)
- F. Avanzini and D. Rocchesso, "Modeling collision sounds: non-linear contact force", Proc.
  COST G-6 DAFx-01, Limerick, 2001.
  [PDF](https://avanzini.di.unimi.it/downloads/publications/avanzini_dafx01_revised.pdf)
- T. D. Rossing and R. Perrin, "Vibrations of bells", *Applied Acoustics* 20(1):41–70, 1987.
  [doi:10.1016/0003-682X(87)90082-X](https://doi.org/10.1016/0003-682X(87)90082-X)
- W. A. Hibbert, *The quantification of strike pitch and pitch shifts in church bells*, PhD
  thesis, The Open University, 2008. [doi:10.21954/ou.ro.0000add2](https://doi.org/10.21954/ou.ro.0000add2)
- O. Inácio, L. Henrique and J. Antunes, "The dynamics of Tibetan singing bowls", *Acta Acustica
  united with Acustica* 92(4):637–653, 2006.
- J.-M. Adrien, "The missing link: modal synthesis", in G. De Poli, A. Piccialli and C. Roads
  (eds.), *Representations of Musical Signals*, MIT Press, 1991.
- S. Bilbao, *Numerical Sound Synthesis*, Wiley, 2009 (ideal and variable-section bars,
  collisions, power-law mallet nonlinearity).
- P. R. Cook, *Real Sound Synthesis for Interactive Applications*, A K Peters, 2002 (modal
  synthesis, strings and bars).
- Synthesis ToolKit in C++, P. R. Cook and G. P. Scavone, MIT-style license:
  [repository](https://github.com/thestk/stk), [`ModalBar`](https://ccrma.stanford.edu/software/stk/classstk_1_1ModalBar.html).
- University of Iowa Electronic Music Studios, [Musical Instrument Samples](https://theremin.music.uiowa.edu/MIS.html).
- Current implementation: [instrument.md](../spec/instrument.md), `crates/dsp/src/modal.rs`.
