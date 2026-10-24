---
description: Where the description of the VST2 interface comes from — our own observation, written into docs/spec/ and kept true by tests, never Steinberg's SDK and never another project's reverse engineering, however permissively licensed
read_when: VST2, VST 2, AEffect, ABI, opcode, dispatcher, audioMaster, clean-room, reverse engineering, vst2sdk, VeSTige, FST, yabridge, plug-in interface description, VST trademark
type: decision
status: draft
last_verified: 2026-10-15
---

# 0022: VST2: where the interface description comes from

- **Status:** proposed
- **Date:** 2026-10-15
- **Task:** the *Not decided yet* item of [ADR 0006](0006-plug-in-hosting.md) › Hosting VST2 directly.
- **Depends on:** [ADR 0006](0006-plug-in-hosting.md) (out-of-process hosting, the formats, the
  scanning rules, the VST2 policy) and [`CONTRIBUTING.md`](../../CONTRIBUTING.md) › Clean-room.

## Context

A host that loads VST2 plug-ins needs the interface described: the 64-bit `AEffect` layout, the
op-codes in both directions, the event list, the time-info structure, and what each op-code's
`index`, `value` and pointer carry. That is a few hundred lines of interface facts — small next to
the hosting around it. The question this ADR answers is **where those facts may come from**, and it
is a clean-room question, not a licence question.

What was checked, on 2026-10-15:

- **Steinberg's VST2 SDK.** Steinberg stopped signing VST2 agreements in 2018 and has said new VST2
  hosts cannot be licensed; the SDK's licence is not GPL-compatible and its headers may not be
  redistributed. Excluded by [ADR 0006](0006-plug-in-hosting.md) already.
