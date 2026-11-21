---
description: Bounded local analysis of an imported instrument sample, explicit root-note proposals, cancellation and checked adoption
read_when: instrument sample analysis, detecting a root note, analysis jobs, sample proposals, cancelling analysis, adopting a proposal
type: spec
status: planned
last_verified: 2026-11-20
covers:
  - crates/dsp/src/pitch.rs
  - crates/instrument-analysis/
  - crates/soundmap/src/features.rs
  - crates/session/src/instrument_analysis.rs
  - crates/session/src/commands/instrument.rs
  - crates/ui/src/detail/
---

# Instrument sample analysis

An imported sample remains playable while an explicit background analysis estimates its root
note. Completing analysis never edits the instrument. The person inspects the result, may
correct the suggested integer root, and accepts it in one undoable command. Reopening uses the
saved source and root; neither playback nor reopening repeats analysis. This is a single-source
pitch proposal, not physical-model fitting, instrument recognition or a multisample map.

## Analysis and limits

The host-independent Rust analysis library accepts decoded interleaved PCM and a checked frame
range. It has no project, browser, file picker, network or window dependency. The session passes
the immutable audio already loaded for the selected instrument, so analysis grants no new file
access and never rereads a path behind the command's authorization checks.
The dependency boundary and host responsibilities are proposed in
[ADR 0041](../decisions/0041-instrument-sample-analysis.md).

Analysis version 2 accepts mono/stereo audio at 1,000..384,000 Hz. It examines at most the first
eight seconds of the selected trim and at most 32 evenly distributed analysis windows per
channel. The report names the actual analyzed range; material beyond it is not assessed.
Preparation uses the shared antialiasing resampler at 22,050 Hz, removes each analysis window's
DC offset and normalizes finite samples before pitch estimation. Nonfinite samples within the
analyzed range, malformed interleaving and invalid ranges are errors. The bounded scratch
budget excludes immutable source PCM already owned by playback.

The shared YIN frame estimator comes from the existing sample map implementation. Its sample
map settings and arithmetic remain unchanged. Instrument analysis uses its own documented
window/range settings and conservative stability checks; a YIN periodicity measure and the
share of voiced windows are measurements, not probabilities that a root note is correct.

