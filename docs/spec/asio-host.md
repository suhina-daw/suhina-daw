---
description: ASIO host initialization and native panels, plus audio output startup, shutdown and diagnostics
read_when: ASIO, audio driver initialization, native control panel, output startup and shutdown, ASIOInit, sysRef
type: spec
status: current
last_verified: 2026-11-10
covers:
  - third_party/asio-sys/
  - third_party/cpal/
  - Cargo.toml
  - crates/output/
  - crates/session/src/projects.rs
  - crates/session/src/player.rs
  - crates/session/src/audio.rs
---

# ASIO host and output lifecycle

An ASIO-enabled Windows build lists installed drivers through cpal. Loading a
driver calls ASIOInit through asio-sys. The host must initialize the entire
ASIODriverInfo before that call: asioVersion is 2, driver metadata buffers start
empty, and sysRef is a valid desktop window handle. The handle is borrowed and
stays valid across audio-owner threads; it is not an audio buffer or an owned
window. The same contract applies when reloading a driver after a rate change.

The bundled SDK reads sysRef and passes it to the driver's init method. Leaving
that field uninitialized is invalid even when an individual driver ignores it.
The crates.io asio-sys 0.4.0 wrapper leaves it uninitialized at both call sites;
a local Apache-2.0 Cargo patch supplies the initialized structure and tests its
inputs without requiring hardware. Application crates still forbid unsafe code.

Before querying a channel's format, the wrapper queries ASIOGetChannels for the
current driver instance and validates the requested index. Enumeration does not
establish this precondition for a later reload: cpal releases the enumerated
driver and opens it again when creating a stream. Hardware reproduction with
Universal Audio Thunderbolt showed ASIOGetChannelInfo returning ASE_NotPresent
after that reload until ASIOGetChannels had been called. The hardware regression
test queries the output format immediately after each of three fresh loads.

Hardware verification must distinguish the selected device from the actual
output: audio.info must report host ASIO, the selected device, and fallback false.
Successful playback on a WASAPI fallback does not establish ASIO support. Tests
of project-specific failures use that project, its plug-ins and the same driver.

## Native control panel and buffer readouts

`audio.openControlPanel` requests the active ASIO stream's native panel through
the stream-owner queue. `ASIOControlPanel` runs on that owner thread, never in an
audio callback or by loading a second driver. Repeated requests are refused while
the panel is pending/open. Non-ASIO outputs and absent outputs return an error.
The dialog can remain modal on the owner until the user closes it; close the panel
before changing outputs or closing the project. Opening it does not reload plugins.

Audio > Output offers **Control panel…** immediately below the active ASIO device.
Audio status reports
**Active buffers** as separate engine-block and observed callback frame counts.
The latter is not a claim about every internal hardware/WDM buffer. The driver
owns its internal settings. External offline panels may edit defaults rather than
the running application's settings; the native call addresses the current instance.

## Buffer sizes

`ASIOGetBufferSize` answers with the smallest, largest and preferred buffer size and a
granularity (ASIO SDK 2.3, section 3): -1 for the powers of two from the smallest to the largest,
a positive step for the smallest and every step after it, and 0 with the smallest equal to the
largest for that one size. Drivers whose size is set in their own control panel commonly answer
with that one size as smallest, largest and preferred, and not always with 0: a driver seen on
Windows answers -1 with it. A driver answering one size is offered that size whatever
granularity it states, also when it is no power of two or step.

Settings › Audio offers the sizes the answer allows within what the engine renders in (64 to 4096
frames): with powers of two every power in the range; with a positive step every allowed
size on it, without reducing long lists to common sizes; with 0 and a range, the preferred size alone, the
one size such a driver is sure to take, or with a preferred size outside its range only the two
ends it declared; and the smallest, largest and preferred when the driver takes them. An answer
that makes no sense (an empty or reversed range) counts as none: the engine's own sizes are
offered and the driver is asked for none. A driver that takes one size has the Buffer size row
say that size, with a note that it is set in the driver's own control panel (**Control panel…**
opens it), instead of a list with nothing to choose.

With no size chosen the driver's preferred size plays and the engine renders in blocks of that
size; the **Default** entry names it. When the driver cannot be asked for its preferred size (off
its steps, or outside what the engine renders in), the size it takes nearest the engine's own
block (1024 frames) plays instead, so the engine's block and the driver's buffer stay the same
size. Should a driver refuse the size it is asked for, the output opens once more asking for none
(the driver plays its preferred size), and a notice says it keeps its own buffer size. An ASIO
input on the same interface is opened at the output's size: input and output share one driver
and one buffer size, and an input opened without it would put the driver back on its own size
under the output. The driver calls the streams' callbacks one after the other in each cycle, the
output's first (its stream is made first), so with an ASIO input it is the input's callback,
once its frames are in the input's ring, that wakes the render thread, not the output's: woken
by the output, the render thread read the input before that cycle's frames were written and
filled the block's input with silence, an input underrun in every cycle it won the race, more
often the smaller the buffer. The render thread also looks every millisecond by itself, and a
look that fell between the two callbacks read too early the same way (about one cycle in 15 at
2048 frames), so once the output has taken from the ring the render thread waits for the input's
frames of that cycle, a quarter of the device's buffer at most (at least half a millisecond),
before it renders without them. Once that grace expires, further render blocks in the same
output callback cycle proceed without restarting the wait. The next output callback gets
a fresh grace period; timeout handling never clears a callback-owned pending-input flag.
When the input closes or fails the output wakes it again and nothing is waited for.