- **The free headers are reverse-engineering results.** VeSTige (GPLv2+; origin known only
  second-hand, DMCA notice in 2018) and FST (GPLv3+; takes names from JUCE sources that shipped
  Steinberg's headers) are both excluded by ADR 0006.
- **Xaymar's `vst2sdk`** (BSD-3-Clause, header-only, <https://github.com/Xaymar/vst2sdk>) is the
  third one, and the only well-documented one. Its README calls it "a completely 'clean room'
  untainted reverse engineered 'SDK' for the VST 2.x interface"; it says the first version came out
  of a clean-room "telephone game" involving external developers and lawyers, later versions were
  extended by automating behaviour scanning of several thousand plug-ins, it grants no patent
  licences and no right to use trademarks, and it advises asking a lawyer anyway.
  It is used by DPF, which vendors it at
  <https://github.com/DISTRHO/DPF/tree/main/distrho/src/xaymar-vst2>. **So DPF is not a second,
  independent source**: it is this header, downstream. Lemon Studio's VST2 host is written from it
  too ([ItsAlphaNeon/Lemon-Studio#47](https://github.com/ItsAlphaNeon/Lemon-Studio/pull/47)).
- **yabridge** (GPL-3.0) is a VST2/VST3/CLAP bridge with its own VST2 structures
  (`src/common/serialization/vst2.h`, `src/common/communication/vst2.h`,
  `src/wine-host/bridges/vst2.*`), not a copy of one of the free headers. ADR 0006 already permits
  reading it for how it solves the same problems.
- **RustAudio `vst2-sys`** (MIT/Apache-2.0) describes itself as "bindings for the VST 2.4 API" — a
  binding written against the SDK's interface, not an independent description; `vst-rs` (MIT,
  archived) implements the same API. Neither removes the SDK question.
- **Our rules.** [`CONTRIBUTING.md`](../../CONTRIBUTING.md) › Clean-room allows open-source code
  under a GPL-compatible licence, ported with its notices kept, and allows observing software
  normally. It forbids notes, patches or write-ups that come from someone else's debugging,
  disassembly or decompilation of a proprietary program — and a plug-in file is a proprietary
  program. It also requires findings to be in our own words under `docs/spec/` **before** they are
  implemented.

Put together: `vst2sdk` being BSD-3-Clause is *necessary* for us to use it, and it is not
*sufficient*. Reading its header is reading another party's reverse-engineering result, which is the
material CONTRIBUTING tells us not to read or use — the same reason ADR 0006 rejected VeSTige and
FST. If we adopted it, the clean-room rule would have to be waived for it; that is a decision about
the rule, not about the interface.

## Decision (proposed)

1. **The description is ours.** `docs/spec/plugin-vst2.md` holds it, in our words, field by field
   and op-code by op-code, and `crates/plugin-host` implements that spec behind the `vst2` feature
   (`unsafe` is allowed there, ADR 0006 decisions 1 and 7). No optional crate outside the tree is
   needed for this.
2. **No third-party reverse-engineering result is a source.** Not Steinberg's VST2 SDK, not
   VeSTige, not FST, and not `vst2sdk` — not as a dependency, not vendored under `third_party/`,
   not quoted in the spec, not in a summary handed to an agent. The permissive licence of
   `vst2sdk` does not change this; the provenance rule decides it.
3. **What the description is built from**, in order of weight:
   - **Our own observation** of plug-in files and running plug-ins, which the rules allow: a mock
     VST2 plug-in we author next to the CLAP/VST3 mocks, a probe that answers the op-codes in both
     directions and records what the host sent, and `#[ignore]` tests against installed plug-ins,
     as `SUHINA_TEST_CLAP` and `crates/plugin-host/tests/manual.rs` already do.
   - **yabridge** (GPL-3.0), already permitted by ADR 0006, read for how a host solves the same
     problems and as a cross-check where our own observation is thin. Where our spec and yabridge
     disagree, the spec says which one we verified and how.
   - **Published, licence-clean documents**: the VST3 SDK's `vst2persistence.h` (MIT) for the VST2
     state layouts — already used by the `.als` importer — and Steinberg's public compatibility
     documentation.
   - **The names an implementation cannot avoid**: the entry-point symbols, `'VstP'`, the op-code
     numbers, the field meanings. These are interface facts; each is recorded with the observation
     that established it.
4. **Every field and op-code enters the spec with two things**: the observation that established it,
   and the test that keeps it true — a mock assertion, an `offset_of!` assertion, or an ignored test
   against a real plug-in. A fact without a test is marked as a guess, and `plugin.*`'s status for
   a VST2 file says so while it is one.
5. **The name.** No VST logo, and no "VST" in Suhina's own name, branding, crate, binary or feature
   name (`suhina-plugin-host`, feature `vst2`, `DeviceKind::Vst2`). The format is named only where a
   format has to be named — the plug-in manager's format column, this spec, the project file's
   device kind, import text — which is nominative use; README's trademark note stays. **Legal advice
   precedes a release that ships it** (ADR 0006, unchanged).
6. **If the answer changes, this ADR changes.** A written statement of provenance from `vst2sdk`'s
   author, or an interface document Steinberg publishes itself, is a reason to amend decision 2.
   The spec and the tests stay as they are either way: they describe the interface, not its source.

## Alternatives considered

- **Vendor `vst2sdk` (BSD-3) and port its facts** — what Lemon Studio did, and by far the cheapest
  route to a working host. Legal if the reverse engineering was lawful where it was done, and BSD-3
  is GPL-compatible. Rejected because reading it is exactly what CONTRIBUTING › Clean-room forbids,
  and one contaminated description would sit under the whole VST2 host. Revisit only with a written
  provenance statement *and* legal sign-off.
- **Waive the rule for interface descriptions.** That is a change to `CONTRIBUTING.md`, not to this
  ADR: if the project decides that a published clean-room interface description is a permitted
  source, this ADR is superseded and `vst2sdk` becomes the source, vendored with its notice.
- **Do not host VST2 at all** and keep replacing it with the installed VST3/CLAP copy (ADR 0006
  decision 8). Still the cheapest option, and the one in force until a plug-in people need exists
  only as VST2.

## Consequences

- The first working host arrives more slowly than a port would: the mock plug-in and the probe are
  the real cost, and both are test code we would want anyway.
- The spec can be wrong until a real plug-in says otherwise. The ignored tests against installed
  plug-ins are what settles that, so they are part of the work rather than an extra.
- The rule generalises: any future abandoned interface (VST1, DirectX plug-ins, Audio Units on
  another platform) is described the same way, from observation, under `docs/spec/`.
- Nothing here unblocks VST2 by itself. The licence question (Steinberg no longer licenses new VST2
  hosts), the trademark line and the legal advice before release stay as ADR 0006 states them, and
  ADR 0006 decision 8 — a project's VST2 plug-in is played by its VST3 or CLAP copy meanwhile —
  stands.
- ADR 0006's *Not decided yet* item for VST2 moves here; the format, scanning, editor and process
  decisions in 0006 are unchanged.

## Sources

- Lemon Studio's VST2 host, and the source list it declares: <https://github.com/ItsAlphaNeon/Lemon-Studio/pull/47>
- Xaymar's `vst2sdk` (BSD-3-Clause), its method and its own disclaimers: <https://github.com/Xaymar/vst2sdk>
- DPF, a downstream user of that header: <https://github.com/DISTRHO/DPF/tree/main/distrho/src/xaymar-vst2>
- yabridge's VST2 structures (GPL-3.0): <https://github.com/robbert-vdh/yabridge/blob/master/src/common/serialization/vst2.h>
- RustAudio `vst2-sys` (MIT/Apache-2.0): <https://github.com/RustAudio/vst2-sys>
- VST2 state in VST3 (`vst2persistence.h`, MIT): <https://github.com/steinbergmedia/vst3_public_sdk/blob/master/source/vst/utility/vst2persistence.h>
- VST2 compatibility FAQ: <https://steinbergmedia.github.io/vst3_dev_portal/pages/FAQ/Compatibility+with+VST+2.x+or+VST+1.html>
- VST2 licensing for hosts: <https://forums.steinberg.net/t/can-i-create-vst-2-support-for-host-application-vst-3-license/202012>
- Clean-room rules: [`CONTRIBUTING.md`](../../CONTRIBUTING.md) › Clean-room
