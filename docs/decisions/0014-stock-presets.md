---
description: Presets for built-in and SDK-authored devices - where stock presets live, their licence (CC0), contribution rules and the shared format
read_when: preset, stock preset, factory preset, preset contribution, CC0, presets folder, preset format, preset licence, DX7 bank, .sd preset
type: decision
status: accepted
built: partly
last_verified: 2026-10-16
covers:
  - presets/**
  - .github/PULL_REQUEST_TEMPLATE/preset.md
  - .github/ISSUE_TEMPLATE/5-preset.yml
  - crates/device-registry/src/lib.rs
---

# 0014: Presets for devices

- **Date:** 2026-10-16

## Context

Built-in effects and instruments ([ADR 0004](0004-built-in-devices.md)) and SDK-authored `.sd`
devices need starting points: presets that ship with Suhina and presets users save themselves.
The only preset Suhina reads today is a DX7 voice for the FM synth
(`device-registry::preset`). The shared preset loader and user preset saving are not implemented.

Presets are data, not code: a device id and parameter values, a name and a few tags. Whether a
contributor made a preset themselves cannot be checked, any more than for code. A plain list of
parameter values has thin protection, if any; what can carry rights is a whole bank, its names
and descriptions, and the files themselves.

## Decision

1. **Stock presets live in this repository under `presets/<device id>/`**, one file per preset,
   so a device and its presets can change in one pull request and be tested by the same checks.
   User-saved presets use the same format in the settings folder. The files can move to a
   repository of their own later without changing the format.
2. **Licence: CC0-1.0.** Everything under `presets/` is dedicated to the public domain, so music
   and presets people make from them carry no conditions. GPL fits data like this badly.
   `presets/README.md` says so and every pull request that adds a preset agrees to it.
3. **What a contribution needs:**
   - the contributor's sign-off ([DCO](../../CONTRIBUTING.md#developer-certificate-of-origin)),
     which states they have the right to submit it under CC0;
   - names, tags and descriptions that do not name or imitate commercial products or their
     presets ("warm bass", not "<product> bass");
   - no banks or files from commercial products, for example the factory voice banks of a
     hardware synthesizer, even when Suhina can read their format;
   - parameter values inside each parameter's range, with every declared parameter given.

   There is no clean-room review for presets: making a sound is not reimplementing a product. If
   a report says a preset infringes, [`IP.md`](../../IP.md) applies like for code.
4. **Format:** one versioned JSON format (at most 64 KiB) applies to built-in and `.sd` devices.
   It stores a device id, role (`effect`, `midi_effect` or `instrument`) and plain values keyed by
   stable parameter ids. For an `.sd` device, `device` is the graph's stable device id and
   `device_version` is its revision; the preset does not contain or replace the `.sd` graph. Only
   declared graph parameters are preset values; graph nodes and their internal state are not
   exposed as preset controls.

   A preset applies only to an existing device with the same id and role. For `.sd` devices, the
   target must also have a revision equal to or newer than `device_version`. Applying an older
   `.sd` preset uses the graph's declared parameter-id migrations from that revision. If there is
   no migration, loading is refused. Unknown parameter ids are ignored and newly added parameters
   take their defaults. Presets do not create a device on an empty track; resolving or installing
   the `.sd` graph is a separate feature.

   ```json
   {
     "format": "suhina-preset",
     "version": 1,
     "device": "org.example.filter",
     "role": "effect",
     "device_version": 2,
     "name": "Soft filter",
     "author": "github-handle",
     "tags": ["gentle", "filter"],
     "params": { "cutoff": 1800.0, "resonance": 0.7 }
   }
   ```

   `device_version` is required for `.sd` presets and omitted for built-in devices. Readers check
   known parameter ids and ranges; submitted presets include every parameter declared by their
   device revision. Missing values use the target parameter's default so a preset remains usable
   when a device adds parameters.

5. **Tests:** every preset file is read and checked against the device's parameter descriptions
   (known parameter ids, values in range). Loading a built-in preset with `device.add {preset}`
   gives the same values; applying an `.sd` preset to a matching existing device gives its migrated
   parameter values. A render of a short phrase through each preset is added when the device has
   golden renders.

## Consequences

- `.github/PULL_REQUEST_TEMPLATE/preset.md` and the *Preset contribution* issue form ask for the
  points in 3; [`CONTRIBUTING.md`](../../CONTRIBUTING.md) links them.
- The browser lists stock presets from `presets/` next to the user's own once the loader exists.

## Not decided yet

- The shared loader in `device-registry`, `.sd` preset application to an existing device, and the
  browser entries for stock and user presets.
- Saving the user's own presets in the settings folder.
- CLAP preset discovery and VST3 presets for plug-ins.
