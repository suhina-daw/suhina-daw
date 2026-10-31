---
description: The VST2 interface as Suhina holds it — the entry points, the AEffect layout field by field, the op-codes in both directions, the event list, the time information and the state chunks, every fact marked with where it comes from, and the mock plug-in, probe and tests that keep each true
read_when: VST2, VST 2, AEffect, opcode, op-code, dispatcher, audioMaster, VstEvents, VstMidiEvent, VstTimeInfo, state chunk, effGetChunk, effSetChunk, VSTPluginMain, main_macho, vestige, mock VST2, probe, vst2 feature, test:vst2, --vst2-probe, SUHINA_TEST_VST2
type: spec
status: draft
last_verified: 2026-10-30
covers:
  - crates/plugin-host/src/vst2/**
  - crates/plugin-host/tests/vst2.rs
  - crates/plugin-host/tests/manual.rs
  - crates/plugins/src/vst2.rs
---

# The VST2 interface

Suhina hosts VST2 plug-ins through the same out-of-process boundary as CLAP and VST3
([ADR 0006](../decisions/0006-plug-in-hosting.md)). This spec holds the interface itself:
where each fact of it comes from, what our code does with it, and what keeps it true. The
interface layer and the host are built, behind the `vst2` feature of `suhina-plugin-host`
(on by default since the ADR's 2026-10-31 amendment lifted the shipping gate; a build can
opt out with `--no-default-features`): the ported definitions, the mock plug-in `test:vst2`,
the probe with its `--vst2-probe` tool, and the host that loads `vst2:<file>` and `test:vst2`
through the engine's boundary — instances, blocks, parameters, state, latency
([#1066](https://github.com/suhina-daw/suhina-daw/issues/1066) steps 1–4). What is not built
yet is below under *Not yet*.

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
- **(observed)** our own probe or an ignored test has seen a real plug-in do it. The
  observed set so far comes from the probe's runs against the installed plug-ins of one
  machine (69 loaded): the entry point's name and its null-effect host calls, the latency
  field, the chunk op-codes' behaviour, the editor rect's shape and indirection, the
  categories, identity strings and parameter labels, a third (`-1`) `CanDo` answer, and
  plug-ins sending MIDI out. The mock is written from the same assumptions, so a
  mock-and-probe pair proves only that our two halves agree; what is *observed* is marked as
  it is settled.

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
exists — it cannot know its effect yet — needs the host's identity answered: the probe's
first run against installed plug-ins *(observed)* showed two of them (Audiomodern's
Filterstep, TAL-NoiseMaker) asking `audioMasterVersion` with a null effect and giving up on
the load when the answer was 0, which the host had answered when this host treated such
calls as nothing to answer. `VSTPluginMain` is the entry name that answered *(observed)*.

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
| `unkown_float` | `f32` | 92 | 60 | VeSTige says `1.0f`, spelling and all; on the 67 installed plug-ins it held 1 exactly where the latency field was non-zero *(observed)* |
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
| 13–15, 19, 20 | editor op-codes (`effEditGetRect`, `effEditOpen`, `effEditClose`, `effEditIdle`, `effEditTop`) | a window id in `ptr` for open (assumed); the rect, close, idle and top requests as the editors section below sets them out | numbers from VeSTige |
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
| 39–42, 45–49 | window, file and offline op-codes | 0, noted | unused by this layer |
| 43, 44 | `audioMasterBeginEdit`, `audioMasterEndEdit` | 0, noted | nothing; the mock sends them around a change of its gain, and the answer staying 0 is the host's choice (the layer keeps no gesture state) |

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
exactly those valid. The host serves the block's events onto it: what sits in a field when
they reach it is the unit the engine named it in — the cycle's beats are quarter-note
positions, and the arrangement's loop given in seconds takes no place in these fields (the
formats that read seconds get it).

## State: chunks and banks

A plug-in either saves its whole state as one opaque chunk, through `effGetChunk` and
`effSetChunk` (both **(assumed)**, above), or as its parameters' values. What a project
file keeps is the standard VST2 bank either way: `CcnK`, then `FBCh` for a chunk bank or
`FxBk` for a program bank, big-endian — the layouts the VST3 SDK's `vst2persistence.h`
describes (MIT), which the Live Set importer already reads and writes
([`crates/plugins/src/vst2.rs`](../../crates/plugins/src/vst2.rs)), one home for the importer, the VST3 hand-over and this host.
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

## The host

The host ([`crates/plugin-host/src/vst2/host.rs`](../../crates/plugin-host/src/vst2/host.rs))
runs a VST2 instance as CLAP and VST3 instances run (ADR 0006): loaded on the process's main
thread, its audio on one of [`crates/plugin-host/src/audio.rs`](../../crates/plugin-host/src/audio.rs)'s
threads through the link, its state and parameters over the control protocol. A plug-in is
named `vst2:<file>` or `vst2:<file>#<id>` (a scan's form, the id checked against what the
file gives back), a macOS bundle's executable looked up as a VST3 bundle's is, or
`test:vst2` for the built-in mock.

The one shape the interface forces, which the other formats do not, is the audio-master
callback: one global function with only the effect pointer to say who is asking. The audio
thread says which instance it is serving in a thread-local set around each `process` call —
no lock, so the audio thread never takes one — and a call from any other thread (an editor's,
a plug-in's worker) finds its instance by its effect under a lock those threads may take.
What the callback answers on the audio thread comes from atomics and lock-free slots only:
the time information field by field, the sample rate and block size, the process level, and
a knob the plug-in turned (`audioMasterAutomate`) through the same lock-free slots the VST3
host uses for its plug-ins' own changes. The `GetTime` answer is a pointer to a per-thread
copy of the time information, valid until the same thread's next `GetTime` *(assumed*
contract, as every host gives*).

- The cycle *(assumed)*: `effOpen`, `effSetSampleRate` (the rate in the call's float),
  `effSetBlockSize` (the size in its value) at load; `effMainsChanged(1)` and
  `effStartProcess` on the audio thread before the first block, `effStopProcess` and
  `effMainsChanged(0)` when it stops (a reconfigure, an unload); `effClose` at unload.
- A block: the song's events become the time information (the transport, tempo, beat, bar
  and cycle of the interface's own fields and validity flags); a parameter the block carries
  (automation) reaches the plug-in through `setParameter` before the block's audio, in the
  order the engine sent — the interface has no sample-accurate parameters *(assumed)*; notes,
  controllers, pitch bend and channel pressure become the interface's MIDI events at their
  frames, as the MIDI wire format carries them, in one `effProcessEvents` call before the
  audio.
- Latency *(assumed)*: the pinned header names no latency field. This host reads the first
  of VeSTige's twelve "zero" bytes as the delay, native byte order, negative read as none;
  `audioMasterIOChanged` makes it read again and tell the engine. The probe reports the
  twelve bytes for every plug-in it meets, which is what settles where a real one keeps its
  delay. A channel-count change with `audioMasterIOChanged` is noted but does not rebuild the
  block's buffers — a limit of this first version, in *Not yet*.
- Parameters: the range is 0 to 1 and each parameter's default is the value it was when the
  plug-in opened, captured once on load *(assumed* both: the pinned header says nothing of a
  range and names no op-code for a default; the value at the opening is the closest to one,
  and a panel's "reset" is that)*. What the plug-in changes itself comes back over the
  protocol as parameter changes, as an editor's move does; the idle calls a plug-in asks for
  (`audioMasterNeedIdle` → `effIdle`) are served from the process's main loop.
- State, as the standard bank above: a chunk plug-in's chunk in a `CcnK`…`FBCh` bank; a
  chunk-less one's parameters as one program in a `CcnK`…`FxBk` bank (the other programs are
  not saved — a limit, below). Loading takes the chunk bank's chunk (`effSetChunk`), a
  program bank's current program (set as the program, then its values as parameters,
  *(assumed)* order), or a bare chunk. A `effSetChunk` answer that is not zero is read as a
  refusal *(assumed)*: the plug-in runs with its defaults and the caller is told, never
  silently restored. A payload whose shape is a standard bank's own (`CcnK` and a bank kind
  in their places) that neither reader took is refused too: a cut-short bank is an error, not
  raw bytes the plug-in would take for its own state.

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
- Missing calls: `a_plug_in_without_calls_is_refused_not_called`. And a flag that says it
  replaces with no replacing call: `a_block_whose_replacing_is_gone_takes_the_deprecated_call`
  runs the block, as the host takes the deprecated call for it.
- The tool: `the_vst2_probe_tool_reports_on_the_mock` (in `tests/manual.rs`) runs
  `--vst2-probe test:vst2` as a process.
- The host, in this process over a real link (`vst2/host_tests.rs`): the latency the assumed
  field holds, blocks with a note at its frame and a parameter the block carries, the
  parameter listing and a set parameter, the idle knob reported as a parameter change, the
  state round trip through both bank forms, a reconfigure; a bank's shape kept out of the
  bare-chunk path, declared counts refused over the cap, the seconds-sized loop out of the
  beat fields, and a lifecycle dispatch's asks served lock-free.
- The host, through the engine's whole boundary (`tests/vst2.rs`): the same round trips with
  the host process started and the control protocol between; the central test of the issue —
  a project naming a VST2 plug-in that only exists as VST2 opens, the plug-in loads with its
  saved bank, plays with it, and the state read back is the settings (`found: itself`); and
  the editor: the mock's editor opens at its rect's size, stays open across the boundary,
  closes (Windows and Linux under an X server; macOS's editor checks run by hand); and the
  editor whose own window the user closed is taken down and told to the engine the same, so
  the panel opens it again (Windows and Linux; elsewhere the hand check).
- The editor over installed plug-ins: `tests/manual.rs`'s editor test takes a VST2 file, so
  an installed editor's open-and-close is the observation for the rect and the handles.
- The real world: `an_installed_vst2_plug_in_answers_the_probe` (ignored, `SUHINA_TEST_VST2`)
  prints the report of an installed plug-in. The facts marked **(observed)** above are the
  probe's own runs against one machine's installed plug-ins; nothing here makes them what
  *every* installation sees until the ignored test runs green against a plug-in of its own.

The crate's CI runs clippy and these tests with the `vst2` feature on (its default since the
ADR's 2026-10-31 amendment) and checks that a build without it still compiles.

## Scanning, folders and the project's choice

The scan reads a VST2 file the same way the host loads it — entry, magic, identity, counts,
category, then close — in the disposable scan worker the other formats use
([`crates/plugin-host/src/vst2/host.rs`](../../crates/plugin-host/src/vst2/host.rs) ›
`describe`). A VST2 plug-in is a `.vst` bundle on macOS, a `.dll` on Windows or a `.so` on
Linux; the system's VST folders are standard ones and any folder can be added for the
format (`plugin.addFolder { folder, format: "vst2" }`), where before the build refused it.
The plug-in manager and the browser name the format `vst2` where a format is named. A
scanned VST2 plug-in's id is its unique id as eight hex digits — the form the model keeps
for a VST2 device ([`DeviceKind::Vst2`](../../crates/model)) — so a project's device and a
scanned plug-in meet by it.

A project's VST2 device is still played by its installed VST3 or CLAP copy when one stands
in for it (ADR 0006 decision 8); the plug-in **itself** — found by its id in the catalog,
`How::Itself` — is the last resort, and its saved state is its own native form. The state's
bank forms are one code path for the importer, the hand-over and this host
([`crates/plugins/src/vst2.rs`](../../crates/plugins/src/vst2.rs)).

## Editors

The editors follow ADR 0006's platform decisions, on the same windows the CLAP and VST3
hosts use ([`crates/plugin-host/src/window.rs`](../../crates/plugin-host/src/window.rs)): a
window of its own (owned by the engine's, when it named one) or an embedded child of the
engine's window where the device panel shows it. The op-code sequence is the interface's
own: `effEditGetRect` for the size, `effEditOpen` given the window's native handle, `effEditIdle`
pumped from the host's idle loop while it is open, `effEditClose` when it closes.

Two of the shapes are settled by the editor tests against installed plug-ins; the rest are
*assumed* as ADR 0022 requires, each with the test that keeps it:

- The rect: `effEditGetRect`'s `ptr` is a pointer slot the plug-in fills with a pointer to
  its own rect, whose four 16-bit fields are top, left, bottom and right — the size the
  corners span *(observed*: the first read took the pointer's bytes for the rect and made a
  window one pixel wide; with the indirection, an installed Filterstep editor reads
  1024 × 768 and TAL-NoiseMaker's 1270 × 695, both their real sizes*)*. The host asks again
  as the editor opens: many plug-ins fill the rect only then.
- The native handle `effEditOpen` takes per platform: a Win32 `HWND` on Windows, the content
  view's pointer on macOS, the X11 id on Linux (*assumed*; the editor tests' mock and the
  ignored editor test against installed plug-ins are the checks).
- `effEditIdle` served from the host's idle loop while the editor is open (*assumed*).
- `audioMasterSizeWindow` asking for a window resize (the width in the call's `index` and
  the height in its `value`), answered confirmed only with an editor open, deferred to the
  main loop to apply, and reported to the engine as an editor resize (`EditorResized`)
  (*assumed*).
- The user's close of the editor's window (its X) closes the editor as the engine's
  "Close Editor" does: the host sees the window ask to close, sends `effEditClose` while the
  window the plug-in opened with still exists, then takes the window down, and tells the
  engine once (`EditorClosed`) — never leaving the editor open to the host alone, so the
  panel's "Open Editor" works again. (*assumed*: the close is the window's and the host's
  observation, no op-code asks a plug-in whether it wants to stay).

A VST2 editor is not user-resizable, as the interface names no op-code to make one so; an
embedded one is sized by the plug-in's ask alone. The room the engine names for an embedded
editor (the fit, #689) is taken by no VST2 call — whether a sizing convention exists is
another open question. On macOS an embedded editor cannot open across processes, and the
rack falls back once to a window of the editor's own (ADR 0006 › 10).

## Not yet

- The `plugin.*` command parts beyond the scan's and the folders' (step 8) — none left found
  so far: the listing, the check and the manager name the format where a format is named.
- The MIDI events a plug-in sends its host: counted, dropped — the engine has no channel for
  a plug-in's MIDI out in any format yet.
- A channel-count change at `audioMasterIOChanged` does not rebuild the block's buffers; the
  latency is re-read and reported, the buffers stay as they were.
- A program bank saves the current program only; the plug-in's other programs are not kept.

## Open questions

Each is an (assumed) fact the probe or the ignored test settles; none may be settled from
another header (ADR 0022 decision 2).

- The 64-bit `AEffect` offsets, against every real plug-in the probe meets (the
  first run's plug-ins read sensibly: counts, id, version, a callable
  `processReplacing`; *(observed, for the ones met so far)*).
- The latency: the host reads the first of VeSTige's twelve "zero" bytes as the delay
  *(assumed)*. The probe reports the twelve and the "1.0f" float for every plug-in it meets;
  a real plug-in's delay showing there — and the host's latency reading matching what its
  maker documents — is the observation that settles it.
- A parameter's default: the pinned header names no op-code to ask; the host captures each
  value as the plug-in opened and reports it as the default. Whether a plug-in's parameters
  move between `effOpen` and its first block (some initialise there) is what the probe
  settles: if they do, the captured values are the wrong snapshot.
- The encoding of `audioMasterGetSampleRate`'s answer (whole number, or a float's bits?).
  The probe answers a whole number; the question is what a real plug-in reads from it.
- The unit of `audioMasterTempoAt`'s answer.
- Whether a plug-in calls its host from inside its entry, and with what effect pointer.
- The buffer sizes of the string op-codes (64 for the name, 128 for the others, assumed).
- The return conventions: the mock answers 0 for a filled request and the probe answers 1
  for its filled strings; what real plug-ins answer for theirs is recorded by the probe.
- `effGetChunk`'s `index` meaning (bank against program) and the answer being the size.
- The `CanDo` strings' spelling, and which side asks which. Arturia's Acid V answered
  `effCanDo` with a third value, **-1**, on `receiveVstEvents` *(observed)*: the layer's
  yes/no read of 1 and 0 stands, and a -1 means the plug-in declaring itself undecided.
- The events a plug-in sends its host: Acid V sent real MIDI rastered to its own step
  (`audioMasterProcessEvents` during `process`) *(observed)* — the window's "no channel yet"
  note gains a real example.
- Entries that crash: three of the 72 scanned files crashed inside their entry calls,
  taking only their disposable probe process with them — the crash behaviour the design
  requires, a scan's failure list the place they show. The cause is theirs; the host never
  reads a plug-in's code.
- A shell file (several plug-ins in one): the pinned header has `effShellGetNextPlugin` and
  nothing of the members' ids, so the scan lists the file's first plug-in and its spec
  checks the id it names; the members' own listing and loading is open until the probe sees
  what a shell answers.
- The state chunk's forms: a real plug-in's chunk (Filterstep's began `VC2!` and came back
  whole through the assumed `effGetChunk`/`effSetChunk` pair, with the size as the answer)
  *(observed for the ones met so far)*; whether a plug-in ever answers the bank-index
  `index` reading 1 (the current program) is unobserved.
- Where an installed plug-in keeps its latency — settled as far as a probe can: over 67
  installed plug-ins run through the probe (one per file, disposable), 52 held a non-zero
  number in the first four of VeSTige's twelve zero bytes — 44 on 33 of them, 32 on 8, 16
  on 6, and 81, 80, 68, 64, 48 on one each — all reading as small frame counts in the byte
  order the host assumes, and 15 held zero, the simple algorithmic effects among them whose
  delay is genuinely none *(observed)*. `AEffect::version`'s neighbour, the field VeSTige
  calls `unkown_float` ("1.0f" in the pinned header), tracked it perfectly: 1 exactly where
  the latency is non-zero, 0 where it is none, on all 67, never another value *(observed)*.
  A plug-in whose documented delay differs from the field would still disprove the read;
  nothing of the kind turned up.

## Sources

- The pinned VeSTige header, as Ardour distributes it: <https://github.com/Ardour/ardour/blob/365cc92cba0e016c30fea34aed168a0957aeeca0/libs/ardour/ardour/vestige/vestige.h>
- [ADR 0022](../decisions/0022-vst2-interface-description.md): the source decision, the
  pinning, and the rule that nothing else may be read.
- [ADR 0006](../decisions/0006-plug-in-hosting.md): the hosting the interface layer serves.
- The VST2 bank layouts in the VST3 SDK's `vst2persistence.h` (MIT), as
  [`crates/plugins/src/vst2.rs`](../../crates/plugins/src/vst2.rs) holds them, one home for the importer, the VST3 hand-over and this host.
- [#1066](https://github.com/suhina-daw/suhina-daw/issues/1066): the work this layer is the
  first step of.
