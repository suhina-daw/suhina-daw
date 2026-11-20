---
description: Export native devices through the existing plug-in ABI boundary, sharing device identity, parameters, rendering and panels
read_when: native device export, stock CLAP, stock VST3, export adapter, plugin factory
type: decision
status: proposed
built: partly
last_verified: 2026-11-20
covers:
  - crates/plugin-host/src/lib.rs
  - crates/plugin-host/src/exports/
---

# 0040: Native-device plug-in exports

## Context

[Issue 931](https://github.com/suhina-daw/suhina-daw/issues/931) requires the native
effects and instruments to work outside Suhina. The comprehensive instrument in
[issue 1579](https://github.com/suhina-daw/suhina-daw/issues/1579) uses this same path.
The native `Device` and registry already separate sound and panel definitions from
the engine. A second synthesis implementation would break that useful boundary.

## Decision

1. **Use the existing ABI dependencies.** CLAP uses `clack-plugin`; VST3 uses `vst3`.
   Both are already dependencies of the sandbox host and licensed MIT or Apache-2.0.
   A general framework would introduce another parameter, state and editor model
   alongside Suhina's. The wrappers stay thin and delegate to the native contract.
2. **Keep ABI code in `plugin-host`.** Add an opt-in library/CDylib target to that
   crate, depending downwards on `device-api` and `device-registry`. No device or
   engine depends on this target. Its `exports` module owns the safe adapter;
   small format-facing modules own raw pointers and entry-point macros. This uses
   the existing permitted unsafe boundary and does not add one elsewhere. Suhina
   still loads these binaries through the sandbox process in ADR 0006.
3. **One factory, the common registry.** Each built-in has its own stable plug-in
   identity derived from its native id. Both formats instantiate the same `Device`.
   An unsupported state/resource capability is explicit, never silently replaced
   with a different instrument or a second DSP implementation.
4. **Names identify persisted parameters.** Numeric host ids are a documented,
   deterministic hash of each stable parameter id, checked for collisions before
   instantiation. Parameter order is only presentation. CLAP exposes native plain
   ranges; VST3 converts using `ParamInfo`'s existing normalization law. State is a
   bounded, versioned envelope containing the device id and stable parameter ids.
   Asset-backed devices additionally require portable resource state before their
   exports can be declared complete.
5. **The adapter owns time, not sound.** Prepared, bounded buffers convert host
   channels to interleaved stereo. Parameter and note events split rendering at
   their frame, advancing musical position for each span. Key input uses
   `Device::process_keyed`; missing keys use `Device::process`. Host values are
   validated at this boundary. Preparation/state parsing/resource loading happen
   off the audio callback; rendering never allocates, frees, locks or performs I/O.
6. **Expression has one contract.** Ordinary events use `device-api::NoteEvent`.
   Per-note identity and expression use the shared contract of ADR 0039 when that
   work lands, including release/voice lifetime and bounded overflow handling.
   Export adapters must not invent their own competing note-identity model.
7. **Panels remain shared.** SDK panel definitions and bindings are the source for
   stock and exported editors. Drawing belongs in a reusable UI-layer component,
   above the toolkit-independent SDK and style engine (ADR 0031), with a default
   theme and portable CSS/SVG/PNG assets. The export does not load application
   preferences or rely on desktop transparency. A parameter-only host editor is
   an explicitly incomplete stage, not completion of the common editor work.
8. **Release exports with the application version.** Package one multi-device CLAP
   binary and one multi-device VST3 bundle for each supported desktop platform.
   Stable class/parameter identities survive releases. CI scans the built artifacts
   through the actual sandbox host and compares fixed renders, notes, automation,
   state and sidechains with native devices. Registry coverage is checked so a new
   device cannot silently miss either factory.

## Consequences and alternatives

The sandbox executable's loading path remains separate from the exported library.
There is no dependency cycle: the adapter reads lower-layer native devices, while
the engine never calls the adapter. The ABI dependencies' own unsafe operations
remain an audited external boundary, not permission to put unsafe in DSP.

A high-level plug-in framework can save boilerplate, but its own editor and
parameter lifecycle would need bridging anyway. A new export crate would require
another unsafe exception without improving this first boundary. Neither is needed
for the existing public bindings. A `.sd` graph is not exported by this decision.

What is built: [the export specification](../spec/device-exports.md) › Status.

## Sources

- [CLAP public headers](https://github.com/free-audio/clap), including events,
  parameters, audio ports and state extensions (MIT).
- [Clack](https://github.com/prokopyl/clack), its released 0.2 API and source
  (MIT/Apache-2.0); read 2026-11-19.
- [VST3 Rust bindings](https://github.com/coupler-rs/vst3-rs), released 0.3 bindings
  and gain example (MIT/Apache-2.0); read 2026-11-19.
- [VST3 interfaces](https://github.com/steinbergmedia/vst3_pluginterfaces), public
  component/controller/process contracts (MIT).
