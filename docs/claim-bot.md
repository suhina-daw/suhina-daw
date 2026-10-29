---
description: How the claim bot moves an issue's card on the project board, holds work, refuses blocked work, lists overlapping work, and how the Depends on check orders pull requests
read_when: claim, /claim, /claim @user, claim for someone else, on behalf, /unclaim, /hold, /ready, claim bot, project board, PROJECT_TOKEN, status automation, good first issue, lapsed claim, duplicate pull request, feature not agreed, project sync, board out of date, cards missing from the board, Touches, overlapping work, overlap, blocked by, blocked, dependency, Depends on, stacked pull requests, merge order, trusted contributor, TRUSTED_CONTRIBUTORS, maintainer, who may claim, claim limit, too many claims
type: howto
status: current
last_verified: 2026-10-28
covers:
  - .github/workflows/trusted-requests.yml
  - scripts/test_trusted_requests.py
  - scripts/build_reuse.py
  - scripts/test_build_reuse.py
  - .github/workflows/claim.yml
  - scripts/claim_bot_test.py
  - .github/workflows/depends-on.yml
  - .github/workflows/overlap.yml
  - scripts/test_pr_workflows.py
  - scripts/depends_on_test.py
  - .github/workflows/project-sync.yml
  - scripts/project_sync.py
  - scripts/test_project_sync.py
---

# The claim bot

