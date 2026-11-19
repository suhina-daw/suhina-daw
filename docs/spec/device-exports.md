---
description: Native-device CLAP and VST3 export identities, host conversion, state and verification contracts
read_when: native device export, stock CLAP, stock VST3, plugin factory, export state
type: spec
status: draft
last_verified: 2026-11-19
covers:
  - crates/plugin-host/src/exports/
---

# Native device exports

The architecture is [ADR 0040](../decisions/0040-native-device-exports.md).
This work belongs to issues 931 and 1579.

## Status

The shared adapter and CLAP entry point are implemented. The CLAP factory lists
parameter-only native audio devices; sample sources, external-file devices and
MIDI effects are excluded until their complete state and output contracts exist.
The adapter has stock render parity, sample-offset automation/key-input and
malformed-state tests. Real CLAP factory scanning is verified on Windows; the
host render/state test is being completed. VST3 is the next format adapter.
No release is yet advertised as covering every native device, resource state,
MPE, editor or supported desktop platform.

## Stable identity and parameters

A CLAP id is `org.suhina.device.<native-id>`. A numeric parameter id is the
32-bit FNV-1a hash of the UTF-8 native parameter id, masked to 31 bits; registry
validation rejects collisions within a device. Display names and ordering may
change without changing automation identity. CLAP values are plain values;
VST3 normalized values use `ParamInfo::to_normalized` and `from_normalized`.

## Audio and events

The prepared adapter accepts at most 32,768 frames per host block. It rejects
larger blocks and invalid channel lengths before touching the native device.
Events must be sorted by frame and fall within the block; equal-frame events
retain host order. Each event is applied before that frame is rendered. A beat
position advances with frame offset only while transport is playing. Devices
receive context before frame-zero notes. A separate stereo key input calls the
same `process_keyed` implementation as stock sidechains.

## State

State uses a bounded binary envelope with magic `SHDV`, version 1, UTF-8 device
id and a count of stable UTF-8 parameter ids with little-endian f64 plain values.
The whole envelope is limited to 1 MiB, parameter count to 4,096 and each id to
256 bytes. Invalid lengths, duplicate ids, mismatched devices, non-finite
values, unknown versions and trailing bytes are rejected before any live value
changes. Unknown parameter ids are ignored for forward compatibility; absent
parameters retain their current values. DSP delay lines and sounding voices
are transient and are reset when the audio processor accepts a state at a block
boundary. The control thread publishes a validated transaction to its own fixed
atomic bank. The audio thread reads one bounded snapshot, checks its generation
once and defers if publication intervenes. It never waits or allocates. The pending
bank has one writer; audio-thread automation writes a separate current-value bank.
Accepted state is immediately visible to host value/save queries, including while
stopped. State is applied before the next block's ordered automation events.

## Not yet

- The shared SDK editor with portable themes and assets.
- The VST3 format adapter and package.
- Asset-backed sample/model/impulse response state and missing-resource handling.
- Canonical per-note expression (ADR 0039), MIDI-effect output and full bus parity.
- Cross-platform packaged releases and all-device host round-trip CI gates.

## Sources

The public format and binding sources are recorded in ADR 0040. No proprietary
implementation, assets or binary analysis is used.
