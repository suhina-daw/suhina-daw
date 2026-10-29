---
description: What Suhina aims at (the clip-launching DAW workflow), what it deliberately leaves out, and the order the missing parts get built in
read_when: scope of the project, what Suhina does not aim at, out of scope, build order, what next, priorities, milestone for recording, automation, MIDI editing, MIDI effects, devices
type: decision
status: proposed
built: partly
last_verified: 2026-10-29
---

# 0016: Scope: what Suhina aims at, what it leaves out, and the build order

- **Date:** 2026-10-13, rewritten 2026-10-29 (#1061)

## Context

Every planning conversation eventually asks *what is missing, and what comes next?* Suhina is a
clip-launching DAW, and a DAW of that kind has a known shape: projects, the Session and Arrangement
views, clips and warping, launching, devices and chains, mixing and routing, a browser, plug-in
hosting, recording, editing and automation. When this record was first written (October 2026), some
of those areas were close to done (the browser and project files), several were partial (views and
editing, the mixer, devices as a *system*), and several whole features were absent rather than
thin: MIDI editing, recording, automation, device groups, MIDI effects. The tiers below were set
then; items that have landed since stay in them as the record of the order.

Two things follow:

1. **The gap is not uniform**, so "how far along" is not one number; it is a list of areas, each
   done, partial or missing. The README's *What it does today* keeps that list in Suhina's own
   areas, and [`ROADMAP.md`](../../ROADMAP.md) the detail.
2. **Some of what a DAW can include is not clip-launching work.** Video, a second programming
   runtime and one vendor's hardware are part of some products, not of the workflow. Leaving them
   out has to be said, or the list of what is missing never ends.

This ADR records the scope, so planning has a boundary everyone agrees on, and an order to build
the missing parts in. It does not schedule the work.

## Decision

1. **The target is the clip-launching DAW workflow, not any product's whole surface.** In scope:
   reading and writing projects, the Session and Arrangement views, clips and warping, launching,
   devices and chains, mixing and routing, the browser and the library, plug-in hosting, and every
   action as a command (UI, CLI, MCP alike). A person who makes music in a clip-launching DAW should
   be able to do their normal work here without keeping a second DAW installed.

2. **Deliberately out of scope:**

   | Out of scope | Why | What happens instead |
   |---|---|---|
   | Video | Not clip-launching work; a scoring feature with its own tooling | Nothing planned |
   | Devices written for another DAW's own patching runtime | A separate runtime and a proprietary ecosystem | [ADR 0008](0008-patch-devices.md): community devices as Pure Data patches; imported devices of that kind stay placeholders |
   | Support for one vendor's hardware controllers through that vendor's control scripts | Hardware integration written for another program | Generic MIDI controllers and mapping ([ADR 0019](0019-midi-controllers-and-mapping.md)) |
   | Writing `.als` | Suhina reads that format; its own project file is `.suhina` ([ADR 0010](0010-project-files.md)) | Other formats by their own specs (M7: DAWproject, `.flp`) |

3. **In scope but unplanned today:** the gaps the roadmap names only in passing, or not at all.
   What gets built, in what order, and what is deliberately refused, is **The build order** below.

4. **Open, and named here so it is not forgotten:** Audio Units on macOS (a supported platform, so
   an open question rather than an exclusion); hosting VST2 rather than only reading its state
   (left open by [ADR 0006](0006-plug-in-hosting.md)); ARA; accessibility (in scope in principle,
   no work planned, and it is not counted as an exclusion above).

5. **Suhina describes itself in its own terms.** The roadmap, the README and this ADR list areas
   and features under Suhina's names. Comparisons with other products, where the maintainers make
   them for planning, are kept out of the repository; format specs still say which format they read
   (AGENTS.md rule 9).

## The build order

This order is decided by four questions:

- **Does it stop someone using Suhina at all?** A DAW you cannot record into and cannot fix a note
  in is a viewer.
- **Does it unblock other work?** Recording precedes comping and bouncing; note commands precede
  the MIDI effects and the cleanup after audio-to-MIDI; device parameters precede automation.
- **Does it define a clip-launching DAW?** Follow actions, launch modes and macros are what make
  launching *performance* rather than playback.
- **What does it cost, and what does it buy?** Cheap things felt daily come before expensive things
  felt rarely.

Sizes are relative (S: a focused change; M: a crate-sized piece; L: a milestone-sized piece), not
calendar promises.

### The first tier: without these it is not a DAW

| # | Item | Size | Why it is here |
|---|---|---|---|
| 1 | **MIDI note editing**: the piano roll in the detail area, note commands (add, move, resize, velocity, deactivate), quantise (`docs/spec/ui-clip-editor.md`) | M-L | Without it a wrong note cannot be fixed. Cheapest path from "plays your set" to "you can work on your music". Unblocks the MIDI effects and the cleanup after audio-to-MIDI. |
| 2 | **Automation**: device and mixer parameters first, then tempo | L | A static mix is not a mix, and it is what makes the built-in effects and instruments worth having. The command layer and the per-block device/plug-in parameter path already exist, so this is model, engine and UI rather than new plumbing. |
| 3 | **Recording and inputs**: audio and MIDI recording, arming, monitoring with latency compensation, count-in | XL | The only way a user's own material gets in, and the precondition for comping, resampling, capture MIDI and per-track bounce. Also the largest and riskiest: it is the first place the engine answers to a device's clock rather than its own. |

Suggested sequence inside the tier: **1, then 2, then 3** if the goal is a working DAW soonest,
because 1 is the cheapest and 3 the largest; **3 first** if the goal is adoption by people whose
material is not already in a project file Suhina reads.

### The second tier: cheap, felt daily, and mostly already scoped

| # | Item | Size | Why it is here |
|---|---|---|---|
| 4 | **Sends and returns, cue, level meters** | M | The model already carries sends and the `.als` importer fills them; only the engine, the commands and the strips are missing. Makes imported sets sound the way their author meant. |
| 5 | **Arrangement editing**: split, consolidate, locators, the arrangement loop brace | S-M | Small commands with a large daily effect; two are open issues (#98, #99) and the clip loop range is already in the model and drawn. |
| 6 | **EQ and filter** - the first device anyone misses; then bus compressor, delay, reverb | S-M each | `dsp` already has biquads and crossovers, so a filter and a parametric EQ are cheap; the rest of the roadmap's named devices follow one at a time. |
| 7 | **Follow actions, launch modes, legato, per-clip quantisation** | M | The remainder of M4 and the product's identity: launching that performs rather than loops. |
| 8 | **Finish what is in flight**: M3's remaining commands, M6's plug-in automation and editors, M7, M8's installers and detail area | - | Opening six fronts against unfinished ones is how a pre-alpha stays pre-alpha. This tier exists to be done *alongside* the first, not instead of it. |

### The third tier: power, breadth and niche - deliberately later

| # | Item | Size | Why it waits |
|---|---|---|---|
| 9 | The remaining named devices (multiband dynamics, saturation, sampler and drum sampler) | M each | Plug-in hosting already covers the need; a built-in is a preference, not a blocker. The sampler and drum sampler rise in the order once beatmaking is a target. |
| 10 | MIDI effects: scale, chord, arpeggiator, velocity | S each | Small, visible and self-contained - the right first device-shaped task for a new contributor - but nothing depends on them. |
| 11 | Device groups, macros, drum pads; then MIDI remote mapping; then tempo sync with other programs and MIDI clock | M-L | Performance and collaboration features for people who already work here. |
| 12 | Comping and take lanes, per-track bounce, resampling | M | Cheap once recording exists, and worth nothing before it. |
| 13 | Browser and file depth: collections, the tag editor, collect files, unused files | M | The browser is already Suhina's strongest area; the missing parts are conveniences for large libraries. |
| 14 | Audio to MIDI, stem separation | L | Differentiators with specs and licence audits already written, and no user is blocked without them. |
| 15 | Grooves, tuning systems, MPE | M each | Niche until asked for; each is self-contained and can land when someone wants it. |

### The fourth tier: we are not building these

Video, devices for another DAW's patching runtime, one vendor's controller integration, packs and
cloud browsing, another program's analysis files, the spectral and vocoder effects, and devices
that would exist only to complete a catalogue rather than to make music. A request for one of these
gets this ADR, and reopening it means superseding this record.

Cross-cutting, outside the tiers and not optional: **accessibility** (in scope, no work planned,
and a product obligation), **performance without regression** (the engine's block deadline is the
one thing that cannot be traded for features), and **clean-room hygiene** (CONTRIBUTING.md ›
Clean-room).

### What this implies for milestones

Each of the first tier's three items is a body of work the current milestones cannot absorb, and
the third tier should not be turned into milestones before the first is empty. The suggestion -
the maintainer decides, and this ADR does not schedule - is one milestone each for MIDI editing,
automation and recording and I/O, in that order, after M7 and M8; the second tier rides along
with whatever is in flight (5, 6 and 7 are small enough to be individual issues rather than a
milestone).

## Consequences

- "What is missing, and what next?" has an answer that separates *behind* from *not trying to*.
- The out-of-scope list is a commitment to say no: a request for one of those gets this ADR as the
  answer, and reopening it means superseding this record.
- The build order puts the three things that make Suhina usable first, and says plainly that the
  third tier waits.
- The first tier is three bodies of work with no milestone. Until the board gives them one, they
  are work everybody agrees is next and nobody is scheduled to do.

## Not decided here

- Milestone numbers for the first tier (the suggestion is one each, after M7 and M8).
- Whether any of the out-of-scope list is reopened (each would supersede this ADR).
- How many built-in effects and instruments Suhina should have: this ADR says "EQ and filter
  first, then the rest one at a time", and no more.
- Where the second tier's small items land: they are issues, not a milestone, and any of them can
  ride along with whatever is in flight.
