---
name: issue-sweep
description: Go through the open issues of suhina-daw/suhina-daw, oldest first, and find the ones the code on main already does, that a merged pull request closed without saying so, that duplicate another issue or that a later decision made obsolete; report each with its evidence and close only the ones the maintainer approves. Use when asked to clean up, sweep, triage or go through the issue list, to find stale or already-done issues, or to check whether an issue is still relevant.
---

An open issue is the only context an agent gets apart from the documents
([`docs/workflow.md`](../../../docs/workflow.md) › Issues), so one that is already done sends the
next agent to build it twice. This sweep finds those. It reads and reports; it changes nothing on
GitHub until the maintainer says which issues to close.

The same steps work locally and in a cloud session. Where `gh` is missing or not signed in, use
the GitHub tools the session has for the same reads; if nothing can close an issue, end with the
`gh` commands for the maintainer to run.

## 1. The starting point

- Compare against `origin/main`, never a local `main`, which can lag by days:
  `git fetch origin` and read files with `git show origin/main:PATH` or a worktree on it.
- List the open issues oldest first, without pull requests:
  `gh issue list --repo suhina-daw/suhina-daw --state open --limit 500 --json number,title,labels,createdAt --jq 'sort_by(.number)[] | "\(.number)\t\(.title)"'`.
- A sweep that was started before continues after the last number its report reached; ask for it
  if the conversation does not say.

## 2. One issue at a time

For each issue, in number order:

1. **Read all of it:** `gh issue view N --repo suhina-daw/suhina-daw --comments`. The acceptance
   criteria decide, not the title. An `epic` is done only when its child issues are closed and
   its own criteria hold.
2. **Find the pull requests that touched it:**
   `gh pr list --repo suhina-daw/suhina-daw --state all --search "N in:body" --json number,title,state,mergedAt`
   and the issue's timeline (`gh api repos/suhina-daw/suhina-daw/issues/N/timeline`, events
   `cross-referenced` and `connected`). A merged pull request that says `Part of #N`, `Refs #N`
   or names the issue's old `(LOOP-…)` key often did the work without closing it.
3. **Look in the code:** `python scripts/docs.py find WORDS` for the documents, `git grep` on
   `origin/main` for the command id, setting, device parameter, file format or function the issue
   asks for, `docs/works-now/` and `ROADMAP.md` for what is listed as working, and
   `docs/decisions/` for a decision that changed the plan.
4. **Check each criterion** against what you found: the file and line that does it, or the test
   that shows it. A name that exists is not proof that the behaviour does.

## 3. The verdict

Give each issue one of these, with the evidence on one line:

| Verdict | When | Evidence |
|---|---|---|
| **Done** | every criterion is met on `origin/main` | the merged pull request and the file or test for each criterion |
| **Partly done** | some criteria are met | what is done, what is left; propose rewriting the issue to the part that is left |
| **Duplicate** | another issue covers the same problem | the other issue, and which one keeps the better description |
| **Obsolete** | a decision, a redesign or a removed feature made it moot | the ADR, the pull request or the commit |
| **Still open** | nothing does it yet | nothing more; do not explain open issues at length |
| **Unsure** | the evidence does not settle it, or it needs the running app | what would settle it |

Leave an issue alone, and say so, when an open pull request says `Closes #N` (merging it closes
the issue) or when it is assigned and claimed: that is someone's work in progress.

## 4. Reporting and closing

- Report in batches of about twenty issues, a table of the issues that are not **Still open**
  plus a count of the open ones, and the last number checked so the sweep can continue. Write the
  report in the language the maintainer uses in the chat.
- Close only the issues the maintainer names. Each close gets a comment in English, one line per
  paragraph (no hard wraps), that says what does it and where, for example
  `Done on main by #812: \`crates/session/src/commands/clip.rs\` (clip.duplicate) and its test in \`crates/session/tests/clip.rs\`.`
  Then `gh issue close N --repo suhina-daw/suhina-daw --reason completed` for **Done**,
  `--reason "not planned"` for **Obsolete** and `--duplicate-of M` for **Duplicate**.
- For **Partly done**, edit the issue's body to the part that is left only when the maintainer
  agrees, and keep the done criteria as ticked boxes so the history stays readable.
- No comment names another DAW, and none links a private agent session
  ([`AGENTS.md`](../../../AGENTS.md) rules 9 and 11).