The trace and `audio.info` expose input short reads as a bounded history of the latest
32 events, each with a sequence number and available/requested frames, and cumulative
backlog-drop counts and dropped frames. A render reads input in chunks of at most 1024 frames, also split at transport
boundaries; several short reads can therefore belong to one device block.
Render diagnostics count output callbacks, ASIO input callbacks, rendered blocks, the
maximum render starts between observed output callbacks, extra starts after the first,
and input-wait expirations. Cycle zero, before any output callback, is excluded from
per-cycle counts.

Input backlog is trimmed once at the start of a whole render call: when more than four
render blocks wait, old input is dropped down to two whole blocks. Internal splits at
1024 frames, loop boundaries, count-in ends, jumps or graph swaps do not run this policy
again or reduce its frame budget. A short piece at a loop boundary must not discard input
needed by the rest of the same render. The queued-input latency for each piece is measured
after any drop, from the frames actually waiting.
An extra render is not by itself a defect: startup, disarming and different device/block
sizes can require it. Hardware traces are needed to correlate it with short reads.

The answer is kept with the device descriptor (below) and asked again each time an ASIO output
opens. When no driver is loaded the driver is loaded for the question and kept loaded until the
stream is built, so it is loaded once. A size changed in the driver's control panel while Suhina
runs reaches Suhina as `kAsioResetRequest` or `kAsioBufferSizeChange`; both invalidate the stream
(a `kAsioBufferSizeChange` repeating the size already playing does not), and the output opens
again with the new answer. For drivers that announce nothing, Suhina asks the driver itself
(`audio.checkDriver`, on the thread that holds the driver) when Settings › Audio opens or
closes and when its window gets the focus back while it shows (the separate Settings window
when detached, otherwise the main window), as after using the driver's
control panel; when the answer differs from the one the output opened with, the output opens
again. Settings › Audio lists the devices again each time an output opens. The trace says
why an output opened again (`audio output failed: the driver asked for a reset`, … `the
driver's buffer sizes changed`), and `audio.info` reports it as `failure`. Each opening writes
the driver's answer, the sizes made from it and the size asked to the trace (`audio driver:`),
and `audio.checkDriver` writes a changed answer before and after (`audio driver check:`),
so a log shows why Suhina plays the size it plays ([debugging](../debugging.md)).

