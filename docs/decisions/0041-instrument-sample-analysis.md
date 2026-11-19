---
description: Keep reusable sample analysis separate from real-time DSP, source access and host-owned jobs
read_when: instrument analysis crate, sample analysis dependency boundary, background analysis admission, root proposal adoption
type: decision
status: proposed
built: no
last_verified: 2026-11-20
covers:
  - crates/instrument-analysis/
  - crates/dsp/src/pitch.rs
  - crates/session/src/instrument_analysis.rs
---

# 0041: Pure sample analysis and host-owned adoption

- **Date:** 2026-11-20
- **Issue:** [#1596](https://github.com/suhina-daw/suhina-daw/issues/1596), part of
  [#1579](https://github.com/suhina-daw/suhina-daw/issues/1579)

## Context

An instrument can use local recordings to propose its root note without changing playback.
The same analysis must be usable by the stock device and future exported editors. Source
permissions, project identity and undo belong to the host; preparation must never enter the
audio callback. The sample map already has a frame pitch estimator worth sharing, but its
classification, file scanning and map layout are unrelated to instrument preparation.

## Decision

1. **Create `suhina-instrument-analysis` as a pure preparation library depending only on DSP.**
   It borrows decoded PCM and returns bounded measurements/proposals. It owns no files,
   projects, windows, threads or device instances. It may allocate bounded scratch memory.
2. **Keep the reusable frame estimator in `suhina-dsp`.** Both sample-map features and instrument
   analysis call it. Preserve the sample map's estimator settings, arithmetic and cache meaning.
   Its prepared buffers distinguish allocation during construction from estimation.
3. **Let each host own jobs and adoption.** The stock session admits at most two workers per
   process and retains at most one job/proposal per session. Cancelled work counts until it
   releases worker resources. Workers receive immutable PCM, never a project or a path.
4. **Accept explicitly through an ordinary edit command.** Completion is informational.
   Acceptance rechecks source identity, trim, original root and device identity; it overrides
   root automation through existing manual-edit semantics and participates in normal undo.
5. **Expose host preparation through an SDK widget.** Analysis controls are commands, not
   automatable sound parameters. An exported host must supply its own jobs/permissions/editor
   integration before claiming equivalent preparation support.

## Consequences

Playback remains independent of analysis and restoration never refits the source. A shared
library can be tested with authored PCM without a DAW, filesystem or GPU. A process-wide
admission limit gives stock sessions a predictable concurrency bound; it intentionally rejects
new work rather than retaining an unbounded queue. The source PCM already held by playback is
outside the scratch budget. Uncertain recordings remain playable without a pitch proposal.

## Alternatives

Putting jobs in the device runtime would mix host identity and permissions with rendering.
Depending on the sample-map crate would pull file-library and classification concerns into an
instrument's preparation boundary. Keeping the entire analyzer in the session would duplicate
it when exported editors gain sample preparation. Automatically applying every estimate would
retune the user's instrument without an explicit, inspectable choice.

## Sources

- [Analysis contract](../spec/instrument-analysis.md), including algorithm sources and limits.
- [Architecture](../architecture.md) and [command model](0002-commands.md).
- Own sample-map estimator, session commands and device SDK host widgets.
