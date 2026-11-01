---
name: suhina-devices
description: Checklists for Suhina's built-in effects, instruments and MIDI effects (`devices/<name>`) - adding a parameter to one, or adding a new device - with the files, the registry, the panel layout, the tests, importing from other formats and the documents to touch. Use when changing a built-in device's parameters, DSP or panel, or adding a device.
---

A built-in device is native Rust in `devices/<name>`, with its DSP built from `crates/dsp` and
its panel from the SDK's UI system (AGENTS.md rule 8;
[ADR 0004](../../../docs/decisions/0004-built-in-devices.md)). `devices/auto-pan` is a small,
complete example to copy from. The SDK's `.sd` example graphs in `crates/device-sdk/examples/`
are not built-in devices. Before finishing, the checks of
[`working-on-suhina`](../working-on-suhina/SKILL.md) apply.

## A parameter on a built-in device

Everything around the device (the wrapper, the panel, saving, automation, old projects) reads
its `INFO` and finds parameters by id, so the change is mostly the device's own crate.

1. **Append** a `ParamInfo` to `PARAMS` in `devices/<name>/src/lib.rs`
   (`ParamInfo::continuous`, `toggle`, `choice`), and its index to `mod param`, raising `COUNT`.
   Append, never insert or reorder: indices are what `set_param` and the tests use, and a
   project stores each value by the parameter's id (`suhina_model::DeviceParam`), so an old
   project that lacks the new id gets its default.
2. **Use it** in the DSP through the device's `ParamBank` (`params.get(param::NAME)`), smoothed
   where a jump would click. Nothing on the audio thread allocates. A Dry/Wet is always id `mix`,
   name "Dry/Wet", 0 to 1 in `Unit::Percent`, blending `dry + (wet - dry) * mix` through its own
   smoother, as in `devices/compressor/src/lib.rs`.
3. **Panel:** place a control with the same `id` in `devices/<name>/panel.json` (or in the Rust
   layout for `sampler`). An expanded panel places every control by hand at absolute bounds:
   widen its `size` when there is no room, and add the id to `hidden_parameters`, which lists the
   parameters the layout draws so the generated list does not show them twice. A parameter the
   layout leaves out still shows in the generated list. `cargo test -p suhina-device-registry`
   fails on a layout naming a parameter the device lacks.
4. **Tests** in `devices/<name>/src/tests.rs`: what the parameter does to a test signal
   (`suhina_device_api::conformance::test_signal`). The conformance checks in
   `crates/device-registry/tests/conformance.rs` run on every listed device by themselves.
5. **Importing:** when another format's device maps onto this one, its reader in
   `crates/als/src/devices.rs` sets the new parameter, with a test in
   `crates/als/src/devices_tests.rs`, and `docs/spec/device-import.md` says so.
6. **Documents:** the device's spec in `docs/spec/<name>.md`, when it has one; the parameter
   count in the device's line of `scripts/parity/devices.json` (the report built from it,
   `docs/research/parity-live-12.md`, is regenerated on `main`); and the crate's row in
   `docs/architecture.md` when it names the parameters.
7. **Build the app and look at the panel** (`docs/workflow.md` › Built-in devices and settings).

## A new built-in device

1. **Spec first** for any behaviour taken from another product: `docs/spec/<name>.md`, written
   from observation (CONTRIBUTING › Clean-room).
2. **Crate** `devices/<name>`, package `suhina-device-<name>`, with workspace settings and an
   entry in the root `Cargo.toml`'s `[workspace.dependencies]`. It exports `INFO: DeviceInfo`
   and a type that implements `suhina_device_api::Device`. Missing DSP goes into `crates/dsp`,
   not into the device.
3. **Registry**, `crates/device-registry/src/lib.rs`: `INFO` in `DEVICES` (browser order; the
   array's length is in its type), its constructor in `create`, and its `panel.json` in
   `LAYOUT_FILES`.
4. **Tests**: the device's own in `devices/<name>/src/tests.rs`; the registry's conformance test
   covers it once it is listed. A device whose sound is the point gets a golden render.
5. **Documents**: the crate's row in `docs/architecture.md` and the crate map; the devices table
   of `ROADMAP.md`; `docs/spec/device-import.md` when it imports.
6. **Build the app and check the panel.**
