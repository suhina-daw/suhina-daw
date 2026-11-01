---
description: The shape of collaboration and project versions in Suhina — three products with three physics kept apart, why it is core and not an extension, why the project becomes a folder, and why every asset is content-addressed
read_when: collaboration, peer-to-peer, P2P, shared session, host and guest, project versions, version browser, merge, content addressing, blob, manifest, project folder, relay, no server, extension or core
type: decision
status: proposed
built: no
last_verified: 2026-10-19
covers:
  - crates/model/src/lib.rs
  - crates/session/src/lib.rs
  - crates/project-file/src/lib.rs
  - crates/automation/src/server.rs
  - crates/plugins/src/protocol.rs
---

# 0023: Collaboration and project versions: the shape

- **Date:** 2026-10-15

## Context

Suhina was asked whether peer-to-peer collaboration would fit it. The answer is that "P2P
collaboration" names **three different products with three different physics**, that the valuable
one is the one nobody has built well, and that most of what it needs is a consequence of decisions
this project has already made for other reasons.

A feasibility pass produced these findings, all of which the decisions below rest on:

1. **Realtime audio and realtime editing are not the same problem.** Streaming a performance needs
   roughly 10–40 ms one way and wired connections; sharing a project's state tolerates 100–300 ms
   and, for editing, seconds. Every previous attempt at collaborative music software that tried to
   do both at once shipped neither, and none of the attempts survived. **All of them were
   services**, which is the part worth noticing: a feature used intermittently could not carry
   perpetual operating cost, and the audio assets were the expensive part.
2. **The architecture is already most of what this needs, none of it by design.** Every
   user-visible action is already a command dispatched through one funnel (`Session::execute` /
   `Projects::execute`), which the UI, the CLI and MCP all call; `model` is pure data with no audio
   (ADR 0002: "Projects are small (no audio data)"); an edit reaches a playing engine as a graph
   swap at a block boundary (ADR 0005), so **a remote edit is architecturally the same thing as a
   local one**; undo is a snapshot of the project, so a command log is enough; `launcher.state` is
   already a read command; a per-caller command refusal with a reason already exists
   (`crates/automation/src/server.rs`); and a local JSON-lines protocol with health, a deadline and
   restart already exists for plug-in hosts (`crates/plugins/src/protocol.rs`).
3. **The one real obstacle is object identity.** Clips, arrangement lanes and scenes have **no id**
   and are addressed by position, and the ids that exist are project-local `u32`s allocated as
   `max + 1`, so two machines collide by construction. A readable diff and a merge are both
   impossible without identity. This is decided in [0025](0025-object-identity.md), and it is
   justified by features that need no collaborators at all.
4. **The measured cost of today's behaviour is in the documents, not the audio.** In an authored
   `.als` project folder of 41 files and 39.2 MB: ten copies of the document (five named versions
   plus five timestamped backups) are **15.0 MB, 38 % of the project**, of which the backups alone
   are 8.3 MB; the audio is 22.8 MB, 58 %; the analysis sidecars are 2 % of the bytes but **48 % of
   the files in the samples folder**, sitting next to the audio. An `.als` is gzip-compressed XML —
   1 619 538 B from 11 847 274 B, 7.3× — and gzip does not deduplicate across files, so ten
   near-identical documents are stored ten times.
5. **Content addressing is a security mechanism here, not an optimisation.** With no server, nothing
   else vouches for the bytes a peer sent; a hash does. It also makes an asset merge a set union
   that **cannot conflict**, because content-addressed blobs are immutable.
6. **Hole punching is not reliable enough to depend on.** A large published measurement found
   success on roughly 70 % (±7.1 %) of attempts, and about 21 % of measured peers had no public
   address at all. A relay is therefore mandatory, not a nicety.
7. **Two things cannot be promised.** Audio clocks on two machines drift, so a stream needs rate
   adaptation; and floating-point summation order, fused multiply-add and plug-in nondeterminism
   mean **bytes can be guaranteed and a bit-identical render cannot**.

## Decision

1. **Three products, three physics, and a boundary that is written down.** Asynchronous sharing and
   merging (no latency budget), a shared live session (100–300 ms, and a launcher already quantised
   to bars, so latency is musically invisible), and a guest performer's streamed audio (10–40 ms,
   monitoring only). **The asynchronous half ships first**, and it is the half that is valuable to a
   single user. Conflating the first two is the documented failure mode; the boundary goes into the
   documentation so the next change does not erase it.
2. **This is core, not an extension.** ADR 0015 §2 says nothing Suhina ships may *need* an
   extension, and collaboration is a **network-effect** feature: its value is zero when the other
   side does not have it. A feature reached through the extension loader would therefore be
   unavailable to most users by construction. An extension boundary exists for a component whose
   **licence the project cannot rely on**, or **whose crash must not take Suhina down** (ADR 0015
   §1), and neither applies here: every dependency is GPL-3.0-compatible and the code is ours.
   **The illustrative contrast is deliberately not a file format.** It would be easy to write
   "bridging a legacy format is an extension because its value is local" and it would be wrong:
   where such a bridge's obstacle is a licence, this project's answer has been to remove the
   obstacle rather than the capability — as the VST2 interface decision being prepared beside this
   one does, making the interface description ours and the host a feature of the host binary
   already in this tree. **An extension is for what the project cannot do itself, not for what it
   would rather not ship.** (That decision was not in the tree when this one was written, so it is
   named rather than linked; a committed document does not link to a file the repository does not
   have.)
