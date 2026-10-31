# AGENTS.md: guide for AI agents and contributors

Suhina is an open-source, native, clip-launching DAW written in **Rust only**, comparable in
workflow to Ableton Live. The product name is always written **Suhina** in user-facing text;
machine names stay lowercase (`suhina-*` crates, binaries, file names). Read this file first,
then [`CONTRIBUTING.md`](CONTRIBUTING.md) and [`docs/architecture.md`](docs/architecture.md)
(layers, data flow, threads, and where new code goes).

**Find documentation by asking, before reading code:** `python scripts/docs.py find WORDS` lists
the documents about something, best first, and `python scripts/docs.py covers PATH` the documents
that describe a file. [`docs/INDEX.md`](docs/INDEX.md) lists every document in a line. Read only
the documents that match your task.

## 1. Golden rules

1. **Clean-room (outranks everything).** Follow [`CONTRIBUTING.md`](CONTRIBUTING.md) › Clean-room.
   Never disassemble or decompile proprietary DAW or plug-in binaries, never use leaked code, never
   commit content from commercial products. Knowledge comes from observation, self-authored files
   and public documentation, and is written down in `docs/spec/` before it is implemented. The one
   third-party reverse-engineering result allowed is the VeSTige header for the VST2 interface,
   at the commit [ADR 0022](docs/decisions/0022-vst2-interface-description.md) pins; the
   exception and its limits are in CONTRIBUTING › Clean-room.
2. **Never crash.** Non-test code never panics: no `unwrap()`, `expect()`, `panic!`,
   `unreachable!`, `todo!` or `unimplemented!`. Return the crate's error type. Input-derived
   numbers are hostile: use `get()` and `checked_*`, cap allocations sized by input. No `unsafe`
   (`unsafe_code = "forbid"` workspace-wide), except in three crates: the two plug-in hosting
   needs, `crates/ipc` and `crates/plugin-host` ([ADR 0006](docs/decisions/0006-plug-in-hosting.md)),
   and the MP3 encoder, `crates/mp3` ([ADR 0029](docs/decisions/0029-mp3-encoder.md)):
   there every `unsafe` block is small, tested and explained in a `// SAFETY:` comment, and the
   crate offers a safe API.
3. **Real-time safety.** Code reachable from the audio callback never allocates, frees, locks,
   logs or does I/O. It blocks only to wait for a plug-in host's answer within the block's
   deadline ([ADR 0006](docs/decisions/0006-plug-in-hosting.md) › 4). Hand data to it through
   lock-free queues (`rtrb`) and swap whole graphs atomically.
4. **One render path.** Offline rendering and real-time playback run the same engine code, so
   offline renders are the test oracle for the engine.
5. **Plug-ins run sandboxed, out of process.** Third-party plug-ins (CLAP, VST3, later bridged
   Windows plug-ins) never run inside the Suhina process. A crashing or hanging plug-in must
   not take down the engine or lose the user's project: the host reports it, keeps the project
   intact, and can restart it. Design the plug-in host boundary as IPC from the first line of
   plug-in code (shared-memory audio buffers, per-block sync with a deadline, plug-in state and
   parameters over messages); never add an in-process shortcut that the rest of the engine
   comes to depend on. The design is [ADR 0006](docs/decisions/0006-plug-in-hosting.md).
6. **Everything is a command.** User-visible behaviour is a command in the engine with an id,
   params and tests. The UI, CLI and MCP all dispatch the same commands.
7. **Tests are the gate.** Every change comes with tests: round-trip and malformed-input tests for
   formats, golden-file renders for DSP and engine behaviour.
