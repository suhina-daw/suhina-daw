# Contributing

- **Language:** the product is Rust only. Scripts in `scripts/` are Python, and the FM synth's
  reference renderer is C++; neither is built into the program.
- **Licence:** Suhina is GPL-3.0-or-later (`LICENSE`, `NOTICE`), and contributions are
  licensed the same way. Dependencies and bundled libraries must be GPLv3-compatible (MIT,
  Apache-2.0, BSD, ISC, Zlib, MPL-2.0, LGPL, GPL-3.0); GPL-2.0-only and proprietary or
  non-commercial licences are not. CI checks every dependency's licence with `cargo-deny`
  (see [Dependencies](#dependencies)).
- **Never crash:** non-test code must not panic and must not use `unsafe` (the only exceptions
  are the plug-in hosting crates `ipc` and `plugin-host`, ADR 0006, and the MP3 encoder crate
  `mp3`, ADR 0029). Errors go through `Result`
  and `?`; input-derived indices and sizes are checked. See `AGENTS.md`.
- **Real-time safety:** code that runs on the audio thread never allocates, locks or does I/O. It
  blocks only to wait for a plug-in host's answer within the block's deadline (ADR 0006 › 4).
  Communicate with it through lock-free queues.
- **Tests:** required for every change. Format code needs round-trip and malformed-input tests;
  DSP and engine code is tested by rendering offline and comparing against golden files.
  A test module over 150 lines lives in a `<file>_tests.rs` beside its source, not inline;
  `python scripts/inline_tests.py move <file>` moves one, and CI checks it (`--check`).
- **Documentation, settings help, parity:** a pull request carries them as
  [`docs/workflow.md`](docs/workflow.md) › What a pull request carries says: the documents that
  describe what changed, the website help of a setting, the parity coverage it moves, and
  *How to check it* steps.
- **Style:** `cargo fmt`, and `cargo clippy --all-targets -- -D warnings`. Comments explain *why*.
- **Names and licence header:** packages are `suhina-<name>` (`suhina-device-<name>` for
  devices), and every source file starts with the licence header
  (`python scripts/license_header.py` adds it); [`docs/architecture.md`](docs/architecture.md) ›
  Crate names, folders and source files has both. CI checks them.
- **Commits:** small, focused, with a clear subject line, and signed off (see
  [Developer Certificate of Origin](#developer-certificate-of-origin)).
- **Issues, labels, branches:** [`docs/workflow.md`](docs/workflow.md) › Tasks: the issue types
  and labels, claiming, branch names, overlapping work.
- **CodeRabbit:** reviews every pull request that is not a draft against AGENTS.md
  (`.coderabbit.yaml`) and shows a **CodeRabbit** check beside CI. Its comments are suggestions, not requirements: answer or resolve
  them, and leave DSP and real-time behaviour to the tests and renders. The check is never red and
  never required. Comment `@coderabbitai review` to run it again.
- **Codex:** the maintainer requests a review with `@codex review` once the `ci` check has
  finished on the pull request's latest push; contributors ask the maintainer for one. The
  reviewer runs no tests or builds and reads the CI results of the push it reviews. Codex follows
  [AGENTS.md](AGENTS.md#code-review-rules) and
  [`docs/code-review.md`](docs/code-review.md). Actionable inline threads must be answered or
  resolved before the merge queue takes the pull request; optional suggestions stay in the
  summary. Hosted review reporting limits are described in the guide.
- **Asking for a review:** ask a maintainer (**Reviewers** on the pull request). Comments by
  `suhina-bot[bot]` are a maintainer's agent's feedback, posted for them; the bot takes no
  requests, and neither it nor CodeRabbit approves anything: a pull request goes in on a
  maintainer's `ready to merge` label.
  Codex and CodeRabbit post through their own apps.
- **Pull request description:** use the sections of `.github/pull_request_template.md`, all of them;
  one line is enough where nothing applies.

## Dependencies

Every crate in `Cargo.lock`, with all features on and for every platform, must have a licence
that [`deny.toml`](deny.toml) allows: the list in AGENTS.md rule 10, plus a few exceptions, each
named for one crate with the reason. Dependencies come from crates.io only, and a crate with an
open RustSec advisory fails unless `deny.toml` ignores it with a reason. The `ci` check runs this
on every pull request and in the merge queue (`.github/workflows/deny.yml`, called from `ci.yml`),
and `deny.toml` needs the owner's review like the workflows. Run it locally with the version pinned
in that workflow:

```sh
cargo install cargo-deny --version 0.20.2 --locked
cargo deny check
```

**Dependabot** opens the updates: a security update as soon as a RustSec or GitHub advisory
names a crate Suhina uses, and on Mondays the week's version updates
([`.github/dependabot.yml`](.github/dependabot.yml)): minor and patch updates of the Cargo
workspace grouped in one pull request, the major updates (for a 0.x crate, 0.14 to 0.15) in a
second one, and the GitHub Actions the workflows use. Every update rewrites `Cargo.lock`, so two
of them in the merge queue at once conflict; queue one, and after it merges ask the other to rebase
(`@dependabot rebase`). They go through CI and review like any other pull request; a major
update that changes an API needs the code changed in the same pull request. One that cannot be
fixed there goes in the `cargo-major` group's `exclude-patterns` in `.github/dependabot.yml`, so it
comes in a pull request of its own and the rest of the group can merge.

**GitHub Actions are pinned to a commit.** Every `uses:` line in `.github/workflows/` names the
action's full commit SHA, with the version it is in a comment beside it
(`actions/checkout@3d3c42e5… # v7.0.1`), never a tag or a branch: a tag such as `v7` or a branch
such as `stable` can be moved to other code by whoever gets into the action's repository, and
that code then runs with the job's secrets, or builds the nightly. Dependabot updates the SHA
and the comment together. A new action is added the same way: the SHA of the release's commit
from the action's own repository, not a fork (`git ls-remote https://github.com/<owner>/<repo>
refs/tags/<tag>^{}`). Workflows of this repository (`uses: ./.github/workflows/…`) are not
actions and need no SHA.

When it rejects a licence, prefer another crate. Add an exception only for a licence that is
GPLv3-compatible, for the crate that needs it, with a comment saying what the crate is for and
why the licence is fine. To update `cargo-deny`, change `CARGO_DENY_VERSION` and
`CARGO_DENY_SHA256` in the workflow together, and the version above.

**C libraries.** The program is Rust only ([AGENTS.md](AGENTS.md)), and one dependency bends it,
recorded here: the Ogg Vorbis encoder, `vorbis_rs`, is bindings to libvorbis — C source packaged
on crates.io that the build compiles. libvorbis is BSD-3-Clause, and the binding's licence is on
the allowlist. A C encoder was chosen for what this build needs from one (checked October 2026):
a bitstream Vorbis encodes strictly, and it is what every player and transcoder has round-tripped
for twenty years; the pure-Rust encoders tried against it then were young and single-maintained
where a mixdown's bytes go straight to the user's file. When a credible pure-Rust encoder has
proved itself, the dependency is revisited ([docs/spec/export.md](docs/spec/export.md) › Formats).

## Working on an issue

How to find free work, claim it, name the branch, build on overlapping work and open the pull
request is in [`docs/workflow.md`](docs/workflow.md) › Tasks, the one copy for people and agents.
In short: search before opening an issue; claim a feature or a piece of an epic with
`/claim Plan: … Touches: …` and open a draft pull request within 48 hours; a bug or a small change
needs no issue and no claim; one pull request per issue, at most three open at a time.

## Trying a pull request

A pull request can be tried without building it: GitHub Actions builds packages of it for
Windows, macOS and Linux ([`docs/releasing.md` › Test builds](docs/releasing.md#test-builds)) and
links the downloads from the pull request in a comment, updated with every new build. A maintainer's own pull requests are
built on every push. A contributor's pull request is built once a maintainer has read the diff and
added the `test-build` label, or a trusted contributor requests it with `/test-build`; a later
push takes the label off, and the new commits need a new request. `/test-build` first reuses a
running build of that commit or a successful build whose requested packages are still available.
A verified main-only update can reuse the previous build. It starts another build only when no
suitable build is available.

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
`ci` is green and every review thread is resolved (CodeRabbit's too), and a red check stops it.
Adding the label is the maintainer's approval of that exact commit, regardless of the author, and
it needs a finished test build of that commit: a successful `/test-build` run whose packages are
still available, or one of an earlier head the branch has only merged `main` into since. Without
one the label comes off with a comment asking for `/test-build`; an existing build is reused, so
asking again starts no second packaging run. A pull request that changes nothing in the binaries (only documentation, scripts or workflows) needs no build. Code-owner approval is still required for
protected paths. A push by anyone
but a maintainer takes the label off, so new commits are never merged unread; taking the label off
by hand stops the merge. Only a maintainer adds it: an agent never adds `ready to merge`, also not
when asked to finish a pull request.

**Workflows and scripts need the owner's review.** To protect code that runs with the
repository's secrets, [`.github/CODEOWNERS`](.github/CODEOWNERS) names the repository owner for
`.github/` and `scripts/`, except `scripts/parity/`. The main ruleset requires a code owner's
approval for the protected paths: `ready to merge` alone does not merge such a change, also
not one from another maintainer. Parity scripts run without environment secrets; the generated
documentation is published in a separate job that does not execute them. Changes confined to
`scripts/parity/` therefore follow the usual review and `ready to merge` process. Keep parity
code out of jobs that use environment secrets.

**The updater needs the owner's review too.** Suhina installs and runs what its updater downloads,
so the same approval applies to `crates/update/` and the updater's two files in `crates/ui`, and a
check fails a pull request that changes where the updater downloads from
([`docs/spec/updates.md` › Where builds come from](docs/spec/updates.md#where-builds-come-from)).

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
- **One named exception for the VST2 interface: VeSTige.** The VeSTige header as Ardour
  distributes it (`libs/ardour/ardour/vestige/vestige.h`, GPL-2.0-or-later, used under
  GPL-3.0-or-later), at the commit [ADR 0022](docs/decisions/0022-vst2-interface-description.md)
  pins, may be read and its interface definitions ported to Rust, with its copyright and licence
  notice kept. It is a third-party reverse-engineering result, which the next list otherwise
  forbids; the project accepts this one source on the record ADR 0022 keeps. The exception covers
  that file only: not Steinberg's VST2 SDK, not leaked code, not FST, not `vst2sdk`, not any other
  header or write-up, and not material from any other reverse engineering. VST2-enabled builds
  may also be distributed, under ADR 0022 › 5 as amended on 2026-10-31.

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
  the program's internals, and its patched copies, may not be read or used. The one exception is
  the VeSTige header above.
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
agent that wrote it was given such material. CI checks the sign-off on pull requests from forks,
on the commits that are not on a branch of this repository already (a pull request built on
another one's branch here does not answer for that branch's commits). GitHub's temporary pull
request merge refs do not count as repository branches: the fork's own commits still need sign-off.

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
