---
description: The timing contract for plug-in delay compensation, device automation at the time of its input audio, and the remaining work across transport, monitoring and rendering
read_when: PDC, delay compensation, delayed automation, device input time, render ahead timing, automation after latency
type: spec
status: planned
last_verified: 2026-11-20
covers:
  - crates/engine/src/plugin.rs
  - crates/engine/src/plugin_params.rs
  - crates/engine/src/automation.rs
  - crates/engine/src/chain.rs
  - crates/engine/src/group.rs
  - crates/engine/src/buses.rs
  - crates/engine/src/graph.rs
  - crates/engine/src/lib.rs
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
points use the existing bounded event storage and keep their offsets in sub-blocks. A stopped
span that already sent its value still sends this closing point before playback resumes;
a fresh processor keeps its loaded state until the playhead moves.

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
rules, and take precedence over a set value at the same frame. Within the hosted slot's
512-event budget, reserve room for all spans' required first values before adding lane
intermediate points or closing points. Set and manual values keep their existing priority.
If those values and the span starts alone exceed the budget, later events are still dropped;
if only optional points exceed it, interpolation detail, including boundary holds, can be
lost, but optional points cannot displace a later span's first value.

All history reads and event planning are bounded and allocate nothing in the audio
callback. Preparation occurs before worker dispatch. Offline rendering uses the same
history and planning path as playback.

## Native audio-effect context

A native audio effect, including an SDK graph effect, receives tempo, beat and playing
state at its device input time even when it has no automation lanes. Its own latency is
excluded. The same cumulative delay used for its automation includes upstream devices,
nested branch paths and the incoming sum of a bus, return or Main.

A delayed block can cross an earlier tempo change, loop, seek or stop/resume boundary.
Processing splits at those historical boundaries as well as at parameter and event
boundaries. Each sub-block receives the context of its first input sample. Context is also
set before a parameter or note event at that sample. Before retained playback starts, the
initial context is extrapolated backwards (the beat can be negative); stopped spans hold
their recorded position. This does not advance automation before arrangement zero.

Context spans are prepared from transport history before worker dispatch, in fixed-capacity
storage reserved with the graph. Graph replacement uses the new path delays while retaining
compatible running devices. The serial, parallel and offline paths share this preparation
and processing. Manual controls keep their next-block response.

This applies to audio effects. Native instruments and MIDI effects keep their existing
source-time contexts until note/event alignment is implemented together with their timing;
moving a context alone would move it away from the notes that drive the instrument.

## Graph swaps

An edit builds a new graph and compensates it for the delays of the devices it built; the running
devices that stay (the same device, with the new values) keep their tails and state. A built-in
device can decide its delay when it is built: an EQ delays by its transform's frame while a band
in use is Spectral, and decides that when it is built, so automation never changes its delay in
the middle of a graph. A running built-in device that, with the new values, would delay by
another amount than the copy the graph was built with gives its place to that copy, at the top
of a chain and inside a device group: the graph never compensates a delay the sound does not
have, nor misses one it has. The device starts again then (its tail is cut), as it does when the
delay changes for any other reason. A plug-in follows a new delay itself.

## History and compensation limits

The supported compensation horizon is two seconds at the renderer's sample rate. Every
cumulative path, including serial devices, nested parallel groups, buses, returns and Main,
must fit within it. The exact limit is accepted. Preparing a graph beyond it returns a
latency-compensation error naming the path's final track (or Main), its required frames and
the supported limit. Reduce a device's latency or bypass it before retrying. A larger latency
is never silently treated as two seconds. Group delay allocations remain bounded even while
preparing a graph that will be rejected.

History storage is allocated when the renderer is prepared, with one record per frame of
the horizon plus the current block's record: `2 * sample_rate + 1` records. This covers even
one-frame callbacks, irregular blocks and transport changes at every boundary. Storage is
bounded by the supported maximum sample rate of 384 kHz (768001 records per renderer).
Larger blocks need fewer records but use the same reserved storage. Zero-frame calls do not
record a transport position. Recording, clearing and reading history allocate nothing.
History survives graph swaps; suspending the renderer clears its contents but keeps storage.
Only priming before the first processed frame extrapolates the initial song backwards;
supported delays never extrapolate through an evicted transport change. Automation before
arrangement zero holds its initial value as described above.

A failed initial build or offline render returns the error. During an edit, graph preparation
fails before queuing the replacement: the existing engine graph remains active. The session
keeps the project edit and reports that the playing engine could not follow it; the user can
undo or reduce latency and retry. This does not establish seamless transitions for a running
plug-in that changes its physical latency before a replacement graph is ready.

## Later work

These slices do not establish the complete PDC guarantee. Native instrument and MIDI-effect
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
