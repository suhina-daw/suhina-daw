# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Check the claim bot: the script `.github/workflows/claim.yml` runs.

The bot moves issue cards on the project board, so a wrong branch would write to the board instead
of failing a test. This runs the script with mocked API clients instead: every command
(`/claim`, `/claim @login`, `/unclaim`, `/hold`, `/ready`) is answered for each state a card can be in — free, held,
taken, closed, blocked by another issue, absent from the board — for the mistakes the bot is meant to
catch (an open pull request, an epic, a status the field does not have, a missing token), and for the
other open work a claim's `Touches:` line overlaps.

    python scripts/claim_bot_test.py

Needs Node.js on the PATH, which the GitHub runner has. The two files it runs are written under
`target/claim-bot-test/` and removed afterwards; nothing else is touched.
"""

from __future__ import annotations

import pathlib
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
WORKFLOW = ROOT / ".github" / "workflows" / "claim.yml"
# Under `target/`, which the build already ignores: a system temporary directory is not writable
# everywhere this runs.
WORK = ROOT / "target" / "claim-bot-test"

# The harness: `claim-bot.js` is the workflow's script, wrapped and exported by this test.
HARNESS = r'''// Runs the claim bot's script against mocked API clients. Written by scripts/claim_bot_test.py.
const main = require("./claim-bot.js");

const OPTIONS = [
  { id: "opt-backlog", name: "Backlog" },
  { id: "opt-ready", name: "Ready" },
  { id: "opt-progress", name: "In progress" },
  { id: "opt-review", name: "In review" },
  { id: "opt-hold", name: "Do not start yet" },
  { id: "opt-done", name: "Done" },
];

function scenario(input) {
  const state = { moved: undefined, added: false, assigned: [], unassigned: [], comments: [] };
  const board = input.board || {};
  const graphql = async (query, vars) => {
    // The claimer's open issues, for the limit on claims without a pull request.
    if (query.includes("filterBy")) {
      if (input.openClaims === "fail") throw new Error("issues are not answered");
      return {
        repository: {
          issues: {
            nodes: (input.openClaims || []).map((other) => ({
              number: other.number,
              title: other.title || `Issue ${other.number}`,
              closedByPullRequestsReferences: { totalCount: other.prs || 0 },
            })),
          },
        },
        // Their open pull requests, whose `Closes #N` counts when GitHub made no link (stacked).
        search: { nodes: (input.ownPulls || []).map((body) => ({ body })) },
      };
    }
    if (query.includes("organization(login")) {
      return {
        organization: {
          projectV2: {
            id: "PVT_1",
            field: {
              id: "PVTSSF_1",
              options: board.renamed
                ? OPTIONS.filter((option) => option.name !== "In progress")
                : OPTIONS,
            },
          },
        },
        node: {
          projectItems: {
            nodes:
              board.onBoard === false
                ? []
                : [
                    {
                      id: "ITEM_1",
                      project: { id: "PVT_1" },
                      fieldValueByName: board.status ? { name: board.status } : null,
                    },
                  ],
          },
        },
      };
    }
    if (query.includes("closedByPullRequestsReferences")) {
      return { node: { closedByPullRequestsReferences: { nodes: board.openPrs || [] } } };
    }
    if (query.includes("blockedBy(")) {
      if (board.blockedBy === "fail") throw new Error("blockedBy is not answered");
      return { node: { blockedBy: { nodes: board.blockedBy || [] } } };
    }
    if (query.includes("pullRequests(states")) {
      state.readPulls = true;
      if (input.pulls === "fail") throw new Error("pull requests are not answered");
      return {
        repository: {
          pullRequests: {
            nodes: (input.pulls || []).map((pull) => ({
              number: pull.number,
              title: pull.title,
              files: { nodes: pull.files.map((path) => ({ path })) },
              closingIssuesReferences: { nodes: (pull.closes || []).map((number) => ({ number })) },
            })),
          },
        },
      };
    }
    if (query.includes("addProjectV2ItemById")) {
      state.added = true;
      return { addProjectV2ItemById: { item: { id: "ITEM_NEW" } } };
    }
    if (query.includes("updateProjectV2ItemFieldValue")) {
      state.moved = vars.option;
      return { updateProjectV2ItemFieldValue: { projectV2Item: { id: vars.item } } };
    }
    throw new Error(`unexpected query: ${query.slice(0, 60)}`);
  };

  process.env.PROJECT_OWNER = "suhina-daw";
  process.env.PROJECT_NUMBER = "1";
  process.env.PROJECT_TOKEN = input.token === undefined ? "token" : input.token;
  // `boss` is always a maintainer; the commenter, `someone`, is one with role "maintainer" and a
  // trusted contributor with role "trusted". GitHub's author association no longer decides either.
  process.env.MAINTAINERS = input.role === "maintainer" ? "boss, someone" : "boss";
  process.env.TRUSTED_CONTRIBUTORS = input.role === "trusted" ? "other @Someone" : "other";
  global.getOctokit = () => ({ graphql });
  global.core = { warning: () => {}, setFailed: (message) => (state.failed = message) };
  global.context = {
    repo: { owner: "suhina-daw", repo: "suhina-daw" },
    serverUrl: "https://github.com",
    payload: {
      issue: {
        number: 42,
        node_id: "I_42",
        labels: (input.issue && input.issue.labels) || [],
        // The event's copy, which can be older than the issue (`stale`).
        assignees: (input.stale || input.assignees || []).map((login) => ({ login })),
      },
      comment: {
        body: input.body,
        author_association: input.association || "NONE",
        user: { login: "someone" },
      },
    },
  };
  // A comment is its text, written by a maintainer, or { body, by } for anyone else (`by` is
  // their author association, which no longer makes anyone a maintainer); { bot, hoursAgo }
  // makes it the bot's own comment, written that many hours ago. Other issues' comments come from
  // `claims`: { number, title, assignees, comments }.
  const claims = input.claims || [];
  const listComments = async (args) => ({
    data: (args.issue_number === 42
      ? (input.issue && input.issue.comments) || []
      : (claims.find((other) => other.number === args.issue_number) || { comments: [] }).comments
    ).map((entry) =>
      typeof entry === "string"
        ? { body: entry, author_association: "MEMBER", user: { login: "boss", type: "User" } }
        : {
            body: entry.body,
            author_association: entry.bot ? "NONE" : entry.by || "COLLABORATOR",
            user: entry.bot ? { login: "github-actions[bot]", type: "Bot" } : { login: "stranger", type: "User" },
            created_at: new Date(Date.now() - (entry.hoursAgo || 0) * 3600 * 1000).toISOString(),
          },
    ),
  });
  global.github = {
    paginate: async (method, args) => (await method(args)).data,
    rest: {
      issues: {
        createComment: async (args) => state.comments.push(args.body),
        get: async () => ({
          data: {
            number: 42,
            labels: (input.issue && input.issue.labels) || [],
            assignees: (input.assignees || []).map((login) => ({ login })),
          },
        }),
        listComments,
        // The open issues with someone assigned: the claims.
        listForRepo: async () => ({
          data: claims
            .filter((other) => (other.assignees || []).length > 0)
            .map((other) => ({
              number: other.number,
              title: other.title,
              assignees: other.assignees.map((login) => ({ login })),
            })),
        }),
        // GitHub answers with the issue, and silently leaves out anyone it may not assign.
        addAssignees: async (args) => {
          const taken = args.assignees.filter((login) => !(input.unassignable || []).includes(login));
          state.assigned.push(...taken);
          const all = [...(input.assignees || []), ...taken];
          return { data: { assignees: all.map((login) => ({ login })) } };
        },
        removeAssignees: async (args) => state.unassigned.push(...args.assignees),
      },
    },
  };
  return { run: main, state };
}

const says = (state, ...parts) => state.comments.some((c) => parts.every((part) => c.includes(part)));

const cases = [
  {
    name: "claim on a free card moves it and assigns the claimer",
    input: { body: "/claim Plan: fix the wheel routing", board: { status: "Ready" } },
    expect: (s) => s.state.moved === "opt-progress" && says(s.state, "Claimed by") && s.state.assigned.includes("someone"),
  },
  {
    name: "a card in Backlog is free as well",
    input: { body: "/claim", board: { status: "Backlog" } },
    expect: (s) => s.state.moved === "opt-progress",
  },
  {
    name: "claim on an issue that is not on the board adds it first",
    input: { body: "/claim", board: { onBoard: false } },
    expect: (s) => s.state.added && s.state.moved === "opt-progress",
  },
  {
    name: "claim while someone else works is refused",
    input: { body: "/claim", board: { status: "In progress" }, assignees: ["other"] },
    expect: (s) => says(s.state, "already **In progress** (@other)") && !s.state.moved,
  },
  {
    name: "a card in review counts as someone working on it",
    input: { body: "/claim", board: { status: "In review" }, assignees: ["other"] },
    expect: (s) => says(s.state, "already **In review** (@other)") && !s.state.moved,
  },
  {
    name: "claim by the person who already has it is named as theirs",
    input: { body: "/claim", board: { status: "In progress" }, assignees: ["someone"] },
    expect: (s) => says(s.state, "already yours") && !s.state.moved,
  },
  {
    name: "claim by the person who has it names their open pull request",
    input: {
      body: "/claim",
      board: { status: "In progress", openPrs: [{ number: 239, title: "Fix", url: "u" }] },
      assignees: ["someone"],
    },
    expect: (s) => says(s.state, "already yours", "#239", "Continue there") && !s.state.moved,
  },
  {
    name: "a claim with no pull request after 48 hours passes to the next claimer",
    input: {
      body: "/claim",
      board: { status: "In progress" },
      assignees: ["other"],
      issue: { comments: [{ body: "Claimed by @other: **In progress**", bot: true, hoursAgo: 49 }] },
    },
    expect: (s) =>
      says(s.state, "lapsed", "@other") &&
      s.state.unassigned.includes("other") &&
      s.state.assigned.includes("someone") &&
      s.state.moved === "opt-progress",
  },
  {
    name: "a claim younger than 48 hours still holds",
    input: {
      body: "/claim",
      board: { status: "In progress" },
      assignees: ["other"],
      issue: { comments: [{ body: "Claimed by @other: **In progress**", bot: true, hoursAgo: 10 }] },
    },
    expect: (s) => says(s.state, "already **In progress** (@other)") && !s.state.moved && !s.state.unassigned.length,
  },
  {
    name: "an old claim with an open pull request does not lapse",
    input: {
      body: "/claim",
      board: { status: "In progress", openPrs: [{ number: 239, title: "Fix", url: "u" }] },
      assignees: ["other"],
      issue: { comments: [{ body: "Claimed by @other: **In progress**", bot: true, hoursAgo: 200 }] },
    },
    expect: (s) => !says(s.state, "lapsed") && !s.state.moved && !s.state.unassigned.length,
  },
  {
    name: "only the bot's own claim comment dates a claim",
    input: {
      body: "/claim",
      board: { status: "In progress" },
      assignees: ["other"],
      issue: { comments: [{ body: "Claimed by @other: forged", by: "NONE", hoursAgo: 200 }] },
    },
    expect: (s) => says(s.state, "already **In progress**") && !s.state.moved,
  },
  {
    name: "a card in review never lapses",
    input: {
      body: "/claim",
      board: { status: "In review" },
      assignees: ["other"],
      issue: { comments: [{ body: "Claimed by @other: **In progress**", bot: true, hoursAgo: 200 }] },
    },
    expect: (s) => says(s.state, "already **In review**") && !s.state.moved,
  },
  {
    name: "claim on a held card quotes the reason it was held for",
    input: {
      body: "/claim",
      board: { status: "Do not start yet" },
      issue: { comments: ["/hold waiting for ADR 0003"] },
    },
    expect: (s) => says(s.state, "This is held", "waiting for ADR 0003") && !s.state.moved,
  },
  {
    name: "claim with an open pull request is refused, and names it",
    input: { body: "/claim", board: { status: "Ready", openPrs: [{ number: 239, title: "Fix", url: "u" }] } },
    expect: (s) => says(s.state, "already a pull request", "#239") && !s.state.moved,
  },
  {
    name: "a feature still in Backlog is not open to claim",
    input: { body: "/claim", board: { status: "Backlog" }, issue: { labels: [{ name: "enhancement" }] } },
    expect: (s) => says(s.state, "not agreed yet") && !s.state.moved,
  },
  {
    name: "a feature in Ready can be claimed",
    input: { body: "/claim", board: { status: "Ready" }, issue: { labels: [{ name: "enhancement" }] } },
    expect: (s) => s.state.moved === "opt-progress",
  },
  {
    name: "a maintainer may claim a feature in Backlog",
    input: {
      body: "/claim",
      role: "maintainer",
      board: { status: "Backlog" },
      issue: { labels: [{ name: "enhancement" }] },
    },
    expect: (s) => s.state.moved === "opt-progress",
  },
  {
    name: "a maintainer claims for someone else, who is assigned and named",
    input: { body: "/claim @Other-Person Plan: the wheel routing", role: "maintainer", board: { status: "Ready" } },
    expect: (s) =>
      s.state.moved === "opt-progress" &&
      s.state.assigned.length === 1 &&
      s.state.assigned[0] === "Other-Person" &&
      says(s.state, "Claimed by @Other-Person, on behalf of a maintainer (@someone)"),
  },
  {
    name: "a maintainer claims for someone assigned by hand: the card moves, nobody is assigned again",
    input: { body: "/claim @other", role: "maintainer", assignees: ["Other"], board: { status: "Ready" } },
    expect: (s) =>
      s.state.moved === "opt-progress" && s.state.assigned.length === 0 && says(s.state, "Claimed by @Other, on behalf"),
  },
  {
    name: "a maintainer may claim a feature in Backlog for someone",
    input: {
      body: "/claim @other",
      role: "maintainer",
      board: { status: "Backlog" },
      issue: { labels: [{ name: "enhancement" }] },
    },
    expect: (s) => s.state.moved === "opt-progress" && s.state.assigned.includes("other"),
  },
  {
    name: "a maintainer's claim for someone already In progress for them changes nothing",
    input: { body: "/claim @other", role: "maintainer", assignees: ["other"], board: { status: "In progress" } },
    expect: (s) => says(s.state, "already **In progress** for @other") && !s.state.moved && !s.state.assigned.length,
  },
  {
    name: "a contributor may not claim for someone else",
    input: { body: "/claim @other Plan: …", board: { status: "Ready" } },
    expect: (s) => says(s.state, "Only a maintainer can claim an issue for someone else") && !s.state.moved && !s.state.assigned.length,
  },
  {
    name: "a contributor naming themselves makes an ordinary claim",
    input: { body: "/claim @Someone Plan: …", board: { status: "Ready" } },
    expect: (s) => s.state.moved === "opt-progress" && says(s.state, "Claimed by @someone:") && !says(s.state, "on behalf"),
  },
  {
    name: "a name that is not a GitHub login is refused and not quoted",
    input: { body: "/claim @bad$(name) @everyone", role: "maintainer", board: { status: "Ready" } },
    expect: (s) =>
      says(s.state, "not a GitHub username") && !says(s.state, "bad$") && !s.state.moved && !s.state.assigned.length,
  },
  {
    name: "a login with a double hyphen is refused",
    input: { body: `/claim @a--b`, role: "maintainer", board: { status: "Ready" } },
    expect: (s) => says(s.state, "not a GitHub username") && !s.state.moved,
  },
  {
    name: "a login longer than 39 characters is refused",
    input: { body: `/claim @${"a".repeat(40)}`, role: "maintainer", board: { status: "Ready" } },
    expect: (s) => says(s.state, "not a GitHub username") && !s.state.moved,
  },
  {
    name: "a trailing comma after the name is not part of it",
    input: { body: "/claim @other, please take this", role: "maintainer", board: { status: "Ready" } },
    expect: (s) => s.state.assigned.includes("other") && s.state.moved === "opt-progress",
  },
  {
    name: "a maintainer's claim for someone on a held card is refused",
    input: {
      body: "/claim @other",
      role: "maintainer",
      board: { status: "Do not start yet" },
      issue: { comments: ["/hold waiting for ADR 0003"] },
    },
    expect: (s) => says(s.state, "This is held", "waiting for ADR 0003") && !s.state.moved && !s.state.assigned.length,
  },
  {
    name: "a maintainer's claim for someone on a taken card is refused",
    input: { body: "/claim @other", role: "maintainer", assignees: ["third"], board: { status: "In progress" } },
    expect: (s) => says(s.state, "already **In progress** (@third)") && !s.state.moved && !s.state.assigned.length,
  },
  {
    name: "a maintainer's claim for someone is refused when a pull request closes the issue",
    input: {
      body: "/claim @other",
      role: "maintainer",
      board: { status: "Ready", openPrs: [{ number: 239, title: "Fix", url: "u" }] },
    },
    expect: (s) => says(s.state, "already a pull request", "#239") && !s.state.moved,
  },
  {
    name: "a maintainer's claim for someone on an epic is refused",
    input: { body: "/claim @other", role: "maintainer", board: { status: "Ready" }, issue: { labels: [{ name: "epic" }] } },
    expect: (s) => says(s.state, "`epic`") && !s.state.moved,
  },
  {
    name: "a lapsed claim passes to the person a maintainer names",
    input: {
      body: "/claim @other",
      role: "maintainer",
      board: { status: "In progress" },
      assignees: ["third"],
      issue: { comments: [{ body: "Claimed by @third: **In progress**", bot: true, hoursAgo: 49 }] },
    },
    expect: (s) =>
      says(s.state, "lapsed", "passes to @other") &&
      s.state.unassigned.includes("third") &&
      s.state.assigned.includes("other") &&
      s.state.moved === "opt-progress",
  },
  {
    name: "someone GitHub will not assign is named in the answer",
    input: { body: "/claim @ghost", role: "maintainer", unassignable: ["ghost"], board: { status: "Ready" } },
    expect: (s) => s.state.moved === "opt-progress" && says(s.state, "Claimed by @ghost", "GitHub did not assign @ghost"),
  },
  {
    name: "/unclaim ignores a name after it",
    input: { body: "/unclaim @other", assignees: ["someone"], board: { status: "In progress" } },
    expect: (s) => s.state.moved === "opt-ready" && s.state.unassigned.includes("someone"),
  },
  {
    name: "claim on an epic is refused",
    input: { body: "/claim", board: { status: "Ready" }, issue: { labels: [{ name: "epic" }] } },
    expect: (s) => says(s.state, "`epic`") && !s.state.moved,
  },
  {
    name: "hold by a contributor is refused",
    input: { body: "/hold because I say so", board: { status: "Ready" } },
    expect: (s) => says(s.state, "Only a maintainer") && !s.state.moved,
  },
  {
    name: "hold without a reason is refused",
    input: { body: "/hold", role: "maintainer", board: { status: "Ready" } },
    expect: (s) => says(s.state, "Write the reason") && !s.state.moved,
  },
  {
    name: "hold with a reason gates the card",
    input: { body: "/hold\nwaiting for ADR 0003", role: "maintainer", board: { status: "Ready" } },
    expect: (s) => s.state.moved === "opt-hold" && says(s.state, "Held"),
  },
  {
    name: "ready opens a held card again",
    input: { body: "/ready", role: "maintainer", board: { status: "Do not start yet" } },
    expect: (s) => s.state.moved === "opt-ready" && says(s.state, "Released"),
  },
  {
    name: "unclaim by the assignee releases and unassigns",
    input: { body: "/unclaim", assignees: ["someone"], board: { status: "In progress" } },
    expect: (s) => s.state.moved === "opt-ready" && s.state.unassigned.includes("someone"),
  },
  {
    name: "unclaim by someone else is refused",
    input: { body: "/unclaim", assignees: ["other"], board: { status: "In progress" } },
    expect: (s) => says(s.state, "Only the person who claimed") && !s.state.moved,
  },
  {
    name: "unclaim on a held card unassigns but keeps the hold",
    input: { body: "/unclaim", assignees: ["someone"], board: { status: "Do not start yet" } },
    expect: (s) => !s.state.moved && s.state.unassigned.includes("someone") && says(s.state, "stays **Do not start yet**"),
  },
  {
    name: "unclaim on a card in review unassigns but keeps it in review",
    input: { body: "/unclaim", assignees: ["someone"], board: { status: "In review" } },
    expect: (s) => !s.state.moved && says(s.state, "stays **In review**"),
  },
  {
    name: "unclaim by a maintainer releases the person who claimed it",
    input: { body: "/unclaim", role: "maintainer", assignees: ["other"], board: { status: "In progress" } },
    expect: (s) => s.state.moved === "opt-ready" && s.state.unassigned.includes("other") && !s.state.unassigned.includes("someone"),
  },
  {
    name: "unclaim on a free card with nobody assigned says it is free",
    input: { body: "/unclaim", role: "maintainer", board: { status: "Ready" } },
    expect: (s) => says(s.state, "already free") && !s.state.moved,
  },
  {
    name: "unclaim reads the assignees again, not the event's old copy",
    input: { body: "/unclaim", stale: [], assignees: ["someone"], board: { status: "In progress" } },
    expect: (s) => s.state.moved === "opt-ready" && s.state.unassigned.includes("someone"),
  },
  {
    name: "a card without a status can be claimed",
    input: { body: "/claim", board: {} },
    expect: (s) => s.state.moved === "opt-progress" && !says(s.state, "null"),
  },
  {
    name: "a hold reason is quoted only from a maintainer, without mentions",
    input: {
      body: "/claim",
      board: { status: "Do not start yet" },
      issue: {
        comments: [
          "/hold waiting for @someone and ADR 0003",
          { body: "/hold @everyone go to evil.example", by: "NONE" },
        ],
      },
    },
    expect: (s) =>
      says(s.state, "waiting for someone and ADR 0003") && !says(s.state, "evil.example") && !s.state.moved,
  },
  {
    name: "a comment that only mentions a command is ignored",
    input: { body: "I think /claim would be nice here", board: { status: "Ready" } },
    expect: (s) => s.state.comments.length === 0 && !s.state.moved,
  },
  {
    name: "claim on an issue blocked by an open issue is refused, and names it",
    input: {
      body: "/claim",
      board: { status: "Ready", blockedBy: [{ number: 7, title: "Model field", state: "OPEN" }] },
    },
    expect: (s) => says(s.state, "blocked by", "#7 Model field") && !s.state.moved && !s.state.assigned.length,
  },
  {
    name: "a blocker that is closed no longer blocks",
    input: {
      body: "/claim",
      board: { status: "Ready", blockedBy: [{ number: 7, title: "Model field", state: "CLOSED" }] },
    },
    expect: (s) => s.state.moved === "opt-progress" && !says(s.state, "blocked by"),
  },
  {
    name: "a blocker in another repository is named with its repository",
    input: {
      body: "/claim",
      board: {
        status: "Ready",
        blockedBy: [{ number: 3, title: "Site", state: "OPEN", repository: { nameWithOwner: "suhina-daw/website" } }],
      },
    },
    expect: (s) => says(s.state, "suhina-daw/website#3 Site") && !s.state.moved,
  },
  {
    name: "a maintainer's claim for someone on a blocked issue is refused",
    input: {
      body: "/claim @other",
      role: "maintainer",
      board: { status: "Ready", blockedBy: [{ number: 7, title: "Model field", state: "OPEN" }] },
    },
    expect: (s) => says(s.state, "blocked by", "#7") && !s.state.moved && !s.state.assigned.length,
  },
  {
    name: "the person who has a blocked issue already is still pointed at their work",
    input: {
      body: "/claim",
      board: { status: "In progress", blockedBy: [{ number: 7, title: "Model field", state: "OPEN" }] },
      assignees: ["someone"],
    },
    expect: (s) => says(s.state, "already yours") && !says(s.state, "blocked by"),
  },
  {
    name: "a blocked-by lookup that fails does not stop the claim",
    input: { body: "/claim", board: { status: "Ready", blockedBy: "fail" } },
    expect: (s) => s.state.moved === "opt-progress",
  },
  {
    name: "a Touches line lists an open pull request that changes the same files",
    input: {
      body: "/claim Plan: the grid\nTouches: crates/ui/src/arrangement/",
      board: { status: "Ready" },
      pulls: [
        { number: 302, title: "Clips package", files: ["crates/ui/src/arrangement/grid.rs", "ROADMAP.md"] },
        { number: 303, title: "Engine only", files: ["crates/engine/src/lib.rs"] },
      ],
    },
    expect: (s) =>
      s.state.moved === "opt-progress" &&
      says(s.state, "Claimed by", "Other open work touches", "#302 Clips package", "crates/ui/src/arrangement/grid.rs") &&
      !says(s.state, "#303") &&
      !says(s.state, "ROADMAP.md"),
  },
  {
    name: "a Touches line after the plan on the same line ends before When",
    input: {
      body: "/claim Plan: grid Touches: `crates/session`, crates/ui/** When: today",
      board: { status: "Ready" },
      pulls: [{ number: 310, title: "Undo", files: ["crates/session/src/undo.rs"] }],
    },
    expect: (s) => says(s.state, "#310 Undo") && !says(s.state, "today`"),
  },
  {
    name: "another claim on the same code is listed",
    input: {
      body: "/claim\nTouches: crates/ui",
      board: { status: "Ready" },
      claims: [
        { number: 50, title: "Piano roll", assignees: ["other"], comments: ["/claim Plan: x\nTouches: crates/ui/src/piano_roll"] },
        { number: 51, title: "Engine", assignees: ["third"], comments: ["/claim\nTouches: crates/engine"] },
      ],
    },
    expect: (s) => says(s.state, "#50 Piano roll (a claim)", "crates/ui/src/piano_roll") && !says(s.state, "#51"),
  },
  {
    name: "a claim taken back with /unclaim is not listed",
    input: {
      body: "/claim\nTouches: crates/ui",
      board: { status: "Ready" },
      claims: [{ number: 50, title: "Piano roll", assignees: ["other"], comments: ["/claim\nTouches: crates/ui", "/unclaim"] }],
    },
    expect: (s) => says(s.state, "No other open pull request or claim touches `crates/ui`") && !says(s.state, "#50"),
  },
  {
    name: "a pull request that closes this issue is not other work",
    input: {
      body: "/claim\nTouches: crates/ui",
      board: { status: "Ready" },
      pulls: [{ number: 320, title: "Mine", files: ["crates/ui/src/lib.rs"], closes: [42] }],
    },
    expect: (s) => s.state.moved === "opt-progress" && !says(s.state, "#320"),
  },
  {
    name: "a claim without a Touches line is told about it and reads no pull requests",
    input: { body: "/claim Plan: x", board: { status: "Ready" }, pulls: [{ number: 1, title: "A", files: ["x"] }] },
    expect: (s) => s.state.moved === "opt-progress" && says(s.state, "A `Touches:` line") && !s.state.readPulls,
  },
  {
    name: "paths that are not plain relative paths are dropped from a Touches line",
    input: {
      body: "/claim\nTouches: ../secret, $(rm -rf), /etc, crates/./ui",
      board: { status: "Ready" },
      pulls: [{ number: 1, title: "A", files: ["crates/ui/x.rs"] }],
    },
    expect: (s) => s.state.moved === "opt-progress" && says(s.state, "A `Touches:` line") && !s.state.readPulls,
  },
  {
    name: "a failed pull request lookup still answers the claim",
    input: { body: "/claim\nTouches: crates/ui", board: { status: "Ready" }, pulls: "fail" },
    expect: (s) => s.state.moved === "opt-progress" && says(s.state, "Claimed by", "No other open"),
  },
  {
    name: "a collaborator GitHub calls COLLABORATOR is not a maintainer for that",
    input: { body: "/hold waiting", association: "COLLABORATOR", board: { status: "Ready" } },
    expect: (s) => says(s.state, "Only a maintainer") && !s.state.moved,
  },
  {
    name: "a collaborator's /ready on a feature in Backlog is refused",
    input: { body: "/ready", association: "COLLABORATOR", board: { status: "Backlog" } },
    expect: (s) => says(s.state, "Only a maintainer") && !s.state.moved,
  },
  {
    name: "a trusted contributor may claim a feature in Backlog",
    input: { body: "/claim", role: "trusted", board: { status: "Backlog" }, issue: { labels: [{ name: "enhancement" }] } },
    expect: (s) => s.state.moved === "opt-progress" && s.state.assigned.includes("someone"),
  },
  {
    name: "a trusted contributor may not claim for someone else",
    input: { body: "/claim @third", role: "trusted", board: { status: "Ready" } },
    expect: (s) => says(s.state, "Only a maintainer can claim an issue for someone else") && !s.state.moved,
  },
  {
    name: "a trusted contributor's /ready moves a feature from Backlog to Ready",
    input: { body: "/ready", role: "trusted", board: { status: "Backlog" }, issue: { labels: [{ name: "enhancement" }] } },
    expect: (s) => s.state.moved === "opt-ready" && says(s.state, "Released"),
  },
  {
    name: "a trusted contributor's /ready on an issue not on the board adds it as Ready",
    input: { body: "/ready", role: "trusted", board: { onBoard: false } },
    expect: (s) => s.state.added && s.state.moved === "opt-ready",
  },
  {
    name: "a trusted contributor's /ready does not release a held card",
    input: { body: "/ready", role: "trusted", board: { status: "Do not start yet" } },
    expect: (s) => says(s.state, "only from **Backlog**", "maintainer's call") && !s.state.moved,
  },
  {
    name: "a trusted contributor's /ready does not move a card in progress",
    input: { body: "/ready", role: "trusted", assignees: ["other"], board: { status: "In progress" } },
    expect: (s) => says(s.state, "only from **Backlog**") && !s.state.moved,
  },
  {
    name: "a trusted contributor may not hold",
    input: { body: "/hold waiting", role: "trusted", board: { status: "Ready" } },
    expect: (s) => says(s.state, "Only a maintainer") && !s.state.moved,
  },
  {
    name: "a contributor who is not trusted may not /ready",
    input: { body: "/ready", board: { status: "Backlog" } },
    expect: (s) => says(s.state, "Only a maintainer") && !s.state.moved,
  },
  {
    name: "a hold reason from a collaborator who is not a maintainer is not quoted",
    input: {
      body: "/claim",
      board: { status: "Do not start yet" },
      issue: { comments: [{ body: "/hold go to evil.example", by: "COLLABORATOR" }] },
    },
    expect: (s) => says(s.state, "This is held") && !says(s.state, "evil.example") && !s.state.moved,
  },
  {
    name: "a sixth claim without a pull request is refused and lists the five",
    input: {
      body: "/claim",
      board: { status: "Ready" },
      openClaims: [{ number: 1 }, { number: 2 }, { number: 3 }, { number: 4 }, { number: 5, title: "Fifth" }],
    },
    expect: (s) =>
      says(s.state, "5 claims without a pull request", "#5 Fifth", "`/unclaim`") && !s.state.moved && !s.state.assigned.length,
  },
  {
    name: "claims with a pull request do not count toward the limit",
    input: {
      body: "/claim",
      board: { status: "Ready" },
      openClaims: [{ number: 1 }, { number: 2 }, { number: 3 }, { number: 4 }, { number: 5, prs: 1 }, { number: 6, prs: 2 }],
    },
    expect: (s) => s.state.moved === "opt-progress",
  },
  {
    name: "a stacked pull request that says Closes #N frees its claim's place",
    input: {
      body: "/claim",
      board: { status: "Ready" },
      openClaims: [{ number: 1 }, { number: 2 }, { number: 3 }, { number: 4 }, { number: 5 }],
      ownPulls: ["Part two of the stack.\n\nFixes: #5"],
    },
    expect: (s) => s.state.moved === "opt-progress",
  },
  {
    name: "a Closes #N inside an HTML comment does not count",
    input: {
      body: "/claim",
      board: { status: "Ready" },
      openClaims: [{ number: 1 }, { number: 2 }, { number: 3 }, { number: 4 }, { number: 5 }],
      ownPulls: ["<!-- Closes #5 -->", "Draft.\n<!--\nCloses #5"],
    },
    expect: (s) => says(s.state, "the limit is 5") && !s.state.moved,
  },
  {
    name: "this issue itself does not count toward the limit",
    input: {
      body: "/claim",
      board: { status: "Ready" },
      openClaims: [{ number: 1 }, { number: 2 }, { number: 3 }, { number: 4 }, { number: 42 }],
    },
    expect: (s) => s.state.moved === "opt-progress",
  },
  {
    name: "a trusted contributor has the same limit",
    input: {
      body: "/claim",
      role: "trusted",
      board: { status: "Ready" },
      openClaims: [{ number: 1 }, { number: 2 }, { number: 3 }, { number: 4 }, { number: 5 }],
    },
    expect: (s) => says(s.state, "the limit is 5") && !s.state.moved,
  },
  {
    name: "a maintainer has no limit",
    input: {
      body: "/claim",
      role: "maintainer",
      board: { status: "Ready" },
      openClaims: [{ number: 1 }, { number: 2 }, { number: 3 }, { number: 4 }, { number: 5 }, { number: 6 }],
    },
    expect: (s) => s.state.moved === "opt-progress",
  },
  {
    name: "a failed count of open claims lets the claim go ahead",
    input: { body: "/claim", board: { status: "Ready" }, openClaims: "fail" },
    expect: (s) => s.state.moved === "opt-progress",
  },
  {
    name: "a missing token is announced instead of failing silently",
    input: { body: "/claim", token: "", board: { status: "Ready" } },
    expect: (s) => says(s.state, "PROJECT_TOKEN"),
  },
  {
    name: "a renamed status names the options the field has",
    input: { body: "/claim", board: { status: "Ready", renamed: true } },
    expect: (s) => says(s.state, "has no **In progress** option", "Do not start yet") && !s.state.moved,
  },
];

(async () => {
  let failed = 0;
  for (const item of cases) {
    const s = scenario(item.input);
    let error = null;
    try {
      await s.run();
    } catch (thrown) {
      error = thrown;
    }
    const ok = !error && item.expect(s);
    if (!ok) failed += 1;
    console.log(`${ok ? "PASS" : "FAIL"}  ${item.name}`);
    if (!ok && error) console.log(`       threw: ${error.message}`);
  }
  console.log(failed === 0 ? `${cases.length}/${cases.length} branches answered as intended` : `${failed} failed`);
  process.exit(failed === 0 ? 0 : 1);
})();
'''


def job_script(text: str) -> str:
    """The workflow's one `script: |` block, read without a YAML parser.

    CI installs nothing but the checkout and Node.js, so the block is taken from the text: its
    indentation is the first non-empty line's, and it ends at the first line that is not part of it.
    """
    lines = text.splitlines()
    starts = [at for at, line in enumerate(lines) if line.strip() == "script: |"]
    if len(starts) != 1:
        raise SystemExit(f"expected exactly one `script: |` block, found {len(starts)}")
    block: list[str] = []
    indent: int | None = None
    for line in lines[starts[0] + 1 :]:
        if not line.strip():
            block.append("")
            continue
        width = len(line) - len(line.lstrip(" "))
        if indent is None:
            indent = width
        if width < indent:
            break
        block.append(line[indent:])
    while block and not block[-1]:
        block.pop()
    return "\n".join(block)


def node(*args: str, cwd: pathlib.Path) -> int:
    try:
        done = subprocess.run(["node", *args], cwd=cwd, capture_output=True, text=True)
    except FileNotFoundError:
        print("node is not on the PATH; the bot's script runs on Node.js")
        return 1
    if done.stdout:
        print(done.stdout, end="")
    if done.returncode != 0 and done.stderr:
        print(done.stderr, end="", file=sys.stderr)
    return done.returncode


def main() -> int:
    script = job_script(WORKFLOW.read_text(encoding="utf-8"))
    shutil.rmtree(WORK, ignore_errors=True)
    WORK.mkdir(parents=True)
    try:
        (WORK / "claim-bot.js").write_text(
            f"async function main() {{\n{script}\n}}\nmodule.exports = main;\n", encoding="utf-8"
        )
        (WORK / "claim-bot.test.js").write_text(HARNESS, encoding="utf-8")
        if node("--check", "claim-bot.js", cwd=WORK) != 0:
            print("the workflow's script does not parse")
            return 1
        return node("claim-bot.test.js", cwd=WORK)
    finally:
        shutil.rmtree(WORK, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
