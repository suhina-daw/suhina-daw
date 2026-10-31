---
description: How a device stored in a Live Set is brought into Suhina: the three import levels, how each Suhina device is learnt and mapped, and which Live device maps to which Suhina device
read_when: device import, importing devices, importing MIDI effects, device mapping, placeholder device, Operator, Live device to Suhina device, which device imports into what, golden renders of imported sets
type: spec
status: current
last_verified: 2026-10-28
covers:
  - crates/als/src/devices.rs
---

# Importing devices from Live Sets

Live's own devices in a Live Set (see [`als-format.md`](als-format.md) › Devices) come into
Suhina at one of three levels: a placeholder that keeps the device's type, name and on/off state
(keeping its parameters is planned); then an approximate mapping to an open-source plug-in over
CLAP; then a Suhina device for the most used ones. After import, the user sees which devices did
not come in: the import reports each placeholder ("imported as a placeholder") and the device
chain shows it as "Not supported". Showing which devices came in approximately is planned, with
that level. No content from Live's library (presets, wavetables, samples, impulse responses) is
used.

- **Observed with:** Live 12.4.6, as in [`als-format.md`](als-format.md).
- **Test files:** the device series of the corpus ([`als-corpus.md`](als-corpus.md)): series 3
  (devices, plug-ins) and series 6 (Utility, Auto Pan-Tremolo, EQ Three, Vinyl Distortion,
  Overdrive). The Utility files are imported and checked; the Delay and Reverb are checked
  against the behaviour-run sets of the device notes (their specs, Known differences). The MIDI
  effects were learnt from a parameter dump, a saved set and a calibration set instead
  ([`als-format.md`](als-format.md) › MIDI effects).

The Suhina devices themselves, what they do and how far they are, are in
[`ROADMAP.md`](../../ROADMAP.md) › Built-in effects and instruments. This document is the format side: which
stored device lands in which Suhina device.

## How a device gets its import

A Suhina device that takes a Live device's parameters is built this way:

1. **Learn what the device does, not how it is coded.** From the vendor's published manual, from
   listening, and from Live Sets we author ourselves, where we change one parameter at a time and
   record in `docs/spec/` how it is stored (name, range, unit, curve). No binaries are opened.
2. **Design our own device** that covers those features, with its own name, parameter layout
   and look. It does not have to match one to one; it has to be able to take the imported
   parameters.
3. **Build the DSP** in `dsp` and the device crate, porting from open-source code where it helps.
   The licence is checked before the first line is ported, and every ported file keeps its
   notices.
4. **Write the import mapping** in `crates/als`: each stored parameter maps to ours, with a
   conversion where the units or curves differ. Parameters with no counterpart are to be kept
   and listed, so a later version can use them (planned; the Utility mapping drops them today).
5. **Test** with golden renders of the device alone and of imported sets, compared against the
   bounces we made in Live when authoring the sets. The goal is close, not identical.

### Example: the FM synth that Live's Operator imports into

- From the manual: four oscillators (operators), each with its own envelope, frequency ratio and
  level; a set of algorithms that connect them; feedback; a filter, an LFO and a pitch envelope.
- Our device: an FM synth whose operator, algorithm and envelope model is a superset of that, so
  every Operator patch has a place to land. Its algorithms are our own table, with the ones
  Operator needs among them.
- DSP: operators, envelopes and feedback ported from MSFA (Apache-2.0), the engine inside Dexed;
  filters and LFOs from our own `dsp` crate.
- Import: a corpus series of Operator patches we make ourselves (one per algorithm, envelope
  shapes, ratios, feedback), each saved as a set and bounced, so the mapping is tested against
  real sound. Until the device exists, Operator comes in as a placeholder that keeps its type,
  name and on/off state; its parameters are not kept yet (planned).

## Which device imports into what

Sources are candidates; each licence is checked when the work starts. **DSP done** means the
Suhina device exists in `devices/` and passes its tests; **import** is whether a Live Set's device
comes in as it (otherwise as a placeholder):

| Imports from Live | Suhina device | DSP from | Status |
|---|---|---|---|
| Utility, Auto Pan | utility, auto-pan | own | DSP done; import done for Utility, planned for Auto Pan |
| Pitch (MIDI effect) | pitch ([spec](pitch.md)) | own | done; import done (the step widths wait for the step buttons) |
| Arpeggiator, CC Control, Chord, Note Length, Random, Scale, Velocity (MIDI effects) | not yet | | placeholder: notes pass on to the instrument unchanged. How each is stored is written down ([`als-format.md`](als-format.md) › MIDI effects), so each import can be written as its device lands |
| EQ Three | dj-eq ([spec](dj-eq.md)) | own crossovers (Linkwitz-Riley) | DSP done; import done |
| Vinyl Distortion | vinyl distortion | own, oversampled | DSP done; import planned |
| Overdrive | overdrive | own, oversampled | DSP done; import planned |
| Compressor | compressor: stereo-linked, soft knee, side-chain high-pass, dry/wet | own | DSP done; import planned |
| Glue Compressor | bus compressor | own, Airwindows (MIT) for reference | planned |
| Limiter | limiter: brickwall, 1.5 ms lookahead | own | DSP done; import planned |
| Multiband Dynamics | multiband dynamics, upward and downward | own crossovers and detectors | planned |
| Delay | delay ([spec](delay.md)) | own delay lines | DSP done; import done |
| Echo | modulation delay | own delay lines | planned |
| Reverb | reverb ([spec](reverb.md)) | own | DSP done; import done |
| Hybrid Reverb | convolution and algorithmic reverb | own convolution, the reverb | planned |
| Saturator | saturator ([spec](saturator.md)) | own, oversampled | DSP done; import done |
| Roar | multi-stage saturation | own, oversampled | planned |
| EQ Eight | EQ ([spec](eq.md)) | own (`dsp` state-variable filters) | DSP done; import done (no analyser, Audition or oversampling yet) |
| Auto Filter | filter | own (`dsp` filters) | planned |
| Operator | FM synth | MSFA (Apache-2.0) via Dexed | DX7-style engine done; Operator's algorithms, waveforms and filter, and import, planned |
| Wavetable | wavetable synth | Vital (GPL-3.0), under its own name | planned |
| Drift, Analog | subtractive synth | own, Surge XT (GPL-3.0) for reference | planned |
| Simpler, Sampler, Drum Rack | sampler, drum sampler | own | planned |
| Collision, Tension, Electric | physical-modelling synth | Mutable Instruments Rings and Elements (MIT) | planned |

## Devices Suhina cannot run

Max for Live devices are planned to come in as placeholders that keep their parameters, with the
device's own files copied along with the project, so a set opened in Suhina and saved again still
has them.
