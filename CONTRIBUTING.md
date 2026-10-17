# Contributing

- **Language:** the product is Rust only. Scripts in `scripts/` are Python, and the FM synth's
  reference renderer is C++; neither is built into the program.
- **Licence:** Suhina is GPL-3.0-or-later (`LICENSE`, `NOTICE`), and contributions are
  licensed the same way. Dependencies and bundled libraries must be GPLv3-compatible (MIT,
  Apache-2.0, BSD, ISC, Zlib, MPL-2.0, LGPL, GPL-3.0); GPL-2.0-only and proprietary or
  non-commercial licences are not. CI checks every dependency's licence with `cargo-deny`
  (see [Dependencies](#dependencies)).
- **Never crash:** non-test code must not panic and must not use `unsafe` (the only exceptions
  are the plug-in hosting crates `ipc` and `plugin-host`, ADR 0006). Errors go through `Result`
  and `?`; input-derived indices and sizes are checked. See `AGENTS.md`.
- **Real-time safety:** code that runs on the audio thread never allocates, locks, blocks or does
  I/O. Communicate with it through lock-free queues.
- **Tests:** required for every change. Format code needs round-trip and malformed-input tests;
  DSP and engine code is tested by rendering offline and comparing against golden files.
- **Documentation:** a change that alters how something works updates the documents that
  describe it in the same pull request (architecture, ADRs, specs, ROADMAP); `AGENTS.md` ›
  *Before you finish a task* lists which document covers what. Leave out what `main` writes after
  the merge: `docs/INDEX.md`, `docs/commands.md`, the parity report's marked parts and figures, and
  the `last_verified` line of a document you change (it is stamped with the day of the merge).
- **Parity measurement:** a pull request that adds, changes or removes something a user can do
  updates its coverage in `scripts/parity/score.py` (and `scripts/parity/devices.json` for a
  built-in effect or instrument) in the same pull request, and leaves the numbers in
  `docs/research/parity-live-12.md` alone: they are regenerated on `main` after the merge. One
  that moves no coverage writes `Parity: no change` and the
  reason in its description. CI warns, without failing, when a change to `crates/`, `devices/` or
  `apps/` does neither. `AGENTS.md` › *Before you finish a task* has the rule.
- **Style:** `cargo fmt`, and `cargo clippy --all-targets -- -D warnings`. Comments explain *why*.
- **Names:** packages are `suhina-<name>` (`suhina-device-<name>` for devices), used in code as
  `suhina_<name>`; directories have no prefix. `AGENTS.md` › *Crate names* says how a new crate
  is added; CI rejects the project's old name (`python scripts/rename_suhina.py --check`).
- **Licence header:** every new source file starts with the licence header; run
  `python scripts/license_header.py` to add it (`AGENTS.md` › *Licence header*). CI checks it
  with `--check`.
- **Commits:** small, focused, with a clear subject line, and signed off (see
  [Developer Certificate of Origin](#developer-certificate-of-origin)).
- **Issues:** small and scoped, with acceptance criteria (the issue forms ask for them: Bug, Plug-in problem, Feature request, New effect or
  instrument, Preset contribution, Task);
  architecture belongs in an ADR under `docs/decisions/`, not in an issue thread. Every issue has
  a type label (`bug`, `enhancement`, `documentation`, `question`) and one or more area labels
  (`engine`, `ui`, `formats`, `browser`, `audio-io`, `plug-ins`, `devices`, `presets`, `tests`),
  plus `windows`, `macos` or `linux` when it concerns one platform. The forms set the type, and
  `.github/workflows/issue-labels.yml` adds area and platform labels from the form's answers.
- **Pull request labels:** `.github/workflows/pr-labels.yml` adds them as `.github/labeler.yml` maps
  them: the kind from the branch name (`feature/` `enhancement`, `fix/` `bug`, `docs/`
  `documentation`, `chore/` `chore`; also `documentation` when every file changed is a document)
  and the areas from the paths changed. It only adds labels; add a platform label by hand.
- **CodeRabbit:** reviews every pull request that is not a draft against AGENTS.md
  (`.coderabbit.yaml`) and shows a **CodeRabbit** check beside CI. Its comments are suggestions, not requirements: answer or resolve
  them, and leave DSP and real-time behaviour to the tests and renders. The check is never red and
  never required. Comment `@coderabbitai review` to run it again.
- **Asking for a review:** ask a maintainer (**Reviewers** on the pull request). Comments by
  `suhina-bot[bot]` are a maintainer's agent's feedback, posted for them; the bot takes no
  requests, and neither it nor CodeRabbit approves anything: a pull request goes in on a
  maintainer's `ready to merge` label.
- **Pull request description:** use the sections of `.github/pull_request_template.md`, all of them;
  one line is enough where nothing applies. `.github/workflows/pr-description.yml` warns about a
  missing section (`scripts/pr_description.py`); it never fails.

## Dependencies

Every crate in `Cargo.lock`, with all features on and for every platform, must have a licence
that [`deny.toml`](deny.toml) allows: the list in AGENTS.md rule 9, plus a few exceptions, each
named for one crate with the reason. Dependencies come from crates.io only, and a crate with an
open RustSec advisory fails unless `deny.toml` ignores it with a reason. The **Dependencies**
workflow (`.github/workflows/deny.yml`) runs this on every pull request. Run it locally with the
version pinned in that workflow:

```sh
cargo install cargo-deny --version 0.20.2 --locked
cargo deny check
```

**Dependabot** opens the updates: a security update as soon as a RustSec or GitHub advisory
names a crate Suhina uses, and on Mondays the week's version updates
([`.github/dependabot.yml`](.github/dependabot.yml)): minor and patch updates of the Cargo
workspace grouped in one pull request, a major update in a pull request of its own, and the GitHub
Actions the workflows use. They go through CI and review like any other pull request; a major
update that changes an API needs the code changed in the same pull request.

When it rejects a licence, prefer another crate. Add an exception only for a licence that is
GPLv3-compatible, for the crate that needs it, with a comment saying what the crate is for and
why the licence is fine. To update `cargo-deny`, change `CARGO_DENY_VERSION` and
`CARGO_DENY_SHA256` in the workflow together, and the version above.

## Working on an issue

A task's state lives in its issue and on the [Suhina project board](https://github.com/orgs/suhina-daw/projects/1),
never in chat or in one person's window. The rules below keep two people from doing the same work
twice without making anyone wait on someone who has gone quiet.

**One issue per problem.** Before you open an issue, search the open ones; when one exists,
comment there instead. A duplicate that slips through is closed with a link to the first one.

**Free work** has no assignee and no open pull request that says `Closes #N` (the issue's
*Linked pull requests* box shows it). On the board it is in `Backlog` or `Ready`, with one
difference: a **feature request** (`enhancement`) in `Backlog` is not agreed yet. Write your idea or
plan in the issue; a maintainer moves it to `Ready` when Suhina wants it, and only then is it open to
claim. A bug in `Backlog` is free. `Do not start yet` means the order is deliberate: the issue and
its comments say what the work waits for, so ask there instead of starting.

**Claim it** in a comment on the issue before you write code:

> /claim Plan: … When: …
> Touches: crates/ui/src/detail, crates/session

The claim bot ([`docs/claim-bot.md`](docs/claim-bot.md)) moves the card to `In progress` and
assigns you, and refuses the claim when the work is not free or another issue it is *Blocked by*
is still open. The `Touches:` line names the paths you will change; it is optional, and with it the
bot's answer lists the open pull requests and other claims that touch the same code. Without the bot, a plain comment
("I'd like to take this. Plan: …") is the claim, and the first one wins: a bot that is down never
means the issue is free.

A maintainer who hands an issue to someone writes `/claim @their-login`, with the plan after it if
there is one: the bot makes the same checks for that person, assigns them and moves the card, also
when they were assigned by hand and the card was never moved. Only a maintainer may claim for
someone else.

**Show the work within 48 hours.** Open a draft pull request that says `Closes #N` as soon as you
have a first commit. A claim with no pull request after 48 hours lapses, and the next `/claim`
takes it over. If you stop, comment `/unclaim`, so the card goes back to `Ready`.

**One pull request per issue.** If a pull request for the issue is already open, continue there
(push to it, or offer your change in its thread) instead of opening a second one; that holds for
a new session of your own as well. When that pull request has stalled, ask its author in it; with
no answer in three days, you may take the work over in that pull request, or in one that replaces
it, names it and is linked from it.

**Name the branch for its kind of work.** `feature/`, `fix/`, `docs/` or `chore/`, then two to
five lowercase words joined by hyphens: `feature/piano-roll-workflow`, `fix/short-panes`,
`docs/latency-readout`, `chore/branch-names`. Agents create such a branch before their first push,
never opening a pull request from a `claude/...` branch (`AGENTS.md` › Tasks › Branch names). The
*Branch name* check warns about other names; it never fails.

**Build on overlapping work, do not repeat it.** Before you write code, look at the open pull
requests that touch the same code (the claim bot lists them for your `Touches:` line). When one
already builds what you need, build on it: stack on its branch when both are in this repository, or
from a fork branch from its head and write `Depends on #N` on a line of its own in your description;
the *Depends on* check keeps your pull request from merging before #N. When two pull requests change
the same code for different reasons, say so in both; when they do the same thing two ways, a
maintainer chooses. An issue that cannot start before another one is marked *Blocked by* that
issue (its *Relationships* on GitHub), and cannot be claimed until it is closed. `AGENTS.md` › Tasks
gives the same rules step by step.

**Keep at most three pull requests open at a time**, unless you are a maintainer. Review time is
what limits how fast Suhina moves, and a stack of open pull requests goes stale as `main` changes.

`good first issue` marks work that needs no knowledge of the engine, `help wanted` work the
maintainers would like a hand with. An `epic` is a container for other issues: claim one of its
issues, not the epic.

**An agent claims and reports like a person does.** An issue is the only context an agent gets
apart from `AGENTS.md` and the documents those point at, so write it to stand on its own: goal,
acceptance criteria, where the code goes. Keep the plan in the issue and the result in the pull
request. An agent searches for an existing issue before it opens one, and for an open pull request
before it opens one; it continues work that is open rather than starting it again. An agent run
by someone with write access opens issues through the
[Open issue](.github/workflows/open-issue.yml) workflow, so they show the bot `suhina-bot[bot]` as
their author rather than that person, and posts the feedback it wrote onto a pull request's
conversation through the [Comment on pull request](.github/workflows/comment-pr.yml) workflow, so it
is the bot's comment and not that person's; it is a comment, not a GitHub review, and approves or
blocks nothing (`AGENTS.md` › Tasks has the commands). The rules in
[AI-assisted contributions](#ai-assisted-contributions) apply to what the agent wrote.

## Trying a pull request

A pull request can be tried without building it: GitHub Actions builds packages of it for
Windows, macOS and Linux ([`docs/releasing.md` › Test builds](docs/releasing.md#test-builds)) and
links the downloads from the pull request in a comment, updated with every new build. A maintainer's own pull requests are
built on every push. A contributor's pull request is built once a maintainer has read the diff and
added the `test-build` label; a later push takes the label off, and the new commits are built when
the label is added again.

## Merging many pull requests

For maintainers. Merge through the merge queue: on each green pull request press **Merge when
ready**, and the queue tests the queued pull requests together on top of `main`, in groups, and
merges them in order, so no one presses *Update branch* by hand. To try several pull requests in
one build before they merge, run Actions › Test build › Run workflow with their numbers
(`302 303 304`). A push that only merges `main` into a pull request is not built again; its last
test build stays linked. [`docs/releasing.md` › Merging many pull requests](docs/releasing.md#merging-many-pull-requests)
says how the queue is set up and what runs in it.

**The `ready to merge` label** does the waiting for you. A maintainer who has read a pull request
adds it, and [`ready-to-merge.yml`](.github/workflows/ready-to-merge.yml) approves the commit it was
added on and turns on *Merge when ready* for that commit: the queue takes the pull request once
`ci` is green and every review thread is resolved (CodeRabbit's too), and a red check stops it. A
contributor's pull request also needs `test-build`, so its build was tried first; Dependabot's
version updates do not, since the queue's build and tests check them. A push by anyone
but a maintainer takes the label off, so new commits are never merged unread; taking the label off
by hand stops the merge. Only a maintainer adds it: an agent never adds `ready to merge`, also not
when asked to finish a pull request.

## Clean-room

Suhina reimplements the *behaviour* of commercial DAWs, Ableton Live in particular, without
using any of their code or content. This rule outranks feature work: one contaminated change can
force a rewrite of everything built on it, so it is not something we clean up later.

**Allowed sources:**

- Using the software normally and observing what it does: playback, timing, quantisation, how
  clips launch, how parameters respond. Recording and measuring its audio and MIDI output.
- Project, clip and preset files **you authored yourself** (`.als`, `.alc`, `.adg`, `.adv`).
  These are gzip-compressed XML; study them by saving variants that differ in one setting and
  diffing them.
- Public documentation: user manuals, the officially published Live Object Model documentation,
  published specs (DAWproject, MIDI, CLAP, VST3), papers and textbooks.
- Open-source code under a licence compatible with GPL-3.0-or-later, ported with its notices
  kept. Code without a licence may not be copied, ported or translated; only its published
  description (a paper, a README) may be learnt from.

**Not allowed:**

- Disassembling, decompiling or debugging the binaries of Ableton Live or any other proprietary
  DAW or plug-in, by hand or with tools (Ghidra, IDA, Hopper, REA, ida-pro-mcp, …). If you have
  done this for a product, do not contribute code for that product's features.
  Running Suhina's own processes (the app, the CLI, the plug-in host) under a debugger is
  allowed, also to see that a crash happened in a third-party module; stepping into,
  disassembling or reading that module's code, or drawing conclusions about its internals from
  it, is not. A fix then follows public documentation (for example what Windows requires of a
  window thread), not the module.
- Reading or using leaked source code, or decompiled scripts of a proprietary program (for
  example the control-surface scripts that circulate decompiled).
- Notes, patches, write-ups or tools that come from someone else's debugging, disassembly or
  decompilation of a proprietary program, for example the notes and binary patches of projects
  that make a commercial DAW run on another operating system. An unmodified, licensed copy of
  the program may still be run on such a platform to observe it; the project's findings about
  the program's internals, and its patched copies, may not be read or used.
- Copying samples, presets, Packs, Core Library content, impulse responses, wavetables or demo
  projects from proprietary products, or committing them as test data.
- Copying the visual design of a proprietary product: icons, artwork, interface text, colours,
  typefaces or the look of its controls. Familiar workflow and the structure of the window (which
  region is where, what a view shows, how it reacts, its shortcuts and menu structure) are
  allowed; [`docs/design/brief.md`](docs/design/brief.md) lists what stays familiar and what
  must be Suhina's own. Screenshots of a proprietary product may be used as a reference for
  structure and behaviour, by people and by design tools, but the look drawn from them must follow
  the brief, not the screenshot; they are never traced or committed, and nothing in them (icons,
  artwork, text) is reused ([ADR 0009](docs/decisions/0009-ui-toolkit-and-look.md)).
- Using the Ableton or Live names, logos or trade dress for Suhina itself.

**How we work:** findings go into prose specs under `docs/spec/` (your own words, no code from
the product). Implementation is written from those specs. AI agents follow the same rules: an
agent that writes Suhina code must never be given decompiled or leaked material.

If you are unsure whether a source is allowed, ask in an issue before using it.

## Developer Certificate of Origin

Every commit in a pull request from outside the project ends with a sign-off line:

```text
Signed-off-by: Your Name <you@example.com>
```

`git commit -s` adds it. By signing off you certify the
[Developer Certificate of Origin 1.1](https://developercertificate.org/): that you wrote the
change or have the right to submit it under the project's licence. For Suhina this includes
the clean-room rules above: the change does not come from disassembled, decompiled or leaked
material, from notes based on such material, or from content of a proprietary product, and no
agent that wrote it was given such material. CI checks the sign-off on pull requests from forks.

## Reporting a rights problem

If you believe Suhina contains code, content or design that infringes your rights, or that a
contribution broke the clean-room rules, see [`IP.md`](IP.md). Such reports are handled before
any other work.

## Effects, instruments and presets

- **A new built-in effect or instrument** follows [ADR 0004](docs/decisions/0004-built-in-devices.md):
  propose it with the *New effect or instrument* issue form, then open the pull request with the
  [device template](https://github.com/suhina-daw/suhina-daw/compare/main...main?template=device.md)
  (choose your branch on that page).
- **Presets** for them go in `presets/<device id>/` under CC0 1.0
  ([ADR 0014](docs/decisions/0014-stock-presets.md)); open the pull request with the
  [preset template](https://github.com/suhina-daw/suhina-daw/compare/main...main?template=preset.md).
  Presets need no clean-room review; their names don't name commercial products, and banks or
  files from commercial products are not accepted.

## AI-assisted contributions

AI coding agents are welcome, under these rules:

- **Credit the model.** Keep the `Co-Authored-By:` trailer your tool adds (for example
  `Co-Authored-By: Claude <noreply@anthropic.com>`), so the history shows how each change was made.
- **No session links.** The repository is public: leave out links to your agent's own session
  (such as `https://claude.ai/code/session_...`) from commit messages, pull requests, issues and
  comments, even when the tool adds one by default.
- **Steps anyone can follow.** A change in what Suhina does, in the app, its sound, the files it
  reads and writes, the CLI or MCP, comes with a *How to check it* section in the pull request:
  numbered steps from opening Suhina with nothing loaded, every control named as it reads on
  screen and where it is, every command ready to copy, and what should happen after each step,
  written so that someone who has never used a music program can check it from the test build.
  [`docs/how-to-check.md`](docs/how-to-check.md) shows how to write them.
- **You are responsible for what you submit.** Before you open a pull request, run the tests and
  try every change by hand to see that it works as intended; you answer review questions, not
  the agent.
- **The clean-room rules apply to agents too.** Never give an agent decompiled, disassembled or
  leaked material, and never point it at proprietary binaries. An agent that has seen such
  material must not write Suhina code.
