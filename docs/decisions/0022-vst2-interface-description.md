---
description: Where the VST2 interface description comes from — the VeSTige header as Ardour distributes it, pinned to one commit, ported to Rust with its notice kept, as the one named exception to the clean-room rule; checked by our own mock plug-in, probe and tests; amended 2026-10-31 to allow shipping VST2-enabled builds
read_when: VST2, VST 2, AEffect, ABI, opcode, dispatcher, audioMaster, clean-room, reverse engineering, VeSTige, vestige.h, Ardour, LMMS, vst2sdk, FST, yabridge, plug-in interface description, VST trademark, provenance
type: decision
status: accepted
built: partly
last_verified: 2026-10-31
---

# 0022: VST2: where the interface description comes from

- **Date:** 2026-10-15; decided 2026-10-30 ([#1066](https://github.com/suhina-daw/suhina-daw/issues/1066))
- **Task:** the *Not decided yet* item of [ADR 0006](0006-plug-in-hosting.md) › Hosting VST2 directly.
- **Depends on:** [ADR 0006](0006-plug-in-hosting.md) (out-of-process hosting, the formats, the
  scanning rules, the VST2 policy) and [`CONTRIBUTING.md`](../../CONTRIBUTING.md) › Clean-room.

## Context

A host that loads VST2 plug-ins needs the interface described: the 64-bit `AEffect` layout, the
op-codes in both directions, the event list, the time-info structure, and what each op-code's
`index`, `value` and pointer carry. That is a few hundred lines of interface facts — small next to
the hosting around it. The question this ADR answers is **where those facts may come from**, and it
is a clean-room question as much as a licence question.

The first version of this ADR (proposed 2026-10-15) answered "only from our own observation": no
third-party header at all. That would have made the mock plug-in and the probe the only source of
every field and op-code. The decision below replaces it with one named, pinned source.

What was checked, on 2026-10-15 and 2026-10-30:

- **Steinberg's VST2 SDK.** Steinberg stopped signing VST2 agreements in 2018 and has said new VST2
  hosts cannot be licensed; the SDK's licence is not GPL-compatible and its headers may not be
  redistributed. Excluded by [ADR 0006](0006-plug-in-hosting.md).
- **VeSTige** (GPL-2.0-or-later). A header written in 2006 by Javier Serrano Polo for LMMS. Its
  notice says it is included "in the good-faith belief" that it was reverse engineered cleanly and
  legally, without reference to the official SDK and without its developers having agreed to the
  SDK licence. The author discussed its making on debian-legal in 2006. Steinberg's 2018 DMCA notice
  targeted the copy of it in the `lv2vst` project (`include/aeffectx.h`); the project's author
  answered it, and the header is still distributed. Ardour ships it as
  `libs/ardour/ardour/vestige/vestige.h` and its VST2 host is built on it.
- **yabridge** (GPL-3.0) **vendors VeSTige too**: `src/include/vestige/aeffectx.h`, the same
  header under the LMMS file name, with the same copyright notice. Its own VST2 structures
  (`src/common/serialization/vst2.h` and the bridges) are built on that header. So yabridge is not a
  source independent of VeSTige; the first version of this ADR said it was, which was wrong.
- **FST** (GPLv3+) takes names from JUCE sources that shipped Steinberg's headers. Excluded.
- **Xaymar's `vst2sdk`** (BSD-3-Clause) is a reverse-engineering result with a documented
  "telephone game" method; DPF vendors it, and Lemon Studio's VST2 host is written from it. It is
  not chosen: one source is enough, and VeSTige is the one with the longest public record in
  GPL hosts.
- **RustAudio `vst2-sys`** and `vst-rs` (MIT/Apache-2.0) are bindings written against the SDK's
  interface, not an independent description. Excluded.
- **Our rules.** [`CONTRIBUTING.md`](../../CONTRIBUTING.md) › Clean-room allows GPL-compatible open
  source, ported with its notices kept, and forbids write-ups or tools that come from someone else's
  reverse engineering of a proprietary program. VeSTige is both: GPL-compatible code, and a
  reverse-engineering result. Using it is therefore a decision about the rule, and CONTRIBUTING now
  names it as the one exception.

This ADR accepts VeSTige as a third-party interface source. It does not claim that Suhina
discovered those definitions independently, that the provenance record is complete, or that a court
has approved their use.

## Decision

1. **The source is VeSTige, pinned.** The VST2 interface definitions come from
   `libs/ardour/ardour/vestige/vestige.h` in Ardour's repository, at commit
   `365cc92cba0e016c30fea34aed168a0957aeeca0` (the file's last change, 2020-08-20; blob
   `0470381141768a2841916af3eeb20014528e65e6`, 10 335 bytes). Moving to another commit or another
   copy is a change to this decision, recorded here with the new hashes.
2. **Nothing else of that kind.** Not Steinberg's VST2 SDK, not FST, not `vst2sdk`, not `vst2-sys`,
   not leaked code and no other reverse-engineering result — not as a dependency, not vendored, not
   quoted in the spec, not in a summary handed to an agent. yabridge may still be read for how a host
   solves the same problems (ADR 0006), not as a second source of interface facts.
3. **Ported to Rust, notice kept.** `crates/plugin-host` holds the Rust definitions behind the `vst2`
   feature (`unsafe` is allowed there, ADR 0006 decisions 1 and 7). A file ported from the header
   keeps its copyright and licence notice and the SPDX line `GPL-2.0-or-later`, and is listed in
   `scripts/license_header.py`'s `PORTED`. Suhina's own code around it stays GPL-3.0-or-later. No
   C header is compiled and no crate outside the tree is added.
4. **The spec will say where each fact comes from.** `docs/spec/plugin-vst2.md`, written with the
   first hosting code, describes the interface in our words and marks each field and op-code as
   *from VeSTige*, *observed* (our mock plug-in, the probe that records what a host and a plug-in
   send, `#[ignore]` tests against installed plug-ins) or *assumed*. A fact enters the spec before
   code relies on it, not all of them before the port begins, and comes with a test that keeps it
   true — a mock assertion, an `offset_of!` assertion or an ignored test against a real plug-in. The VST2 state layouts come from the VST3
   SDK's `vst2persistence.h` (MIT), as the `.als` importer already reads them.
   **The pinned header is incomplete.** It defines only part of the interface; the op-codes that
   get and set a plug-in's state chunk, for one, are not in it. The spec will list every fact the host
   needs that the header lacks, and each one is *observed* or *assumed*, never taken from another
   header (decision 2). A mock plug-in written from our own assumption cannot establish such a
   fact: only the probe or an ignored test against a real plug-in turns *assumed* into *observed*.
5. **The name, and shipping.** No VST logo, and no "VST" in Suhina's own name, branding, crate,
   binary or feature name (`suhina-plugin-host`, feature `vst2`, `DeviceKind::Vst2`). The format is
   named only where a format has to be named — the plug-in manager's format column, the spec, the
   project file's device kind, import text — which is nominative use; README's trademark note stays.
   VST2-enabled builds may be distributed, and the `vst2` feature may be on by default; release
   notes name VST2 hosting as built on VeSTige.
   *Amended 2026-10-31, see Amendment: shipping VST2.*
6. **If the answer changes, this ADR changes.** A request or claim from Steinberg, a credible new
   claim against the header, or an interface document Steinberg publishes itself does not by
   itself stop distribution; it is a reason to reconsider, and the outcome (keep shipping, answer
   the claim, or turn the default off) is recorded as an amendment. Legal advice against shipping
   turns the default off. Turning it off removes only the `vst2` feature: a project's VST2 device
   is then opened with the installed VST3 that declares it replaces it, or stays a *missing
   plug-in* the user can relink by hand (ADR 0006 decision 8). The spec and the tests stay as they
   are either way: they describe the interface, and where a fact is observed they do not depend
   on its source.
   *Amended 2026-10-31, see Amendment: shipping VST2.*

## Amendment: shipping VST2 (2026-10-31)

[PR 1076](https://github.com/suhina-daw/suhina-daw/pull/1076). The gate in decision 5, legal
advice before a release ships VST2, is lifted: VST2-enabled builds may be distributed. The gate
existed because of Steinberg's SDK, and decisions 1 to 3 already keep it out. What ships is the
pinned VeSTige header, ported with its GPL-2.0-or-later notice, and facts observed by our own probe
and tests; Ardour, LMMS and yabridge have distributed hosts on the same header for years, and the
2018 DMCA notice did not remove it (see Context). This is the project's judgement, not legal
advice, and it makes no claim that a court has approved the header. The naming rules of decision 5
stay. Decision 6 now says what a claim or request does: it starts a reconsideration recorded as an
amendment, it does not stop distribution by itself; legal advice against shipping does.

## Alternatives considered

- **Our own description from observation only** — the first version of this ADR. The cleanest
  provenance, and by far the slowest route: every field and op-code would have to be established by
  a probe against real plug-ins before the host could load one. Replaced because the project accepts
  VeSTige's public record; the probe and the mock plug-in remain, as the check rather than the source.
- **Vendor `vst2sdk` (BSD-3) and port its facts** — what Lemon Studio did. Also a
  reverse-engineering result, with a shorter record in GPL hosts; choosing it would need the same
  exception. Not chosen; one source is enough.
- **Do not host VST2 at all** and keep replacing it with the installed VST3/CLAP copy (ADR 0006
  decision 8). Still what happens until the host ships, and still the path for a VST2 plug-in that
  has a VST3 or CLAP version.

## Consequences

- CONTRIBUTING › Clean-room names VeSTige as its one exception, with the limits of decision 2;
  AGENTS.md rule 1 points at it.
- The host can be written from known definitions, so the mock plug-in and the probe test it instead
  of discovering it. Both are still part of the work, and the ignored tests against installed
  plug-ins settle what the header does not.
- The ported files carry GPL-2.0-or-later notices inside a GPL-3.0-or-later program, as the MSFA
  (Apache-2.0) and Signalsmith (MIT) ports carry theirs.
- VST2-enabled builds may ship (amendment of 2026-10-31). The trademark line stays, and a claim
  or legal advice against shipping is handled as decision 6 says.
- ADR 0006's *Not decided yet* item for VST2 moves here; the format, scanning, editor and process
  decisions in 0006 are unchanged.

## Sources

- VeSTige as Ardour distributes it, with its notice: <https://github.com/Ardour/ardour/blob/365cc92cba0e016c30fea34aed168a0957aeeca0/libs/ardour/ardour/vestige/vestige.h>
- The author on how VeSTige was made (debian-legal, 2006): <https://lists.debian.org/debian-legal/2006/08/msg00231.html>
- Steinberg's 2018 DMCA notice concerning `lv2vst`: <https://github.com/github/dmca/blob/master/2018/2018-06-04-Steinberg-2.md>
- The same notice on linux-audio-dev: <https://lists.linuxaudio.org/archives/linux-audio-dev/2018-June/037132.html>
- The `lv2vst` author (an Ardour developer) on answering that notice: <https://www.kvraudio.com/forum/viewtopic.php?p=8094620>
- yabridge's vendored VeSTige header (GPL-3.0 project): <https://github.com/robbert-vdh/yabridge/blob/master/src/include/vestige/aeffectx.h>
- Xaymar's `vst2sdk` (BSD-3-Clause), its method and its own disclaimers: <https://github.com/Xaymar/vst2sdk>
- Lemon Studio's VST2 host: <https://github.com/ItsAlphaNeon/Lemon-Studio/pull/47>
- RustAudio `vst2-sys` (MIT/Apache-2.0): <https://github.com/RustAudio/vst2-sys>
- VST2 state in VST3 (`vst2persistence.h`, MIT): <https://github.com/steinbergmedia/vst3_public_sdk/blob/master/source/vst/utility/vst2persistence.h>
- VST2 compatibility FAQ: <https://steinbergmedia.github.io/vst3_dev_portal/pages/FAQ/Compatibility+with+VST+2.x+or+VST+1.html>
- VST2 licensing for hosts: <https://forums.steinberg.net/t/can-i-create-vst-2-support-for-host-application-vst-3-license/202012>
- Clean-room rules: [`CONTRIBUTING.md`](../../CONTRIBUTING.md) › Clean-room
