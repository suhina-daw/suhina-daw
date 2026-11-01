---
description: How a built-in device hands live values (levels, gain change, a spectrum) to its panel - Device::readouts, a fixed table of atomics per track beside the meters, #rN values a canvas reads, and the real-time rules it keeps
read_when: readouts, live values, device meter, gain reduction display, spectrum on a panel, signal in a device panel, #r0, readout expression, device to panel data, sample position
type: decision
status: current
last_verified: 2026-10-29
covers:
  - crates/device-api/src/lib.rs
  - crates/engine/src/meters.rs
  - crates/engine/src/chain.rs
  - crates/ui/src/app/detail_area.rs
  - crates/device-sdk/src/ui.rs
---

# 0032: Device readouts — live values from a device to its panel

## Context

A dynamics device's panel needs to show what the device does while it plays: the level each band
is at, how much it cuts or lifts, the spectrum it hears, its output level. Until now a panel drew
only from parameters; the one live value that left a device was the Sampler's read head, which the
meters' table carried to the UI. The audio thread may not allocate, lock or wait (AGENTS.md rule 3),
and a panel is drawn by the UI thread at its own pace.

## Decision

1. **`Device::readouts() -> Option<[f32; MAX_READOUTS]>`** (35 values, default `None`), read after
   each block. The device keeps the values in its own fixed array, updated in `process`; their
   order is the device's and its panel's agreement, written down in the device's spec.
2. **One device per track publishes**, the first in its chain that returns `Some`, like the
   Sampler's read head: `Chain::readouts` finds it and `Levels::finish` stores its values in the
   track's meter row (35 `AtomicU32`s and the device's position + 1). Storing is a handful of
   relaxed atomic writes per block; nothing allocates or waits. A graph swap clears the row.
3. **The UI hands them to the canvas as values `#r0` … `#r34`** beside the parameters
   (`MeterMonitor::readouts(track, device)`, then `GraphParameter::readout`). They are never listed
   or changed by a control. A canvas reads them with the `readout` expression or names them in a
   drawing; validation accepts these ids in drawings, never in bindings. The UI repaints at about
   30 frames per second while a device publishes. `#expanded` (1 while the device's expanded
   panel is open) is handed in the same way, so a compact panel can rearrange itself.

## Consequences

- A device adds a display without new channels or threads; the values arrive at most one block late
  and a frame is drawn from the latest. Values in between are lost, so a device smooths what it
  shows (a follower, a falling peak).
- Only one device per track shows live values at a time. A second one needs the row to carry more
  than one device; the table grows then, not the trait.
- Plug-ins have no readouts; their own editors show theirs.
