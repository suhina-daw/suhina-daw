---
description: Why the FM synth grows in place from its DX7-style engine into a full FM instrument instead of becoming a second device, how DX7 voices and old projects keep their sound, why two envelope models live side by side, and why a theme changes only the panel's styles
read_when: FM synth, fm-synth, full FM instrument, DX7 compatibility, two envelope models, time envelopes, FM waveforms, FM filter, FM panel, theme limits on a built-in panel, ported engine changes
type: decision
status: accepted
built: yes
last_verified: 2026-11-02
covers:
  - devices/fm-synth/**
---

# 0034: The FM synth grows in place

- **Date:** 2026-11-02
- **Issues:** [#1139](https://github.com/suhina-daw/suhina-daw/issues/1139), [#390](https://github.com/suhina-daw/suhina-daw/issues/390)

## Context

`devices/fm-synth` is a port of MSFA: six sine operators, 32 algorithms, envelopes of four rates
and four levels, one LFO, a mono output and 173 parameters in a generated list. It reads DX7 voice
files and matches the original engine to within 1e-6 in its golden renders. Musicians expect more of
an FM instrument: waveforms, a filter, envelopes in seconds that can loop, a stereo spread, a
modulation matrix and a panel that shows a sound at a glance. A second device was considered and
rejected: two FM instruments would share most of their work and leave users choosing between them.

## Decision

1. **One device.** The id `fm-synth` stays and the crate grows. The new panel replaces the generated
   list. DX7 is a file format the device reads, not a mode a user selects.
2. **Old sound stays exact.** Every new part is neutral by default and the sine path without
   per-sample modulation runs the ported kernels unchanged, so the golden renders do not change.
   Loading a voice resets the new parts, so a voice sounds as its file says.
3. **Parameters are added after the existing ones.** Existing ids and indices stay. A project lists
   values by id, so an old project gets the neutral defaults for the rest.
4. **`Patch` stays the DX7 voice.** New settings live in Suhina's own structure beside it. Voice
   files limit the algorithm to 1..=32; the parameter also reaches the new algorithms.
5. **Two envelope models.** Each operator selects the DX7 rates and levels or an envelope in time.
   The second cannot be expressed in the first: rates do not convert to seconds exactly, and loop and
   sync modes have no place in it. Both produce the level the kernels read.
6. **Ported files stay ported.** Changes to them are recorded in their notices; new files are
   GPL-3.0-or-later. Shared DSP goes in `crates/dsp` (AGENTS.md rule 8).
7. **Harmonics are parameters.** A drawn waveform is 64 parameters per operator, hidden from
   generated lists, so undo, automation, MCP and project files need nothing new.
8. **A theme changes styles only.** The panel takes its colours from the palette and the `--device-*`
   tokens and its fonts from the theme. Its canvases, control positions, pages and display contents
   are fixed: with this many controls a changed layout would stop being usable, and
   [ADR 0021](0021-device-graph-files.md) keeps panels at a fixed size anyway. The panel holds no
   fixed colours, and a test checks that.

## Consequences

- The parameter count grows by several hundred, most of them harmonics. Until parameters can be
  marked as not automatable (#669) they appear in automation choices.
- The canvas limit of 128 controls is too small for per-operator controls and partials. A `.sd` file
  keeps that limit, as it comes from anywhere; a built-in's layout ships with Suhina and has a larger
  one, and controls for other operators are hidden with `visible_when`.
- Spread doubles the work of a voice while it is on; 32 voices with spread is the case to measure.
- Sample-accurate modulation runs a separate kernel path, so the ported path stays untouched.