[`.github/workflows/claim.yml`](../.github/workflows/claim.yml) moves an issue's card on the
[Suhina project board](https://github.com/orgs/suhina-daw/projects/1) when someone comments one of
four commands, so that taking a task works the way [`CONTRIBUTING.md`](../CONTRIBUTING.md) ›
*Working on an issue* describes, without a maintainer moving cards by hand. The first line of the
comment carries the command; the rest is the plan.

| Comment on an issue | What happens |
|---|---|
| `/claim` | Refused when the card is held, when an issue it is *Blocked by* is open, when an open pull request closes the issue, when the issue is an `epic`, when it is a feature (`enhancement`) still in `Backlog` and the commenter is not a maintainer, or when the status says someone is already on it. A claim `In progress` with no pull request 48 hours after the bot wrote *Claimed by* has lapsed: the claimer is unassigned and the new claim goes ahead. Otherwise the issue is added to the board if it is not there, Status becomes `In progress`, and the commenter is assigned. When the commenter has the claim already, the answer names the open pull request to continue in. A `Touches:` line in the claim makes the answer list other open work on the same paths ([Overlapping work](#overlapping-work)) |
| `/claim @login` | A maintainer only: the same claim, made for `@login` instead of the commenter. Every refusal above applies to that person, the card becomes `In progress`, `@login` is assigned, and the answer reads *Claimed by @login, on behalf of a maintainer*. Someone already assigned by hand keeps the assignment and the card moves; a feature in `Backlog` may be claimed, as with a maintainer's own claim. Anyone else naming another person is refused (they may claim only for themselves; naming yourself is an ordinary claim), and a name that is not a GitHub login (letters, digits and single hyphens, up to 39 characters) is refused without being quoted. When GitHub will not assign the person (no access, and never commented on the issue), the claim stands on the card and the answer says so |
| `/unclaim` | By the assigned person, who is unassigned, or a maintainer, who unassigns whoever claimed it. A card `In progress` goes back to `Ready`; a held card or one `In review` keeps its status |
| `/hold <why>` | A maintainer only, and the reason is required: Status becomes `Do not start yet` |
| `/ready` | A maintainer: Status goes back to `Ready`, so the card can be claimed again. A trusted contributor ([Who is who](#who-is-who)): only from `Backlog` or a card without a status, which agrees a feature so that anyone may claim it; a held card or any other status is refused |
| anything else | Ignored; a comment that does not start with `/` does not even start a run |

Contributors keep working without it: a plain comment that says "I'd like to take this" is still a
claim, and the bot says so in the issue when it cannot do its job.

The bot reads the board rather than hard-coding it: it finds the project's `Status` field by name,
treats `Backlog`, `Ready` and a card without a status as free (a feature only in `Ready`), `Do not start yet` as held, `In progress` and `In review` as
someone already on it, and every other value as not open to start; all of them are matched without
case. The names live at the top of the script, so renaming a status is a one-line change there;
until then the bot says so in the issue and names the options the field does have.

## Lapsed claims and features not agreed

A claim keeps work for 48 hours without a pull request (`LAPSE_HOURS` at the top of the script).
The bot dates a claim by its own *Claimed by* comment, so a claim made only in a plain comment, or
one whose comment was deleted, never lapses on its own; a maintainer frees it with `/unclaim`. A
claim with an open pull request that says `Closes #N` never lapses: stalled pull requests are taken
over by asking in them, as [`CONTRIBUTING.md`](../CONTRIBUTING.md) › *Working on an issue* says.
A card `In review` never lapses either.

A feature request (`enhancement`) lands in `Backlog` like any issue, but it is not agreed until a
maintainer moves it to `Ready` (`/ready`); until then a contributor's `/claim` is refused with that
explanation, so two people never start the same idea in two shapes. A trusted contributor
([Who is who](#who-is-who)) may claim it in `Backlog` without waiting, or agree it with `/ready`, also
for an issue they opened themselves. A bug in `Backlog` is free.

A claim without a pull request is a promise, so one person holds only a few at once: anyone who is
not a maintainer may hold at most five (`MAX_OPEN_CLAIMS`), counted as the open issues assigned to
them that no open pull request closes, this issue left out. The sixth `/claim` is refused and lists
them; a pull request that says `Closes #N` (a draft is enough), or an `/unclaim`, frees a place. A
pull request stacked on another branch counts too: GitHub links `Closes #N` only from a pull request
into the default branch, so the bot also reads the claimer's open pull requests for it. A
maintainer's `/claim @login` is not refused by the limit, and afterwards counts among `@login`'s
claims like any other; a count that fails lets the claim go ahead.

## Who is who

The bot does not take roles from GitHub's author association, which calls everyone with any access
to the repository, triage included, a `COLLABORATOR`. It reads two lists of logins instead:

| Role | Listed in | May |
|---|---|---|
| Maintainer | the repository variable `MAINTAINERS` (Settings → Secrets and variables → Actions → Variables), logins separated by commas or whitespace | everything in the table above: `/hold`, `/ready`, `/claim @login`, `/unclaim` for anyone, claim a feature in `Backlog`, any number of claims |
| Trusted contributor | the repository variable `TRUSTED_CONTRIBUTORS` (Settings → Secrets and variables → Actions → Variables), logins separated by commas or spaces | claim a feature in `Backlog` without `/ready`, and `/ready` a card in `Backlog` so anyone may claim it; not `/hold`, not release a held card, not claim for others |
| Anyone else | | claim free work for themselves, `/unclaim` their own claim |

Both lists accept an optional `@` and match whole logins without case. `MAINTAINERS` is shared
by Claim, Overlap, Ready to merge, Test build, Test build links and Trusted requests. Changing
maintainers requires editing that variable only; there is no hardcoded fallback. An unset or
empty list grants nobody the maintainer role. Keep these as repository variables, not environment
overrides, so all workflows see the same policy. Only people allowed to administer repository
variables should manage these lists: `MAINTAINERS` controls merge authorization as well as bot
commands. Membership does not grant GitHub's own write/admin permissions.

Trusted contributors may also request the bot actions below; no repository write access comes with the list. Merging stays a maintainer's: only a maintainer's
`ready to merge` label queues a pull request ([`CONTRIBUTING.md`](../CONTRIBUTING.md) › Merging many
pull requests), and a `/hold` reason is quoted only when a maintainer wrote it.

## Trusted bot requests

Maintainers and the GitHub logins in `TRUSTED_CONTRIBUTORS` may comment these commands.
[Trusted requests](../.github/workflows/trusted-requests.yml) runs from the default branch,
checks the actual comment author, and writes as `suhina-bot[bot]` with **On behalf of @login**.
Bot authors and unlisted people are refused. Names are case-insensitive, separated by commas
or whitespace; an optional `@` is accepted.

| Comment | Where | Action |
|---|---|---|
| `/close` | An issue | Close it |
| `/reopen` | An issue | Reopen it |
| `/test-build` | An open pull request | Reuse a suitable existing build or request one for the current commit; a new commit needs a new comment |
| `/open-issue` followed by JSON | An existing issue or pull request | Create a new issue as the bot and reply with its number |

Post a **new comment**, with the command on its own first line. Editing a comment does not
run it. The source comment is fetched again immediately before carrying out the request;
changed authors, bodies, update timestamps and deleted comments are rejected before any write.
This is a second check, not an atomic transaction with GitHub: an edit after the final read
cannot cancel a write already starting. Close/reopen and test-build take no arguments;
close/reopen never act on pull requests.
A completed request is not repeated when its Actions run is rerun. Issue creation recovers an
issue created before a failed reply. A partially failed test-build request may repeat a label
event for the same commit, but cannot authorize another commit. A new build request gets one
reply carrying the proof the build checks (its last line). A reuse reply links the existing run
and grants no new build or merge authorization.

Authorized test-build requests for the same PR run one at a time; a newer pending request replaces
an older pending one. Repeated comments reuse an active build for the same head, or a successful
build with all requested packages still available. Verified main-only updates can reuse a
successful ancestor build ([Test builds](releasing.md#test-builds)). A five-minute dispatch window
also prevents duplicate requests before a newly requested run becomes visible. After a failed
build or expired packages, a new comment requests another build. Other commands remain independent
per comment. Membership and the source comment are checked again before writing.

For example, comment on the issue that prompted the new work:

```text
/open-issue
{"title":"Browser: preserve the search","body":"Describe the problem and acceptance criteria here.","type":"bug","labels":["browser"]}
```

The JSON accepts only `title`, `body`, `type` and optional `labels`. Types are `enhancement`,
`bug`, `documentation`, `chore`, `epic`; labels are existing area/platform labels listed in
CONTRIBUTING. Search for existing work first. Set Priority and Phase on the board afterwards
as for other issues. GitHub's ordinary **New issue** remains available.

This uses the existing `suhina-bot` environment and App secrets described in
[open-issue.yml](../.github/workflows/open-issue.yml), with Issues and Pull requests write
permissions scoped to this repository. No Actions write permission is needed: the App's label
event starts the build. The read-only workflow token lists runs, jobs and artifacts; the App
needs no extra permissions. Only default-branch automation is checked out. PR git objects are
fetched for patch comparisons, but PR files are never checked out or executed.
A rejected request fails in **Actions → Trusted requests** without changing the issue;
the log explains the refusal. Workflow dispatch still requires GitHub write access; trusted
contributors use the comment commands instead.

Agents post those commands using the user's existing GitHub connection. They need no admin
access, App key or broader token scopes. The workflow-dispatch posting convention applies to
callers who already have write access; other contributors may post ordinary issues and PR
comments as themselves. See CONTRIBUTING's *No elevated access just to post* rule. A connector's
own restrictions still apply and must not be bypassed to reach the bot.

Merge, hold and claim-for-others policies are unchanged. The ordinary **Open issue** workflow
also attributes new issues to its authenticated actor.

Check the gateway and build authorization without network writes:

```sh
python -m unittest discover -s scripts -p 'test_trusted_requests.py'
```

## Blocked work

An issue that cannot start before another one is done is marked on GitHub: the issue's
*Relationships › Blocked by*. While any issue it is blocked by is open, `/claim` is refused, for a
maintainer's `/claim @login` too, and the answer lists the blocking issues (`owner/repo#N` when one
lives in another repository) so the claimer can take one of those instead. Once they are closed, the
same `/claim` goes through. The person who has the claim already is answered as before (*already
yours*), so a second session of theirs is still pointed at its pull request. A lookup that fails is
a warning in the run's log and does not stop the claim: as with the open pull request check, the
board decides.

## Overlapping work

Two issues often need the same code. A claim may carry a line naming the paths the work will change,
on its own line or after the plan:

```text
/claim Plan: the grid lines follow the zoom. When: today.
Touches: crates/ui/src/arrangement, crates/session
```

The bot reads up to 20 plain relative paths from it (a trailing `/`, `/*` or `/**` is dropped; a path
with `..`, a leading `/` or any other character is ignored), accepts the claim exactly as without the
line, and adds to its answer the other open work on those paths:

- **open pull requests** whose changed files lie under one of the paths (the 100 most recently
  updated, with their first 100 files each), except a pull request that closes this issue;
- **other claims**: open issues with someone assigned (the 50 most recently updated) whose newest
  `/claim`, not followed by an `/unclaim`, has a `Touches:` line that overlaps.

It lists at most ten, each with the paths that overlap, and points at `AGENTS.md` › Tasks for what to
do about them. It never refuses a claim for an overlap: two pieces of work on the same crate are
often fine, and only the people doing them can tell. A claim without the line is answered with a
sentence saying what the line does. A lookup that fails leaves its part out of the answer.

A bug fix or a small change needs no issue and no claim, so the same lookup runs on pull requests
too: the [Overlap](../.github/workflows/overlap.yml) workflow comments on a pull request when
another open pull request or a claim's `Touches:` line covers its files, and keeps that one comment
current with each push. It reads every page of open pull requests before checking overlaps and
stack relationships. If a later page fails, known overlaps are still compared; an existing notice
is preserved, and any new notice says the scan is incomplete. Renamed files count under both their old and new names in either pull
request. If one pull request's full file lookup fails, the notice compares its already known files
and continues with the other pull requests; the run log warns about the incomplete lookup.
It skips pull requests in the same stack and `Cargo.lock` and the generated documents, and it never fails.

## The Depends on check

[`.github/workflows/depends-on.yml`](../.github/workflows/depends-on.yml) orders pull requests that
build on each other when stacking cannot. A pull request names each one it needs on a line of its
own in its description: `Depends on #123`, `Depends-On: #123`, or a link to an issue or pull request
of this repository (lines inside HTML comments do not count). The check fails, naming them, while any
is not done: a pull request is done when it is merged, an issue when it is closed, and a pull request
closed without merging still blocks. It runs again when the description is edited or commits are
pushed, and in the merge queue, where it reads the description of the pull request being tested
(from the queue's branch name), so a dependency that was reopened is caught there too. Stacked pull
requests in this repository (`gh stack`) are ordered by their base branches and need no line.

For the check to stop the merge queue it has to be a required status check in the branch rules for
`main` (Settings → Rules), as `Depends on / depends-on`.

## Holding work

`Do not start yet` is neither a claim nor a failure: it says the order is deliberate — the work
waits for a decision, another issue or an unfinished spec. The reason belongs in the issue rather
than in the card, because the issue is what the next person, and the next agent, reads: a claim on a
held card is refused with the most recent `/hold` reason quoted back, so the gate explains itself.
Only a maintainer's `/hold` is quoted (anyone else's was refused), and without its `@` signs, so a
quote never pings anyone.

The board's `Status` field needs that option before the first `/hold` (Project → ⋯ → Settings →
Status → add `Do not start yet`). It has `Backlog`, `Ready`, `In progress`, `In review` and `Done`
today; until the gate exists, the bot names the options the field does have.

## Setting it up

The workflow's `GITHUB_TOKEN` has no access to a project owned by an organization, so the job needs
a token of its own, the secret **`PROJECT_TOKEN`** in the environment **`project-board`** (Settings →
Environments → `project-board` → Add environment secret). The environment's *Deployment branches and
tags* allow `main` only, so a workflow pushed to another branch, or run by hand from one, cannot read
the token; `claim.yml` (on `issue_comment`) and `project-sync.yml` run from `main`. It is not a
repository secret, which a workflow on any branch could read.

- **Fine-grained, recommended:** owned by the organization, with **Organization permissions →
  Projects: Read and write**, and on `suhina-daw/suhina-daw` the repository permissions
  **Issues: Read** and **Pull requests: Read**.
- **Classic:** the scopes `project` and `public_repo` (and `repo`, if the repository ever becomes
  private).

Both kinds expire, and a fine-grained token expires within a year. Its date is kept here and where
the release process keeps its reminders ([`releasing.md`](releasing.md) › Reminders):

- `PROJECT_TOKEN` expires: fill in

## Project sync

The claim bot moves one card when someone comments. Cards drift anyway: an issue nobody put on the
board, a pull request merged without its card moving, claims made while the bot was off.
[`scripts/project_sync.py`](../scripts/project_sync.py) brings the whole board up to date, and
[`.github/workflows/project-sync.yml`](../.github/workflows/project-sync.yml) runs it with the same
`PROJECT_TOKEN`. It reads the repository's open issues and open pull requests and every card, then:

| The repository says | The card |
|---|---|
| An open issue or open pull request with no card | Added, in `Backlog` unless a row below gives it another Status |
| An open issue with an open pull request that closes it (`Closes #N`, `Fixes #N` or a linked pull request) or says `Part of #N` | `In review` |
| An open issue that is assigned, or whose newest `/claim` is not followed by an `/unclaim`, with no such pull request | `In progress`; a held card (`Do not start yet`) stays held |
| A closed issue, or a merged pull request | `Done` |
| Anything else | Keeps its Status |

A `/claim` or `/unclaim` that the bot refused (its answer matches one of the refusals listed in
`REFUSALS` and `REFUSED_ANYWHERE` at the top of the script) does not count. Only Status is written: Priority and Phase are
left for a maintainer, cards are never deleted, and cards of other repositories and draft cards are
not touched.

To run it: **Actions → Project sync → Run workflow**, first with **apply** unchecked. That dry run
writes nothing; its log, and the run's summary page, list every card it would add or move with the
reason. Read it, then run it again with **apply** checked to write exactly that. It can be run
whenever the board drifts. When it fails, the log names what it could not read or which Status
option the board lacks, and nothing was written if it stopped before its first change.

From a checkout, with a token of your own that may read the board:

```sh
PROJECT_TOKEN=... python3 scripts/project_sync.py            # dry run
PROJECT_TOKEN=... python3 scripts/project_sync.py --apply    # write
```

## When it fails

The bot's comment on the issue is the diagnosis: it says which part it could not do, and the run's
log is under Actions → Claim. Nothing else has to be repaired, because the bot keeps no state of
its own — a wrong card is fixed by hand on the board. Deleting the secret turns the bot off
completely: claims then work as plain comments, exactly as they did before it existed.

The open pull request check is a courtesy, not a lock: anyone who opens a pull request, a draft
included, that says `Closes #N` makes the bot refuse claims of that issue until it is closed. A
maintainer who finds such a pull request abandoned closes it, or moves the card by hand.

## How it is checked

```sh
python scripts/claim_bot_test.py
python scripts/depends_on_test.py
python -m unittest discover -s scripts -p 'test_project_sync.py'
python -m unittest discover -s scripts -p 'test_pr_workflows.py'
```

The first runs the workflow's script against mocked API clients, because a wrong branch would write
to the board instead of failing a test: every command is answered for each state a card can be in
(free, without a status, held, taken, in review, absent from the board), for `/unclaim` by the
claimer and by a maintainer, for `/claim @login` by a maintainer (for someone new, for someone
already assigned, on a held or taken card) and by a contributor, and for the mistakes the bot is meant to catch (an open pull request, an
`epic`, a status the field does not have, a missing token, a `/hold` written by someone who may not
hold, a name that is not a GitHub login, assignees newer than the event), for an issue blocked by
an open, a closed and another repository's issue, and for a `Touches:` line that overlaps an open
pull request, another claim, a claim taken back, or nothing, and one whose paths are not plain. It needs Node.js on the
PATH — the CI `docs` job runs it on every change — writes its two files under `target/claim-bot-test/`
and removes them again.

The second runs the Depends on check's script the same way, for each way of writing a dependency
and each state it can be in, on a pull request and in the merge queue.

The third, which the CI `docs` job runs with the other script tests, runs the project sync against
recorded GraphQL answers, with no network: every row of the table above, refused commands, paging,
a dry run that writes nothing, and a missing Status option that stops the run before its first
write.

The PR workflow tests run the Overlap script against mocked API clients, including renamed files
on either side and a failed file lookup followed by another overlapping pull request. They also
run the CI DCO shell step in a temporary Git repository with upstream branches and a synthetic PR
merge ref: upstream commits are exempt, but unsigned fork commits fail even when stacked on an
upstream branch. These tests need Git, Bash and Node.js and run in the CI `docs` job.
