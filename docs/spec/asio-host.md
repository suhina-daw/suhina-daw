---
description: ASIO host initialization and native panels, plus audio output startup, shutdown and diagnostics
read_when: ASIO, audio driver initialization, native control panel, output startup and shutdown, ASIOInit, sysRef
type: spec
status: current
last_verified: 2026-10-30
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

## Device catalog during playback

CPAL 0.18.2's ASIO device descriptors cache metadata without retaining a driver.
Suhina collects those descriptors before opening the first ASIO stream, and reuses
them for both settings and selection during this app run. ASIO enumeration loads
each driver in turn; while a different driver is already loaded, CPAL ends that
iterator. Enumerating during playback would therefore hide installed devices and
needlessly call drivers from the settings window. Newly installed drivers become
available after restarting Suhina. Opening the chosen descriptor still reports
an error if its hardware is unavailable.

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