8. **Build every device's panel on the SDK, and its DSP from shared blocks.** Every built-in MIDI
   effect, instrument and audio effect is native code, never an `.sd` graph, and gets its panel
   from the `device-sdk` UI system: the generated parameter list, or a layout (the `.sd` UI part
   only) loaded by its id, whose parameters come from the device's `DeviceInfo`
   ([ADR 0021](docs/decisions/0021-device-graph-files.md) › Amendment: device panels). Its DSP is
   built from blocks in `crates/dsp` that the SDK's nodes share: when a device needs DSP the shared
   blocks cannot provide, add it to `crates/dsp`, with an SDK node over it when `.sd` authors need
   it too, so each block exists once
   ([issue #406](https://github.com/suhina-daw/suhina-daw/issues/406)).
9. **Other DAWs are named only where a format is handled.** Product names such as Ableton,
   Live or FL Studio may appear in format crates (`crates/als`), their tests, `docs/spec/` and
   `docs/decisions/`, to say which file format the code reads, and in the design brief
   (`docs/design/brief.md`), to say what Suhina's look must not resemble. They also appear in
   [`docs/research/parity-live-12.md`](docs/research/parity-live-12.md) and the scripts under
   `scripts/parity/`, which measure what Suhina has against another DAW's public manual: status
   analysis of our own project, never content from that manual (its PDF, text, tables, screenshots
   and parameter lists are never committed; the scripts read a copy the user owns and write to
   `target/parity/`). ROADMAP and README
   describe Suhina's own devices and features under their own names; which device of another
   product imports into which is a spec ([`docs/spec/device-import.md`](docs/spec/device-import.md)).
   Product terms and set phrases of other DAWs are not used as generic words either: "drum sampler",
   not "drum rack"; "built-in effects and instruments", not "built-in devices". "Device" on its own
   is a generic word and stays (the `device.*` commands, `devices/`). The model, engine, CLI core and
   UI describe concepts neutrally ("a clip that belongs to one track", "a shared pattern").
   User-facing text may name a file format when it imports or exports it ("a Live Set to
   import"), never as Suhina's own branding.
10. **GPL-compatible dependencies only.** Suhina is GPL-3.0-or-later. Dependencies must be
   compatible with GPLv3: MIT, Apache-2.0, BSD, ISC, Zlib, MPL-2.0, LGPL, GPL-3.0. Not GPL-2.0-only,
   not proprietary or "non-commercial" licences. CI checks it with `cargo-deny`
   ([`deny.toml`](deny.toml), `.github/workflows/deny.yml`); run `cargo deny check` before adding
   or updating a dependency ([CONTRIBUTING.md › Dependencies](CONTRIBUTING.md#dependencies)).
11. **No links to private agent sessions.** The repository is public. Never put a link to an
    agent's own session (such as `https://claude.ai/code/session_...`) in a commit message, a pull
    request description, an issue or a comment, even when the tool's default attribution adds
    one: leave that line out. The `Co-Authored-By:` trailer stays.

## 2. Workspace map

```text
crates/
  model    L0 DAW-neutral project as pure data (tracks, channels, clip contents, clips, launcher)
  style    L0 native style sheets, scoped selectors, typed values, cascade, Taffy layout and source edits; no GUI or I/O
  stretch  L0 time-stretching and pitch-shifting DSP: a pure-Rust port of Signalsmith Stretch (MIT)
  engine   L1 offline/real-time rendering of notes and audio clips, real-time time-stretching (ADR 0003)
           and the clip launcher; no file I/O
  als      L1 Live Set (.als) reader into model; later one crate per format (dawproject)
  flp      L1 FL Studio project (.flp) container and event stream; no model mapping yet (epic #271)
  project-file L1 Suhina's own project file (.suhina, versioned JSON): reads and writes the model (ADR 0010)
  media    L2 finds and decodes a project's audio files (symphonia) into the engine's AudioPool;
           reads MIDI files, file metadata (tempo, key) and detects tempo; writes the files an
           export renders (WAV, FLAC, Ogg Vorbis, MP3)
  mp3      L2 MP3 encoding through the vendored LAME 3.100 encoder (ADR 0029); no file I/O
  library  L2 the browser's library: added folders, an index kept on disk and watched, search, pins,
           recent items, colour tags (ADR 0012)
  soundmap L2 the sample map: features of samples, layouts by how they sound (t-SNE), grids, kinds, the feature cache
  update   L2 updates: whether a newer nightly or test build is out (GitHub, through the system's
           curl), downloading it, checking its SHA-256 and putting it in place of the running build
  theme-package L1 theme packages (.theme.suhina): a theme's files with a SHA-256 manifest, packed, read
           and validated; used by the app's export and import and suhina-cli theme
  separation L2 stem separation: chunking, weighted overlap-add and resampling around an inference backend
             trait, and a deterministic test backend (a band splitter); no model or commands yet (#146)
  session  L2 open projects (tabs, ADR 0007), undo history, the command registry (ADR 0002), the
           attached player, each project's decoded audio and previews (ADR 0011), backups
           (autosave and the offer after a crash, ADR 0010)
  automation L3 MCP server (rmcp) over a session, file roots, WAV rendering (docs/mcp.md); the app's
           endpoints: one instance per build, agents attached to the open projects (ADR 0028)
  output   L3 the audio device: lists hosts and devices (cpal; ASIO with the `asio` feature), opens the one
           chosen (WASAPI exclusive through `wasapi`), plays an engine in blocks of the chosen buffer;
           opens the input device chosen beside it for the tracks' audio inputs (ADR 0017)
  midi     L3 MIDI controllers: lists the input ports and opens them (midir); their notes play the armed
           tracks' instruments (ADR 0017)
  ui       L3 the main window, the browser and the views (egui, ADR 0009); every change is a command
  dsp      L0 signal-processing blocks for built-in effects and instruments: smoothing, attack/release followers, filters, crossovers, LFOs and tempo sync, shapers, oversampling, delay lines (ADR 0004)
  device-api L0 the Device trait, parameter descriptions, contract tests every device runs
  device-sdk L0 builds and validates data-only `.sd` effect graphs, compiles DSP primitives and delayed feedback for real-time execution; shared node contracts and embedded custom UI canvases/images for authors and agents; the layouts of
           built-in devices (`builtin`: the UI part only, no graph); native sampler tempo synchronisation uses stretch
  device-registry L1 every built-in effect, instrument or MIDI effect by its stable id: makes and lists them (ADR 0004),
           and gives its panel: parameters from DeviceInfo and its layout, or the generated list (ADR 0021)
  ipc      L0 audio blocks to a plug-in host process: shared memory, block sync with a deadline (ADR 0006; may use unsafe)
  realtime L0 real-time priority for audio threads: MMCSS Pro Audio on Windows, SCHED_FIFO or rtkit on Linux
  plugins  L1 the engine's side of plug-in hosting: start a host process, control protocol, health, state, restart; scanning; offline runs
  test-support  dev-dependency only: ProjectBuilder, write_wav, test signals, golden files (SUHINA_BLESS)
  plugin-host  the suhina-plugin-host executable plug-ins run in (ADR 0006; may use unsafe): CLAP, VST3, test plug-ins;
               behind the `vst2` feature (ADR 0022): the ported VeSTige definitions, the mock plug-in,
               the probe and `--vst2-probe`
devices/
  sampler  single-source sample and granular instrument; SDK runtime and theme-aware panel
  utility  gain, balance, width, mono, bass mono, phase, channels, DC filter; depends on dsp, device-api
  auto-pan LFO panning and tremolo, free in Hz or synced to the song position
  dj-eq    three bands with kills, Linkwitz-Riley crossovers at 24 or 48 dB/octave
  eq       eight bands (cuts, shelves, bells, notch) on state-variable filters, two curves: Stereo, L/R, M/S
  vinyl-distortion  tracing (even), pinch (odd, out of phase) and crackle, oversampled
  overdrive  pedal-style: band, drive, tone, dynamics, dry/wet; oversampled
  compressor stereo-linked, soft knee, side-chain high-pass, attack/release, dry/wet
  fm-synth six operators, 32 algorithms, DX7 voice files; the engine is a port of MSFA (Apache-2.0)
  limiter  brickwall peak limiter with 1.5 ms lookahead; never over the ceiling
  nam-amp-modeler  a `.nam` model through `nam-rs` (MIT) inference, with trims, EQ, HQ, a trim delay; the NAM folders and panels
  nam-cab-loader  a `.wav` impulse response through the `dsp` convolver, with a phase invert, high pass, tone cuts, delay, volume
  delay    stereo delay: synced or free times, band-pass in the loop, LFO, Repitch/Fade/Jump, ping-pong, freeze
  reverb   algorithmic reverb: input band, early reflections, damped feedback delay network, chorus, freeze
  pitch    MIDI effect: transposes notes by semitones or scale degrees, keeps them in a key window (Block, Fold, Limit)
apps/
  suhina-cli   headless CLI: render, play, mcp, commands, audio, plugins, plugin, fm-synth
  suhina       the desktop app (eframe): opens a project and shows it in the ui;
               theme file operations and live reload run on one background worker
docs/
  architecture.md  how the crates, threads and commands fit together
  decisions/  architecture decision records; read 0001 before changing the model, 0004 before a device
  spec/       clean-room specs written from observation (formats, behaviour, UI views)
  design/     UI design brief, component measurements and reference mockups (ADR 0009)
  research/   status analysis of our own project: parity against another DAW's public manual
              (ADR 0016; an ordinary document whose marked parts scripts/parity/ keeps current)
              and its two figures
  INDEX.md    generated list of documents, one line each (scripts/docs.py find and covers query it)
  commands.md generated list of every command (suhina-cli commands --markdown)
assets/brand/ the logo as SVG: `suhina-logo.svg` (the name with the wave, drawn on the splash by
              `crates/ui/src/kit/logo.rs`) and `suhina-mark.svg` (the wave mark alone, for icons);
              `suhina-icon.png`, the app's icon (`Suhina.app` on macOS, by `scripts/macos_app.sh`)
third_party/  vendored source: the ASIO SDK's host part (GPL-3.0, built with `asio`) and LAME 3.100's
              encoder (LGPL, always built for MP3 export; Suhina's own fft.c, ADR 0029)
scripts/   research helpers (e.g. extracting and diffing .als files), the parity measurement
           (parity/, reading a manual the user owns; its output is never committed), docs.py
           (the docs index), release_notes.py and discord_post.py (release notes, Discord posts),
           agent_post.py (an agent's issue or PR comment, as the bot or as the person, § 3)
```

A crate may depend only on crates in its own or a lower layer, never in a cycle. Nothing below
the UI layer may depend on a GUI toolkit.

### Crate names

Every package is named `suhina-*`; the directory has no prefix. The project had another name
until October 2026 (README); nothing new may use it, in names, text or code.

| Kind | Directory | Package (`Cargo.toml`) | In Rust code |
|---|---|---|---|
| Library crate | `crates/<name>` | `suhina-<name>` | `suhina_<name>` |
| Built-in effect or instrument | `devices/<name>` | `suhina-device-<name>` | `suhina_device_<name>` |
| App (binary) | `apps/<name>` | `suhina` or `suhina-<name>` | |

A new crate inherits the workspace settings (`version.workspace = true`, `edition`, `license`,
`rust-version`, `[lints] workspace = true`), is added to `[workspace.dependencies]` in the root
`Cargo.toml` (`suhina-<name> = { path = "crates/<name>" }`), and other crates depend on it with
`suhina-<name>.workspace = true`. Add it to the map above and to
[`docs/architecture.md`](docs/architecture.md). Environment variables start with `SUHINA_`.
CI runs `python scripts/rename_suhina.py --check`, which fails when the old name is left anywhere
in the tree (links to the repository and lines marked `rename_suhina: keep` excepted). A branch started
before the rename catches up as the script's docstring describes.

### Licence header

Every source file (`.rs`, `.py`, `.sh`, `.ps1`) starts with the licence header, after a `#!`
line when there is one, then an empty line:

```text
// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later
```

(`#` instead of `//` in scripts). Run `python scripts/license_header.py` after adding a file; it
adds the header where it is missing and nothing else, and CI runs it with `--check`. Ported code
keeps its original notice and gets only the SPDX line of its own licence; the script's docstring
lists those files and what else is left out (`third_party/`, fixtures, generated files).

## 3. Tasks

Work is tracked in the GitHub project **Suhina**
(<https://github.com/orgs/suhina-daw/projects/1>), and nowhere else. New work, bugs found while
testing and follow-ups a change leaves open go there as items, written in English, with a
Status, a Priority (P0 to P2) and a Phase (the ROADMAP milestone). A feature or a piece of an epic
gets an issue, so the pull request can say `Closes #N`; a bug fixed at once
or a small change goes straight to a pull request that says what it fixes. An issue gets a type label and
area labels (`CONTRIBUTING.md` › Issues lists them); a larger piece split into issues is an
`epic`, and repository, build and release work is a `chore`. Items imported from the earlier
board keep their old key in the title (`(LOOP-27)`); a pull request for one names that key.
Before starting a feature or a piece of an epic, claim it the way [`CONTRIBUTING.md`](CONTRIBUTING.md) › *Working on an issue*
describes, for a person and for an agent alike: no assignee and no open pull request for it,
`Backlog` or `Ready` on the board (`Ready` for a feature, unless you are a trusted contributor), and a `/claim` comment with the plan.
A bug or a small change needs no issue and no claim: the [Overlap](.github/workflows/overlap.yml) notice on the
pull request lists other open work on the same files.

**What belongs in one pull request.** A fix found while working that is small and in the same code
goes in the same pull request, under *Also fixed* in its description. Make it a pull request of its
own, or an issue, when it is larger, in other code, or in code another open pull request or claim
is changing.

Several agents often run at once, so an agent never starts work that exists already:

- **Before opening an issue**, search the open issues for the same problem; when one exists,
  comment there instead.
- **Check access before posting as the bot.** The two bot workflows below can be started only by
  an account with write access. Before either, check the person's account once:
  `gh api repos/suhina-daw/suhina-daw --jq .permissions.push` (or the GitHub MCP's repository
  permissions). `true`: use the workflow. `false`: post from the person's own account, as the next
  bullets say, and tell the user it went out under their account and why. A dispatch answered
  `HTTP 403: Must have admin rights to Repository` means the same as `false` (GitHub's message
  names the wrong right): fall back, never retry it or try another token.
  [`scripts/agent_post.py`](scripts/agent_post.py) makes the check and takes either path, from the
  same `request.json` the workflows take, and prints `OPENED_ISSUE=#N <url>` or
  `COMMENTED_PR=#N <url>` both ways:

  ```sh
  python scripts/agent_post.py issue request.json     # or: comment request.json
  ```

  If the workflow posted but its successful completion could not be confirmed, the helper exits
  with an error that includes the created issue or comment's marker and the run id. Inspect that
  run and complete any missing follow-up; do not post again.

- **Open an issue as the bot.** An agent with write access to this repository opens issues
  through the [Open issue](.github/workflows/open-issue.yml) workflow, not as the person it runs
  for, so the issue's author is `suhina-bot[bot]`. Start it with the GitHub MCP's
  `actions_run_trigger` or with `gh`, then read the issue number from the log of that run
  (`OPENED_ISSUE=#N <url>`).
  Use the run the start returns, never the latest run, which may be another agent's. A run that
  failed after it opened the issue still prints the line; re-running that run does not open the
  issue again, but starting a new one does. The inputs go in as JSON on stdin (`--json`), not as
  `-f` arguments, so a long body is not cut off by the command-line limit (about 32 000 characters
  on Windows).

  ```sh
  python -c "import json; print(json.dumps({'title': 'Piano roll: snap notes to the grid', \
    'body': open('issue.md', encoding='utf-8').read(), 'type': 'enhancement', 'labels': 'ui', \
    'parent': '123'}))" > request.json
  run_url="$(gh workflow run open-issue.yml --json < request.json)"
  run_id="${run_url##*/}"
  gh run watch "$run_id" --exit-status; gh run view "$run_id" --log | grep OPENED_ISSUE
  ```

  `type` is one of `enhancement`, `bug`, `documentation`, `chore`, `epic` and sets the type label
  and the issue type; `labels` are the area and platform labels; `parent` is an open epic. Set the
  board's Priority and Phase afterwards. A pull request itself — its branch, its description and
  its merge — stays under the person's account.

  Without write access, open it with `gh issue create --body-file` under the person's account,
  identifying it as AI-assisted. Before opening it, check that a supplied parent is an open issue
  labelled `epic`, as the workflow does; if the parent cannot be checked, stop before posting.
  With `triage` access (`.permissions.triage`) give it the type label and the area labels, set its
  issue type and add it under the parent epic, as the workflow would. Without triage, leave the
  labels out: the body ends with the labels a maintainer should add, and `Part of #N` names the
  parent, since the account cannot link a sub-issue.
- **Use the caller's existing access; do not request elevated access to post.** The two bot
  workflows require repository write access, not admin access. Their posting rules apply only
  when the caller already has the required access and the configured connector permits the action.
  Without write access, post ordinary issues and PR comments through the user's authenticated
  account, within the user's authorization; identify agent-written feedback as AI-assisted.
  Trusted contributors can instead use the scoped comment commands in
  [Trusted bot requests](docs/claim-bot.md#trusted-bot-requests). Post those requests as the user;
  the bot records their authenticated identity. Do not ask for admin/write access, broader token
  scopes, App keys or secrets to satisfy the bot posting convention. A denied connector action
  is not permission to bypass its restriction with another tool: explain that limit and leave
  the prepared text for the user when no authorized posting route is available.
- **Comment on a pull request as the bot when the caller has write access and the comment is an agent's feedback.** A review an
  agent wrote, the changes it asks for or a summary of what it found goes onto the pull request's
  conversation through the [Comment on pull request](.github/workflows/comment-pr.yml) workflow, so
  it is written by `suhina-bot[bot]` and the person who ran the session is not its author. It is a
  comment, not a GitHub review: it neither approves nor requests changes, and no merge rule sees
  it; approving stays the maintainer's `ready to merge` label. Start it the same way,
  with the inputs as JSON on stdin, and read `COMMENTED_PR=#N <url>` from the log of the run the
  start returns. Re-running a failed run does not comment twice. Without write access, post it
  with `gh pr comment <N> --body-file …` under the person's account, ending with a line that
  says an agent wrote it.

  ```sh
  python -c "import json; print(json.dumps({'pr': '600', \
    'body': open('review.md', encoding='utf-8').read()}))" > request.json
  run_url="$(gh workflow run comment-pr.yml --json < request.json)"
  run_id="${run_url##*/}"
  gh run watch "$run_id" --exit-status; gh run view "$run_id" --log | grep COMMENTED_PR
  ```

  This posting rule applies to agents a person runs. Installed reviewers such as Codex and
  CodeRabbit post through their own apps and follow [Code Review Rules](#code-review-rules).

- **Before writing code for a feature or a piece of an epic**, comment `/claim` with the plan and a `Touches:` line naming the paths
  you will change (`Touches: crates/ui/src/detail, crates/session`), and read the bot's answer.
  When it says the work is taken, that it is blocked by another issue, or that it is already yours
  and names a pull request, do not start a second one: continue in that pull request, take the
  blocking issue instead, or stop and tell the user.
- **Before opening a pull request**, check the issue's linked pull requests; when one is open,
  push to it rather than opening another.

**Branch names.** A branch is `<kind>/<short-description>`: `feature/` for something a user can
do, `fix/` for a bug, `docs/` for documentation, `chore/` for repository, build and release work,
and two to five lowercase words joined by hyphens (`feature/piano-roll-workflow`,
`fix/short-panes`). Before the first push, create that branch from the starting point and open the
pull request from it, even if the session was started on a `claude/...` branch; never open a pull
request from a `claude/...` branch. A session started for you can be given its branch up front
(`outcome_branch`). The [Branch name](.github/workflows/branch-name.yml) check warns about a name
that does not follow the rule; it never fails.

Continuing open work is always allowed, in a new session too. A second, competing pull request for
the same issue is opened only when the user asks for one, and its description says
`Alternative to #N`.

**Overlapping work across issues.** Two issues often need the same code: a helper, a command, a
model field. The claim bot lists the open pull requests and other claims that touch the paths in
your `Touches:` line, and the Overlap notice lists them again on the pull request as its files
change; without a claim, list them yourself before writing code
(`gh pr list --state open --json number,title,files`), and read every one that touches the crates,
views or commands you will change. Then:

- **It already builds what you need:** do not write it again; build on it. On a branch of this
  repository, stack on its branch (`gh stack`). From a fork, which cannot be stacked, branch from
  its head and write `Depends on #N` on a line of its own in your description; the
  [Depends on](.github/workflows/depends-on.yml) check keeps your pull request out of the merge
  queue until #N has merged, so keep it a draft until then.
- **It changes the same code for another purpose:** comment in both pull requests what overlaps and
  which should merge first, and leave the order to a maintainer when it is not obvious.
- **It does the same thing another way:** do not merge the two yourself, edit the other pull
  request, or open a third; comment in both and leave the choice to a maintainer.

When an issue cannot start before another one is done, mark it on GitHub (the issue's
*Relationships › Blocked by*), not only in prose: the claim bot refuses claims on it until the
blocking issue is closed.

**Merging is a maintainer's decision.** An agent never adds the `ready to merge` label, which
approves a pull request and sends it to the merge queue
([CONTRIBUTING.md › Merging many pull requests](CONTRIBUTING.md#merging-many-pull-requests)), and
never merges or queues a pull request itself unless the user asks for that one. It gets a pull
request green and mergeable and tells the user it is ready; the maintainer adds the label.

## 4. Before you finish a task

When the task changed code (a review changes none and runs nothing: [Code Review Rules](#code-review-rules)):

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check   # when Cargo.lock changed (CONTRIBUTING.md › Dependencies)
```

Keep the tree building at every step.

**Build and verify built-in devices.** Built-in devices are compiled into the app and are not
`.sd` files; nothing is bundled beside it. After changing a built-in device, its parameters or its
layout, run `cargo test -p suhina-device-registry` (it fails on a layout that names a parameter its
device lacks or that does not load), rebuild the desktop app, and check the device's panel in it:
its layout, or the generated parameter list. Report the app's path. Do this automatically without
waiting for a separate rebuild request.


**A new or moved setting follows [`docs/spec/ui-settings.md`](docs/spec/ui-settings.md) › Adding a setting**
(page by task, groups of at most seven outside Advanced, no status or commands among settings, one
sentence of description, one control per kind of choice, the stored key unchanged).

**A change in what Suhina does comes with steps to check it.** When a pull request changes
anything a user sees, hears or does, in the app or outside it (a view, a menu, a dialog, a
shortcut, a setting, drag and drop, playback and sound, a file it opens, imports or writes, a
`suhina-cli` command, an MCP tool), its description has a **How to check it** section: numbered
steps that someone who has never used Suhina or any other music program can follow from a test
build ([CONTRIBUTING.md › Trying a pull request](CONTRIBUTING.md#trying-a-pull-request)) and
finish in a few minutes. [`docs/how-to-check.md`](docs/how-to-check.md) is the guide to writing
them, for any agent or person. The steps

- start from opening Suhina with nothing loaded, and say how to get everything the check needs:
  which menu creates the project, track or clip, or which file to use and where it comes from (a
  file in the repository, one the steps make, or a short way to make one); never "open a project
  with some clips";
- name every control as it reads on screen and say where it is ("the **+** button at the bottom
  of the track list", "right-click the empty slot under the track name"), with the shortcut as
  well as the click; give every command whole, ready to copy, run from the unzipped test build's
  folder; explain a music term the first time it is used ("a slot, one of the boxes in a track's
  column that holds a clip");
- say after each step what should happen, seen or heard ("the slot turns green and the clip
  plays from its start"), and what a failure looks like when it is not obvious;
- end with the edge cases the change handles (undo, an empty project, a second click), each as
  its own step;
- say the platform when only one is affected.

Write them by doing them in the build, not from the code. A change with nothing a user can notice
(a refactor, tests, CI, documentation) says so in one line instead; an engine change a user can
hear or measure is not one of them.

**A change in what Suhina does updates the parity measurement.** Always, in the same pull request:
every change that adds, changes or removes something a user can do (the same changes that need a
*How to check it* section) updates the coverage of the chapters it moves in
[`scripts/parity/score.py`](scripts/parity/score.py), and
[`scripts/parity/devices.json`](scripts/parity/devices.json) for a built-in effect or instrument,
and nothing else: the numbers in [`docs/research/parity-live-12.md`](docs/research/parity-live-12.md)
(its marked parts, the command count among them) are regenerated on `main` after the merge, so a
pull request leaves them alone, even when it adds a command. A pull request that moves no
coverage writes `Parity: no change` and the reason in its description ("Parity: no change, a
refactor"). When a pull request changes `crates/`, `devices/` or `apps/` and does neither, the
[Parity notice](.github/workflows/parity-notice.yml) check warns; it does not fail.

**Settings include their website help.** Whenever you add, remove, move, rename or change a
setting, complete its guide update automatically in the same pull request. Read
[`docs/spec/settings-guide-export.md`](docs/spec/settings-guide-export.md), the control in
`crates/ui/src/settings/settings_rows.rs` and its implementation. Update
`docs/guide-data/settings-help.json`: description, summary when present, every choice's name
and explanation, when to change it, implementation references and any work-in-progress status.
Explain grouped controls, defaults, dependencies and when a control has no effect. Migrate IDs
for moved settings and remove help for deleted settings. Review behavior changes even when
the registry metadata and fingerprint stay unchanged. Copy the current source fingerprint
only after reviewing the text; never refresh fingerprints merely to make CI pass.

Run `python scripts/settings_guide.py` and commit `docs/guide-data/settings.json` with the
authored help. This export is intentionally committed, unlike the generated files below.
Then run `python scripts/settings_guide.py --check` and
`python -m unittest discover -s scripts -p 'test_settings_guide.py'`. Check every changed
setting, not just the first error: the exporter stops at the first missing or stale entry.
Do not defer the guide to another repository or a follow-up, and do not wait for a separate
request to document a setting.

**Documentation is part of the change.** A pull request that changes how something works updates,
in the same pull request, every document that describes it, so the docs never describe code
that no longer exists. [`docs/README.md`](docs/README.md) says what each kind of document holds
(an ADR, a spec, a system document), how an ADR is amended and what the status fields mean;
`python scripts/docs.py check` enforces it.

| If you change | Update |
|---|---|
| A crate's role, a dependency between crates, threads, how commands or data flow | [`docs/architecture.md`](docs/architecture.md), and the crate map above |
| A decision recorded in an ADR | that ADR (status, consequences), or a new ADR that supersedes it |
| What is read from a format, or behaviour described in a spec | the spec in `docs/spec/` |
| What a `.suhina` or `.sd` file holds | its `VERSION`, a file for the new version and a row in the spec's *Versions* ([ADR 0010](docs/decisions/0010-project-files.md) › Amendment: format versions) |
| MCP tools, or how agents use commands | [`docs/mcp.md`](docs/mcp.md) (the command list, [`docs/commands.md`](docs/commands.md), is generated) |
| What works for users | [`ROADMAP.md`](ROADMAP.md) › What works now, and [`README.md`](README.md) if it shows it |
| What the parity measurement says (a milestone that lands, a feature that appears or is dropped) | the coverage of the chapters it changes in `scripts/parity/score.py`; the marked parts of [`docs/research/parity-live-12.md`](docs/research/parity-live-12.md) are regenerated on `main`, and its prose is edited in that file like any other document |
| A setting in the Settings window (`settings_rows.rs`) | the table and rules in [`docs/spec/ui-settings.md`](docs/spec/ui-settings.md), and `docs/guide-data/settings-help.json` |
| A rule in this file | this file, and [`CONTRIBUTING.md`](CONTRIBUTING.md) if it repeats it |

A limit or a planned part that a change resolves is removed from the docs in the same change.

**Write documents so parallel pull requests do not collide.** In a list that several changes add
to (ROADMAP's *What works now*, the crate map), add a bullet or a row of your own rather than
extending someone else's sentence. Leave generated files out of the change:
[`docs/INDEX.md`](docs/INDEX.md) (from the documents' front matter) and
[`docs/commands.md`](docs/commands.md) (from the command registry) are regenerated on `main` after
every merge by [`.github/workflows/docs-generated.yml`](.github/workflows/docs-generated.yml), and
a pull request that edits one of them fails CI rather than losing the edit silently. The same
workflow writes three more things a pull request leaves alone, so that two pull requests never
conflict on them:

- the parts of [`docs/research/parity-live-12.md`](docs/research/parity-live-12.md) between its
  `parity:begin` and `parity:end` markers, which `scripts/parity/update_report.py` writes from
  `scripts/parity/score.py`, `scripts/parity/devices.json` and the command registry. The rest of
  the report is an ordinary document that a pull request edits freely. CI checks only that the
  inputs render (`update_report.py --check-inputs`), so marked parts that are stale, or that a pull
  request regenerated anyway, never fail it;
- its two figures, rendered from the same numbers;
- `last_verified` in a document's front matter: after the merge, `python scripts/docs.py stamp`
  moves it up to the day of the document's last change on `main`.

When a merge of `main` into a branch conflicts inside the marked parts, take `main`'s side there
(`git checkout --theirs` on the file when the branch changed nothing else in it); the workflow
writes them again after the merge either way.

```sh
python scripts/docs.py impact   # documents that cover code you changed but you have not updated
                                # (a notice only, for files three or more documents cover)
python scripts/docs.py check    # what CI runs: front matter, covered paths, links and #anchors
python scripts/docs.py index    # prints the index with your documents in it; writes nothing
python scripts/parity/update_report.py --check-inputs   # what CI runs for the parity report
```

Every file under `docs/` starts with front matter (format in `scripts/docs.py`): `description`,
`read_when`, `type` (system, decision, spec, howto, plan), `status`, `last_verified` and
`covers`, the code paths it describes. A pull request that changes a document has checked it, and
`main` stamps `last_verified` with the day of the merge (above), so leave the line as it is. Set it
by hand only when you checked a document against the code without changing anything else in it, or
for a new document. A commit that changes documents without checking them (a rename across all of
them) carries a `Last-verified: keep` line in its message. CI warns when the date is over 90 days
old. A new document lists in `covers` the code
it describes, so `impact` can point the next change to it.

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