A size chosen is remembered whatever the driver says: while the driver does not take it, the
nearest size it takes plays with a notice saying what the driver takes, and the list keeps the
chosen size, saying the device does not take it now; it plays again once the driver takes it.
Default (the driver's preferred size) applies only when no size is chosen.

## Device catalog during playback

CPAL 0.18.2's ASIO device descriptors cache metadata without retaining a driver.
Suhina collects those descriptors before opening the first ASIO stream, and reuses
them for both settings and selection during this app run. ASIO enumeration loads
each driver in turn; while a different driver is already loaded, CPAL ends that
iterator. Enumerating during playback would therefore hide installed devices and
needlessly call drivers from the settings window. Newly installed drivers become
available after restarting Suhina. Opening the chosen descriptor still reports
an error if its hardware is unavailable.

The other hosts are listed anew each time, and the host says which of its devices play: a device
that cannot say its formats yet, as a Bluetooth endpoint often cannot, stays listed and reports its
error when opened. While Settings › Audio shows, a background look reads their devices' names every
two seconds and the list is made again when they change. It never touches ASIO.

## Switching from the settings form

Driver selection is pending until the user selects Device. One `audio.setOutput`
command then names both; no intermediate default driver opens. The active output
continues while the choice is pending. The rule applies to every host and device,
without matching any vendor name.

## Initial callback burst

The ASIO SDK 2.3 specification, section 5, describes callbacks that fill the first buffers
before hardware streaming starts; its example gives the first two callbacks the same time.
Suhina therefore primes enough silence for two known device periods before starting the
stream. This is a startup reserve only; the ordinary render queue target stays unchanged.

## Repeated buffer indices

A buffer index identifies the memory to process, not a unique processing cycle.
Some installed drivers use buffer 0 for every cycle. CPAL 0.18.2's index-only
repeat guard drops all but the first callback from these drivers. A silent
three-second reproduction with FL Studio ASIO delivered one callback before
including the driver timestamp in the comparison, and 63 afterwards.

Compare both index and timestamp to suppress duplicate notifications from the
same cycle while allowing a new timestamp for the same buffer. Apply the same
rule to input and output, without driver-name checks. This does not change which
ASIO buffer is read or written. Drivers with missing or repeated timestamps need
separate validation; the test does not establish their compatibility.

## Backward clock readings

ASIO timestamps are nanoseconds derived from a wrapping 32-bit millisecond clock.
A small backward reading is not a full clock wrap: the local CPAL patch extends
the epoch only for a backward step exceeding half that period (about 24.9 days).
Until a backward clock catches up, the last published instant is repeated.
Raw clock readings remain separate from this monotonic floor. All operations use
atomics, without allocation, I/O or locks added to the callback.

This workaround is adapted from the still-open CPAL change
[#1396](https://github.com/RustAudio/cpal/pull/1396). It guards against erroneous
driver timestamps and coarse `now()` readings but does not replace that method's
clock source or redesign concurrent epoch extension. Deterministic tests cover
the arithmetic without hardware. Audible crackling and Windows shared-output
recovery require separate hardware verification; these tests establish neither.

## Recovery after an output fails to open

An unsuccessful output change can leave the project without a player. A subsequent
explicit output selection opens a new stopped player even when none remains; it
must not report success while leaving the project silent. The original driver
open error and requested host/device configuration are recorded in the diagnostic
trace. Transport commands without an output direct desktop users to Audio settings.
This recovery does not silently select a different backend.

## Output changes while the control panel is open

While a native panel request is pending or open, output replacement, project opening,
tab switching and project closing report "close the ASIO control panel first".
They leave the current player attached. Rejected project opens do not enter the recent-project
list. Rejected output choices restore the settings selection without changing device history;
an actual device-open failure retains the requested choice for retry. Only successful output
selections update the remembered device. Switching away from a failed choice preserves that
driver’s last working device; the failed choice is not recorded as the previous device. This prevents waiting on an audio-owner thread
that is occupied by a modal driver dialog. Application exit still uses bounded
background session cleanup; driver calls themselves cannot be forcibly timed out.

## Implementation ownership

ASIO builds patch cpal's `asio-sys` dependency with the vendored Apache-2.0 copy
in `third_party/asio-sys`. It initializes the SDK's host inputs and queries the
current instance's channel count before channel metadata, including after a
driver reload (the host contract above). The application's safe audio
boundary and render path remain in `output`.

ASIO device descriptors are cached before the first stream opens and reused while
it plays; settings never enumerate other ASIO drivers over a loaded driver.
The active output's native control-panel request travels from the
session through the owner's command queue to the retained ASIO stream; it does
not load a separate driver or launch an external settings application. The native
dialog runs on the owner thread, independently of render and device callbacks.
A superseded plug-in start is released on a worker thread, since unloading its host
may wait for a sibling loading after a sample-rate change.

## Audio output startup and diagnostics

Before starting a device stream, the output owner primes its ring with silence to the normal
queue target or two device periods, whichever is larger. This covers immediate startup
callbacks even when the render thread has not run yet; it does not increase the steady-state
queue target. Reopening restores the transport at
its current beat even when read-ahead delays the new renderer's start.

At the device boundary, main and cue audio rise from silence over 5 ms when the
first nonzero frame arrives; priming silence does not consume this fade. Closing
a stream requests a 5 ms fade of the actual queued waveform in its device callback.
The callback then keeps supplying silence for the reported hardware delay plus two
device periods before acknowledging shutdown. The output owner waits at most
500 ms, or until a device error, then closes the stream and returns its renderer.
CPAL and exclusive WASAPI share this envelope. No callback waits, locks or allocates;
the bounded shutdown wait runs only on the owner. This prevents software signal
steps at open/close, but cannot suppress a pop generated internally by hardware.

The engine publishes the duration and queue lead of a render call with late plug-in blocks
through atomics. The UI writes these diagnostics and the actual output identity to the trace;
the real-time thread never writes the log.

## Sources

- The vendored SDK, `third_party/asiosdk/common/asio.cpp`, ASIOInit.
- [asio-sys 0.4.0 source](https://github.com/RustAudio/cpal/tree/e1612d5d98152f8dc2a62e1b51ef7cbf4f7f26b7/asio-sys).
- [PortAudio ASIO host](https://github.com/PortAudio/portaudio/blob/master/src/hostapi/asio/pa_asio.cpp), LoadAsioDriver and the desktop window used by its Windows host.
- [Ardour's PortAudio integration](https://github.com/Ardour/ardour/blob/master/libs/backends/portaudio/portaudio_io.cc), which opens its ASIO stream through PortAudio.

- [CPAL 0.18.2 ASIO device enumeration](https://github.com/RustAudio/cpal/blob/v0.18.2/src/host/asio/device.rs).

- [Steinberg ASIO SDK 2.3 specification, section 5](https://dspace.tul.cz/bitstream/handle/15240/49466/ASIO_SDK_2.3.pdf?sequence=1).
- [CPAL single-buffer compatibility report](https://github.com/RustAudio/cpal/pull/1077).
