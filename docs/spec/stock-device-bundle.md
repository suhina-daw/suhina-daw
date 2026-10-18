---
description: Build-time .sd definitions of the native devices shipped with Suhina
read_when: stock device bundle, build devices, native_device node, bundled sd
type: spec
status: current
last_verified: 2026-10-17
covers:
  - crates/device-bundle/**
  - crates/device-registry/src/stock.rs
  - crates/device-sdk/src/native.rs
  - apps/suhina/build.rs
---

# Stock device bundle

Every registered device under devices/ has a generated .sd definition containing
its stable identity, role, parameter contract and SDK panel. Heavy native DSP is
represented by a native_device node, as ADR 0021 specifies. This node names a
registered implementation; it never names a library, executable or source file.

The node is permitted only as a whole-device wrapper: effects connect audio_input
to native_device to audio_output; instruments connect native_device to audio_output.
There is no feedback, additional processing or arbitrary native code. A host
prepares it through an explicitly supplied factory, checks role and parameter
identity/order/ranges, and calls the same native Device implementation as before.
Generic primitive compilation rejects this node without a factory. Saved Sampler
wrappers from revisions 1-9 may omit the appended LFO destinations, shape and source fades, warp controls and LFO timing selectors (and the oldest
may also omit `panel_view`). Those known historical parameter prefixes prepare
with zero new modulation depths. Every declared audio parameter still matches
exactly; arbitrary missing parameters or changed ranges/order remain errors.


The bundle build generates and validates all .sd files, checks that every devices/
package is registered, and embeds their contents. The app build also writes them
inside its `OUT_DIR/devices/`, without watching its own output. Startup and browser discovery use the embedded
definitions, so moving the executable without that directory does not remove its
stock instruments or effects. No user files are written at startup.

Importing a native wrapper resolves its trusted implementation and preserves its
definition in the project's device extensions, separate from sampler source state.
Its controls use the wrapper's declarations and panel. Existing native device ids
and project state remain compatible.

Set `SUHINA_DEVICES_DIR` when the build should export definitions to a custom directory.
The explicit export override is optional; ordinary builds write only in `OUT_DIR`.

Legacy Sampler wrappers accept only the canonical ordered parameter prefixes of known revisions.
Omitted parameters are explicitly initialized to their registered defaults during preparation.
