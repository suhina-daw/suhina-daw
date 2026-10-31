# AGENTS.md: guide for AI agents and contributors

Suhina is an open-source, native, clip-launching DAW written in **Rust only**, comparable in
workflow to Ableton Live. The product name is always written **Suhina** in user-facing text;
machine names stay lowercase (`suhina-*` crates, binaries, file names). This file holds the rules
every task needs; the rest is one link away (§ 2), read only when the task needs it.

**Ask for documentation, do not read the index:** `python scripts/docs.py find WORDS` lists the
documents about something, best first, and `python scripts/docs.py covers PATH` the documents that
describe a file. Read only the ones that match your task.

## 1. Golden rules

1. **Clean-room (outranks everything).** Never disassemble or decompile proprietary binaries,
   never use leaked code, never commit content from commercial products; knowledge comes from
   observation, self-authored files and public documentation, written down in `docs/spec/` before
   it is implemented. The rules and their one exception (the VeSTige header,
   [ADR 0022](docs/decisions/0022-vst2-interface-description.md)) are in
   [CONTRIBUTING.md › Clean-room](CONTRIBUTING.md#clean-room).
2. **Never crash.** Non-test code never panics (no `unwrap()`, `expect()`, `panic!`,
   `unreachable!`, `todo!`, `unimplemented!`): return the crate's error type, and treat
   input-derived numbers as hostile (`get()`, `checked_*`, allocations capped). No `unsafe`, except
   small, tested blocks with a `// SAFETY:` comment behind a safe API in `crates/ipc`,
   `crates/plugin-host` ([ADR 0006](docs/decisions/0006-plug-in-hosting.md)) and `crates/mp3`
   ([ADR 0029](docs/decisions/0029-mp3-encoder.md)).
3. **Real-time safety.** Code reachable from the audio callback never allocates, frees, locks,
   logs or does I/O, and blocks only to wait for a plug-in host's answer within the block's
   deadline (ADR 0006 › 4). Hand it data through lock-free queues (`rtrb`) and swap whole graphs.
4. **One render path.** Offline rendering and real-time playback run the same engine code, so
   offline renders are the test oracle for the engine.
5. **Plug-ins run sandboxed, out of process.** A crashing or hanging plug-in never takes down the
   engine or loses the project. The boundary is IPC from the first line of plug-in code, with no
   in-process shortcut ([ADR 0006](docs/decisions/0006-plug-in-hosting.md)).
6. **Everything is a command.** User-visible behaviour is a command in the engine with an id,
   params and tests; the UI, CLI and MCP all dispatch the same commands.
7. **Tests are the gate.** Every change comes with tests: round-trip and malformed-input tests for
   formats, golden-file renders for DSP and engine behaviour. A test never depends on timing: no
   `sleep` to give a thread time, no wall-clock deadline or elapsed-time bound, no polling loop
   that waits for another thread, no result that depends on the order threads run in. CI runners
   are slow and uneven, and such a test fails there once in a while and teaches everyone to
   re-run the job. Render a fixed number of frames with `process`, use a fake clock, or wait
   on an explicit signal (a barrier such as `Session::sync_recording`, a channel, a join).
8. **Build every device's panel on the SDK, and its DSP from shared blocks.** Built-in effects,
   instruments and MIDI effects are native code, never `.sd` graphs, with a panel from the
   `device-sdk` UI system: the generated parameter list or a layout
   ([ADR 0021](docs/decisions/0021-device-graph-files.md) › Amendment: device panels). Their DSP
   comes from `crates/dsp`, where missing DSP is added so each block exists once.
9. **Other DAWs are named only where a format is handled:** format crates (`crates/als`), their
   tests, `docs/spec/`, `docs/decisions/`, the design brief, and the parity measurement
   (`docs/research/parity-live-12.md`, `scripts/parity/`; never content from another product's
   manual). ROADMAP, README, the model, engine, CLI and UI describe Suhina's own features under
   their own names and neutral words, and other DAWs' product terms are not used as generic words
   ("drum sampler", not "drum rack"). User-facing text may name a format it imports or exports.
   The `daw-compare` skill checks text before it is posted.
10. **GPL-compatible dependencies only** (MIT, Apache-2.0, BSD, ISC, Zlib, MPL-2.0, LGPL,
    GPL-3.0; not GPL-2.0-only or non-commercial). Run `cargo deny check` before adding or updating
    one ([CONTRIBUTING.md › Dependencies](CONTRIBUTING.md#dependencies)).
11. **No links to private agent sessions** (`https://claude.ai/code/session_...`) in commits,
    pull requests, issues or comments, even when the tool adds one. The `Co-Authored-By:`
    trailer stays.

## 2. Where to read more

| When | Read |
|---|---|
| Where code goes, what a crate does, adding a crate or a source file | [`docs/architecture.md`](docs/architecture.md): layers, the crate table, crate names, the licence header |
| Opening an issue, claiming, naming a branch, opening a pull request, posting as the bot | [`docs/workflow.md`](docs/workflow.md) › Tasks |
| Writing a pull request: *How to check it*, parity, settings help, devices, which documents to update | [`docs/workflow.md`](docs/workflow.md) › What a pull request carries |
| A common change: a command, a device parameter, a setting, an engine or UI test | the area skills in `.claude/skills/` (`working-on-suhina` lists them) |
| Reviewing a pull request | [`docs/code-review.md`](docs/code-review.md) |
| Writing or changing a document | [`docs/README.md`](docs/README.md) |

## 3. Tasks

Issues, claims and branch names are for work that will become a pull request; local work, reviews
and questions skip them. The workflow is in [`docs/workflow.md`](docs/workflow.md), the one copy
for people and agents. An agent never adds `ready to merge`, and never merges or queues a pull
request unless the person asks for that one.

## 4. Before you finish a task

When the task changed code (a review changes nothing and runs nothing):

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check             # when Cargo.lock changed
python scripts/docs.py check
```

Keep the tree building at every step. Report each check as passed, failed (with the output) or not
run (and why). A pull request also carries what [`docs/workflow.md`](docs/workflow.md) › What a
pull request carries lists: *How to check it*, parity, settings help, a rebuilt app for a device,
and the documents that describe what changed.

## Code Review Rules

Before reviewing a pull request, read [`docs/code-review.md`](docs/code-review.md). Follow its
procedure, the rules in this file and any more specific instructions for the changed paths. A
review runs no tests or builds: it reads the CI results of the push it reviews.

Trace changed behaviour through callers, shared code and side effects. Check the agreed scope
against relevant issues and documents, previous fixes and existing review feedback. First check
whether an open or recently merged pull request already solves the same problem; when one does,
that is the first finding. Equivalent
operations share project semantics, but desktop and MCP authorization may deliberately differ;
batch and nested execution must preserve the caller's restrictions.

Report actionable defects supported by the current code. Keep verified defects, unmeasured
risks and optional improvements apart. Optional suggestions belong in the summary, not inline
threads: the merge queue waits until every thread is resolved. State the reviewed head commit
and material limits; use the severity definitions and reporting limits in the review guide.

Review output does not grant permission to approve, request changes through a formal GitHub
review, merge or queue a pull request. Only a maintainer adds `ready to merge`
([CONTRIBUTING.md](CONTRIBUTING.md#merging-many-pull-requests)); a clean review or green tests
are never permission to merge.