A report distinguishes silence, insufficient duration, no stable pitch and a stable proposal.
It includes fractional MIDI pitch, Hz, proposed integer MIDI root, remaining cents offset,
periodicity, pitch spread, voiced/active window counts, analysis version and analyzed bounds.
The initial supported pitch-search range is 40..4,000 Hz, also limited by the source Nyquist
frequency. Results outside it are not clamped to a boundary note. A failed or uncertain estimate
never fabricates a middle-C proposal. A stable proposal needs three quarters of the voiced
windows within 50 cents of their median: vibrato swinging around one note (a cello's, a voice's)
keeps its note, while a melody, trill or glide does not (version 1 demanded an interdecile spread
of at most 35 cents, which ordinary vibrato exceeds). The spread is still reported. Polyphonic,
strongly inharmonic, noisy and changing-pitch material may have no useful single root. Stereo channels are assessed separately, so reversed
polarity cannot cancel a valid tone; inconsistent channel pitches prevent automatic proposal.
Windows with a zero-crossing rate above the admitted band are conservatively rejected rather
than assigning an out-of-band tone a lower octave; this can also reject bright mixtures.
Periodicity does not establish that a recording is monophonic: harmonically related notes can
have the same waveform as a single source with a missing fundamental. Version 1 does not screen
polyphony. The command report names `sourceAssumption: singleIsolatedPitchedNote` and
`polyphonyAssessed: false`; the ready UI always warns about ambiguous chords. Conservative
mixture screening and its evaluation remain a separate follow-up, not a delivered capability.
That remaining acceptance point is tracked in
[Follow-up #1639](https://github.com/suhina-daw/suhina-daw/issues/1639).

Accepting an integer root does not remove the displayed residual cents offset. The person may
choose a different root, but that choice is explicit and is not presented as a measured result.

### Loops and melodic material

A loop or phrase with several notes has no single pitch, so the note path above withholds its
proposal. Analysis version 2 then proposes the material's **tonal center** as the root: the
tonic of its estimated key, so playing the loop from another key transposes it musically. This
proposal is only made when no stable single note was found and the analyzed range is active.

- **Pitch-class profile.** Each channel's prepared 22050 Hz signal is cut into consecutive,
  non-overlapping 8192-sample Hann windows (at most 21 per channel within the eight-second
  bound; every channel needs at least two, about 0.75 s). In each window, spectral peaks between
  55 Hz and 4000 Hz are located with parabolic interpolation on log magnitude. A peak counts
  when it is within 40 dB of the window's strongest and stands 20 dB above the median of its
  neighbours 3..32 bins away, which unpitched noise does not do. Of the 64 strongest, a peak
  within 25 cents of 2..8 times a lower kept peak is that peak's harmonic and is left out, so
  every note does not also add its fifth and third. The kept peaks' magnitude-weighted circular
  mean deviation from equal temperament is the tuning offset; each kept peak's magnitude then
  adds to the pitch class it lies nearest to after that offset. Peaks, not a single fundamental,
  are used, so chords and bass lines contribute and stereo polarity cannot cancel anything.
- **Key.** The twelve-value profile is correlated (Pearson) with the 24 rotations of the major
  and minor key profiles published by Krumhansl and Kessler. The best key's tonic is the
  proposal's pitch class. A correlation below 0.6 means the material has no clear tonal center
  (unpitched drums, noise, atonal material) and no proposal is made. The runner-up key and both
  correlations are reported; relative major/minor keys share their pitch classes and often
  differ by little, which the interface shows instead of hiding. Correlations are measurements
  of fit to a profile, not probabilities.
- **Root key.** The root is the tonic's MIDI note nearest middle C (MIDI 60, shown as C3), the
  lower one on a tie. A loop's root only says which key plays it unchanged, so this octave keeps
  the keys around the middle of the keyboard within six semitones of the original speed and
  pitch, whatever the loop's register. The median of each window's lowest prominent pitch (its
  lowest kept peak within 20 dB of its strongest) is reported as the material's register. The
  tuning offset is shown in cents and, as for a single note, is not removed by accepting the
  integer root.

The analyzed range is the selected trim, as for a single note; loop start and end do not
change it. The command report marks such a result with outcome `tonalCenter` and a `key`
object (tonic, mode, correlation, runner-up, tuning cents, root). Acceptance, correction and
undo work exactly as for a single-note proposal. A key estimate from one short loop can be
wrong, for example between relative keys or when the loop dwells on its dominant; the person
listens and corrects it before accepting.

## Session jobs and commands

The command path is shared by desktop, CLI and MCP:

- `instrument.analyzeSample` starts a bounded job for a named device and returns its job id.
- `instrument.analysis` reads that device's latest job/proposal and progress.
- `instrument.cancelAnalysis` cooperatively cancels a named job without waiting on the UI thread.
- `instrument.acceptAnalysis` accepts a completed, current proposal and optionally a corrected
  MIDI root. It changes the existing root parameter and overrides its enabled automation lane
  through the normal manual-edit contract, preserving lane points, in one undo/redo step.

Starting, reading and cancelling analysis do not change project state or undo history. There is
at most one retained job/proposal per session. Worker admission is bounded across sessions;
cancelled but still exiting work counts against the limit. No command or window joins a running
worker. Cancellation is checked during preparation and between bounded estimator work units.
No wall-clock deadline determines correctness, and background work never runs in the audio
callback. Tests can wait on an explicit completion signal.

Each job retains the session-owned target identity, device id, immutable source identity, source
reference, trim and original root. Moving within its channel keeps the device's stable identity; removal, replacement,
source/trim/root changes or a different job make a result stale. Acceptance revalidates all these
preconditions before changing anything. A stale, cancelled, incomplete or failed job cannot edit
the project or consume an undo step. Completion messages never target another open project.

## Preview and interface

The already imported sample is the playable preview. Analysis shows what was measured without
silently retuning held or newly played notes. The sample controls expose Analyze, progress,
Cancel, the proposed root and cents, a manual root correction and Accept when a current proposal
exists. Keys are shown as note names (60 is C3); the correction also accepts a typed note name
(`A2`, `C#3`, `Db3`) or a MIDI number. Silence, unstable pitch, unsupported input and stale results have distinct explanatory
messages. An analysis action is a command control, never a fake automatable sound parameter.

The expanded editor's Sample page places these controls through the SDK `SampleAnalysis`
widget. Compact controls and the expanded editor use the same instrument state. Pages and
window state are not instrument parameters. Sample and Hybrid playback remain available while
analysis runs. Exported editors still need their own host preparation integration.

## Verification

Self-authored PCM fixtures cover known tones, harmonic spectra, amplitude/DC changes, supported
rates, silence, short data, noise, changing pitch, malformed samples, stereo polarity and channel
disagreement. Tests cover root rounding and the reported residual separately. Existing sample map
feature tests and an extraction parity fixture protect its outputs/cache meaning.

Deterministic worker tests use explicit channels/signals for cancellation and late completion.
Command tests cover nested/stable device identities, replacement/removal, source/trim/root edits,
job ids, failed acceptance, root automation, one-step undo/redo and save/reopen. Fixed renders
verify the accepted root changes transposition as expected without changing the source audio.
UI tests exercise actual command controls, availability and stale/error messages. No test sleeps
or asserts an elapsed-time bound.

## Not yet

Folder/multisample import and zone maps remain with their coordinated workstream. This slice
does not fit a resonator, infer geometry/articulation/pedals, create portable content packages,
separate sources, adjust sample tuning automatically or publish/import training data.

## Sources

- Own existing `suhina-soundmap` pitch estimator and sample/command/undo implementation.
- Carol L. Krumhansl and Edward J. Kessler, *Tracing the dynamic changes in perceived tonal
  organization in a spatial representation of musical keys*, Psychological Review 89(4),
  334-368 (1982), [DOI](https://doi.org/10.1037/0033-295X.89.4.334): the published major and
  minor key profile values; Carol L. Krumhansl, *Cognitive Foundations of Musical Pitch*
  (Oxford University Press, 1990) describes correlating a pitch-class distribution with them.
- Takuya Fujishima, *Realtime chord recognition of musical sound: a system using Common Lisp
  Music*, ICMC 1999: the pitch-class profile of a spectrum. No implementation is copied.
- Alain de Cheveigne and Hideki Kawahara, *YIN, a fundamental frequency estimator for speech
  and music* (2002), [paper](https://www.ee.columbia.edu/~dpwe/e6820/papers/deChevK02-yin.pdf),
  [DOI](https://doi.org/10.1121/1.1458024). The algorithm informs the original Rust estimator;
  no external implementation is copied.
- [Instrument epic #1579](https://github.com/suhina-daw/suhina-daw/issues/1579),
  [local analysis #1596](https://github.com/suhina-daw/suhina-daw/issues/1596).
