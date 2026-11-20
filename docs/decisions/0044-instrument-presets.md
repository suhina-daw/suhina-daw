---
description: Native Instrument presets preserve the canonical model and a bounded source identity through the existing preset codec and commands
read_when: Instrument preset, collected preset, modal profile persistence, source digest, preset version 2
type: decision
status: proposed
built: no
last_verified: 2026-11-20
covers:
  - crates/device-registry/
  - crates/session/src/devices.rs
  - crates/model/src/lib.rs
---

# 0044: Native Instrument presets and collected sources

## Context

The preset codec in [ADR 0014](0014-stock-presets.md) already identifies devices and their
parameters. Instrument also needs its accepted canonical modal profile and optional sample.
Saving only its controls loses the sound. Its legacy sample reference occupies device state,
while the canonical profile is a separate model extension and must survive path rebasing.

## Decision

1. **Extend the existing preset document.** Version 2 carries Instrument's canonical model
   payload and one optional sample identity. Other devices continue writing version 1, which
   remains readable. The document stays bounded to 64 KiB; no second model schema or renderer
   is introduced. Unknown revisions are refused before project mutation.
2. **Reference by default, collect explicitly.** An ordinary source reference is relative to
   the user's library. A collected preset places the source beside the preset, under an asset
   directory containing its content digest, preserving the owner's filename. Its path is
   relative to the preset folder. Moving that folder preserves the sound without reanalysis.
3. **Verify identity before use.** Source metadata records SHA-256 and encoded byte length.
   SHA-256 is already used by Suhina's shipped package implementations; this small preset
   boundary does not claim to implement ADR 0026's proposed general project manifest.
   A digest never grants permission to read an otherwise untrusted path.
4. **Publish complete data and preserve explicit edits.** Validate and prepare before adding
   a device. Save the preset last with the existing atomic file writer; failed saves cannot
   truncate an earlier preset. Missing sources keep their reference and use the existing
   locate/relink workflow. A matching relink preserves the model; replacing source material
   is an explicit sample-load edit.
5. **Reuse command and authorization boundaries.** `device.savePreset`, `device.add` and
   `track.add` share this codec. Existing agent-origin library-write restrictions remain.
   File decoding, hashing and collection happen outside audio processing.

## Consequences

One plain preset and its optional asset folder are portable. Archive transport, multi-source
maps, future model migrations and plug-in resource state remain separate deliveries under
#1620 and #931. This decision does not assert an asset's redistribution rights or convert
user sources into factory content.

## Alternatives

Embedding decoded audio would make ordinary saves large. Depending on the theme package would
couple instruments to style validation. The existing project ZIP extractor is project-specific
and permits partial filesystem extraction; it is not an atomic preset loader. None is needed
for this first collected-folder delivery.

## Sources

- Existing preset, sample-reference, collection and relink implementations.
- [Instrument presets](../spec/instrument-presets.md), the bounded format and command contract.
- [ADR 0036](0036-library-and-shipped-content.md), library-relative references and preset saving.
