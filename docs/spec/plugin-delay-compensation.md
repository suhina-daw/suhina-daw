---
description: The timing contract for plug-in delay compensation, device automation at the time of its input audio, and the remaining work across transport, monitoring and rendering
read_when: PDC, delay compensation, delayed automation, device input time, render ahead timing, automation after latency
type: spec
status: planned
last_verified: 2026-11-19
covers:
  - crates/engine/src/plugin.rs
  - crates/engine/src/plugin_params.rs
  - crates/engine/src/automation.rs
  - crates/engine/src/chain.rs
  - crates/engine/src/group.rs
---

# Plug-in delay compensation

The timing contract for [#1566](https://github.com/suhina-daw/suhina-daw/issues/1566).
Its first implementation, [#1567](https://github.com/suhina-daw/suhina-daw/issues/1567),
aligns device automation with the audio arriving at each device. Audio path alignment and
hosted song positions already follow [device chains](device-chains.md) and
[plug-in hosting](plugin-hosting.md). Later parts below remain separate work.

## The times a render uses

- **Source time** identifies the arrangement frame that produced a sample. A loop or seek
  changes arrangement time; it does not reverse the number of frames processed.
- **Device input time** identifies the source frame of the audio reaching that device.
  It accounts for the chain's input latency and the devices before this one on its branch.
  A parallel branch contributes its latency only after the branches have been aligned and
  summed. A device's own latency belongs to its output, not its input.
- **Output time** identifies when the compensated mix is presented to the listener. Hardware
  buffering and the output queue add waiting after processing.
- **Scheduling lead** is how far ahead a worker computes audio. It does not change the source
  time of that audio or add device latency to the compensation graph.

Device automation is evaluated at device input time. The transport history that supplies
hosted song positions supplies that time too. A current arrangement frame minus a delay
is not sufficient: after a loop wrap, the input may still contain the end of the previous
pass. History is read in processed-frame order and can return several spans for one block,
each retaining its offset in that block. A loop, seek or play/stop boundary inside the
delayed block separates spans. A sub-block receives events relative to its own start.

When another historical span follows inside the block, a hosted lane also sends its value
at the preceding span's final sample, even when that value has been constant. Together
with the next span's first value this preserves a discontinuity for processors that
interpolate parameter points, rather than starting the change before the delayed audio
arrives. A priming span holds the value at zero through its final sample. These boundary
points use the existing bounded event storage and keep their offsets in sub-blocks.

For example, an effect delays audio by 257 frames. A gain step at source frame 1000 on the
next device must reach that device at processed frame 1257. Both the audio and the step
then travel through any remaining devices and output compensation together. The result
is verified against a self-authored reference signal, not merely against another engine mode.

## Device automation

Hosted parameter lanes and native device lanes use the same history spans and cumulative
input delay. Native lanes include devices inside nested groups. Bus, return and Main
chains include the latency of their incoming sum. Rebuilding the graph updates these
delays even when the running device instance is retained.

Automation before arrangement frame zero holds its value at frame zero. It must not run
forward repeatedly while the input is still being primed. At a historical stopped span,
native lanes retain their last value and hosted lanes follow the stopped-playhead rules
in [automation](automation.md). Once the input represents stopped playback, moving the
stopped playhead updates the hosted lane immediately, without waiting through silent
history. A transport transition reaches a delayed lane with its
audio. An output suspended when changing projects discards the old history, as it discards
the old buffered audio.

Manual parameter values retain their next-block behaviour. They are not held in the
automation history; disabling a lane and moving a control must remain responsive. Set
values and queued manual values are emitted once per block, even when its automation
contains several historical spans. Lanes retain their existing event limits and smoothing
rules, and take precedence over a set value at the same frame.

All history reads and event planning are bounded and allocate nothing in the audio
callback. Preparation occurs before worker dispatch. Offline rendering uses the same
history and planning path as playback.

## Limits and later work

The existing history retains 8192 block records, not a guaranteed number of seconds at
every block size and sample rate. Beyond retained history, hosted song positions currently
extrapolate from the oldest record. Full coverage of the compensation horizon, and explicit
handling of an exceeded limit, remain part of #1566; an extrapolated result is not evidence
of correct timing across a discarded transport change.

The first slice does not establish the complete PDC guarantee. Native device process
contexts, MIDI/note timing, tempo-map envelope conversion, mixer automation across transport
discontinuities, dynamic-latency transitions, late-block fallback, recording placement and
live/render-ahead handoffs still need their own implementation and acceptance evidence.
Their existing issues and dependencies are linked from #1566. A low-latency monitoring
mode remains separate: compensation cannot remove the processing latency of a device on
the played path.

## Sources

- Suhina's own engine and self-authored deterministic render fixtures, based on the
  implementation of #1525 and #1564; no proprietary implementation was consulted.
- [ADR 0006](../decisions/0006-plug-in-hosting.md): sandboxed plug-in processing and events.
- [ADR 0038](../decisions/0038-rendering-on-several-cores.md): parallel and ahead rendering.
- [Steinberg VST3 parameter queues](https://steinbergmedia.github.io/vst3_doc/vstinterfaces/classSteinberg_1_1Vst_1_1IParamValueQueue.html):
  points describe linear segments; a jump needs the old and new values on adjacent samples.