3. **The project operates no infrastructure.** No accounts, no discovery service, no hosted relay,
   no downloader. An invitation carries the addresses and a key, so a first connection needs no
   third party at all; a **relay is a separate binary the user runs**, with a documented protocol so
   a third party can write one. This is the structural difference from every previous attempt, and
   it is a licence position as much as an architectural one.
4. **A peer is a fourth front end, not a new abstraction.** It dispatches the same commands as the
   UI, the CLI and MCP, and its permissions and path confinement extend the patterns those callers
   already have. It is untrusted **data**, not untrusted code, which is why it does not need the
   process boundary a plug-in or an extension needs.
5. **All positions are musical time, and the tempo map merges by key.** Exactly one representation
   for where something is, and the tempo map is a list of changes keyed by beat (by bar for the time
   signature), so two versions merge it three-way against their ancestor. Decided in
   [0024](0024-musical-time-and-tempo.md), and
   independently of whether collaboration ever ships, because being wrong here is a model-wide
   rewrite later.
6. **Objects have stable identity, and identity never comes from content.** A hash cannot be an
   object's id: identity must survive an edit, and two identical objects must stay distinguishable
   from one object referenced twice. A hash is right for *content* and *blobs*. Decided in
   [0025](0025-object-identity.md).
7. **A project is a folder** — a document, a manifest and, when collected, its audio — and **the
   manifest maps each asset to its path and its content hash**. Referencing is the default and
   collecting is an explicit action. Derived analysis never lives in the project and never beside
   the user's samples; the one exception is metadata the **user** set, which is a decision rather
   than derived data and so needs its own store, keyed by content hash and **not** versioned.
   Decided in [0026](0026-project-folder-and-package.md).
8. **Git tracks the document and the manifest, never the audio.** Committing unchanged audio does
   not duplicate it, but every new take, bounce or render is a permanent blob and removing it means
   rewriting history. The manifest is small, textual, hash-keyed, and the only thing a merge has to
   touch.
9. **Merge is three-way against a common ancestor, and needs no git.** Objects present in the
   ancestor are matched by the ancestor's ids; objects created since are imported with the
   destination allocating their ids. Assets merge as a set union. A real `git merge` is made to call
   the same merge through a merge driver. Decided in [0027](0027-merge-and-versions.md).
10. **Versions are named and the browser is a version browser, not a graph.** Musicians already
    version by hand with "Save As"; the browser formalises that. The genuinely new part is the
    **musical diff** — which tracks changed, auditioned side by side or in sequence — which no
    textual diff can do.
11. **One host holds the truth.** A shared session is host-authoritative. That removes consensus
    from the problem entirely and is what makes global ids, change metadata and tombstones
    unnecessary. **A CRDT is not adopted**, and is revisited only if simultaneous editing of one
    project by several writers is ever wanted, because a CRDT makes every future model field
    merge-compatible forever.
12. **Nothing here is claimed to be the product's reason to exist.** A DAW is adopted for being a
    DAW, so this is a retention and differentiation feature rather than an acquisition one, and it
    does not compete with finishing the DAW. The honest claim is narrower and defensible: **version
    control for music is valuable to one person**, which is exactly what the previous generation of
    collaborative tools was not.

## Consequences

- Five further decisions are needed before implementation, each its own ADR: musical time
  ([0024](0024-musical-time-and-tempo.md)), object identity ([0025](0025-object-identity.md)), the
  folder and the package ([0026](0026-project-folder-and-package.md)), merge and versions
  ([0027](0027-merge-and-versions.md)), and later the session protocol, the transport and the audio
  link. **Until those exist, the work must not start**: an agent or a contributor given only the
  shape will fill the gaps confidently and invisibly.
- A new crate for the session protocol and the peer boundary sits beside `automation`, and a
  `suhina-relay` binary beside `suhina-plugin-host`. Nothing below the UI depends on a network
  library, and no new code is reachable from the audio callback (rule 3).
- Two parts are cheap now and expensive later, and should land early even though the epic is late:
  **musical time** and **object identity**. Every week in which real projects are saved makes
  migrating them more expensive, and there is no installed base today.
- `SECURITY.md` already scopes "any control channel"; a listening socket with authentication, a
  permission model, path confinement and rate limiting needs a written threat model, because with
  no relay in front of it there is nothing to absorb abuse.
- Documents grow: the schema is versioned independently of any replication format, migrations are
  idempotent and commutative, and syncing across incompatible major versions is refused rather than
  merged lossily.
