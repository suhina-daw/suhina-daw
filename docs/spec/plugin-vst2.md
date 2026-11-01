---
description: The VST2 interface as Suhina holds it — the entry points, the AEffect layout field by field, the op-codes in both directions, the event list, the time information and the state chunks, every fact marked with where it comes from, and the mock plug-in, probe and tests that keep each true
read_when: VST2, VST 2, AEffect, opcode, op-code, dispatcher, audioMaster, VstEvents, VstMidiEvent, VstTimeInfo, state chunk, effGetChunk, effSetChunk, VSTPluginMain, main_macho, vestige, mock VST2, probe, vst2 feature, test:vst2, --vst2-probe, SUHINA_TEST_VST2
type: spec
status: draft
last_verified: 2026-10-30
covers:
  - crates/plugin-host/src/vst2/**
  - crates/plugin-host/tests/manual.rs
---

# The VST2 interface

Suhina hosts VST2 plug-ins through the same out-of-process boundary as CLAP and VST3
([ADR 0006](../decisions/0006-plug-in-hosting.md)). This spec holds the interface itself:
where each fact of it comes from, what our code does with it, and what keeps it true. The
host that runs a project's plug-ins through the engine's boundary is not built yet; what
exists is the interface layer behind the `vst2` feature of `suhina-plugin-host`: the ported
definitions, the mock plug-in `test:vst2`, the probe and its `--vst2-probe` tool
([#1066](https://github.com/suhina-daw/suhina-daw/issues/1066) steps 1–3).

## Where every fact comes from

[ADR 0022](../decisions/0022-vst2-interface-description.md) names one source of the
interface: VeSTige, as Ardour distributes it, at the commit the ADR pins
(`365cc92cba0e016c30fea34aed168a0957aeeca0`, blob `0470381141768a2841916af3eeb20014528e65e6`,
10 335 bytes). It is ported into
[`crates/plugin-host/src/vst2/vestige.rs`](../../crates/plugin-host/src/vst2/vestige.rs) with
its copyright and licence notice kept (`GPL-2.0-or-later`, the one named exception to the
clean-room rule), and no other header, SDK, binding or write-up may be read for a fact of
this interface. Every fact below carries one of three marks:

- **(from VeSTige)** the pinned header states it — the value, the field, the layout. The
  port transcribes it and the tests pin the transcription.
- **(assumed)** the pinned header lacks it, and the layer had to choose something to write
  code at all: an op-code number it does not list, which argument of a call carries what,
  what an answer means. The choice is ours, named here, and *it is not established*: only a
  probe run or an ignored test against an installed plug-in can turn it **(observed)**.
- **(observed)** our own probe or an ignored test has seen a real plug-in do it. Nothing in
  this layer is observed yet: the mock is written from the same assumptions, so a
  mock-and-probe pair proves only that our two halves agree.

A fact marked (assumed) that a later run disagrees with is a bug in this spec's assumption,
to be fixed with what the run showed — never by reading another header.

## The entry points

A VST2 plug-in is a dynamically loaded library that exports one entry point, called with the
host's audio-master callback, returning the `AEffect` that is the plug-in instance. The
pinned header has no entry point at all, so everything here is **(assumed)**: the names, the
signature and the order. The probe tries them in this order and its report says which
answered:

| Name | Tried | Notes |
|---|---|---|
| `VSTPluginMain` | first, everywhere | the name this layer expects a current plug-in to export |
| `main_macho` | second | kept for the macOS bundle convention |
| `main` | last | the older name, tried only when the others gave nothing |

An entry that returns null is taken as the plug-in declining to load **(assumed)**; an
answer that is not null is checked for the `AEffect` magic before anything else is called
**(assumed)**. A call a plug-in makes of its host from inside its entry, before the effect
exists, is answered 0 and recorded nowhere **(assumed)**; whether real plug-ins make such
calls is an open question below.

## The `AEffect` layout

One struct per plug-in instance, plain data the plug-in filled; VeSTige's own warning stands:
no virtual functions on it. The field order and the types are VeSTige's; the offsets follow
from C's alignment rules for each pointer width, and are pinned by the tests
(`the_ported_structs_lay_out_as_the_header_orders_their_fields`). VeSTige's own offset
comments describe the 32-bit layout; the 64-bit offsets are **(assumed)** against real
64-bit plug-ins until the ignored probe test has read them off one.

| Field | Type | 64-bit | 32-bit | What it is |
|---|---|---|---|---|
| `magic` | `i32` | 0 | 0 | "PtsV" little-endian, "VstP" big-endian (from VeSTige) |
| `dispatcher` | call | 8 | 4 | everything but audio and parameter values (from VeSTige) |
| `process` | call | 16 | 8 | the deprecated audio call (from VeSTige, "quite sure") |
| `setParameter` | call | 24 | 12 | one parameter's new value (from VeSTige) |
| `getParameter` | call | 32 | 16 | one parameter's value back (from VeSTige) |
| `numPrograms` | `i32` | 40 | 20 | (from VeSTige) |
| `numParams` | `i32` | 44 | 24 | a host reads it before asking any parameter (from VeSTige) |
| `numInputs`, `numOutputs` | `i32` | 48, 52 | 28, 32 | the channels (from VeSTige) |
| `flags` | `i32` | 56 | 36 | the bits below (from VeSTige) |
| `ptr1`, `ptr2` | `*mut c_void` | 64, 72 | 40, 44 | the plug-in's own (from VeSTige) |
| `empty3` | `[c_char; 12]` | 80 | 48 | VeSTige says zeroes; what a real plug-in holds there is an open question (latency) |
| `unkown_float` | `f32` | 92 | 60 | VeSTige says `1.0f`, spelling and all; the probe reports what it really holds |
| `ptr3`, `user` | `*mut c_void` | 96, 104 | 64, 68 | the plug-in's own (from VeSTige); the mock keeps its state in `user`, its own business |
| `uniqueID` | `i32` | 112 | 72 | the four-character id (from VeSTige) |
| `version` | `i32` | 116 | 76 | (from VeSTige) |
| `processReplacing` | call | 120 | 80 | the audio call a host uses (from VeSTige) |

Total 128 bytes on a 64-bit build, 84 on a 32-bit one. The size of the struct and the offset
of every field are asserted per pointer width; the assertions are the test that keeps this
table true. The call fields are `Option` in the port, so a null one cannot be called by
accident, and the probe refuses a call the plug-in does not have instead of making it.

The flags, as far as the pinned header goes: `effFlagsHasEditor` is 1,
`effFlagsCanReplacing` is `1 << 4` (VeSTige itself marks the bit "very likely"),
`effFlagsIsSynth` is `1 << 8` (all from VeSTige). A plug-in that saves its whole state as a
chunk declares it in a bit the header does not name: this layer assumes `1 << 5`
**(assumed)**.

## The op-codes the host sends the plug-in

Through `AEffect::dispatcher(effect, opcode, index, value, ptr, opt)`. The numbers are
VeSTige's where it lists the op-code; which argument carries what, and what the answer means,
the header almost never says, so those are **(assumed)** per row. The layer's own names for
them are in `EffectOp` (`crates/plugin-host/src/vst2/mod.rs`).

| Value | Name | What travels | Mark |
|---|---|---|---|
| 0 | `effOpen` | nothing; first call after the entry | number from VeSTige, meaning assumed |
| 1 | `effClose` | nothing; last call | from VeSTige (it marks the op-code "currently unused") |
| 2 | `effSetProgram` | the program number in `value` | number from VeSTige, meaning assumed |
| 3 | `effGetProgram` | answers the program number | as above |
| 5 | `effGetProgramName` | `ptr` a buffer of 64 bytes (VeSTige's `VestigeMaxNameLen`) | number from VeSTige, buffer size assumed |
| 6 | `effGetParamLabel` | `index` the parameter, `ptr` a buffer of 128 bytes | **(assumed)**: the number is not in the pinned header; it sits between two that are (5 and 8) |
| 7 | `effGetParamDisplay` | as `effGetParamLabel`, the value's text | **(assumed)**, as above |
| 8 | `effGetParamName` | as `effGetParamLabel`, the name | number from VeSTige, the rest assumed |
| 10 | `effSetSampleRate` | the rate in the call's `opt` float | number from VeSTige, meaning assumed |
| 11 | `effSetBlockSize` | the size in the call's `value` | number from VeSTige, meaning assumed |
| 12 | `effMainsChanged` | non-zero `value` resumes, zero suspends | number from VeSTige, meaning assumed |
| 13–15, 19, 20 | editor op-codes (`effEditGetRect`, `effEditOpen`, `effEditClose`, `effEditIdle`, `effEditTop`) | a window id in `ptr` for open (assumed); nothing yet — editors are a later step | numbers from VeSTige |
| 23 | `effGetChunk` | `index` 0 the bank or 1 the current program (assumed); `ptr` points at a pointer the plug-in fills (assumed); the answer is the chunk's size in bytes (assumed) | **(assumed)**: the number is not in the pinned header |
| 24 | `effSetChunk` | `index` as above (assumed); `value` the size (assumed); `ptr` the bytes (assumed) | **(assumed)**, as above |
| 25 | `effProcessEvents` | `ptr` the event list below | number from VeSTige, meaning assumed |
| 26 | `effCanBeAutomated` | `index` the parameter; 1 means yes (assumed) | number from VeSTige, meaning assumed |
| 35 | `effGetPlugCategory` | answers VeSTige's category numbers | from VeSTige |
| 45, 47, 48, 49 | `effGetEffectName`, `effGetVendorString`, `effGetProductString`, `effGetVendorVersion` | `ptr` buffers of 64, 128, 128 bytes (assumed sizes); the version answers as a number | numbers from VeSTige, sizes assumed |
| 51 | `effCanDo` | `ptr` a C string naming what is asked (assumed); 1 means yes (assumed) | number from VeSTige, meaning assumed |
| 53, 56, 58 | `effIdle`, `effGetParameterProperties`, `effGetVstVersion` | the properties fill `ptr` with VeSTige's struct; the version answers 2400 for a 2.4 plug-in (assumed) | numbers from VeSTige |
| 67, 68 | `effBeginSetProgram`, `effEndSetProgram` | bracket a program change | from VeSTige |
| 70 | `effShellGetNextPlugin` | not used yet: shells are a scanning matter | from VeSTige |
| 71, 72 | `effStartProcess`, `effStopProcess` | bracket the processing cycle | from VeSTige |

The port also carries, with VeSTige's values and unused: nothing between 0 and 72 beyond
these — the pinned header lists no other op-code, and this layer invents none beyond the
four marked (assumed).

## The op-codes the plug-in sends the host

Through the audio-master callback the entry was given:
`callback(effect, opcode, index, value, ptr, opt)`. Every number below is **(from VeSTige)**
(the header lists them all); what our probe answers is the host's own choice, and which
argument carries what is **(assumed)** per row where the layer has to read one.

| Value | Name | What the probe answers | What travels |
|---|---|---|---|
| 0 | `audioMasterAutomate` | 0 | the parameter in `index`, the new value in `opt` (assumed) |
| 1 | `audioMasterVersion` | 2400 | nothing; 2400 stands for a 2.4 host (assumed answer) |
| 2 | `audioMasterCurrentId` | the effect's own `uniqueID` | the meaning is assumed |
| 3, 4, 6, 14 | `audioMasterIdle`, `PinConnected`, `WantMidi`, `NeedIdle` | 0, noted | nothing |
| 7 | `audioMasterGetTime` | a pointer to the host's `VstTimeInfo` | the answer being a pointer, not a filled buffer, is assumed |
| 8 | `audioMasterProcessEvents` | 0 | `ptr` the plug-in's event list (assumed) |
| 10 | `audioMasterTempoAt` | 0 | the unit of the answer is not observed; the probe declares no tempo |
| 13 | `audioMasterIOChanged` | 0, and the counts re-read | nothing |
| 15, 18, 19 | `SizeWindow`, `GetInputLatency`, `GetOutputLatency` | 0 (the probe is no window and no device) | the width and height in `index`/`value` (assumed) |
| 16, 17 | `GetSampleRate`, `GetBlockSize` | 48000 and 512 | the rate answers as a whole number; the encoding of a float answer is an open question |
| 23 | `GetCurrentProcessLevel` | 4 (offline) | the answer values 0–4 are assumed |
| 24 | `GetAutomationState` | 1 (off) | the answer values 0–4 are assumed |
| 32, 33, 34 | `GetVendorString`, `GetProductString`, `GetVendorVersion` | "Suhina", "Suhina VST2 probe", 1 | `ptr` buffers of 128 bytes (assumed); 1 says filled (assumed) |
| 35, 36 | `VendorSpecific`, `SetIcon` | 0, noted | nothing |
| 37 | `CanDo` | 1 for the three strings below, 0 for anything else | `ptr` a C string (assumed) |
| 38 | `GetLanguage` | 1, VeSTige's `kVstLangEnglish` | from VeSTige |
| 39–49 | window, file and offline op-codes | 0, noted | unused by this layer |

The `CanDo` strings are **(assumed)** in spelling and in who asks whom for them: the plug-in
asks the host for `sendVstEvents`, `sendVstMidiEvent` and `sendVstTimeInfo`; the host asks
the plug-in for `receiveVstEvents`, `receiveVstMidiEvent` and `receiveVstTimeInfo`. A probe
report of what an installed plug-in actually asks is the observation that settles the
spelling.

## The event list

A block's events travel as a `VstEvents`: the count, a reserved pointer, then the event
pointers in line behind the struct, where C's flexible array member puts them (from
VeSTige). Each MIDI event is a 32-byte `VstMidiEvent`, all fixed-width fields, the same on
every build (from VeSTige, whose own offset comments say so): the type (1 is MIDI, from
VeSTige), the struct's size, the sample offset into the block, flags the header names no
values for, a note's length and offset, four MIDI bytes, detune, note-off velocity and two
reserved bytes.

The layer reads a list with a bound (at most 1024 events; a negative or lying count reads
nothing or to the bound) and builds one as a `repr(C)` triple — head, pointers, events —
pointed at its own events by `EventList::ready` once it has its final address. Both halves
are tested against each other and against a list whose count lies.

## The time information

A `VstTimeInfo` of eight doubles and six 32-bit integers, 88 bytes, the same on every build
(from VeSTige): the sample position, the sample rate, the clock, the quarter-note position,
the tempo, the bar start, the cycle's start and end, the time signature, the SMPTE fields
and the clock remainder. Which fields hold a value is what `flags` says, with VeSTige's
validity bits (`kVstTempoValid`, `kVstPpqPosValid`, `kVstTimeSigValid`, and the rest). The
probe fills the rate, the tempo, the quarter-note position and a 4/4 signature, and marks
exactly those valid.

## State: chunks and banks

A plug-in either saves its whole state as one opaque chunk, through `effGetChunk` and
`effSetChunk` (both **(assumed)**, above), or as its parameters' values. What a project
file keeps is the standard VST2 bank either way: `CcnK`, then `FBCh` for a chunk bank or
`FxBk` for a program bank, big-endian — the layouts the VST3 SDK's `vst2persistence.h`
describes (MIT), which the Live Set importer already reads and writes
([`crates/als/src/vst2.rs`](../../crates/als/src/vst2.rs)); the same layouts serve here.
A plug-in's chunk is copied out at once and capped at 1 MiB; a chunk's size is the plug-in's
own claim, not trusted.

## The mock plug-in and the probe

`test:vst2` ([`crates/plugin-host/src/vst2/test_vst2.rs`](../../crates/plugin-host/src/vst2/test_vst2.rs))
is a gain, one parameter, two channels in and out, its state the gain as four bytes — the
same shape as `test:vst3`, so the two formats' test paths stay comparable. It records every
call the host makes and makes the calls a plug-in makes: version and `CanDo` and the block
sizes as it opens, the process level and the time information as it runs, and on the first
`effEditIdle` it turns its gain through `audioMasterBeginEdit`/`Automate`/`EndEdit` as a
user turning a knob would.

The probe ([`crates/plugin-host/src/vst2/probe.rs`](../../crates/plugin-host/src/vst2/probe.rs))
is the host half: it loads a file or the mock, hands its callback over, and runs the whole
conversation — open, sizes, identity, parameters, a chunk round trip, one block with events,
the cycle's stop, close — recording both directions and printing a report that marks its
assumptions. It answers its plug-in through a registry keyed by the effect, as a real host
looks an effect up in its own.

```sh
# The tool, by hand (a disposable process, as the scanner's are):
suhina-plugin-host --vst2-probe /path/to/a/plug.vst
suhina-plugin-host --vst2-probe test:vst2

# The ignored test, against an installed plug-in (the report is printed; this is what
# settles the assumed facts):
SUHINA_TEST_VST2="/path/to/a/vst2 plug.vst" \
  cargo test -p suhina-plugin-host --features vst2 --test manual -- --ignored --nocapture
```

## What keeps each fact true

- The ported values and the layout: `the_ported_op_codes_hold_vestiges_numbers_and_our_assumed_four`
  pins every op-code number, the magic, the flag bits and the packing; `the_ported_structs_lay_out_as_the_header_orders_their_fields`
  pins every offset and size per pointer width. A typo in the port or a silent change of an
  assumption fails one of these.
- The conversation: `the_mock_and_the_probe_agree_on_the_conversation` runs the probe over
  the mock and checks both directions — every op-code the host sent, the arguments each
  carried, every call the mock made and what it was answered.
- The state round trip: `a_chunk_round_trip_keeps_the_state_through_a_parameter_move`.
- The reader's bounds: `event_lists_round_trip_and_a_lying_count_reads_to_its_bound`.
- Missing calls: `a_plug_in_without_calls_is_refused_not_called`.
- The tool: `the_vst2_probe_tool_reports_on_the_mock` (in `tests/manual.rs`) runs
  `--vst2-probe test:vst2` as a process.
- The real world: `an_installed_vst2_plug_in_answers_the_probe` (ignored, `SUHINA_TEST_VST2`)
  prints the report of an installed plug-in. **Nothing is (observed) until this runs green
  against one.**

The crate's CI runs clippy and these tests with the `vst2` feature on, so the gated code is
checked although nothing ships it (ADR 0022 decision 5: legal advice precedes any release
that does).

## Not yet

- The host itself: a `vst2:<file>` name in the host process, instances behind the engine's
  boundary, blocks through the shared memory with the block deadline, latency, state on
  save, restarts (step 4 of #1066).
- Scanning VST2 files, the folder format choice becoming available, `add-folder --format
  vst2`, the plug-in manager's format column (step 5).
- Programs and banks loading and saving, and the hand-over of VST2 state to a VST3 or CLAP
  replacement in both directions in one place (step 6, with the epic's migration work).
- Editors (step 7), the `plugin.*` command parts (step 8), and the settings guide's VST2
  entries losing their "unavailable" wording in the same change that makes them work.

## Open questions

Each is an (assumed) fact the probe or the ignored test settles; none may be settled from
another header (ADR 0022 decision 2).

- The 64-bit `AEffect` offsets, against a real 64-bit plug-in (the layout test pins our
  struct; reading a real one's fields — a sane `numParams`, a callable dispatcher, a sane
  id — is the observation).
- The latency: the pinned header names no latency field, and a host needs one. VeSTige's
  twelve "zero" bytes and its "1.0f" float are where a real plug-in's numbers would show;
  the probe reports both for every plug-in it meets.
- The encoding of `audioMasterGetSampleRate`'s answer (whole number, or a float's bits?).
  The probe answers a whole number; the question is what a real plug-in reads from it.
- The unit of `audioMasterTempoAt`'s answer.
- Whether a plug-in calls its host from inside its entry, and with what effect pointer.
- The buffer sizes of the string op-codes (64 for the name, 128 for the others, assumed).
- The return conventions: the mock answers 0 for a filled request and the probe answers 1
  for its filled strings; what real plug-ins answer for theirs is recorded by the probe.
- `effGetChunk`'s `index` meaning (bank against program) and the answer being the size.
- The `CanDo` strings' spelling, and which side asks which.

## Sources

- The pinned VeSTige header, as Ardour distributes it: <https://github.com/Ardour/ardour/blob/365cc92cba0e016c30fea34aed168a0957aeeca0/libs/ardour/ardour/vestige/vestige.h>
- [ADR 0022](../decisions/0022-vst2-interface-description.md): the source decision, the
  pinning, and the rule that nothing else may be read.
- [ADR 0006](../decisions/0006-plug-in-hosting.md): the hosting the interface layer serves.
- The VST2 bank layouts in the VST3 SDK's `vst2persistence.h` (MIT), as
  [`crates/als/src/vst2.rs`](../../crates/als/src/vst2.rs) already reads them.
- [#1066](https://github.com/suhina-daw/suhina-daw/issues/1066): the work this layer is the
  first step of.
