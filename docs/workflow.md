---
description: The one copy of how work is done on Suhina - issues and their types, claims, posting as the bot, branch names, overlapping work, merging, and what a pull request carries (how to check it, parity, settings help, documents, generated files, front matter)
read_when: opening an issue, claiming, /claim, opening a pull request, branch name, issue type, labels, overlapping work, Depends on, post as the bot, agent_post, how to check it, parity, settings help, which documents to update, generated files, front matter, last_verified, merging
type: howto
status: current
last_verified: 2026-11-01
covers:
  - scripts/agent_post.py
  - scripts/branch_name.py
  - .github/workflows/open-issue.yml
  - .github/workflows/comment-pr.yml
  - .github/workflows/branch-name.yml
  - .github/workflows/overlap.yml
  - .github/workflows/docs-generated.yml
  - .github/labeler.yml
---

# How work is done

The one copy of the workflow, for people and agents alike. [`AGENTS.md`](../AGENTS.md) holds the
rules every task needs and points here; [`CONTRIBUTING.md`](../CONTRIBUTING.md) holds the
clean-room rules, the DCO and the licences.

**Local work skips the first half.** Issues, claims and branch names are for work that will
become a pull request. Work kept on a local branch, a review, or a question about the code
needs none of them; a review writes nothing to GitHub unless the person asks for it.

## Tasks

