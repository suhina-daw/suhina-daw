---
description: Experimental Instrument modal core, local single-source import and reusable native runtime
read_when: Instrument, modal synthesis, sample-to-instrument, comprehensive instrument
type: spec
status: planned
last_verified: 2026-11-20
covers:
  - devices/instrument/**
  - crates/dsp/src/modal.rs
---

# Instrument

Instrument is an experimental native instrument. Its public Rust runtime is shared by stock
and future export adapters. The stock panel uses the device SDK. It has no network operation,
training service, proprietary library dependency or background fitting requirement.

## Sound and controls

Source selects Modal, Sample or Hybrid. Modal is **Struck modal v1**, an original mathematical
prototype, not a calibrated piano, string or orchestral instrument. Sixteen damped modes per
voice describe a struck resonant body. Sixteen voices bound its work and memory. A key sets the
fundamental in equal temperament; velocity sets excitation amplitude. Brightness shapes the
mode amplitudes, Structure stretches their frequency ratios, Strike position changes the
excitation pattern and Decay sets the fundamental's time to lose 60 dB. High modes decay faster.
Quality selects 8 or 16 modes. Modes at or above 0.45 times the sample rate are omitted, not
folded into the audible band.
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

## Import and project state

`device.loadSample` accepts Instrument and loads an ordinary local audio file. The command is
one undoable project edit and selects Sample, so the next note plays the imported source.
Like manually selecting Source, import overrides an enabled Source automation lane while keeping
its points. Undo restores the previous source reference, Source value and lane state together;
redo restores the complete import.
Root defaults to MIDI 60; the user sets it when the file was recorded at another pitch. Pitch
detection, automatic segmentation and fitting are not silently guessed. Source bytes use the
existing decoded-audio and sample-reference lifecycle: project save rebases the path, keeps
an absolute fallback, and the existing collect/missing-file/relink workflow applies.

All sound controls are stored as stable native parameter IDs. `model_revision` persists 1 as
metadata using the existing native parameter state, hidden from the stock panel. Unknown
revisions retain their value and silence the modal body rather than running another revision.
This temporary metadata parameter is not a sound control; export adapters must preserve it in
state without exposing it as an automatable control. Future revisions must preserve its rendering
contract or provide a migration and retained renderer. The sample reference is `suhina-sample-v1`,
independent of the model revision. Opening a project decodes its local source and restores
controls; it never refits. Portable plug-in assets are not implemented by this source-reference
contract, which currently belongs to the stock project's model layer.

## Bounds and quality gates

The modal core has at most 256 active modes and no dynamic storage. The sampler is configured
for at most 16 voices; Hybrid can therefore run 16 modal and 16 sample voices together. Sample
rendering reuses the shared runtime and its documented source limits. Preparation, decoding and graph
replacement happen away from the audio callback. Rendering, note events, parameter changes
and reset perform no allocation, deallocation, locking, logging or I/O.
The final mix saturates only at the finite `f32` limits, including when positive output gain
would otherwise overflow an extreme imported float sample. This is not an audio limiter.

Deterministic tests cover tuning/decay, bounded invalid inputs, note release/pedal/reset,
identical output across block partitions, sample rendering and project reopen. A release-mode
benchmark reports fixed rendered-frame workloads rather than making CI pass on wall-clock
thresholds. The initial development reference machine is a Ryzen 5 5600X; this is a measurement
context, not a claim of a measured real-time polyphony budget. The provisional acceptance target
is at most 20% of one core on average at 48 kHz / 64 frames with all 16 voices, including the
dense-retrigger workload, and a 99th-percentile block shorter than one block deadline. Modal
voice state is bounded below 2 KiB per voice; the whole inline runtime target is 256 KiB, excluding
shared source PCM and the SDK's prepared stretch buffers. Report those exclusions with the
measurement rather than calling them free. `cargo run --release -p suhina-device-instrument
--example benchmark` measures Modal, Sample and Hybrid at 64/256 frames, with pitch movement
every block and a separate 16-retriggers-per-block workload. Listening and calibrated acoustic
quality remain separate release gates.

## Not yet

The complete plan remains in #1579. No family F01-F18 is declared complete by this prototype.
Calibrated piano, bowed/plucked strings, winds, brass, reeds, voices, percussion and the other
planned families still need their own model, independently licensed references, articulation
coverage and listening validation. Folder/multisample import builds on the shared zone work
in #739; model candidates, automatic fitting, provenance and portable preset bundles remain
future work. The broader resonator request #394 also includes excitation and coupling models
that this first struck body does not supply. MPE (#921) and CLAP/VST3 exports (#931) are parallel
implementation workstreams that must use this same core.

## Sources

- Self-authored modal model: each mode is a complex oscillator multiplied by
  `exp(-ln(1000)/(sample_rate*decay_seconds))` per sample; a finite normalized sum is its output.
- Existing shared sample contract: [Sampler](sampler.md).
- Native device architecture: [ADR 0004](../decisions/0004-built-in-devices.md).
