---
description: An opt-in laboratory for a single effect chain switching between live processing and a bounded queue before its mixer
read_when: render path prototype, switching paths, render ahead, draining, catching up, phase 3, 1508
type: spec
status: planned
last_verified: 2026-11-20
covers:
  - crates/engine/src/render_path_probe.rs
  - crates/engine/src/render_path_probe_tests.rs
  - crates/engine/examples/render_path_probe.rs
  - crates/engine/src/chain.rs
---

# Render path prototype

The first experiment for [#1508](https://github.com/suhina-daw/suhina-daw/issues/1508)
tests [ADR 0038](../decisions/0038-rendering-on-several-cores.md) decision 10 with one
effect chain. It is a laboratory, enabled by the engine's `render-path-prototype` Cargo
feature, not Studio mode in the desktop application. The example's `--lead-ms` setting
defaults to 0 (off); 50, 100 and 200 enable a lead rounded up to whole blocks.

## The experiment

`render_path_probe::Probe` takes a fresh renderer at frame zero and extracts one leaf effect chain.
Its caller supplies deterministic stereo audio at the requested source frame. This tests
the real device chain and its delay compensation, without a second implementation of DSP.
It does not play the project's clips. Instruments, buses, incoming sidechains, sends and
ARA hosts are refused rather than silently rendered without their inputs or destinations.

The chain has one device state and a monotonically increasing render cursor. Its output
before the mixer is stored in a fixed-capacity queue. A separate play cursor consumes each
block exactly once. Device automation and transport use the render cursor; mixer automation,
fader smoothing, pan and the existing output compensation delay run at the play cursor.
Switching paths never resets devices or changes that delay. The laboratory transport uses the
initial tempo and 4/4 throughout; it does not reproduce a project's tempo or signature changes.

- **Live:** render and play one block; the queue is empty afterwards.
- **Catching up:** render at most two blocks per call, including any block needed immediately,
  until the queue reaches its target. A caller may withhold the extra block when it has no
  spare processing budget; catching up then pauses without interrupting playback.
- **Ahead:** replace the block played, keeping the lead.
- **Draining:** stop producing extra audio, play the queued blocks, then resume live at the
  next source frame. Reversing a request retains the queue and continues from there.

A continuous switch does not crossfade: changing any samples would invalidate the
bit-identical oracle. Stop, seek, graph replacement and their declicking are not exposed by
this initial experiment. They need transport integration before the prototype can run in
the application. The current engine's handling of those operations is unchanged.

## Reproducing the sample-domain measurements

Run `cargo run -p suhina-engine --example render_path_probe --features render-path-prototype
-- --lead-ms 50` (one command). Repeat with 100 and 200. The example prints its sample rate,
block size, rounded lead, sample comparison and the first changed sample after a device
parameter change, beside the response of the live mixer. It fails if the switching render
differs from the ordinary chain or the measured sample offsets violate the queue contract.
These are audio-frame measurements, not wall-clock performance or hardware latency results.

## Not yet

- The desktop setting and commands, automatic live-track selection, arrangement/launcher
  source scheduling, stop/seek declicking, graph swaps and per-track worker scheduling.
- Actual controller-to-output and quantized-launch measurements on Windows and macOS,
  with the hardware and run metadata required by ADR 0038. The laboratory's scheduled
  input must not be presented as a measurement of the launcher's scheduling.
- A decision about actions that require immediate response: draining a full 50–200 ms
  queue already exceeds a 10 ms bound in the sample domain. The ADR is not changed or
  marked validated by this experiment; #1508 remains open for the integration and measurements.
