---
description: What Suhina measures itself against in another DAW, what it deliberately leaves out, and how the parity measurement is kept honest and free of the manual's content
read_when: parity, feature comparison, how far are we from Live, scope of the project, what Suhina does not aim at, milestone for recording, automation, MIDI editing, MIDI effects, devices
type: decision
status: draft
last_verified: 2026-10-13
---

# 0016: Parity target: what Suhina compares itself to, and what it leaves out

- **Status:** proposed
- **Date:** 2026-10-13
- **Task:** the parity measurement in
  [`docs/research/parity-live-12.md`](../research/parity-live-12.md), produced by `scripts/parity/`.

## Context

Suhina says it is "comparable in workflow to Ableton Live" (README), and every planning
conversation eventually asks *how far are we*? Until now the answer was an impression. It is now a
measurement: the Live 12 manual's 999 pages of feature text, chapter by chapter, against this
repository.

What it says (at commit `eeb8899`):

| Measure | Value |
|---|---|
| Page-weighted coverage of everything the manual documents | about 16 % |
| Without Push 1 and 2, Max for Live and video | about 19 % |
| Built-in devices | 8, against Live's 63 (42 audio effects, 8 MIDI effects, 13 instruments) |
| Chapters with nothing at all behind them | MIDI Tools, MPE, Grooves, Tuning, Recording, Comping, Racks, Automation, Clip Envelopes, MIDI effects, Push, Link, video, accessibility |

Three things follow from reading it:

1. **The gap is not uniform.** One area is close to parity (the browser and project files, 41 %),
   several are partial (views and editing, the mixer, devices as a *system*), and several whole
   features are absent rather than thin.
2. **The device catalogue dominates the number.** The two device reference chapters are 259 of
   999 pages; no amount of polish elsewhere moves the headline as much. Whether Suhina should
   chase 63 devices is therefore a product decision, not a backlog accident.
3. **Some of the manual describes things Suhina should not build.** Push, Max for Live and video
   are in the manual because they are part of Live, not because they are part of clip-launching DAW
   work. Counting them makes the project look further behind than it is, and leaving them
   uncounted without saying so makes the number look better than it is.

This ADR records the second decision - the scope - so the measurement has a denominator everyone
agrees on. It does not schedule the work.

## Decision

1. **The target is the clip-launching DAW workflow, not Live's whole surface.** In scope: reading
   and writing projects, the Session and Arrangement views, clips and warping, launching, devices
   and chains, mixing and routing, the browser and the library, plug-in hosting, and every action
   as a command (UI, CLI, MCP alike). A person who works in Live should be able to do their normal
   work here without keeping a second DAW installed.

2. **Deliberately out of scope** - the manual's chapters that describe these count as out of scope
   in the measurement, and the report states the number both ways:

   | Out of scope | Why | What happens instead |
   |---|---|---|
   | Video (ch 28) | Not clip-launching work; a scoring feature with its own tooling | Nothing planned |
   | Max for Live and its devices (ch 32-33) | A separate runtime and a proprietary ecosystem | [ADR 0008](0008-patch-devices.md): community devices as Pure Data patches; imported Max devices stay placeholders |
   | Push 1 and Push 2 (ch 35-36) | Manuals for one vendor's hardware controllers, written for Live's own control scripts; Push 1 is also a generation from before the current one | Nothing planned |
   | Writing `.als` (part of ch 5) | Suhina reads Live's format; its own project file is `.suhina` ([ADR 0010](0010-project-files.md)) | Other formats by their own specs (M7: DAWproject, `.flp`) |

3. **In scope but unplanned today:** the gaps the roadmap names only in passing, or not at all.
   What gets built, in what order, and what is deliberately refused, is **The build order** below.

4. **Open, and named here so it is not forgotten:** Audio Units on macOS (a supported platform, so
   an open question rather than an exclusion); hosting VST2 rather than only reading its state
   (left open by [ADR 0006](0006-plug-in-hosting.md)); ARA; accessibility (in scope in principle,
   no work planned, and it is not counted as an exclusion above).

5. **The measurement is repeated, not remembered.** `scripts/parity/` re-runs it when a milestone
   closes, and the report's date and commit are updated with it. A headline number older than the
   roadmap it describes is worse than no number.

6. **The measurement never brings the manual into the tree.** No page, table, screenshot or device
   parameter list of another product's manual is committed; the scripts read a copy the user owns,
   write to `target/parity/`, and the repository holds only our own numbers and words
   (CONTRIBUTING.md › Clean-room; AGENTS.md rule 9).

## The build order

The measurement ranks the gaps by manual pages, which is the wrong order to work in: it puts the
device catalogue first and the things a user notices in the first five minutes nowhere. This order
is the one to build from, decided by four questions:

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

| # | Item | Manual | Size | Why it is here |
|---|---|---|---|---|
| 1 | **MIDI note editing**: the piano roll in the detail area, note commands (add, move, resize, velocity, deactivate), quantise | ch 10, `docs/spec/ui-clip-editor.md` | M-L | A MIDI clip can only be imported today; a wrong note cannot be fixed. Cheapest path from "plays your set" to "you can work on your music". Unblocks the MIDI effects and the cleanup after audio-to-MIDI. |
| 2 | **Automation**: device and mixer parameters first, then tempo | ch 26 | L | A static mix is not a mix, and it is what makes the device catalogue worth having. The command layer and the per-block device/plug-in parameter path already exist, so this is model, engine and UI rather than new plumbing. |
| 3 | **Recording and inputs**: audio and MIDI recording, arming, monitoring with latency compensation, count-in | ch 19 | XL | The only way a user's own material gets in, and the precondition for comping, resampling, capture MIDI and per-track bounce. Also the largest and riskiest: it is the first place the engine answers to a device's clock rather than its own. |