Work is tracked in the GitHub project **Suhina**
(<https://github.com/orgs/suhina-daw/projects/1>), and nowhere else: a task's state lives in its
issue and on the board, never in chat or in one person's window. New work, bugs found while
testing and follow-ups a change leaves open go there as items, written in English, with a
Status, a Priority (P0 to P2) and a Phase (the ROADMAP milestone).

### Issues

- **One issue per problem.** Before opening an issue, search the open ones; when one exists,
  comment there instead. A duplicate that slips through is closed with a link to the first one.
- **What gets an issue:** a feature or a piece of an epic, so the pull request can say
  `Closes #N`. A bug fixed at once or a small change goes straight to a pull request that says
  what it fixes.
- **Small and scoped**, with acceptance criteria (the issue forms ask for them: Bug, Plug-in
  problem, Feature request, New effect or instrument, Preset contribution, Task). Architecture
  belongs in an ADR under `docs/decisions/`, not in an issue thread. An issue is the only context
  an agent gets apart from the documents, so it stands on its own: goal, acceptance criteria,
  where the code goes.
- **Labels.** One type label: `enhancement`, `bug`, `documentation`, `chore` (repository, build
  and release work) or `epic` (a larger piece split into issues); `question` is for asking, not
  for work. One or more area labels (`engine`, `ui`, `formats`, `browser`, `audio-io`, `plug-ins`,
  `devices`, `presets`, `tests`), and `windows`, `macos` or `linux` when it concerns one platform.
  The forms set the type, and `.github/workflows/issue-labels.yml` adds area and platform labels
  from the form's answers.
- Items imported from the earlier board keep their old key in the title (`(LOOP-27)`); a pull
  request for one names that key.

### Posting as the bot

An agent run by someone with write access opens issues and posts its feedback on pull requests
as `suhina-bot[bot]`, so the person who ran the session is not their author.

- **Check access first**, once: `gh api repos/suhina-daw/suhina-daw --jq .permissions.push` (or
  the GitHub MCP's repository permissions). `true`: use the workflows below. `false`: post from
  the person's own account and tell them it went out under their account and why. A dispatch
  answered `HTTP 403: Must have admin rights to Repository` means the same as `false` (GitHub's
  message names the wrong right): fall back, never retry it or try another token.
  [`scripts/agent_post.py`](../scripts/agent_post.py) makes the check and takes either path, from
  the same `request.json` the workflows take, and prints `OPENED_ISSUE=#N <url>` or
  `COMMENTED_PR=#N <url>` both ways:

  ```sh
  python scripts/agent_post.py issue request.json     # or: comment request.json
  ```

  If the workflow posted but its successful completion could not be confirmed, the helper exits
  with an error that includes the created issue or comment's marker and the run id. Inspect that
  run and complete any missing follow-up; do not post again.
- **Open an issue as the bot** through the [Open issue](../.github/workflows/open-issue.yml)
  workflow. Start it with the GitHub MCP's `actions_run_trigger` or with `gh`, inputs as JSON on
  stdin (`--json`, so a long body is not cut off by the command-line limit), then read the issue
  number from the log of the run the start returns, never the latest run, which may be another
  agent's. A run that failed after it opened the issue still prints the line; re-running it does
  not open the issue again, but starting a new one does.

  ```sh
  python -c "import json; print(json.dumps({'title': 'Piano roll: snap notes to the grid', \
    'body': open('issue.md', encoding='utf-8').read(), 'type': 'enhancement', 'labels': 'ui', \
    'parent': '123'}))" > request.json
  run_url="$(gh workflow run open-issue.yml --json < request.json)"
  run_id="${run_url##*/}"
  gh run watch "$run_id" --exit-status; gh run view "$run_id" --log | grep OPENED_ISSUE
  ```

  `type` is one of the type labels above and sets the issue type too; `labels` are the area and
  platform labels; `parent` is an open epic. Set the board's Priority and Phase afterwards. A
  pull request itself (its branch, its description and its merge) stays under the person's
  account.
- **Without write access**, open it with `gh issue create --body-file` under the person's
  account, identifying it as AI-assisted. Check first that a supplied parent is an open issue
  labelled `epic`, as the workflow does; if it cannot be checked, stop before posting. With
  `triage` access (`.permissions.triage`) give it the type and area labels, set its issue type and
  add it under the parent epic; without, leave the labels out, end the body with the labels a
  maintainer should add and write `Part of #N` for the parent.
- **Comment on a pull request as the bot** when the comment is an agent's feedback (a review, the
  changes it asks for, a summary of what it found), through the
  [Comment on pull request](../.github/workflows/comment-pr.yml) workflow, started the same way;
  read `COMMENTED_PR=#N <url>` from the log of the run the start returns; re-running a failed run
  does not comment twice. It is a comment, not a
  GitHub review: it neither approves nor requests changes. Without write access, post it with
  `gh pr comment <N> --body-file …` under the person's account, ending with a line that says an
  agent wrote it.

  ```sh
  python -c "import json; print(json.dumps({'pr': '600', \
    'body': open('review.md', encoding='utf-8').read()}))" > request.json
  run_url="$(gh workflow run comment-pr.yml --json < request.json)"
  run_id="${run_url##*/}"
  gh run watch "$run_id" --exit-status; gh run view "$run_id" --log | grep COMMENTED_PR
  ```

- **No elevated access just to post.** The workflows need existing write access, not admin
  access. Never ask for admin or write access, broader token scopes, App keys or secrets to post
  as the bot, and a denied connector action is not permission to get round it with another tool:
  explain the limit and leave the prepared text for the person. Trusted contributors can instead
  use the comment commands in [Trusted bot requests](claim-bot.md#trusted-bot-requests).
- **What the helper checks.** With triage access it checks the repository's labels before
  creating an issue, applies only those that exist and names the missing ones in the body; a
  failed label lookup stops before creation, and a failed create is never retried. If no marker
  can be read after a dispatch, the outcome is unknown and the workflow may still post: inspect
  the named run instead of starting another request.
- **Trusted bot actions.** Contributors listed in `TRUSTED_CONTRIBUTORS` may ask `suhina-bot` to
  create issues with `/open-issue`, close or reopen issues with `/close` and `/reopen`, and request
  a pull request's test build with `/test-build`; each action records **On behalf of @login**,
  taken from the authenticated commenter ([Trusted bot requests](claim-bot.md#trusted-bot-requests)
  has the issue JSON and limits). The list grants neither write access nor merge approval. The
  repository variable `MAINTAINERS` sets the maintainer role across all bot workflows, merge
  authorization included, with no fallback when it is empty. Both lists are comma- or
  whitespace-separated GitHub logins, case-insensitive, with an optional `@` (Settings › Secrets
  and variables › Actions › Variables). For fork test builds maintainers use `/test-build` too,
  since fork runs may lack the variables a label would need.
- Installed reviewers such as Codex and CodeRabbit post through their own apps and follow
  [`code-review.md`](code-review.md).

### Claiming

**Free work** has no assignee and no open pull request that says `Closes #N` (the issue's
*Linked pull requests* box shows it). On the board it is in `Backlog` or `Ready`, with one
difference: a **feature request** (`enhancement`) in `Backlog` is not agreed yet. Write your idea
or plan in the issue; a maintainer moves it to `Ready` when Suhina wants it, and only then is it
open to claim. A trusted contributor, named in the repository variable `TRUSTED_CONTRIBUTORS`, may
claim a feature in `Backlog` without waiting, or move it to `Ready` with `/ready` so others can
claim it ([`claim-bot.md`](claim-bot.md) › Who is who). A bug in `Backlog` is free.
`Do not start yet` means the order is deliberate: the issue and its comments say what the work
waits for, so ask there instead of starting.

**Bugs and small changes need no issue and no claim.** Open a pull request that says what it
fixes. The *Overlap* notice ([`overlap.yml`](../.github/workflows/overlap.yml)) comments on it when
another open pull request or a claim changes the same files, and keeps the comment up to date with
each push; it never fails.

**Claim a feature or a piece of an epic** in a comment on the issue before writing code, with the
plan and a `Touches:` line naming the paths you will change:

> /claim Plan: … When: …
> Touches: crates/ui/src/detail, crates/session

The claim bot ([`claim-bot.md`](claim-bot.md)) moves the card to `In progress` and assigns you,
and refuses the claim when the work is not free or another issue it is *Blocked by* is still open.
Read its answer: when it says the work is taken, that it is blocked by another issue, or that it is
already yours and names a pull request, do not start a second one; continue in that pull request,
take the blocking issue instead, or stop and tell the person. The bot's answer and the *Overlap*
notice find your claim by the `Touches:` paths. Without the bot, a plain comment ("I'd like to take
this. Plan: …") is the claim, and the first one wins: a bot that is down never means the issue is
free. Hold at most five claims without a pull request at a time. An `epic` is a container: claim
one of its issues, not the epic.

A maintainer who hands an issue to someone writes `/claim @their-login`, with the plan after it if
there is one; only a maintainer may claim for someone else.

**Show the work within 48 hours.** Open a draft pull request that says `Closes #N` as soon as there
is a first commit. A claim with no pull request after 48 hours lapses, and the next `/claim` takes
it over. If you stop, comment `/unclaim`, so the card goes back to `Ready`.

**One pull request per issue.** Before opening one, check the issue's linked pull requests; when
one is open, push to it (or offer your change in its thread) rather than opening another, also from
a new session of your own. When that pull request has stalled, ask its author in it; with no answer
in three days, you may take the work over in that pull request, or in one that replaces it, names
it and is linked from it. A second, competing pull request is opened only when the person asks for
one, and its description says `Alternative to #N`. Continuing open work is always allowed.

**What belongs in one pull request.** A fix found while working that is small and in the same code
goes in the same pull request, under *Also fixed* in its description. Make it a pull request of its
own, or an issue, when it is larger, in other code, or in code another open pull request or claim
is changing. Keep at most three pull requests open at a time, unless you are a maintainer.

`good first issue` marks work that needs no knowledge of the engine, `help wanted` work the
maintainers would like a hand with.

### Branch names

A branch is `<kind>/<short-description>`: `feature/` for something a user can do, `fix/` for a
bug, `docs/` for documentation, `chore/` for repository, build and release work, then two to five
lowercase words joined by hyphens (`feature/piano-roll-workflow`, `fix/short-panes`). Before the
first push, create that branch from the starting point and open the pull request from it, even if
the session was started on a `claude/...` branch; never open a pull request from a `claude/...`
branch. A session started for you can be given its branch up front (`outcome_branch`). The
[Branch name](../.github/workflows/branch-name.yml) check warns about a name that does not follow
the rule; it never fails.

`.github/workflows/pr-labels.yml` keeps a pull request's labels as `.github/labeler.yml` maps
them: the kind from the branch name (`feature/` `enhancement`, `fix/` `bug`, `docs/`
`documentation`, `chore/` `chore`; also `documentation` when every file changed is a document) and
the areas from the paths changed. At every push it takes off the ones that no longer match; labels
it does not map (a platform, `test-build`, `ready to merge`) are left alone, and a platform label
is added by hand.

### Overlapping work

Two issues often need the same code: a helper, a command, a model field. The claim bot lists the
open pull requests and other claims that touch your `Touches:` paths, and the Overlap notice lists
them again on the pull request; without a claim, list them yourself before writing code
(`gh pr list --state open --json number,title,files`), and read every one that touches the crates,
views or commands you will change. Then:

- **It already builds what you need:** do not write it again; build on it. On a branch of this
  repository, stack on its branch (`gh stack`). From a fork, which cannot be stacked, branch from
  its head and write `Depends on #N` on a line of its own in your description; the
  [Depends on](../.github/workflows/depends-on.yml) check keeps your pull request out of the merge
  queue until #N has merged, so keep it a draft until then.
- **It changes the same code for another purpose:** comment in both pull requests what overlaps and
  which should merge first, and leave the order to a maintainer when it is not obvious.
- **It does the same thing another way:** do not merge the two yourself, edit the other pull
  request, or open a third; comment in both and leave the choice to a maintainer.

When an issue cannot start before another one is done, mark it on GitHub (the issue's
*Relationships › Blocked by*), not only in prose: the claim bot refuses claims on it until the
blocking issue is closed.

### Merging

Merging is a maintainer's decision. An agent never adds the `ready to merge` label, which approves
a pull request and sends it to the merge queue
([CONTRIBUTING.md › Merging many pull requests](../CONTRIBUTING.md#merging-many-pull-requests)), and
never merges or queues a pull request itself unless the person asks for that one. It gets a pull
request green and mergeable and says it is ready; the maintainer adds the label.

## What a pull request carries

The description uses every section of `.github/pull_request_template.md`; one line is enough where
nothing applies (`.github/workflows/pr-description.yml` warns about a missing one). Commits are
signed off (CONTRIBUTING › Developer Certificate of Origin).

### How to check it

When a pull request changes anything a user sees, hears or does, in the app or outside it (a view,
a menu, a dialog, a shortcut, a setting, drag and drop, playback and sound, a file it opens,
imports or writes, a `suhina-cli` command, an MCP tool), its description has a **How to check it**
section: numbered steps that someone who has never used Suhina or any other music program can
follow from a test build ([CONTRIBUTING.md › Trying a pull request](../CONTRIBUTING.md#trying-a-pull-request))
and finish in a few minutes. [`how-to-check.md`](how-to-check.md) is the guide to writing them. The
steps

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

### Parity

Every change that adds, changes or removes something a user can do (the same changes that need a
*How to check it* section) updates, in the same pull request, the coverage of the chapters it
moves in [`scripts/parity/score.py`](../scripts/parity/score.py), and
[`scripts/parity/devices.json`](../scripts/parity/devices.json) for a built-in effect or
instrument, and nothing else: the numbers in [`research/parity-live-12.md`](research/parity-live-12.md)
(its marked parts, the command count among them) are regenerated on `main` after the merge, so a
pull request leaves them alone, even when it adds a command. A pull request that moves no coverage
writes `Parity: no change` and the reason in its description ("Parity: no change, a refactor").
When a pull request changes `crates/`, `devices/` or `apps/` and does neither, the
[Parity notice](../.github/workflows/parity-notice.yml) check warns; it does not fail.

### Built-in devices and settings

- **A built-in device** is compiled into the app, not an `.sd` file. After changing one, its
  parameters or its layout, run `cargo test -p suhina-device-registry` (it fails on a layout that
  names a parameter its device lacks or that does not load), rebuild the desktop app, and check the
  device's panel in it: its layout, or the generated parameter list. Report the app's path. Do this
  without waiting for a separate request. The `suhina-devices` skill has the checklists.
- **A new or moved setting** follows [`spec/ui-settings.md`](spec/ui-settings.md) › Adding a
  setting (page by task, groups of at most seven outside Advanced, no status or commands among
  settings, one sentence of description, one control per kind of choice, the stored key
  unchanged).
- **A setting includes its website help**, in the same pull request, without waiting to be asked.
  Whenever you add, remove, move, rename or change a setting, read
  [`spec/settings-guide-export.md`](spec/settings-guide-export.md), the control in
  `crates/ui/src/settings/settings_rows.rs` and its implementation, and update
  `docs/guide-data/settings-help.json`: description, summary when present, every choice's name and
  explanation, when to change it, implementation references and any work-in-progress status.
  Explain grouped controls, defaults, dependencies and when a control has no effect. Migrate ids
  for moved settings and remove help for deleted ones. Review behaviour changes even when the
  registry metadata and fingerprint stay unchanged; copy the current source fingerprint only after
  reviewing the text, never to make CI pass. Run `python scripts/settings_guide.py` and commit
  `docs/guide-data/settings.json` with the help (this export is committed on purpose), then
  `python scripts/settings_guide.py --check` and
  `python -m unittest discover -s scripts -p 'test_settings_guide.py'`. Check every changed
  setting, not just the first error: the exporter stops at the first missing or stale entry. Do
  not defer the help to another repository or a follow-up.

### Documents

A pull request that changes how something works updates, in the same pull request, every document
that describes it, so the docs never describe code that no longer exists.
[`README.md`](README.md) says what each kind of document holds (an ADR, a spec, a system
document), how an ADR is amended and what the status fields mean; `python scripts/docs.py check`
enforces it, and `python scripts/docs.py impact` lists the documents that cover code you changed.

| If you change | Update |
|---|---|
| A crate's role, a dependency between crates, threads, how commands or data flow | [`architecture.md`](architecture.md) and its crate table |
| A decision recorded in an ADR | that ADR (status, consequences), or a new ADR that supersedes it |
| What is read from a format, or behaviour described in a spec | the spec in `docs/spec/` |
| What a `.suhina` or `.sd` file holds | its `VERSION`, a file for the new version and a row in the spec's *Versions* ([ADR 0010](decisions/0010-project-files.md) › Amendment: format versions) |
| MCP tools, or how agents use commands | [`mcp.md`](mcp.md) (the command list, [`commands.md`](commands.md), is generated) |
| What works for users | a new bullet as a file of its own in [`works-now/`](works-now/README.md), folded into [`ROADMAP.md`](../ROADMAP.md) › What works now on `main`; a bullet already there is changed in `ROADMAP.md`; [`README.md`](../README.md) if it shows it |
| What the parity measurement says (a milestone that lands, a feature that appears or is dropped) | the coverage of the chapters it changes in `scripts/parity/score.py`; the marked parts of [`research/parity-live-12.md`](research/parity-live-12.md) are regenerated on `main`, and its prose is edited in that file like any other document |
| A setting in the Settings window (`settings_rows.rs`) | the table and rules in [`spec/ui-settings.md`](spec/ui-settings.md), and `docs/guide-data/settings-help.json` |
| A rule in `AGENTS.md` or this file | that file only: each rule is written once |
| A checklist a skill in `.claude/skills/` gives | that skill (`docs.py check` checks the paths and symbols it names) |

A limit or a planned part that a change resolves is removed from the docs in the same change.

**Write documents so parallel pull requests do not collide.** In a list that several changes add
to (the crate table), add a row of your own rather than extending someone else's sentence; a new
*What works now* bullet is a file of its own in `docs/works-now/`.

**Generated files stay out of the change.** [`INDEX.md`](INDEX.md) (from the documents' front
matter) and [`commands.md`](commands.md) (from the command registry) are regenerated on `main`
after every merge by [`docs-generated.yml`](../.github/workflows/docs-generated.yml), and a pull
request that edits one of them fails CI rather than losing the edit silently. The same workflow
writes three more things a pull request leaves alone, so that two pull requests never conflict on
them:

- the parts of [`research/parity-live-12.md`](research/parity-live-12.md) between its
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

**Front matter.** Every file under `docs/` starts with front matter (format in `scripts/docs.py`):
`description`, `read_when`, `type` (system, decision, spec, howto, plan), `status`,
`last_verified` and `covers`, the code paths it describes. A pull request that changes a document
has checked it, and `main` stamps `last_verified` with the day of the merge, so leave the line as
it is. Set it by hand only when you checked a document against the code without changing anything
else in it, or for a new document. A commit that changes documents without checking them (a rename
across all of them) carries a `Last-verified: keep` line in its message. CI warns when the date is
over 90 days old. A new document lists in `covers` the code it describes, so `impact` can point the
next change to it.