- A degraded state is a first-class state: a merged project whose assets or plug-ins are absent must
  be **better than no collaboration at all**, which means an explicit per-asset state, deterministic
  silence in the offline path, and a golden file for exactly that case. ADR 0006 already helps here
  and for an unrelated reason: plug-ins run out of process, so a peer without one still opens a
  coherent project.
- The largest measured win is not where it was expected. Deduplicating and delta-compressing the
  **document** across versions saves more than anything done to the audio, and it works because
  ADR 0010 chose readable JSON over a compact binary format. That choice now has a second
  justification that was not available when it was made.
- Behaviour specs come before code (rule 1): `docs/spec/session-sharing.md`,
  `docs/spec/project-package.md`, `docs/spec/project-versions.md`.

## Alternatives considered

- **Build it as an extension** (ADR 0015's shape: a separate program the user obtains and points
  Suhina at). Rejected: §2 forbids a shipped feature needing one, and the network-effect argument
  makes it fatal rather than merely inconvenient. A relay *is* legitimately extension-shaped, but it
  is better served by a core binary with a documented protocol, because its whole purpose is to be
  trivially available at the moment it is needed.
- **Run a service**, as every previous attempt did. Rejected on the evidence: the ones that were
  services are gone, and the reason recorded for each is the business model rather than the
  technology. A self-hostable relay converts an unbounded operating cost into a bounded, optional
  one and removes the thing that can be shut down.
- **Do it as a separate repository or an SDK the user installs.** Rejected: a git repository has no
  legal meaning (code that links Suhina's crates is one combined program wherever it lives), so it
  is not a licence boundary, and a network-effect feature has to be in the release. **The one part
  that does stand apart is the relay, and what makes it apart is not where its source lives.** Its
  reference implementation sits in this tree beside `suhina-plugin-host` (see Consequences), because
  a protocol and its implementation stay in step only if they ship together; what is separate is its
  **release** and its **documentation**, which is what lets a third party run one, reimplement one,
  or take it to a repository of its own. Starting it in a repository of its own would trade the
  thing that matters — that it is trivial to obtain at the moment a connection fails — for the thing
  that does not: where the source lives. ADR 0004 §1 keeps the same door open for devices, which are
  engine-independent precisely so they *can* move out one day.
- **Adopt a CRDT from the start.** Rejected while a single host holds the truth, and for a reason
  that outlives the first version: a convergent document makes every future model field
  merge-compatible forever, which is a permanent tax on every change (rules 6 and 7 make it
  expensive). It stays available if simultaneous multi-writer editing is ever wanted.
- **Realtime first**, since that is what "collaboration" usually means. Rejected: it is the half
  with the hard physics, the half that is already a commodity elsewhere, the half blocked on
  recording, and the half that musicians demonstrably did not buy.
- **Change nothing, since projects can already be zipped and shared by hand.** Rejected, but
  honoured in the ordering: this is genuinely the status quo and it works, which is exactly why the
  asynchronous half must come first and be measured against it.

## Not decided yet

- The session protocol: the wire format, the invitation, and the permission model in detail.
- The transport: which library, and what happens on a connection with no public address.
- The audio link: the jitter buffer, the rate adaptation for clock drift, and the quantisation of
  the measured delay to a musical interval.
- Whether git is optional in the ordinary sense, and precisely how its absence degrades.
- Whether a version is a package, a tag or a commit in the user's terms.
- Whether the project's own sample store is per project, per user or shared, and how assets are
  collected back (reference counting versus a reachability scan).

## Sources

- Ableton Link, on tempo changes as timeline segments (read for the first version of
  [0024](0024-musical-time-and-tempo.md)):
  <https://help.ableton.com/hc/en-us/articles/209776125-Link-features-and-functions-FAQ>
- The published large-scale measurement of hole punching (4.4 M attempts, 85 000 networks, 167
  countries): <https://ar5iv.labs.arxiv.org/html/2510.27500>
- An empirical study of network latency in ensemble performance, which found quality degrading
  smoothly with delay rather than falling off a cliff:
  <https://chrischafe.net/effect-network-latency-asymmetry/>
- Network-audio tools read as prior art for the streaming layer: <http://cockos.org/ninjam/>,
  <https://jamulus.app/wiki/Getting-Started>,
  <https://ccrma.stanford.edu/groups/soundwire/software/jacktrip/index.html>. SonoBus is GPL-3.0 and
  JackTrip is MIT, so either may be ported with its notices kept and credited in `ATTRIBUTION.md`
  under rule 10; the clean-room rule concerns proprietary products and neither is one. What ports is
  the streaming structure and the jitter buffer — the alignment problem, which is the hard part, this
  project already solves (ADR 0005, ADR 0006).
- This repository: ADR 0001, ADR 0002, ADR 0005, ADR 0006, ADR 0010, ADR 0011, ADR 0012, ADR 0013,
  ADR 0015, ADR 0007 (copying between open projects, listed there as not decided),
  `docs/spec/als-format.md` and `docs/spec/als-corpus.md` (where the observations of an authored
  `.als` project folder belong, and where its "what Collect All and Save does" item is already
  listed), `docs/spec/missing-files.md`, `SECURITY.md`, and issues #325, #352, #402.
