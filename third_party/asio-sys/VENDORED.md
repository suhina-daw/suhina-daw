# asio-sys 0.4.0

Source: the crates.io `asio-sys` 0.4.0 distribution, upstream commit
`e1612d5d98152f8dc2a62e1b51ef7cbf4f7f26b7`, directory `asio-sys` in
<https://github.com/RustAudio/cpal>. Licensed Apache-2.0; LICENSE is from that
upstream commit. Registry bookkeeping and the crate's separate lockfile are omitted.

Local changes:

- Keep the driver slot occupied through final destruction and explicit
  `Driver::destroy`, even after its weak reference can no longer upgrade. Loads
  during teardown return `DriverAlreadyExists`; failed destruction keeps the
  slot unavailable. A deterministic test covers the zero-strong-reference gap
  without loading hardware. This addresses the same race as upstream commit
  `bfb4f70214366ae59c6dfc107faf4864c3c0659a`, using a nonblocking occupied flag
  rather than adding a destructor/load mutex and its lock-order constraints.

- Expose ASIOControlPanel through the retained Driver. Bindgen refreshes an older
  cached binding file if this entry point is absent. The call holds no callback
  lock and belongs to the driver's non-audio owner thread.

- Both ASIOInit call sites use a fully initialized ASIODriverInfo: host ASIO
  version 2, zeroed driver outputs, and the desktop HWND on Windows.
- A windows-sys dependency supplies GetDesktopWindow. The returned HWND is
  borrowed; the wrapper neither creates nor destroys a window.
- A unit test checks the input contract without loading a hardware driver.
- Channel metadata queries call ASIOGetChannels first on the current instance
  and reject channel indices outside that direction's channel count. This
  supports drivers whose channel metadata is populated by that call, including
  Universal Audio Thunderbolt after the enumeration instance has been released.
- An opt-in hardware test repeats fresh load / immediate format query / release
  three times. Set SUHINA_ASIO_TEST_DRIVER to the connected driver's name and run
  `cargo test --manifest-path third_party/asio-sys/Cargo.toml --lib freshly_loaded_driver -- --ignored`.

The SDK reads sysRef before it writes driver metadata. The upstream use of
MaybeUninit passed an indeterminate window pointer and host version to drivers.
The patch stays inside the existing foreign-function dependency; no Suhina
crate gains unsafe code or a different audio backend.

To update: compare a new crates.io release with these changes, run the initializer
test and the Windows hardware tests, then remove the Cargo patch when upstream
provides equivalent initialization, channel query ordering and teardown exclusion. Do not edit
Cargo's shared registry copy.
