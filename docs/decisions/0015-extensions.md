---
description: How Suhina uses optional components it does not ship — separate programs with a manifest, a licence and declared permissions, never libraries linked into the process, and never downloaded by Suhina
read_when: extension, add-on, optional component, model weights, model licence, inference runtime, non-free, plug-in host boundary, extension manifest, third-party model, ONNX, separation
type: decision
status: proposed
built: no
last_verified: 2026-11-02
---

# 0015: Extensions: optional components outside the tree

- **Date:** 2026-10-13
- **Task:** stem separation ([#146](https://github.com/suhina-daw/suhina-daw/issues/146)) needs an
  inference runtime and model weights Suhina may not distribute. The shape decided here is general,
  so the next such feature does not invent its own.

## Context

Some of what Suhina wants to do needs a component the project cannot ship:

- **Model weights** whose licences are non-commercial, unstated, or contradicted by their own author:
  nearly every published separation checkpoint is trained on MUSDB18, whose terms forbid commercial
  use; the Demucs maintainer states the pretrained weights "are not covered by the MIT license, and
  are provided only for scientific purposes". The audit is in [#146](https://github.com/suhina-daw/suhina-daw/issues/146)
  and `docs/research/separation-weights-licence-audit.md`.
- **Native inference runtimes** such as ONNX Runtime: a C++ library and a build-time binary
  download, against the project's pure-Rust preference (rule 10 allows it; the preference does not).
- **User code and user assets**: patches ([ADR 0008](0008-patch-devices.md)), and later analysers,
  instruments or effects written by other people.

Suhina has already decided every piece that touches this, each in its own place: third-party code
runs out of process (AGENTS.md rule 5, [ADR 0006](0006-plug-in-hosting.md)); a user's patch reuses
that host, "because a patch is user code and can hang or crash like a plug-in"
([ADR 0008](0008-patch-devices.md) §2); devices are engine-independent so that they can be "moved to
its own repository" ([ADR 0004](0004-built-in-devices.md) §1, [ROADMAP](../../ROADMAP.md)); and the
ASIO SDK is vendored behind a cargo feature with `.cargo/config.toml` pointing at it and **nothing
downloaded** ([README](../../README.md) › ASIO, [`third_party/asiosdk/VENDORED.md`](../../third_party/asiosdk/VENDORED.md)).

What is missing is one concept for "an optional component the user has and Suhina does not": how it
is found, what it must declare, what may be assumed about it, and where the licence line runs.

The constraints: GPL-compatible dependencies only (rule 10); assets and weights need a licence the
project may rely on ([`ATTRIBUTION.md`](../../ATTRIBUTION.md); [ADR 0013](0013-sample-map-analysis.md)
sets the same bar for a learned model — its licence *and* its training data); GPL-3.0 §10 forbids
further restrictions on the covered work; §5 permits an aggregate of separate programs under other
licences. The FSF reads components that "share data structures" — shared memory with complex
structures included — as one combined program, and one reached by a simple `fork`/`exec` without
intimate communication as separate: "a main program that is separate from its plug-ins makes no
requirements for the plug-ins".

## Decision (proposed)

1. **An extension is a separate program, never a library.** Suhina starts it and speaks the boundary
   plug-ins already use ([ADR 0006](0006-plug-in-hosting.md): a message protocol, a deadline,
   health, restart). It is not a dynamic library that links Suhina's crates or shares its data
   structures: that would make it one combined program, so its licence would have to be
   GPL-compatible — which defeats the purpose — and a crash in a native inference runtime would be
   Suhina's crash.
2. **Nothing Suhina ships needs an extension.** A missing one is an ordinary state with a message
   that names it and says what it provides; every other feature keeps working. No shipped path may
   require a component whose licence the project cannot rely on.
3. **Suhina does not download extensions or models.** The user obtains the component and points
   Suhina at it, as the ASIO build points at the SDK: nothing is fetched, and no HTTP client enters
   the tree. A downloader would be its own ADR, and would never fetch weights whose terms forbid the
   use it enables.
4. **The extension describes itself in a manifest**: id, version, what it provides, the programs with
   their SHA-256, the licence (an SPDX id, or the rights holder's exact string) with the source it was
   read from and the date it was checked, and the protocol version it speaks. **A manifest without a
   licence is refused**; "no licence stated" is a blocker, not a blank — the same rule the model
   audit applies.
5. **What it may do is declared and shown.** As with patch devices ([ADR 0008](0008-patch-devices.md)
   §3), the manifest says what the extension reaches (read the project, run a job, write files in a
   folder it names); Suhina shows that and the licence before the user adds it, and the boundary
   enforces it rather than trusting it.
6. **Its licence travels with it**: a licence file at the top level of its folder, shown before it is
   added (the rule Blender's add-on guidelines set). What the project ships keeps the existing bar
   (GPL-compatible assets and notices); what the user installs is the user's, and Suhina asserts
   nothing about it.
7. **Extensions live in the settings folder**, `<settings>/extensions/<id>/`, with an environment
   override for development. A project never stores one: an instrument patch is project data
   ([ADR 0008](0008-patch-devices.md) §4), an inference runtime is not.
8. **The concept lands with its first user.** Stem separation is the first extension; the behaviour
   spec is written before the loader, and no extension point is built "just in case".

## Not decided yet

- **The transport for job-style extensions.** The plug-in host exchanges audio blocks with a deadline
  ([ADR 0006](0006-plug-in-hosting.md)); a separation is a long job that answers with a progress
  stream and files, so it may want a simpler request/response channel beside it.
- **One host binary extensions plug into, or one program per extension** — the `Hosting` choice
  [ADR 0006](0006-plug-in-hosting.md) already offers for plug-ins.
- **The protocol's version policy**: what an extension that is too new or too old gets, and whether
  Suhina refuses it or runs it with a warning.
- **Whether the app may open the rights holder's page** for a model the user must obtain itself (a
  link, not a fetch).
- **The audio thread.** Today: no extension runs on it; off-line jobs only.
- **Trust** beyond the SHA-256 in the manifest and the user's explicit add: no signature, no sandbox
  beyond the process boundary and the declared permissions, no registry or store.
- **GPU.** An extension may use one; the core does not grow a GPU dependency.

## Consequences

- A feature can land whose runtime or weights the project may not distribute, without weakening rules
  5 and 9, and a crash in a third-party runtime stays out of the editor.
- Two artefacts to install and keep in version step, and a support burden for "the extension is
  missing": the UI has to say it well.
- The loader reuses the plug-in host's process model (start, health, restart, kill) instead of a
  second supervisor.
- A new crate and an `extensions.*` command family; the crate map in
  [`docs/architecture.md`](../architecture.md) gains its row when code lands.
- Behaviour spec: [`docs/spec/extensions.md`](../spec/extensions.md).

## Alternatives considered

- **Bundle the runtime and the weights.** Impossible for the models that matter: a non-commercial
  field-of-use term cannot attach to a GPL work (§10). It would also break the never-crash rule and
  the pure-Rust preference.
- **Download weights on first use** (as whisper.cpp does). Common, and no enforcement action against
  the pattern is known, but it makes the end user the licensee of terms the project cannot accept on
  their behalf, and the project the one naming and fetching the artefact. Rejected for now; if it ever
  lands it needs the licence shown and accepted at the moment of the fetch, and never for
  non-commercial weights.
- **An in-process Rust library.** The FSF would call it one combined program, so its licence would
  have to be GPL-compatible, and a native runtime's crash would take the app down. Rejected.
- **Leave it to the user**, who runs an external tool by hand. The least work, but no placement of the
  result, no progress, no undo step: the feature would not be a feature.
- **Ship only models Suhina trains itself.** The clean answer, and still worth doing, but it is a
  training project rather than a feature; the extension point lets the feature exist before such a
  model does. Weights trained on permissively licensed or self-made audio, published under MIT or
  Apache-2.0 with a recorded data manifest, remain the goal.

## Sources

- FSF, GNU GPL FAQ: [plug-ins and combined programs](https://www.gnu.org/licenses/gpl-faq.html#GPLPlugins),
  [plug-ins under another licence](https://www.gnu.org/licenses/gpl-faq.html#GPLAndPlugins),
  [mere aggregation](https://www.gnu.org/licenses/gpl-faq.html#MereAggregation),
  [further restrictions](https://www.gnu.org/licenses/gpl-faq.html#OrigBSD),
  [additional permissions and controlled interfaces](https://www.gnu.org/licenses/gpl-faq.html#GPLIncompatibleLibs).
- [GPL-3.0 §5](https://www.gnu.org/licenses/gpl-3.0.en.html#section5) (aggregate) and §10 (no further
  restrictions).
- FSF, [working on freedom in machine-learning applications](https://www.fsf.org/news/fsf-is-working-on-freedom-in-machine-learning-applications)
  (parameters are both output and input of the software; a free ML application needs them free).
- OSI, [Open Source AI Definition](https://opensource.org/ai/open-source-ai-definition) (code,
  parameters and data information are separate) and the Linux Foundation's
  [OpenMDW](https://openmdw.ai/faq/) (a licence for weights that warns when mixed with copyleft).
- Blender: the [licence page](https://www.blender.org/about/license/) (the Python API makes published
  add-ons GPL) and the
  [add-on licensing guidelines](https://raw.githubusercontent.com/jlampel/blender_add-on_guidelines/refs/heads/main/09_licensing.md)
  ("any code that cannot be open source must be created as a separately run program and communicated
  with via an add-on that acts only as a bridge"; a licence file at the top level).
- [VS Code's Marketplace FAQ](https://raw.githubusercontent.com/microsoft/vscode-docs/00d84c75db5e8472604620e95eaebaa887809b63/docs/supporting/faq.md)
  (a marketplace's terms reaching a project's ecosystem),
  [GIMP's plug-in model](https://developer.gimp.org/resource/about-plugins/) (separate processes over
  a documented protocol), [Audacity's OpenVINO modules](https://github.com/audacity/mod-openvino-macos)
  (an AI module shipped and installed separately).
- Debian's [Social Contract](https://www.debian.org/social_contract) §1 and §5,
  [FFmpeg's legal page](https://ffmpeg.org/legal.html),
  [whisper.cpp's model downloader](https://raw.githubusercontent.com/ggml-org/whisper.cpp/master/models/README.md).
- This repository: [ADR 0006](0006-plug-in-hosting.md), [ADR 0008](0008-patch-devices.md),
  [ADR 0004](0004-built-in-devices.md), [ADR 0013](0013-sample-map-analysis.md),
  [`ATTRIBUTION.md`](../../ATTRIBUTION.md), [`third_party/asiosdk/VENDORED.md`](../../third_party/asiosdk/VENDORED.md),
  README › ASIO.
