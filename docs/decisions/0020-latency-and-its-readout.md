---
description: What Suhina's latency number is and what it includes, where it is shown, how output and round-trip latency are established, and when a readout warns
read_when: latency, output latency, round-trip latency, monitoring latency, buffer size, engine block, control bar, performance readout, CPU load, RAM, underruns, xruns, audio.info, device buffer, WASAPI shared period, live playing, live input latency
type: decision
status: proposed
built: no
last_verified: 2026-10-15
covers:
  - crates/output/src/lib.rs
  - crates/output/src/devices.rs
  - crates/session/src/audio.rs
  - crates/ui/src/app.rs
  - crates/ui/src/settings/settings_view.rs
---

# 0020: Latency: what the number is, and where it is shown

- **Date:** 2026-10-15
- **Issue:** [#449](https://github.com/suhina-daw/suhina-daw/issues/449) — this ADR is its decision
  step; the issue stays open until the readout itself is built.

## Context

Suhina already measures the latency it plays at, and already publishes it: `OutputInfo` carries
`latency_ms` together with the device's measured `buffer_frames`, the engine's `engine_block`,
`underruns` and `failed`; `audio.info` reports them to agents ([mcp.md](../mcp.md)), and
Settings › Audio › Now prints the number while a project plays (`crates/ui/src/settings/settings_view.rs`).
What the window does not have is a surface for it while someone works: the control bar shows the
CPU load from the engine's monitor and the program's RAM
([components.md](../design/components.md) › Control bar), and the number a person tunes the buffer
for is two pages away.

Two facts decide the shape of this:

1. **The number is measured, not reported.** It is the frames the render thread has queued in the
   ring ahead of the device at the moment of a callback, plus the delay the device itself reports
   from its timestamps (`measure_latency`, `Progress::latency`, `crates/output/src/lib.rs`). It is
   rewritten on every callback.
2. **Half of it is not ours to set.** The device's own buffer is Suhina's to choose on ASIO and in
   WASAPI exclusive mode, and Windows' in WASAPI shared mode, where the stream plays the period the
   system gives the device (ADR 0006 › the block). A readout that does not say which of the two it
   is showing cannot be acted on.

## What the number is, and what it is not

- It is the **output** half: from the engine having rendered a frame to that frame leaving the
  speaker. It includes the ring Suhina keeps ahead of the device and the device's own delay.
- It does **not** include the time the engine spends rendering a block: that is what the CPU load
  measures, and what a block's deadline is about.
- It does **not** include input, because there is none yet (#267).
- It follows the block size: a larger block is more frames queued, and so a larger number. That is
  the trade the buffer setting exists for.

## Decision

1. **One measured number, published once.** The session's `OutputInfo` stays the single source of
   the output's live figures, and a surface is a view of it. No second measurement, no second
   definition of "latency", and no new command: `audio.info` already reports it.
2. **It says what it includes.** A surface calls it *output* latency and names the device in use,
   the device's measured buffer, the engine's block and the underruns beside it. The word "latency"
   on its own is not used until a round-trip number exists, since that is what a person reading it
   will assume.
3. **It is reachable where the work happens, on demand.** The control bar's performance block opens
   a small panel with those figures instead of adding a third always-visible number. An
   always-visible block, or the same readout behind a Settings option, are accepted
   variants: they change the layout, not this decision.
4. **It is sampled, not per frame.** A surface reads the value about once a second, or smooths it as
   the CPU load is smoothed. A control bar repaints continuously, and a number rewritten per
   callback would flicker.
5. **Agents see the same number.** If a reported field changes so that the window and an agent can
   disagree, `launcher.state` gains `latency_ms` — a field, not a new tool.
6. **A warning is a policy, and is not decided here.** A threshold that warns ("too deep to play
   live"), or a path that shortens the render queue while someone plays, is its own decision: it is
   added to this ADR or supersedes it.

## The round trip (deferred, and what it needs)

- Input does not exist (#267), and a round trip needs one; measuring it is asked for in #265.
- There are two honest ways to state a round trip. **Summed**: what the driver and the plug-ins
  report, which is what the chain claims. **Measured**: a signal that leaves and comes back, which
  is what happens. They answer different questions and will disagree.
- This ADR fixes only this much: when a round-trip number appears, it says which of the two it is,
  so two surfaces cannot contradict each other without anyone knowing why. The measurement itself
  belongs with the input work, on the device layer, and gets its own record.

## The buffer is a separate decision

What a chosen buffer size means, what may be asked of a device's driver, and how Suhina renders
when the device's buffer is not hers to set, is ADR 0006 › the block. This ADR assumes it and does
not repeat it; a readout that shows the two numbers apart is what makes that decision visible.

## Consequences

- A readout is a view of `OutputInfo`: no new measurement, no new command, no change to the device
  layer or the engine, and no architecture behind it.
- Tests it needs: a window test for the surface (there while an engine plays, gone when none does),
  and a unit test for how the value is written (below 1 ms, and before the first callback).
- Documents it changes when it is built: `docs/design/components.md` › Control bar (the block and
  its panel), `docs/spec/ui-session-view.md` (the control bar's contents), `ROADMAP.md` › What works
  now, and `docs/mcp.md` if a reported field changes.
- Risk: a performance readout that is always on can be mistaken for something to dial. That is the
  reason the panel names the device and says which buffer it is showing, and the reason the default
  is on demand.
- What it does not promise: input latency, a round trip, or a warning. All three wait for the input
  work.

## Alternatives

- **A third always-visible block** (`12.3 ms`, beside the CPU load and RAM): the same number for
  about 56–64 px of the control bar, and it competes with the position readout in a narrow window.
  Kept as a variant of decision 3, not as the default.
- **Nothing new** (only Settings › Audio › Now and `audio.info`): costs nothing and hides the number
  exactly when it is wanted.
- **Deriving the number from what the driver and the plug-ins report**, instead of measuring it:
  rejected for the output half, because a sum cannot see the ring the engine actually keeps ahead —
  the part Suhina controls and the part that changes when the block changes.
