---
description: Bounded single-recording modal fitting, measured limits, temporal validation and explicit native Instrument adoption
read_when: fitting a sample, modal fitting, resonance estimation, decay estimation, instrument.fitSample, instrument.acceptFit
type: spec
status: planned
last_verified: 2026-11-20
covers:
  - crates/instrument-analysis/src/modal.rs
  - crates/instrument-analysis/src/modal_tests.rs
  - crates/session/src/instrument_analysis.rs
  - crates/session/src/commands/instrument.rs
  - crates/ui/src/detail/instrument_analysis.rs
---

# Fit a decaying recording

An Instrument with a loaded sample can fit an **experimental linear modal model** of one
isolated struck sound. The user sets the sample trim and root, chooses **Fit resonances**, then
inspects the number of retained modes and temporal envelope error before accepting. Fitting
does not change the instrument. Acceptance installs the canonical modal profile, selects Modal,
and resets Body decay, Brightness, Structure and Strike to the profile's neutral controls,
enabling the full sixteen-mode quality budget, in one
undo step. The sample remains available as the source. **Fit folder…** fits a folder of such
recordings in one job (§ Fitting a folder). Existing model-profile adoption and
project persistence are reused; reopening never starts fitting.

The reference key is locked to the sample's current root, with its equal-tempered reference Hz.
Measured resonance frequencies are stored as ratios to that reference. This does not declare
the lowest resonance or a missing fundamental to be the pitch. Root-note analysis can be used
first, and its proposal can be corrected before acceptance.

## Estimator and limits

Algorithm version 2 admits mono/stereo PCM at 1000..384000 Hz and at most the first eight
seconds of the selected trim. It checks finite input and chooses the channel with greatest AC
energy; the chosen zero-based channel and inspected source-frame range are reported. Opposite
stereo polarity therefore cannot cancel the source. Version 4 keeps a stereo recording's width
(see **Stereo** below); the modes are found and fitted on the chosen channel.
A channel whose values do not vary (silence or a constant offset) is reported as silent.
The existing shared resampler prepares one normalized channel at 22050 Hz. A relative trailing
noise floor limits the useful analysis interval; the source itself is never trimmed or edited.