Suggested sequence inside the tier: **1, then 2, then 3** if the goal is a working DAW soonest,
because 1 is the cheapest and 3 the largest; **3 first** if the goal is adoption by people whose
material is not already a Live Set.

### The second tier: cheap, felt daily, and mostly already scoped

| # | Item | Manual | Size | Why it is here |
|---|---|---|---|---|
| 4 | **Sends and returns, cue, level meters** | ch 18 | M | The model already carries sends and the `.als` importer fills them; only the engine, the commands and the strips are missing. Makes imported sets sound the way their author meant. |
| 5 | **Arrangement editing**: split, consolidate, locators, the arrangement loop brace | ch 6 | S-M | Small commands with a large daily effect; two are open issues (#98, #99) and the clip loop range is already in the model and drawn. |
| 6 | **EQ and filter** - the first device anyone misses; then bus compressor, delay, reverb | ch 29 | S-M each | `dsp` already has biquads and crossovers, so a filter and a parametric EQ are cheap; the rest of the roadmap's named devices follow one at a time. |
| 7 | **Follow actions, launch modes, legato, per-clip quantisation** | ch 16 | M | The remainder of M4 and the product's identity: launching that performs rather than loops. |
| 8 | **Finish what is in flight**: M3's remaining commands, M6's plug-in automation and editors, M7, M8's installers and detail area | - | - | Opening six fronts against unfinished ones is how a pre-alpha stays pre-alpha. This tier exists to be done *alongside* the first, not instead of it. |

### The third tier: power, breadth and niche - deliberately later

| # | Item | Manual | Size | Why it waits |
|---|---|---|---|---|
| 9 | The remaining named devices (multiband dynamics, saturation, sampler and drum sampler) | ch 29, 31 | M each | Plug-in hosting already covers the need; a built-in is a preference, not a blocker. The sampler and drum sampler rise in the order once beatmaking is a target. |
| 10 | MIDI effects: scale, chord, arpeggiator, velocity | ch 30 | S each | Small, visible and self-contained - the right first device-shaped task for a new contributor - but nothing depends on them. |
| 11 | Racks, macros, drum pads; then MIDI remote mapping; then Link and MIDI clock | ch 25, 34, 37 | M-L | Performance and collaboration features for people who already work here. |
| 12 | Comping and take lanes, per-track bounce, resampling | ch 19-21 | M | Cheap once recording exists, and worth nothing before it. |
| 13 | Browser and file depth: collections, the tag editor, collect files, unused files | ch 4-5 | M | Suhina's browser is already its strongest area (41 %); the missing parts are conveniences for large libraries. |
| 14 | Audio to MIDI, stem separation | ch 13, 22 | L | Differentiators with specs and licence audits already written, and no user is blocked without them. |
| 15 | Grooves, tuning systems, MPE, video-length automation extras | ch 12, 14, 15 | M each | Niche until asked for; each is self-contained and can land when someone wants it. |

### The fourth tier: we are not building these

Video, Max for Live, Push 1 and 2, packs and cloud browsing, `.asd` analysis files, amp and cabinet
models, the spectral and vocoder effects, and the remaining devices that exist to complete a
catalogue rather than to make music. A request for one of these gets this ADR, and reopening it
means superseding this record.

Cross-cutting, outside the tiers and not optional: **accessibility** (in scope, no work planned,
and it is a product obligation rather than a parity item), **performance without regression**
(the engine's block deadline is the one thing that cannot be traded for features), and **the
IP hygiene** of decision 6.

### What this implies for milestones

Each of the first tier's three items is a body of work the current milestones cannot absorb, and
the third tier should not be turned into milestones before the first is empty. The suggestion -
the maintainer decides, and this ADR does not schedule - is one milestone each for MIDI editing,
automation and recording and I/O, in that order, after M7 and M8; the second tier rides along
with whatever is in flight (5, 6 and 7 are small enough to be individual issues rather than a
milestone).

## Consequences

- "How far are we from Live?" now has an answer with a method behind it, and the answer separates
  *behind* from *not trying to*.
- The out-of-scope list is a commitment to say no: a request to support Push or Max devices gets
  this ADR as the answer, and reopening it means superseding this record.
- **The build order no longer follows the measurement.** The report ranks the gaps by manual pages
  and therefore puts the device catalogue first; this ADR puts the three things that make Suhina
  usable first, and says plainly that the third tier waits. Both lists stay, and they disagree on
  purpose.
- The first tier is three bodies of work with no milestone. Until the board gives them one, they
  are work everybody agrees is next and nobody is scheduled to do.
- Devices stay the largest single lever, and the device list in the report is what a decision about
  breadth will be made from - a decision this ADR deliberately does not make beyond "EQ and filter
  first, then the rest one at a time".

## Not decided here

- Milestone numbers for the first tier (the suggestion is one each, after M7 and M8).
- Whether any of the out-of-scope list is reopened (each would supersede this ADR).
- How deep a device must go to count as having a counterpart: the report counts a device as
  equivalent when it is the same *kind*, which flatters Suhina where Live's device has modes,
  sidechains and displays that Suhina's does not.
- Where the second tier's small items land: they are issues, not a milestone, and any of them can
  ride along with whatever is in flight.
