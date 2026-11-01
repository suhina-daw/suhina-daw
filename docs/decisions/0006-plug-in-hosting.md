---
description: How third-party plug-ins are hosted out of process (process model, IPC, deadline, crashes), the unsafe exception, formats and the VST2 policy
read_when: plug-in, plugin host, CLAP, VST3, VST2, sandbox, out of process, IPC, shared memory, unsafe, plug-in crash, scanning, plug-in editor
type: decision
status: accepted
built: yes
last_verified: 2026-11-02
covers:
  - crates/ipc/**
  - crates/plugins/**
  - crates/plugin-host/**
  - crates/realtime/**
  - crates/engine/src/plugin.rs
  - crates/session/src/rack.rs
  - crates/output/src/lib.rs
  - crates/engine/src/chain.rs
  - crates/session/src/devices.rs
---

# 0006: Plug-in hosting

- **Date:** 2026-10-11
- **Issue:** [#30](https://github.com/suhina-daw/suhina-daw/issues/30)
- **Amended:** 2026-10-12 to 2026-10-31 (editors, scanning, groups, output changes, stopping
  instruments); written as one decision 2026-11-02 (#759), with the detail in
  [plug-in hosting](../spec/plugin-hosting.md) and [the plug-in protocol](../spec/plugin-protocol.md)

Community `.sd` effects are a separate, data-only path (ADR 0021): the SDK validates and
prepares their supported graph nodes as fixed Rust processors in the engine. They load no
community executable code and do not cross the plug-in-host IPC boundary.

## Context

AGENTS.md rule 5: third-party plug-ins never run in the Suhina process. A plug-in that crashes
or hangs must not stop the engine or lose the project; the host reports it, keeps the project
intact and can restart it. Projects imported from other DAWs already keep their plug-ins' ids
and state (`DeviceKind::Clap`, `Vst3`, `Vst2` with `Device::state`); nothing runs them yet.

**What other hosts do** (sources at the end):

- Bitwig Studio runs plug-ins in sandbox processes: all together (its default), per vendor, per
  plug-in, or per instance, with a "Reload Plug-in" button when one crashes. REAPER bridges
  plug-ins into one shared or one dedicated process on request; its default is in process.
- Carla's bridges and yabridge keep audio in shared memory and synchronise once per block.
  yabridge uses one process per plug-in by default and groups for plug-ins whose instances talk
  to each other.
- Ardour does not sandbox: by its estimate each crossing costs two context switches, about 30 µs,
  which adds up when every plug-in crosses on its own. Apple measured about 40 µs per render
  cycle for out-of-process Audio Units. Grouping plug-ins so that one crossing serves several of
  them is what keeps the cost down.

**Formats and licences** (checked 2026-10-11):

- CLAP is MIT. Rust: `clap-sys` and `clack` (MIT/Apache-2.0).
- The VST3 SDK is MIT since version 3.8 (October 2025); GPLv3 and Steinberg's own licence no
  longer apply to it. Rust: `vst3` (MIT/Apache-2.0), bindings generated from the SDK.
- VST2: Steinberg stopped signing VST2 agreements in 2018 and has said new VST2 hosts cannot be
  licensed. Its SDK licence is not GPL-compatible. Free headers exist, all reverse-engineering
  results: VeSTige (GPLv2+, from LMMS, shipped by Ardour and vendored by yabridge; the target of a
  DMCA notice in 2018), FST (GPLv3+; takes names from JUCE sources that shipped Steinberg's headers)
  and Xaymar's `vst2sdk` (BSD-3-Clause, used by DPF and by Lemon Studio).
  [ADR 0022](0022-vst2-interface-description.md) takes VeSTige, pinned, as the one source of the
  interface, a named exception to the clean-room rules; the others stay excluded.
- VST3 has a documented way to replace a VST2 plug-in: the VST3 version declares the VST2 class
  it replaces (`moduleinfo.json` `Compatibility`, SDK 3.7.5 and later, or
  `IPluginCompatibility`, or the documented class-id convention built from the VST2 id and
  name), and accepts the VST2 state (fxb/fxp) in `setState`. Cubase, Nuendo, REAPER and Studio One
  use it.

**`unsafe`:** loading a plug-in is calling into a foreign C ABI, and memory shared between
processes can change under Rust's feet; neither is possible without `unsafe`, which the
workspace forbids.

## Decision

1. **The engine never loads a plug-in.** A separate executable, `suhina-plugin-host`, loads
   plug-ins and runs them; the engine talks to it only through the `ipc` crate.
2. **Process model.** By default one host process per plug-in binary (every instance of one
   plug-in together), so plug-ins whose instances share state still work and one crash takes
   down one product, not all. Per plug-in the user can choose *its own process per instance* or
   *one process per vendor* (`plugin.setHosting`); an ARA plug-in is never hosted alone
   ([ADR 0033](0033-ara-hosting.md) › 2). A process runs any number of instances, each with its
   own link and audio thread, and a restart replaces the whole process when it died or hung.
   Consecutive enabled plug-ins of a chain that live in the same process run in **one round
   trip** per block (#108), with the same output as one round trip each.
3. **The boundary** is shared memory and messages. One region per instance carries a block's
   header, its audio buffers and up to 1024 events with their frames (notes, controllers,
   parameter values, the song's transport and tempo), so automation stays sample-accurate and
   neither side allocates. The engine's side makes each region and the operating system frees it
   when both sides let go, so a crash leaves nothing. Control messages go over a local socket or
   pipe with a **versioned protocol** that both sides check. The messages, the regions and their
   versions are in [the plug-in protocol](../spec/plugin-protocol.md).
4. **Block sync and deadline.** The engine signals the host, the host processes and signals
   back, on the system's own primitive (a futex on Linux, events with a high-resolution timer on
   Windows, short naps on macOS). The host's audio thread runs at the engine's real-time
   priority (`suhina-realtime`). The engine waits at most until the block's deadline, the one
   block allowed on the audio thread (AGENTS.md rule 3); on a miss it outputs the plug-in's input
   (an effect) or silence (an instrument) and counts the miss. Repeated misses, a dead process or
   an unanswered ping mark the plug-in *crashed* or *hung*, and the engine ends the process and
   offers a restart. Chains that nothing plays into run their plug-ins **at the same time**
   (#151), so the budget is set by the slowest chain, not the sum. Offline renders wait without
   a deadline. How the deadline is set, the priorities each system gives and the measurements
   behind them: [plug-in hosting](../spec/plugin-hosting.md) › Block sync and the deadline.
5. **State survives crashes.** The host reads each plug-in's state on save and every 30 s; the
   project keeps it in `Device::state`, and a restart loads the last state with the latency the
   chain was compensated for. A state changed in the editor makes the project unsaved without an
   undo step ([ADR 0010](0010-project-files.md) › 7). Renders and exports of an open project
   use the states as they are heard.
6. **`unsafe` only in two crates.** `crates/ipc` (shared memory and block sync, linked into the
   engine) and `crates/plugin-host` (the plug-in host process) may use `unsafe`; every other
   crate stays `forbid`. Both offer safe APIs, keep `unsafe` blocks small, explain each one in a
   `// SAFETY:` comment (`clippy::undocumented_unsafe_blocks` denied there) and test them. `ipc`
   runs inside the engine process, so it stays small.
7. **Formats.** CLAP first, through `clack-host`, then VST3, through the `vst3` bindings, on the
   same boundary. The link goes through a plug-in's main audio ports by one channel rule for both
   formats, every other port gets silent buffers with the channels it declared, and both formats
   have a test plug-in built into the host (`test:clap`, `test:vst3`) so the whole path is tested
   without a plug-in installed.
8. **VST2: through VST3.** A project that names a VST2 plug-in is opened with the installed
   VST3 that replaces it (by declaration, by the SDK's id convention, then by name and newer
   version), and the saved VST2 state is handed to it. Without one the device stays in the
   project as a *missing plug-in* with its state intact. Hosting VST2 itself is a separate
   decision (below).
9. **Scanning** runs in short-lived worker processes with a timeout per file, a cache keyed by
   path, modification time and size, and the failures kept with their reasons and tried again
   later; it reads metadata without loading a plug-in where it can. It looks in the system's
   standard folders plus folders the user adds, each for one format or every one, and any folder
   or plug-in can be switched off. The folders, the catalog, the plug-ins switched off and the
   hosting choices are kept as JSON in Suhina's settings folder.
10. **Editors** open from the host process, floating above Suhina's window and owned by it by
    default; Settings › Plug-ins › Editors › *Device view* embeds them in the device panel where
    the window system allows (Windows, X11). The window thread never waits for a host process.
    On Linux the desktop app opens an X11 window whenever an X server is there, so editors can
    be owned or embedded.
11. **Other architectures later, same protocol:** a 32-bit Windows host, an x86-64 host on Apple
    silicon, Windows plug-ins on Linux through Wine.
12. **Windows plug-ins on Linux: yabridge first.** Linux has native CLAP and VST3 plug-ins, which
    load like any other. Windows-only plug-ins reach Linux through yabridge (GPL-3.0), which
    wraps each one as a Linux CLAP or VST3 file; Suhina loads those like native plug-ins, with
    nothing of its own to build. It costs a second process hop (our host, then yabridge's Wine
    process). Windows plug-ins that exist only as VST2 do not work this way (8).
13. **Plug-ins inside device groups** still run out of process; a group is not a process
    boundary. A plug-in's key counts the same plug-ins before it through the channel's whole tree
    of devices, so moving it into or out of a group keeps its process and state.
14. **Instances are kept across an output change and an instrument stops softly.** A change of
    device, rate or block keeps the running instances and reconfigures them in place
    (`reconfigure`), and a hosted instrument fades its output over 5 ms before it is told the
    transport stopped, so an instrument that resets on stop does not click
    ([plug-in hosting](../spec/plugin-hosting.md) › Keeping instances across an output change,
    Stopping a hosted instrument).

## Not decided yet

- **Hosting VST2 directly.** Where the interface description comes from is decided in
  [ADR 0022](0022-vst2-interface-description.md): the VeSTige header as Ardour distributes it,
  pinned. Nothing is built yet: the header is to be ported to Rust with its notice, checked by our
  own mock plug-in and probe, and written up in `docs/spec/plugin-vst2.md`. The work is
  [#1066](https://github.com/suhina-daw/suhina-daw/issues/1066). The process, scanning, editor and
  format decisions above are unchanged; shipping VST2-enabled builds is allowed by ADR 0022's
  2026-10-31 amendment.
- **Our own Wine host** for Windows plug-ins on Linux (`suhina-plugin-host` built for Wine,
  speaking the same protocol, so one hop instead of two and one place for crashes, scanning and
  editors). Decided after native CLAP and VST3 work and the cost of the yabridge path has been
  measured; yabridge's code (GPL-3.0) can be read for how it solves the same problems.
- macOS block sync and audio workgroup details, after the prototype.
- Audio Units on macOS, LV2 on Linux. ARA (LOOP-15) is [ADR 0033](0033-ara-hosting.md) (proposed).

## Consequences

- Every host process costs a round trip per block, tens of microseconds; at small blocks with
  many plug-in products this is noticeable. Grouping by product, joint round trips (2) and
  chains running at the same time (4) keep it bounded: measured, four plug-ins in one joint
  round trip took about a third of four round trips, and three chains of three plug-ins half
  the time of running a stage at a time ([plug-in hosting](../spec/plugin-hosting.md) ›
  Measurements). *In process* is not offered.
- A crash costs the engine one block of the plug-in's output, not the session or the project.
- Latency is compensated across tracks from what each plug-in reports, including a latency that
  changes while it runs (#110).

## Sources

- Bitwig plug-in hosting modes: https://www.bitwig.com/userguide/latest/vst_plug-in_handling_and_options
- REAPER bridging: https://reaper.blog/2012/02/run-plugin-as-dedicated-process/
- Carla bridge protocol and semaphores: https://raw.githubusercontent.com/falkTX/Carla/main/source/utils/CarlaBridgeDefines.hpp, https://raw.githubusercontent.com/falkTX/Carla/main/source/utils/CarlaSemUtils.hpp
- yabridge architecture: https://github.com/robbert-vdh/yabridge/blob/master/docs/architecture.md
- Ardour on in-process plug-ins: https://ardour.org/plugins-in-process.html
- Apple, out-of-process Audio Units (WWDC 2015, session 508): https://nonstrict.eu/wwdcindex/wwdc2015/508/
- CLAP: https://github.com/free-audio/clap; clack: https://github.com/prokopyl/clack
- VST3 SDK under MIT: https://github.com/steinbergmedia/vst3sdk; vst3 crate: https://github.com/coupler-rs/vst3-rs
- VST2 licensing for hosts: https://forums.steinberg.net/t/can-i-create-vst-2-support-for-host-application-vst-3-license/202012
- DMCA notice against VeSTige (2018): https://lists.linuxaudio.org/archives/linux-audio-dev/2018-June/037132.html
- VeSTige as Ardour distributes it: https://github.com/Ardour/ardour/blob/365cc92cba0e016c30fea34aed168a0957aeeca0/libs/ardour/ardour/vestige/vestige.h
- FST: https://sources.debian.org/src/fst/0.177.0-1/README.md
- VST3 `moduleinfo.json` and compatibility: https://steinbergmedia.github.io/vst3_dev_portal/pages/Technical+Documentation/VST+Module+Architecture/ModuleInfo-JSON.html
- VST2 compatibility FAQ: https://steinbergmedia.github.io/vst3_dev_portal/pages/FAQ/Compatibility+with+VST+2.x+or+VST+1.html
- VST2 state in VST3 (`vst2persistence.h`): https://github.com/steinbergmedia/vst3_public_sdk/blob/master/source/vst/utility/vst2persistence.h
- Hosts that migrate VST2 to VST3: https://u-he.com/support/faq/troubleshooting/vst3-migration/
- Cross-process window embedding on Windows: https://devblogs.microsoft.com/oldnewthing/20130412-00/?p=4683
