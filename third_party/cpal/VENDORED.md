# CPAL 0.18.2

Source: crates.io cpal 0.18.2, upstream commit
e1612d5d98152f8dc2a62e1b51ef7cbf4f7f26b7 in https://github.com/RustAudio/cpal.
License: Apache-2.0, preserved in LICENSE.

Local change: ASIO input and output reject only a repeated pair of buffer index
and driver timestamp, rather than every repeated index. Single-buffer drivers
otherwise deliver one callback and then stall. The helper's unit tests cover
single-buffer cycles, duplicate notifications, alternating buffers and clock reset.

Background: https://github.com/RustAudio/cpal/pull/1077 documented single-buffer
drivers; the later index-only duplicate guard reintroduced this incompatibility.
Silent hardware probe with the installed driver: one callback in three seconds
before the fix, 63 after it. See docs/spec/asio-host.md in the parent repository.

No driver-name checks or changes to the buffer addresses. Other backends are unchanged.

The ASIO Stream also exposes `control_panel()` on its retained driver so the
application's stream owner can open the active instance's settings without loading
another driver or launching an external executable.

The ASIO clock extension also rejects small backward readings as wrap events and
holds the last published instant until the raw clock catches up. This adapts the
arithmetic proposed in https://github.com/RustAudio/cpal/pull/1396 (head
0681c97ec3b27d9d1354197f03f1b68015835e0a, still unmerged when adopted).
Raw readings and the published floor are distinct: the former detect the 32-bit
millisecond wrap, while the latter clamps backward motion. The wrap condition is
written explicitly; arithmetic wraps without a debug overflow panic, as in #1374.
Tests cover backward readings, millisecond rounding, repeated reversals, duplicate
readings, the half-period boundary and genuine wraps. No device is opened by them.
This does not replace now()'s clock source, redesign cross-thread epoch handling,
or establish a fix for audible crackling or Windows endpoint failures.

On a future CPAL update, compare these clock tests and the existing local patches
with upstream before removing them. The upstream PR remains under review; this
is a locally maintained workaround, not an upstream-approved release change.