**Finding resonances.** A Hann-windowed FFT over the start of the analyzed interval, with the
largest power-of-two length up to 8192 samples that fits it, proposes at most 192 prominent
peaks: a local maximum within 60 dB of the strongest that stands 20 dB above the median of its
neighbours 3..8 bins away. When no peak stands that far out (algorithm version 4), the local
maxima within 30 dB of the strongest that stand 10 dB above that median are proposed instead: a
breathy flute's breath and flutter widen its partials, and a flute G#5 stab had no candidate at
all, so no model, though its 856 Hz partial stood 33 dB above everything else. This detection
window is independent of the envelope windows, so
the partials of a low note in a short trim are resolved (version 1 searched only the first
envelope window, as short as 1024 samples, where neighbouring partials of a low note hid each
other). Candidates closer than two envelope-window bins (the Hann main lobe's half width) are
merged into the stronger one. Bounded interval refinement on the detection window estimates
each peak frequency.

**Partials along the string.** A piano note has dozens of partials, a bass note a hundred under
10 kHz, and most are far weaker than the few that prominence finds; a model of only those
sounds like a few sine waves (published modal pianos use about 100 partials for a bass note,
Bank, Zambon and Fontana 2010; a differentiable piano model uses 96 and found an explicit
stiff-string inharmonicity essential, Renault, Mignot and Roebel 2022). So after the prominent
peaks are refined, the fit predicts the rest from the stiff string, whose partials lie at
`k * f0 * sqrt(1 + B * k^2)`. The measured first partial `f1` is itself stretched
(`f0 * sqrt(1 + B)`), so against it the k-th partial is `k * f1 * sqrt((1 + B * k^2) / (1 + B))`:

1. `f1` is the refined peak nearest the reference key's frequency, within 70 cents; without
   one, no partials are predicted (a bell or bar is left to prominence).
2. With `r = f / f1`, `B` is the least-squares fit through the origin of
   `(r/k)^2 - 1 = B * (k^2 - (r/k)^2)` over the other refined peaks within 5 % of a whole
   multiple `k` of 2..32 of `f1`, from at least three such peaks, clamped to 0..0.01;
   otherwise 0.
3. For each `k` from 1 while the predicted partial is below the analysis ceiling (at most 128 partials), when no
   candidate lies within the candidate separation or 1 % of it, the strongest detection bin
   within 1 % of it becomes a candidate if it is a local maximum within 80 dB of the
   strongest bin and stands 6 dB above the median of its neighbours 3..8 bins away; candidates
   stop at 192 in all (version 4; 96 before, which a piano chord's four notes used up: 124 were
   found once the limit was raised).

Every candidate is then measured and fitted alike (a predicted partial is kept, loose or
validated, only when its envelope says so).

**Measuring decay.** The interval is cut into 6..12 consecutive non-overlapping Hann windows
with a power-of-two size from 1024 to 8192. A complex projection at the mode's frequency
measures its amplitude in each window. Each mode uses its own span: it starts at its loudest of
the first three windows (an attack inside the first window is not a decay), and ends before the
first window more than 60 dB below that peak or 6 dB above the lowest level so far (the
recording's floor, or the next strike in the trim). Within the span, even windows supply
estimation data and odd windows are held out for validation. Least squares on log amplitude
versus time estimates exponential decay; T60 is minus ln(1000) divided by the slope. The
window's damping-dependent amplitude bias is compensated before storing relative magnitudes.

**Partials a neighbour hid (algorithm version 4).** A partial a few hertz beside a much
stronger one fails the prominence test, since the stronger one is among the neighbours whose
median it must stand above; a piano chord's 1..2 kHz partials were missing that way, and a noise
tail standing in for them sounded like applause behind the notes. So once the candidates are
fitted (and a chord's notes have added theirs), the fitted modes are rendered and subtracted from
the recording over a detection-window length that starts after the 0.25 s attack (whose broadband
knock would hide them again; at the start when the interval is too short), and the residual's
spectrum is searched like the recording's:
local maxima within 60 dB of the recording's strongest that stand 6 dB above the median of
their neighbours 3..8 bins away **in the residual** (as a string's predicted partials must; the
recording's 20 dB let two of 134 through on a piano chord), strongest first, not within the candidate
separation of a candidate, until 384 candidates in all. Each is refined on the residual and
measured and fitted on the recording like any candidate. Since a later pass can refine onto a peak an earlier one fitted,
two modes within one detection bin are one: the more energetic stays (before, such a pair made
the ratios not increase and the whole fit fail).

**Fast partials (algorithm version 4).** The envelope windows suit partials that ring for
seconds, but the upper partials of a struck or plucked note (a synthesizer note whose filter
closes, a piano's treble partials) fall 60 dB within a few tenths of a second, inside one or two
of those windows, so no fit accepted them. They were only in the 0.25 s attack, and once it
faded the note lost its brightness at once: measured on a synthesizer bass, the 2 kHz octave
fell from -58 dB to below -130 dB at 0.3 s, and a piano chord's 500 Hz..2 kHz octaves were 6 dB
low from 0.3 s on. So a candidate the envelope windows leave without a mode is measured again
with **short windows**: the smallest power-of-two Hann size from 512 samples, below the envelope
window size, whose main lobe stays clear of the nearest other candidate that is at most 20 dB
weaker in the detection spectrum (size at least `4 * 22050 / gap`, the gap in Hz; a weaker
neighbour leaks too little to matter, and counting it kept a piano's strong 523 Hz partial, 14 Hz
from a partial 30 dB weaker, out of the short windows its null needs), cut from the start of the interval into at most 48
consecutive windows. Its span, estimation and validation windows, error limits, late retry and
loose rule are those above, and the window bias is compensated for the short window. A partial
the short windows describe is kept like any other and competes for the recording's 64 modes by
the same energy; one that fits neither stays out.

**A fast first stage (algorithm version 4).** A piano partial often falls fast for its first
second and then rings on slowly (two strings drifting out of phase, energy moving into the
soundboard). In a long recording the envelope windows are long too (8192 samples, 0.37 s), so
that first stage lies within one or two of them and the fit describes only the slow tail: a
measured piano chord's 786 Hz partial was 8 dB too quiet at 0.5 s and 11 dB too loud at 1.5 s, and
the note seemed to be damped as soon as its attack faded. So a mode fitted on the envelope
windows without a fast part is measured again with the short windows it would get as a fast
partial. From the loudest of the first four, while a window stands at least 1 dB above the
slow decay (as that window would measure it), the excess is fitted as a second exponential. It
becomes the mode's fast part when it covers at least three windows, decays faster than half the
slow T60 (at least 1 ms) and lowers those windows' RMS error by at least 1 dB; its window bias
is compensated like the slow part's.

**Stereo (algorithm version 4).** A model of one channel plays every partial in the middle,
while a stereo recording spreads them: a piano's bass strings to one side and its treble to the
other, a pad or a chord as wide as its two microphones (a measured piano chord has as much side
as middle). So the other channel is prepared like the chosen one (its own mean, the chosen
channel's scale, the same resampling, trailing-floor cut and vibrato flattening), and once the
recording's modes are chosen, each is projected on both channels through the detection window.
The ratio of the other channel's complex amplitude to the chosen channel's is the mode's
**other-channel gain** (0..8) and **phase offset** (-pi..pi); a mode the chosen channel does not
carry above 1e-9 of the largest projection keeps gain 1 and offset 0. The attack's other channel
is the residual of the other channel against the modes with those gains and offsets, faded in
the same way, and both attack channels are kept or dropped together (by their summed residual
energy). A mono recording, or a stereo one whose channels match within 1e-6, stores no stereo
part and plays as before.

**Accepting a mode.** A mode is kept when it has at least three estimation and three validation
windows, its slope decays with a T60 within 0.001..60 s, the fitted level falls at least 1 dB
across the estimation windows and at least three times the larger RMS envelope error, the
estimation error is at most 2.5 dB and the validation error at most 3 dB. A real strike rarely
decays as one exponential: beating partials and a fast early decay followed by a slower one
leave decibels of error that version 1's limits (1.0 dB estimation, 1.5 dB validation) rejected,
so a recorded strike could yield no model at all. When a span of nine or more windows fails,
the fit is repeated once on its last two thirds, which describes the sustained part after a fast
early decay. A mode whose fitted level changes by at most 3 dB across its whole span without
clearly decaying, within the same error limits, is **sustained** (the late retry never makes a
mode sustained, since beating can flatten a decaying partial's late windows): a bowed, blown or held partial
(a cello, an organ, a voice) is stored with the profile's longest T60, 60 s, and is reported as
sustained; note-off releases it like any modal voice. This approximates a held tone by its
spectrum and is not a bowing or breath model. Algorithm version 3 first measures a vibrato and
resamples the recording flat before this step, and gives a held recording a layer of vibrato,
per-mode onsets, vibrato-coupled levels, band noise, a release and a spectral envelope that stays
in Hz ([held tones](instrument-sustained-models.md)); the steps here are otherwise unchanged. The result contains at most 16 modes within the
canonical profile's bounds. Rising envelopes, noise and short sounds yield no model; the
playable sample remains usable.

**The fitted recording.** Besides the schema-1 profile, a fit returns a `ModelRecording` for a
revision-4 set ([modal profiles](instrument-modal-profile.md#fitted-model-sets-revision-4)):

- **Up to 64 modes.** Detection and the string's partials propose at most 192 candidates; the
  64 with the most energy (`magnitude^2 * T60`) are kept. The schema-1 profile keeps the 16 largest magnitudes.
- **Phase.** The complex projection of a mode's first span window gives its phase at that
  window, carried back to the strike (the start of the analyzed interval) by its frequency. With
  the phases, the modes start where the recording's partials do, so the attack below hands over
  to them without cancelling.
- **Loose partials.** A candidate whose envelope fails every fit above (a deep beat between
  two or three strings of one note dips more than the error limits allow, and its rise after a
  dip ends the span early) is still a partial of the sound; dropping it leaves a few smooth
  partials that sound like sine waves. It is kept **loose** when, from its loudest of the first
  three windows until 60 dB below that peak (no rise rule), it has at least six windows, a
  least-squares line through them falls by at least 6 dB with a T60 within 0.001..60 s, and the
  line's RMS envelope error is at most 15 dB, and the mean level of each third of those windows
  is at least 1 dB below the third before it (a burst followed by a flat floor is no decay).
  As a last resort (algorithm version 4), a candidate that still fails but clearly fades is kept
  loose too: from its loudest window in the first third of its windows until 60 dB below it, at
  least four windows whose first third stands at least 3 dB above their last, with the
  least-squares decay (a T60 within 0.001..60 s) and no error limit. On a piano chord the
  partials this keeps (1062, 1478, 1548, 1620, 1994, 2057 and 2122 Hz among them, uneven from
  beating and from energy moving between strings) were the 1..2 kHz energy the model lacked. A
  loose mode joins the fitted recording with that
  T60 (and may get a beating partner below), but never the schema-1 profile or its reported
  validation errors, unless every mode of the sound is loose (algorithm version 4): then the
  profile takes the loose modes, so a breathy stab whose partials no fit validates still has a
  model (before, it had none).
- **Held partials at an uneven level (algorithm version 4).** A bowed, blown or sung note
  holds its partials, but not at one level: the bow's pressure and speed, a bow change, the
  breath and beating between a partial and its neighbours move each partial by several decibels
  from one envelope window to the next, each partial its own way. Measured on a cello E4 without
  a detectable vibrato, its three strongest partials (325, 651 and 977 Hz, about -14 dB) moved
  by +-4 dB: the decay fits rejected them for their error, the loose rules for not falling, and
  the model kept the partials above 1 kHz only; after the attack the 250 and 500 Hz octaves were
  40 dB low and the note sounded plucked. A note also has an accent at its start and often a
  release at its end, which made a held note's partials look like decays (a cello C3 whose
  release starts at 0.65 s was fitted as decaying, partly held). So first the recording is told
  to be a **held note**: its RMS level over 1024-sample windows of the analyzed interval obeys
  the rule below. In a held note every candidate is first tried as held, before the null
  search and the decay fits, which it skips when it holds; outside a held note the rule is only
  the last resort above, against the note's trend, so a struck note's partials that the decay
  and loose fits describe are fitted exactly as before. The rule, on a partial's
  envelope windows: the **held part** starts at the first window centered at least 0.15 s in
  (after the accent) and runs to the last window at most 6 dB below the median of the first
  half of the windows from there (what follows is the release, which the held layer measures);
  it needs at least five windows, at most a quarter of them more than 20 dB below the held
  part's median (a bow change's or a beat's null), and the median level of its last third at most
  3 dB below its first third's (a swell is allowed: the held layer gives the mode its rise).
  Medians, since a few dips pull a mean down as a decay would. A held partial is sustained (the
  profile's longest T60, 60 s) at its mean amplitude over the held part, its phase from the
  held part's first window; its reported estimation and validation errors are the RMS of its
  levels around that mean. A held note always gets a held layer
  ([held tones](instrument-sustained-models.md)), whatever share its sustained modes hold. On
  the cello E4 the 500 Hz..2 kHz octaves then stayed within 6 dB of the recording through the
  note (the 250 Hz octave dipped 12 dB at 0.3 s, where a beat was fitted on the uneven
  fundamental), and on a cello C3 the held part stayed within 2 dB of most octaves, with a
  measured 2.9 s release.
- **Partials that follow a fading note (algorithm version 4).** A wind or bowed note that is
  not held may fade as a whole, a trombone E3 by about 6 dB per second, while one of its
  partials stays at an uneven level that no fit describes and that never clearly falls: the
  trombone's 329 Hz partial moved by 4 to 9 dB between windows, was dropped, and its octave
  played 26 dB low at 1 s. So the whole level also gives the note's **trend**: the least-squares
  slope (at most 0) of its RMS level over 1024-sample windows from 0.15 s on, while within 40 dB
  of its loudest window (none in a held note, and none from fewer than five windows). As a last
  resort after the loose rules, a partial whose loudest window is within 30 dB of the note's
  loudest level (hum and rumble under a piano note are no part of it) is tried by the held rule
  below with its levels measured against that trend; when it holds against it, it follows the
  note: its slope is the trend (at most the profile's longest T60, 60 s), its level its mean
  amplitude against the trend, and it is loose. On the trombone its octave then stayed within
  6 dB of the recording to 1 s.
- **Prompt sound and aftersound (algorithm version 4).** A piano note's two or three strings,
  tuned a little apart and coupled through the bridge, give a partial a fast "prompt sound" and
  a slow "aftersound" that can start in opposite phase (Weinreich 1977): the partial falls fast,
  vanishes in a deep null where the two cross, and comes back as a quiet, long ring. Measured on
  a piano C4, the fundamental fell from -7 to -47 dB at 1.3 s and rose back to -36 dB, where it
  stayed; a model of two parts in one phase cannot dip, played 20 dB too loud around the null,
  and the note lost the swell that makes it sound like a piano. So before the fits above, a
  mode's short windows (the size it would get as a fast partial, over the first 6 s; the
  envelope windows of a long recording, 0.37 s, smooth a null) and then its envelope windows
  are searched
  for a **null**: from its loudest of the first three windows, the
  first window that is the lowest of the two windows on each side, at least 10 dB below that
  peak, with a later window at least 6 dB above it. The aftersound is the least-squares line
  from the loudest window after the null while within 20 dB of it (a beat's next null comes
  later) and 60 dB below the peak (at least three windows, not rising, its RMS error at most 3 dB, so periodic
  beating with null after null is left to the beat estimate); the prompt sound is the line
  through `ln(measured + aftersound)` over the windows before the null where the measurement is
  at least three times the aftersound (nearer the null the sum flattens the line) (at least three, falling faster than the aftersound). The pair
  is kept when its envelope `|prompt - aftersound|`, floored 60 dB under the peak, stays within
  6 dB RMS of the windows (at 9 dB a piano C4's 523 Hz partial got a pair whose null fell 0.7 s
  late), leaving out the null and the window on each side of it (the pair's
  exact zero and the measured floor always differ there). The mode's slow part is then the aftersound in opposite phase, its
  fast part the prompt sound, and its magnitude their difference at the strike; the aftersound
  is at most half the prompt sound there (the fast share is then at most 2). Such a mode is
  loose (it stays out of the schema-1 profile).
- **Two-part decay.** When a span of nine or more windows has a valid fit on its last two thirds
  (the slow part), the excess of the earlier windows over that line is fitted as a second,
  faster exponential. It is kept as `fast_decay_seconds` and `fast_share` when its T60 is under
  half the slow one, it has at least two windows, and over the whole span it lowers the RMS
  envelope error by at least 1 dB compared with one exponential, or leaves at most a third of an
  error of at least 0.25 dB, or one exponential failed. (A fast part shorter than a window still
  bends the first windows: one exponential then fits within half a decibel yet misses it.)
- **Beating.** Each kept mode's amplitude is also measured on 1024-sample Hann windows every 256
  samples. After removing the fitted decay, the residual log envelope's normalized
  autocorrelation is searched over beat periods of 0.1 to 5 s (shorter than half the measured
  span). A peak of at least 0.5 with an RMS ripple of at least 1 dB is a beat: its rate gives
  `beat_ratio = 1 + rate / frequency`, and the ripple's peak-to-peak depth `D = 2 * sqrt(2) * RMS`
  dB gives the partner's share `a2 / (a1 + a2)` from `(a1 + a2) / (a1 - a2) = 10^(D / 20)`, at most
  0.5. The sign of the detuning is not observable and is taken as positive.
- **Level.** `level` is the sum of the modes' magnitudes in the source's own full-scale units,
  so the fitted model at the recording's key is as loud as its sample.
- **Attack.** The fitted modes, with their phases, two-part decays and partners, are rendered
  over the first 0.25 s of the interval (or less, if shorter); the difference from the signal,
  faded out with a raised cosine over its second half and divided by `level`, is the attack.
  Version 4 renders and cuts it at the **source's own rate** when that is at most 48 kHz and the
  recording was not resampled flat for a vibrato (the source channel without its mean, at the
  fitted channel's scale), and at the prepared 22050 Hz otherwise. A 22050 Hz attack lost the
  knock's top octave, and read up to 48 kHz by linear interpolation it mirrored its content
  into 11..22 kHz: on a piano note the 14 kHz octave of the first 50 ms played 26 dB above the
  recording's, and the strike lost its edge. It is kept only when its energy is at least 0.1 % (-30 dB) of the signal's in
  that window, so a purely modal sound carries none. The attack holds what the modes miss, so the
  body response is not applied twice.
- **Tail (algorithm version 4).** What the modes miss does not end with the attack: a
  piano's hammer and soundboard noise, partials too weak or too crowded to fit, a chord's
  sympathetic strings. Measured on a recorded piano chord, the 1..2 kHz octaves fell 5..7 dB
  between 0.15 s and 0.3 s as the attack faded, and stayed that much below the recording as
  the notes rang on, so a note sounded rich for its first tenth of a second and then thin. So
  the fit also renders the modes (with their stereo placements) over the whole interval and
  measures, in Hann frames of 2048 samples every 1024, how much power the recording has beyond
  them in each of the eleven 2/3-octave noise bands of the held layer
  ([held tones](instrument-sustained-models.md)): the median of `max(0, recording - modes)`
  over the band's bins further than two bins (the Hann main lobe) from every part of every mode,
  divided by `ln 2` (a noise bin's median against its mean) and scaled to the band's whole width
  (a band with fewer than two such bins in a frame counts nothing there), for the fitted channel
  and the other one. The median is the noise between the peaks: a mode's small level error is
  not taken for noise, and neither is a partial the modes miss, which a tail summing every bin
  played back as hiss. From the first frame centred after
  0.1 s, while a band's summed missing power stays within 50 dB of its largest and 10 dB above
  the band's floor (the median of its last eight frames: the recording's own noise, into which
  a decay must not be fitted, or the fitted chord's 4..8 kHz tail rang 10 dB too long), a least-squares
  line through its logarithm gives the band's level at the strike and its decay; a band is kept
  when it has at least four frames, decays (its T60 within 0.05..60 s) and falls at least 6 dB
  across them, so a recording's steady hiss is not taken for a tail, and when it holds at least
  -30 dB of what the modes play in the band over those frames (below that it is the modes' own
  small error, and inaudible beside them) and at least -60 dB of what they play in all bands
  together (a band with no modes of its own otherwise passed on their sidelobes' tiny error). Each channel's level is
  stored as an amplitude density in the recording's normalized units per square-root hertz, as
  the held layer's noise is; the decay is one per band for both channels.

The report lists per mode the fast part, the beat and the phase, and the attack's duration.
Acceptance and addition to a set are described with the revision-4 commands.

Report errors are RMS dB differences of a mode's amplitude envelope, not waveform errors,
probabilities, realism percentages or independent listening results. Odd windows belong to
the **same recording**: they do not establish held-out note, velocity, gesture or session
performance. Mode discovery itself has a fixed small search budget; no converged global
optimum is claimed. Initial phases and the attack are measured from the one recording, not
reconstructed as excitation or contact physics; room response, nonlinear dynamics and coupled
modes are not separated, and the attack and modes may carry the room. Fitted resonances must
not be presented as a uniquely recovered physical body.

## Splitting a chord into notes

A recorded chord fits as one recording whose modes are every string's partials: it plays the
whole chord on each key, and its four or five notes share the 64 modes, so the upper partials
run out first and the chord thins out as it rings. When the modes hold two or more notes, the
fit (algorithm version 4) also splits them into one recording per note, from all its accepted
modes (before the 64-mode limit), so each note keeps up to 64 modes of its own:

1. **Notes.** Every accepted mode from 27.5 Hz to 4.2 kHz is a candidate fundamental `f0`. A
   candidate's partial `k` (1..32) lies at `k * f0 * sqrt((1 + B k^2) / (1 + B))`, with the
   inharmonicity `B` from 0, 1e-4, 3e-4 and 1e-3 that scores best; a mode within 1 % of one is
   that partial (the nearest when two are). A candidate needs its fundamental and at least three
   partials among the modes. Its score is the sum over its partials of `min(left, cap) / k`,
   where `left` is what the notes found so far left of that mode's magnitude and `cap` is twice
   the candidate's own fundamental: a weak note an octave or two below cannot claim the chord's
   strong partials, and the `1 / k` weight favours the note whose fundamental a partial is. The
   best candidate becomes a note and takes `min(left, cap)` from each of its partials, leaving
   the rest for a note that shares them (an octave above, say); this repeats while the best
   score is at least a tenth of the first note's, up to eight notes, skipping a candidate whose
   nearest key is already a note's.
   Once the notes are known, each adds its own string series as candidates, like the reference
   note's (**Partials along the string**: its inharmonicity fitted to the candidates, partials
   2..128 below the analysis ceiling), except that a predicted partial takes the local peak
   nearest it within 1 % that is not within the candidate separation of a candidate, since
   partials of different notes lie close together; candidates stop at 384 in all. These are
   measured and fitted like any candidate, and the notes are found again from all the modes. On
   a recorded piano chord this gave its C3 51 modes where it had 14.
2. **Modes.** A mode one note predicts (within 1 %) goes to that note. A mode several notes
   predict, as an octave's partials coincide, is shared out among them equally, each part with
   the mode's decays and phase, so the notes played together give it back whole and a note
   played alone keeps its share. A mode no note predicts (a soundboard or body resonance, or an
   upper partial stretched beyond the tolerance) goes to the note whose partial series lies
   nearest it: the relative distance to that note's nearest partial (1..32) plus 0.0002 per
   partial number. A mode below every note's fundamental goes to the lowest note.

3. **Recordings.** A note's recording has the key nearest its fundamental, ratios against that
   key's frequency, its modes with their decays, fast parts, beats, phases and stereo placements
   (at most the 64 with most energy), and a level and attack normalized like any recording. Its
   attack is the residual against all the notes' kept modes divided by the note count: the notes'
   attacks are the same waveform, so played together they add up to the recorded attack. A note has
   no held layer.

With fewer than two notes nothing is split. **Accept as notes** (`instrument.acceptFit` with
`notes`) adopts the notes as a new set, one recording per key: playing a note of the chord plays
that string's partials alone, and keys between the notes interpolate as in any set.

## Fitting a folder

A sampled instrument usually comes as a folder with one note per file, often at several
velocities. `instrument.fitFolder` takes an Instrument and a folder `path` and starts one job
on the shared preparation worker (the same two-worker admission budget, cancellation and target
checks as a single fit):

1. It lists the folder's audio files (not its subfolders), sorted by name. A folder without
   audio files, or with more than 128, is refused before any work starts.
2. For each file, on the worker: decode it; if it has more than two channels or no frames it is
   skipped with that reason. Root analysis over the first eight seconds proposes its key: a
   stable single-note proposal, or else the file is skipped as having no stable pitch (a folder
   of loops is not an instrument). The file is then fitted exactly as a single sample with that
   key as its reference, keeping the fitted recording; a file without a fit is skipped with the
   fit's reason.
3. Files fitted at the same key are **velocity layers**: ordered by their level, quietest first,
   they stand for velocities `(i + 1) / n`; a key with one file stands for 0.8.
4. At most 32 recordings fit a set. With more, an evenly spaced selection of the recordings
   ordered by key and velocity is kept (`round(i * (n - 1) / 31)` for `i` = 0..31) and the rest are
   reported as left out for capacity.

Recordings of one instrument are not always in one folder. `instrument.fitFiles` takes a list
of audio files (`paths`, at most 128, from anywhere) instead of a folder and runs the same job
on them, sorted by file name; a path that is not an audio file refuses the command before any
work starts. Dropping two or more audio files at once on an Instrument's sample waveform runs
it: the drop reads "Fit N samples into one model", and nothing is loaded as the sample.

The job's report lists every file with its key, velocity and outcome (kept, left out, or the
reason it was skipped), and the recordings kept. Nothing changes until the person accepts:
`instrument.acceptFit` with a folder job adopts all its recordings as a new set (as for one
recording, with the model controls reset); `instrument.addFit` adds them all to the device's
existing set (all or none: duplicates of a key, velocity and source already there, or more than
32 in all, refuse the whole addition), with an optional `source` or `newSource` for all of them. The folder job
is stale once the device is removed or its fitted model changes; the device's own sample, trim
and root do not concern it. The Sample page offers **Fit folder…**, which asks for the folder
with the system's dialog.

## Audition and comparison

Before accepting, the person can listen to the fit against its source. `instrument.auditionFit`
takes the ready (or just accepted) fit's job and `which` (`original` or `fitted`) and plays, through
the existing preview player at the preview level:

- **original**: the analyzed range of the measured channel, as recorded;
- **fitted**: the fitted recording rendered by the native Instrument (revision 4, a one-recording
  set, Attack 0, Sustain 1, Gain 0 dB) at the source's sample rate, its key and velocity held for
  the same duration.

Both are compared before playing, and the fitted render is played at the original's RMS level so
the comparison is of timbre, not loudness (a level-matched A/B). The command reports, and the
Sample page shows, the level difference before matching and a **spectral distance**: average power
spectra (2048-sample Hann windows, hop 1024, at most eight seconds) summed into octave bands
centered on 63 Hz..16 kHz below Nyquist; for each band within 60 dB of the original's loudest,
the absolute difference in dB after level matching; and their mean. Smaller is closer. These are
measurements of the same recording, not a realism or listening score. Without an audio output the
command still reports the comparison and says it played nothing. Auditioning changes neither the
project nor its undo history.

## Shared jobs and commands

`instrument.fitSample` accepts the same track and full device identity as root analysis, but
requires Instrument. It shares the session's single retained preparation job and process-wide
two-worker admission budget. `instrument.analysis` reports job kind, settings/version, progress,
result and limits; `instrument.cancelAnalysis` cancels either kind. `instrument.acceptFit`
requires the exact current ready job and uses the shared profile-adoption helper. A removed or
replaced device, changed source/PCM/trim/root or newly adopted profile invalidates the proposal.
Failed edits do not consume it. Cancellation is checked between bounded scanning chunks,
resampling chunks, frequency refinements and mode measurements; no UI join or audio-thread work.

The canonical fixed-capacity model, validated codec, revision routing and native render path
belong to the modal-profile prerequisite. The fitter creates that data; it has no second
renderer or state format. Source buffers are borrowed through immutable handles. Working
buffers and candidate counts have fixed caps independent of the complete source duration.
The FFT dependency is the already-used, permitted RustFFT version in this workspace; preparation
alone invokes it. Ordinary use is offline.

## Validation and remaining work

Deterministic CI fixtures use original analytic inharmonic decays with known frequencies and
T60, not the fitter's own synthesized output as the sole oracle. They cover hostile inputs,
silence, growing and noisy sources, sustained (constant) tones, repeatability, opposite-polarity stereo, cancellation
and temporal estimation limits. Session tests cover explicit/stale adoption, transaction
rollback, undo/redo, save/reopen and native render consistency. Builds and test execution for
this change occur in CI only at the maintainer's request.

This is the bounded first fitting slice tracked by #1646, extended in #1599 with attack
residuals, two-part decays, beating partners and parameter-space fusion of up to 32 recordings
by key and velocity. Full fitting #1624 retains feature locks beyond root/trim, morphing
controls, room and microphone separation, independent held-out notes and gestures, level-matched
candidate audition before acceptance and musician evidence.
F11's calibrated-model, content and release gates remain open.

## Sources

- Lagrange and Scherrer, [Two-Step Modal Identification for Increased Resolution Analysis of
  Percussive Sounds](https://www.dafx.de/paper-archive/2008/papers/dafx08_21.pdf), DAFx 2008,
  sections 2–5: damped mode parameters, Fourier candidates, temporal decay fitting and limits
  of unresolved modes. Suhina implements its own bounded Fourier approximation, not their
  ARMA/ESPRIT second stage.
- Skare and Abel, [Rapid Approximation of Weakly-Nonlinear Mode Parameters](https://aes.org/publications/elibrary/elibrary-page/?id=20974),
  AES 2020, public abstract: separated-window estimates trade precision for speed. No private
  paper, code or recordings are used.
- [RustFFT documentation](https://docs.rs/rustfft/6.4.1/rustfft/): fixed-size planning, explicit
  scratch and unnormalized Fourier output.
- Suhina's shared modal bank, resampler, sample preparation jobs and profile codec; original
  Rust implementation and analytic test signals.
