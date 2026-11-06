---
description: Scope comparison between plugdata's documented object ecosystem and Suhina's executable data-only SDK nodes
read_when: plugdata, Pure Data parity, device SDK node coverage, patch devices
type: system
status: current
last_verified: 2026-10-22
covers:
  - crates/device-sdk/src/nodes.rs
  - crates/device-sdk/src/runtime.rs
  - crates/device-sdk/src/midi.rs
  - crates/device-sdk/src/ui.rs
  - crates/device-sdk/src/editor.rs
  - docs/spec/sdk-nodes.md
  - docs/decisions/0008-patch-devices.md
---

# plugdata and the device SDK: scope comparison

This is a capability comparison, not a claim that the two systems execute the same patches.
plugdata is a graphical Pure Data environment with vanilla objects and bundled libraries; the
Suhina SDK accepts a validated, data-only graph with a deliberately smaller set of operations.
Similar names do not imply matching signal, message, timing, or parameter semantics.

## Sources and method

- plugdata's [reference data](https://plugdata.org/assets/reference-data.json) is the upstream
  object-catalog source. Its [documentation sources](https://github.com/plugdata-team/plugdata/tree/develop/Resources/Documentation)
  and [published documentation](https://plugdata.org/documentation.html) provide context for
  Pd concepts, bundled libraries, and host integration.
- Suhina coverage is taken from the executable node table in [the SDK node reference](../spec/sdk-nodes.md)
  and `crates/device-sdk/src/nodes.rs`, checked on 2026-10-15.
- The complete JSON was fetched and enumerated on 2026-10-15: 1,653 object keys. The origin
  counts were vanilla 292, ELSE 553, cyclone 223, Gem 528, heavylib 29, MERDA 21, pdlua 2, and
  5 without an origin. Categories overlap, so their counts below are not additive.
- Statuses describe capability families, not one-to-one object equivalence. plugdata's catalog
  changes with its branch and bundled libraries; consult its live reference for exact names and
  availability. No upstream descriptions or object records are reproduced here.

## Capability map

| plugdata / Pd capability family (reference records) | Suhina SDK coverage | Status and boundary |
|---|---|---|
| Audio I/O (22), mixing/routing (84) | `audio_input`, `audio_output`, `stereo`, `mono`, `mid_side_encode`, `mid_side_decode`, `pan` | **Partial.** Endpoints and basic channel operations exist; Pd graph/canvas and multichannel conventions do not. |
| Signal math (116), data math (109), converters (69), logic (36) | Arithmetic, comparisons, signal shaping, selection, clamp, mix, and dB conversions in the node table | **Partial.** SDK composition covers common signal math; exact Pd message behavior, operators and edge cases are not interchangeable. |
| Signal generators (105), random/noise (48), envelopes/LFOs (38) | `oscillator`, `synced_lfo`, `noise`, `smooth`, `sample_hold`, `envelope` | **Partial.** Basic generators and controls exist; no general Pd scheduler or matching object timing contract. |
| Effects (103), filters (64), analysis (66) | `filter`, `delay`, `sample_delay`, `envelope` | **Partial / mostly absent.** Biquad and bounded delay exist; FFT, convolution, oversampled nonlinear processing, broad analysis meters and arbitrary externals do not. |
| MIDI (74), triggers/clocks (68), tuning (20) | MIDI note input/output/transpose, key/velocity/gate signals, `tempo`, `beat`, `playing` | **Partial.** Limited timestamped note events and transport signals; no general Pd message/list routing, broad MIDI utility set, or tuning system. |
| Data management (167), arrays/tables (39), buffers (33), file management (28), data structures (18) | None as general-purpose Pd objects | **Not covered.** SDK graphs have no Pd arrays, named buffers, file-writing objects, or mutable data-structure runtime. Audio delay memory is a bounded DSP primitive, not a general buffer API. |
| UI (88), graphics (528) | SDK parameters and custom data-driven device UI | **Different scope.** Device panels are not a Pd canvas, GUI-object library, or Gem graphics engine. |
| Networking (14), audio process/subprocess objects, plugdata host objects | None as SDK nodes | **Not covered.** No network, subprocess, host transport, plugdata `[param]` or `[playhead]` contract in the SDK graph. |
| Other bundled libraries (ELSE 553, cyclone 223, Gem 528, heavylib 29, MERDA 21, pdlua 2; some overlap the categories above) | No external-object loading | **Not covered as libraries.** These are upstream catalog origin totals, not counts of unique missing DSP features; an SDK graph can only use implemented node kinds. |

## What the comparison says

The SDK's version-3 format stores node positions, explicit port anchors and wire bend
points with validated references to its processing graph. SDK round-trip, malformed-input and
audio-invariance checks live in `crates/device-sdk/tests/editor.rs`; project persistence is
covered in `crates/session/tests/devices.rs`. This is authoring-data support, not an interactive
patch editor or Pd patch compatibility. Component hierarchy, named layers and editable
envelope documents are still planned.

This audit enumerated every record in the supplied reference JSON; the table groups the gap by
the catalog's own categories, including non-DSP facilities so they are not mistaken for missing
effect nodes. It does not claim that each of the 1,653 records was individually ported or has a
semantically equivalent Suhina node. The SDK already covers a useful compositional subset for self-contained effects and simple
instruments: signal math, modulation, filtering, bounded delay, stereo utilities, parameters and
basic MIDI note handling. A polyphonic `sampler` source can feed these signal nodes with
PCM bound by Rust authors off the audio thread; typed sample-resource ports and smaller
playback/granular building blocks are supported within the bounded native graph runtime. Missing breadth is primarily the Pd execution model and ecosystem:
message/list semantics, arbitrary objects and externals, patch editing, and host-specific
objects. Adding a similarly named node alone would not establish compatibility; each addition
needs a clean-room behavior spec, explicit port and timing rules, validation, and runtime tests.

This comparison is separate from the proposed direct Pure Data patch-device direction in
[ADR 0008](../decisions/0008-patch-devices.md). Loading Pd patches would be a distinct execution
path, not an automatic extension of the data-only SDK.
