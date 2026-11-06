# Session sharing: the peer protocol and a peer's command permissions

**Type:** enhancement · **Area:** proposes **`collaboration`** (see Notes — no existing area label covers this) · **Phase:** M8 or later · **Priority:** P2

**This issue must not be started without the decisions it waits for.** It is the highest-risk item in [the epic](https://github.com/suhina-daw/suhina-daw/blob/main/issue-collaboration-and-project-versions.md): it is the first code in this tree that accepts commands from another machine, and `SECURITY.md` scopes *"the CLI, MCP server and any control channel"* without saying yet what a peer is allowed to do. A peer that can run Read commands can read the whole project; a peer that can name a path can point it at the host's disk. Guessing at this produces a vulnerability with a friendly name, so the guard-rails below come before the first line of code. [ADR 0023](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0023-collaboration-and-project-versions.md) says the same thing in its own words and is the authority for this issue's status: *"**Until those exist, the work must not start**: an agent or a contributor given only the shape will fill the gaps confidently and invisibly."*

**Depends on:** a written **threat model** and `docs/spec/session-sharing.md`; issue 02 (a guest's commands name objects) and issue 03 (what a project is on disk); and the ADR that decides whether permission policy lives in `session` as caller identity or in the peer layer — [ADR 0023](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0023-collaboration-and-project-versions.md) leaves the wire format, the invitation and the permission model in detail under its *Not decided yet*, so **they are genuinely not decided and this issue is not the place to decide them silently.** **Until they exist, this issue is a placeholder and its board item belongs in `Do not start yet`.**

## Goal

A host can invite a guest to a running session, the guest's commands go through the same funnel the UI, CLI and MCP use, and every command the guest is not permitted to run is refused **with a reason** and with nothing changed.

## The shape this is meant to take, and why it is small

A peer is **a fourth front end over the same commands**, not a new abstraction. That is not optimism; the pieces exist:

| Needed | Already there |
|---|---|
| One funnel every caller uses | `Session::execute(id, &params)` / `Projects::execute` (`crates/session/src/lib.rs`, `crates/ui/src/app.rs`) — 92 commands, all with JSON parameters |
| Refusing a command to a caller, with a reason | each command's `Policy::agents` and `refused(id)` already do exactly this for agents (`crates/automation/src/server.rs`) |
| Confining a path a command names | MCP's read roots already check every path a command reads (`crates/automation/src/workspace.rs`) |
| A protocol with a deadline and a restart, over a local socket | `crates/plugins/src/protocol.rs` — JSON lines, `interprocess` already a dependency, health and deadline (ADR 0006) |
| Describing every command to a caller | `commands.list` returns every command with its parameters |

So the work is a protocol, a permission policy, and the confinement that already has a pattern to copy — **not** a new command system.

## Scope (outline only — the spec decides the details)

1. **The threat model first**, extending `SECURITY.md`: what a peer may do; what a peer must never do; what happens on a malformed message, a flood, a huge parameter, a command that names a path, a disconnect mid-edit and a repeated invitation. It is a document under `docs/`, not a paragraph in an issue.
2. **Authentication and the invitation.** A guest is authenticated before it can run anything, the invitation carries what is needed to prove it, and an invitation cannot be replayed or forged. The transport is issue 11; this issue says what the protocol needs from it.
3. **The permission model.** A permission set per caller, checked before `execute`, with a reason string on refusal — the shape `refused(id)` already has. Sensible defaults, stated: what a guest may do (launch clips, move a fader, follow the transport), what it may not (delete tracks, save, add or remove plug-in and library folders, name a path outside the project), and **who may change the set — the host only**.
4. **Peer capability is local policy.** A peer must never be able to loosen the host's policy, and a customisable setting (a key, a template, a permission) must never be settable by a remote caller. This is a product rule the epic's discussion arrived at and it belongs in writing before the code.
5. **Path confinement** for every command a guest runs, by the pattern `crates/automation/src/workspace.rs` already uses.
6. **What a guest can learn.** Read commands expose the project. The permission set decides how much, and the spec says so rather than leaving it implied by the allow-list.
7. **One caller, one undo history.** Undo is a snapshot of the project (ADR 0002); the spec says what a guest's edit does to the host's undo stack, and what an agent's does today, so the two are consistent.
8. **What a disconnect does.** Nothing on the host is lost; the host keeps playing and its project is untouched — the same promise, and the same reasoning, as a crashed plug-in host (ADR 0006).
9. **Tests** (AGENTS.md rule 7), including a refusal for every permission class and confinement case, and a fuzz-style hostile-input test for the protocol.

## Acceptance criteria

- [ ] A threat model exists under `docs/` and `SECURITY.md` links to it.
- [ ] `docs/spec/session-sharing.md` exists and describes the protocol, the permission classes, the refusals and the disconnect behaviour **before** the code lands.
- [ ] A guest that is not authenticated can run nothing, and a replayed or forged invitation fails.
- [ ] Every command a guest runs is checked against its permission set; a refused command returns the reason, changes nothing and records no undo step.
- [ ] A path-naming command from a guest cannot reach outside the project, tested with `..`, an absolute path, a symlink and a very long path.
- [ ] A guest cannot change the host's permission set, settings, keymap, theme or library folders — tested per class.
- [ ] Malformed, oversized, truncated and flooded input is refused by name and never panics or allocates without bound (AGENTS.md rule 2); the caps are stated.
- [ ] Disconnecting a guest leaves the host's project exactly as it was and playing.
- [ ] **No new code is reachable from the audio callback:** `Renderer::process` still never allocates, locks, blocks or does I/O (AGENTS.md rule 3), tested as the existing ones are.
- [ ] With no peer connected, everything behaves exactly as it does today.
- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` pass.
- [ ] Documents updated: a new crate in the map in `AGENTS.md` and `docs/architecture.md`, the new area label, `SECURITY.md`, `ROADMAP.md`, and `ATTRIBUTION.md` if a dependency is added.

- [ ] The pull request's description says **how to check the change** — numbered steps a
      non-musician can follow from a test build — or, when nothing a user can notice changed, one line
      saying so.
## Not in scope

- **The transport.** Direct connection, hole punching and the relay are issue 11, and the relay binary is issue 12. This issue is the protocol and the policy that sit on it, and it should be testable over a local socket with no network at all.
- **Simultaneous editing of one project.** One host holds the truth; no CRDT, no global ids, no change metadata (the epic excludes them).
- **Audio.** A shared session's audio link is issue 14 and needs issue 13 first. This issue moves commands and state.
- **A discovery service, accounts, or anything the project operates.**

## Notes

- **Area label.** The available set is `engine`, `ui`, `formats`, `browser`, `audio-io`, `plug-ins`, `devices`, `presets`, `tests`. None of them means "another machine". **`collaboration` is proposed**; until it exists, this issue carries `formats` as a placeholder and the mismatch should be resolved rather than tolerated.
- **Where the code goes** (`docs/architecture.md` › Where new code goes): a new layer-2/3 crate for the protocol and the peer boundary, beside `automation`, with the permission policy either there or in `session` as caller identity — **and that placement is a decision, not a convenience.** The existing agent denylist lives in `automation` because MCP is one caller; a shared session has several roles at once (the host, a guest, a guest with a pencil), so the policy may belong one layer down. **Nothing below the UI may depend on a network library**, and the protocol must be exercisable in `cargo test` with no network.
- **The nearest thing to a template is the plug-in host's protocol** (ADR 0006): JSON lines, a deadline, health, restart, and a crash that costs a sound rather than the project. It has no authentication, so it is a shape to copy and not a design to reuse.
- **Related:** [SECURITY.md](https://github.com/suhina-daw/suhina-daw/blob/main/SECURITY.md) (the scope this makes concrete), ADR 0002 (commands, callers, one undo step), ADR 0005 (edits while playing — a remote edit is architecturally a local one), ADR 0006 (protocol shape, and a disconnect that costs nothing), ADR 0015 §2 (absence is an ordinary state), issue 02, issue 03, issue 11, issue 12, issue 14.
- **Decisions that belong in a spec or ADR, not here:** the message format; the permission classes and their defaults; where the policy lives; what an invitation contains; whether a guest may edit at all in the first version; what a guest may read; and how the protocol's version is negotiated.
- **A question to resolve first, and this issue should not proceed until it is answered:** **is the first version read-only for a guest?** A guest that may only watch, follow the transport and hear the result needs no edit permissions, no undo interaction and a much smaller threat model — and it delivers the "watch and listen" half of the feature on its own. If the answer is yes, split the edit permissions into a follow-up and this issue becomes small and much safer.
  - **Proposed answer: yes, read-only first.** Nothing is wasted — a read-only guest needs the same protocol, the same transport and the same authentication — and it takes the three hardest questions out of the first version at once: what a guest's edit does to the host's undo stack, how a guest's edit is attributed, and the whole "a remote caller wrote to my disk" class of bug. It also matches how the feature was first described: open somebody else's session in another tab, watch and listen to what they are doing.
  - **The counter-argument, which is real:** a guest who can do nothing may feel useless, and the first thing a collaborator asks is why they cannot move a fader. The restriction therefore has to be **visible and explained** — "you are watching; the host can hand you the pencil" — rather than silent. If the host cannot grant edit rights without a restart, that is the part worth designing here even when the rights themselves come later.
- **Board:** `enhancement`; Phase M8 or later; Priority P2; **Status: `Do not start yet`** until the threat model and the spec exist.
