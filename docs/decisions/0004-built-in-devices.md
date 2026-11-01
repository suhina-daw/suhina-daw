---
description: The interface of built-in effects and instruments (Device trait, parameters, real-time rules, contract tests) and the dsp crate
read_when: device, built-in effect or instrument, effect, instrument, dsp, filter, parameter, automation, utility, device-api, conformance
type: decision
status: accepted
built: yes
last_verified: 2026-10-30
covers:
  - crates/dsp/**
  - crates/device-api/**
  - devices/**
  - crates/device-registry/**
  - crates/engine/src/chain.rs
  - crates/engine/src/buses.rs
  - crates/engine/src/sends.rs
  - crates/session/src/devices.rs
---

# 0004: Built-in effects and instruments

- **Date:** 2026-10-11; decisions 8 to 15 added 2026-10-12 to 2026-10-25, the main channel's
  chain 2026-10-25 (#687); written as one decision with its detail in
  [device chains](../spec/device-chains.md) 2026-11-02 (#759)
- **Task:** LOOP-33

## Context

Suhina ships its own instruments and effects ([`ROADMAP.md`](../../ROADMAP.md) › Built-in
effects and instruments): first a utility and an auto-pan, then a DJ EQ, distortion, dynamics, delays, reverbs
and synths. Projects imported from other DAWs refer to those DAWs' devices, which come in as
placeholders today (`DeviceKind::Foreign` in the model) and should later map onto ours.

Built-in effects and instruments run inside the engine (AGENTS.md rule 5 covers third-party plug-ins only), so
they are bound by the same rules as the engine: no allocation, locking or I/O on the audio
thread, and no panics. They also have to work from every front end: each parameter is a command
(rule 6) and can be automated.

At the time of writing, the engine mixed everything into one stereo bus and had no per-track
chains, and making edits reach a playing engine (LOOP-26, LOOP-32) was in progress. Devices were
not to wait for that, and the device code was not to change when it landed; chains landed with
decision 9.

## Decision

1. **Three kinds of crate.** `crates/dsp` (`suhina-dsp`) holds building blocks: gain
   conversions, parameter smoothing, filters, crossovers, LFOs, shapers, DC filtering and
   oversampling, delay lines and allpasses. `crates/device-api` (`suhina-device-api`) holds the
   interface. Each device is its own crate under `devices/` (`suhina-device-<name>`), depending
   on those two and, for the SDK's widgets and runtimes, on `device-sdk` (golden rule 8 asks every
   built-in to use the SDK; the sampler is the first that does). None of them depends on the
   model, the engine or each other, so a device can be tested alone and later built as a CLAP
   plug-in or moved to its own repository. *Amended 2026-11-02 (#759): the rule was `dsp` and
   `device-api` only, which the sampler had already left.*
2. **A device describes itself statically.** `DeviceInfo` gives a stable id (the id stored in the
   model as `DeviceKind::Builtin`), a display name and the parameters in a fixed order. Each
   `ParamInfo` has a stable id, a name, a kind (continuous with a linear or logarithmic scale,
   toggle, or choice), a range, a default and a unit, and may name a group, the part of the
   device it belongs to ("Op 1", "LFO"), which lists of many parameters gather it under.
3. **Parameter values are plain values in their unit** (decibels, hertz, a fraction for a
   percentage, an option index for a choice), not 0..1. A stored value keeps its meaning when a
   range changes, and import mappings convert units directly. `ParamInfo` converts to and from
   the 0..1 travel of a knob or an automation lane, and `clamp` turns any number, NaN included,
   into a valid value. A device keeps its values in a `ParamBank` (`device-api`), which clamps
   each value it stores and ignores an index past the end, so every device's `set_param` and
   `param` follow that rule by one tested path and the device writes only what it does when a
   value changes.
4. **The `Device` trait.** Construction, `prepare(sample_rate)` and drop run off the audio thread
   and may allocate. `reset`, `set_param(index, value)`, `param(index)` and
   `process(context, buffer)` run on it and must be real-time safe and panic-free whatever they
   are given. `process` works in place on interleaved stereo, the engine's format, and gets a
   `ProcessContext` (tempo, song position in beats, whether the transport runs) for devices that
   follow the tempo (`tempo_or_default` gives one between 1 and 1000 BPM, or 120 when the host's
   makes no sense). `latency()` reports added delay for compensation. A device is made prepared
   for `DEFAULT_SAMPLE_RATE` (48 kHz).
5. **Automation: the host splits, the device smooths.** The host addresses parameters by index
   and splits a block where an automation point falls, so a change lands on its frame, and gives
   each part the song position of its own first frame; the device ramps a change that would click (20 ms for the utility). The output must not depend on
   how a stream is divided into blocks.
6. **Every device passes the same contract tests.** `device_api::conformance::check` runs a
   device through hostile parameter values and input (NaN, infinities, extremes), several block
   sizes with parameter changes, `reset` against a fresh device, and every sample rate from
   8 kHz to 384 kHz. An effect or an instrument prepared at other rates and then at 48 kHz again
   must sound as a fresh one does, and at its defaults an effect's impulse must first be heard
   within 4 frames of its `latency()`, at 44.1, 48 and 96 kHz. Each device crate calls it from a
   test (`conformance_test!`), next to its own tests of what it sounds like, and
   `device-registry` runs it on every device it lists, as it makes them, so a crate that leaves
   its own test out is still checked.
7. **Order of work per device** stays as the roadmap says: observed behaviour written down, our
   own design, DSP, import mapping, tests. The import mapping lives in the format crate
   (`crates/als`), never in the device.
8. **Instruments take notes as events, between blocks.** `DeviceInfo::role` says whether a device
   is an `Effect` or an `Instrument`. `Device::event` hands an instrument a `NoteEvent` at the
   frame where the next `process` starts: the host splits a block where an event falls, exactly as
   for automation, so one mechanism times both and the trait needs no event list. Whole-number
   parameters have their own kind, `ParamKind::Integer`. The contract tests add a fixed
   performance and hostile events for instruments.
9. **The engine runs device chains on tracks.** `device-registry` makes a device from its id; a
   track whose channel holds devices gets a chain that its sources play into, with the track's
   pan and volume after the devices. Muted and unsoloed chains keep running at zero gain, a graph
   swap hands a chain's running devices on when the same devices sit on the same track, and the
   main channel runs a chain of its own, last, when it holds devices or follows a lane.
10. **A group with devices is a bus.** The tracks routed into it play into its chain, its devices
    run over the sum, and buses are ordered after everything that feeds them. Latency is
    compensated along every path, from each device's `latency()`.
11. **Automation splits a chain on the arrangement's grid**, at every 64th frame where a value
    moved, so the sound does not depend on the block size (5) and the device smooths each step.
12. **Sends feed return chains**, a return being a bus like a group's, in one topological order
    over outputs and sends; a send that would close a loop is left out.
13. **A device group is one slot of a chain** (`Slot::Group`): branches with buffers of their
    own, summed at their volume and pan and mixed with the dry input, with latency compensated
    inside the group.
14. **Built-in MIDI effects change the events before the instrument** through
    `Device::process_events`, in a fixed room for events per block; a device short of room leaves
    out note-ons first, so a note it started always ends.
15. **Data-only community effects** use the separate `suhina-device-sdk` and `.sd` format
    ([ADR 0021](0021-device-graph-files.md)). Unlike built-in Rust devices, these graphs contain
    only supported node data; the engine prepares them off-thread and runs them in the same
    real-time-safe chain. Their parameter declarations generate UI controls.

How 8 to 14 behave in detail (orders, room for events, latency, what a swap keeps) is in
[device chains](../spec/device-chains.md).

## Not decided yet

- **Mono, multichannel and side-chain inputs.** The trait is stereo in, stereo out.
  Side-chained dynamics extend it ([sidechain](../spec/sidechain.md), planned).
- **Per-note expression.** `NoteEvent` has no channel and no per-note controls yet (MIDI
  Polyphonic Expression, per-note pitch); they come with the first instrument that uses them.

## Consequences

- Devices were written and tested in parallel with the engine work, and plugged into the
  engine without changes once it had chains.
- The first device, `utility`, proves the shape: thirteen parameters, all smoothed, bit-exact
  passthrough at the defaults.
- Devices are edited through commands ([device chains](../spec/device-chains.md) › Devices added
  by command).
- Parameter ids and device ids are stored in projects: renaming one is a breaking change, like a
  command id (ADR 0002).
- An instrument that works in fixed internal blocks applies an event at its next internal block
  boundary, which keeps its output independent of the host's block sizes at the cost of up to one
  internal block of timing.
