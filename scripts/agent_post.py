#!/usr/bin/env python3
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Opens an issue or comments on a pull request the way AGENTS.md › Tasks asks an agent to: as
`suhina-bot[bot]` when the person it runs for can start the bot workflows, otherwise as that person.

    agent_post.py issue request.json     # {"title", "body", "type", "labels", "parent"}
    agent_post.py comment request.json   # {"pr", "body"}

The request is the JSON the workflows take (open-issue.yml, comment-pr.yml). First it asks GitHub
what the authenticated `gh` account may do in the repository (`.permissions`):

- push (write access): starts the workflow with the request on stdin, waits for the run it started
  and prints the line its log ends with. A dispatch answered with 403 falls through to the next case.
- no push: posts with the person's own account, `gh issue create` or `gh pr comment`. With triage
  access the issue gets its labels, its issue type and its parent epic as the workflow would give
  them; without triage the body says which labels a maintainer should add, and `Part of #N` names
  the parent. Before opening an issue, its parent is checked to be an open epic. Issues and
  comments say they were written by an agent. With triage, repository labels are checked before
  creating the issue; missing labels are left out and named in its body for a maintainer.

Either way it prints `OPENED_ISSUE=#N <url>` or `COMMENTED_PR=#N <url>` and says on stderr whose
account posted and why. It never retries a refused dispatch or uses another token. Exits 0 when
posted and any workflow completed successfully, 1 when GitHub refused or workflow completion
could not be confirmed, 2 for a request that is not valid. If the workflow posted before failing
or losing its connection, the error preserves the marker so the caller can follow up without
posting again. GitHub CLI pipes use UTF-8, including on Windows with a legacy locale.
Needs the GitHub CLI, signed in; standard library only.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from typing import Callable

REPO = "suhina-daw/suhina-daw"

# The type input: its label, and the organization's issue type it maps to (as open-issue.yml).
TYPES = {"enhancement": "Feature", "bug": "Bug", "documentation": "Task", "chore": "Task", "epic": None}
LABEL = re.compile(r"^[a-z0-9][a-z0-9 .:-]{0,49}$")
NUMBER = re.compile(r"^[0-9]{1,7}$")

AGENT_NOTE = "*AI-assisted: written by an agent and posted from this account.*"

# Seconds a `gh` call may take: the plain ones answer at once, `run watch` may wait out tens of
# minutes of CI. A call that outlives its timeout comes back as a failed result, which the
# existing error paths report; nothing is retried, and nothing is posted twice.
GH_TIMEOUT = 120
RUN_WATCH_TIMEOUT = 3600


class RequestError(Exception):
    """The request is not one the workflow would take."""


class PostError(Exception):
    """GitHub refused, or did not say what was posted."""


@dataclass
class Result:
    returncode: int
    stdout: str
    stderr: str


Runner = Callable[[list[str], str | None], Result]


def run_gh(args: list[str], stdin: str | None = None) -> Result:
    timeout = RUN_WATCH_TIMEOUT if args[:2] == ["run", "watch"] else GH_TIMEOUT
    try:
        done = subprocess.run(
            ["gh", *args], input=stdin, capture_output=True, text=True,
            encoding="utf-8", check=False, timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        return Result(124, "", f"gh {' '.join(args)} gave no answer within {timeout} s")
    return Result(done.returncode, done.stdout, done.stderr)


def say(message: str) -> None:
    print(message, file=sys.stderr)


def issue_request(raw: dict) -> dict:
    title = str(raw.get("title", "")).strip()
    body = str(raw.get("body", ""))
    kind = str(raw.get("type", ""))
    if not 1 <= len(title) <= 256:
        raise RequestError("The title must be 1 to 256 characters.")
    if not body.strip() or len(body) > 60000:
        raise RequestError("The body must be 1 to 60 000 characters.")
    if kind not in TYPES:
        raise RequestError(f'Unknown type "{kind}"; one of {", ".join(TYPES)}.')
    labels: list[str] = []
    for label in (part.strip() for part in str(raw.get("labels", "")).split(",")):
        if not label:
            continue
        if not LABEL.match(label):
            raise RequestError(f'"{label}" is not a plain label name.')
        if label in TYPES:
            raise RequestError(f'"{label}" is a type; give it as the type instead.')
        if label not in labels:
            labels.append(label)
    parent = str(raw.get("parent", "")).strip().removeprefix("#")
    if parent and not NUMBER.match(parent):
        raise RequestError(f'"{parent}" is not an issue number.')
    return {"title": title, "body": body, "type": kind, "labels": labels, "parent": parent}


def comment_request(raw: dict) -> dict:
    pr = str(raw.get("pr", "")).strip().removeprefix("#")
    body = str(raw.get("body", ""))
    if not NUMBER.match(pr):
        raise RequestError(f'"{pr}" is not a pull request number.')
    if not body.strip() or len(body) > 60000:
        raise RequestError("The body must be 1 to 60 000 characters.")
    return {"pr": pr, "body": body}


def permissions(gh: Runner) -> dict:
    """What the signed-in account may do in the repository; nothing when it cannot be asked."""
    result = gh(["api", f"repos/{REPO}", "--jq", ".permissions"], None)
    if result.returncode != 0:
        say(f"Could not read your access to {REPO}: {result.stderr.strip()}")
        return {}
    try:
        found = json.loads(result.stdout or "{}")
    except json.JSONDecodeError:
        return {}
    return found if isinstance(found, dict) else {}


def via_workflow(gh: Runner, workflow: str, request: dict, marker: str) -> str | None:
    """Starts the bot workflow and returns its `marker` line, or `None` when the dispatch got 403."""
    started = gh(["workflow", "run", workflow, "--repo", REPO, "--json"], json.dumps(request))
    if started.returncode != 0:
        if "403" in started.stderr:
            return None
        raise PostError(f"Starting {workflow} failed: {started.stderr.strip()}")
    # The run this start made, never the latest run, which may be another agent's.
    found = re.search(r"/actions/runs/([0-9]+)", started.stdout + started.stderr)
    if not found:
        raise PostError(f"{workflow} started but named no run: {started.stdout.strip()}")
    run_id = found.group(1)
    watched = gh(["run", "watch", run_id, "--repo", REPO, "--exit-status"], None)
    log = gh(["run", "view", run_id, "--repo", REPO, "--log"], None)
    # A run that failed after posting still prints the line.
    line = re.search(rf"{marker}=#[0-9]+ \S+", log.stdout)
    if not line:
        raise PostError(
            f"Could not obtain the {marker} line from run {run_id} of {workflow}; "
            "the posting outcome is unknown. The workflow may still post. "
            f"Do not post again; inspect gh run view {run_id} --repo {REPO}."
        )
    if watched.returncode != 0:
        # A failed watch may mean a failed run, a timeout or a lost connection. The marker
        # confirms the post, but not the later steps (such as linking an issue to its parent).
        raise PostError(
            f"{line.group(0)} was posted, but successful completion of run {run_id} of {workflow} "
            f"could not be confirmed (watch exit {watched.returncode}). "
            f"Do not post again; inspect gh run view {run_id} --repo {REPO}."
        )
    return line.group(0)


def body_file(text: str) -> str:
    handle, path = tempfile.mkstemp(suffix=".md", prefix="agent-post-")
    with os.fdopen(handle, "w", encoding="utf-8") as out:
        out.write(text)
    return path


def fallback_issue_body(request: dict, triage: bool, omitted_labels: list[str] | None = None) -> str:
    body = request["body"].rstrip("\n")
    parent = request["parent"]
    if parent and f"Part of #{parent}" not in body:
        body += f"\n\nPart of #{parent}"
    labels = omitted_labels if triage else [request["type"], *request["labels"]]
    if labels:
        wanted = ", ".join(f"`{label}`" for label in labels)
        body += f"\n\nLabels for a maintainer to add: {wanted}."
    if AGENT_NOTE not in body:
        body += f"\n\n{AGENT_NOTE}"
    return body + "\n"


def validate_parent(gh: Runner, parent: str) -> None:
    """Check the same open-epic requirement as the workflow before creating anything."""
    result = gh(["api", f"repos/{REPO}/issues/{parent}"], None)
    if result.returncode != 0:
        raise PostError(f"Could not check parent #{parent}; no issue was opened: {result.stderr.strip()}")
    try:
        epic = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise PostError(f"Could not read parent #{parent}; no issue was opened.") from error
    if not isinstance(epic, dict) or not isinstance(epic.get("labels"), list):
        raise PostError(f"Could not read parent #{parent}; no issue was opened.")
    if "pull_request" in epic:
        raise RequestError(f"#{parent} is a pull request, not an epic.")
    if epic.get("state") != "open":
        raise RequestError(f"#{parent} is not open.")
    if not any(label == "epic" or isinstance(label, dict) and label.get("name") == "epic" for label in epic["labels"]):
        raise RequestError(f"#{parent} is not labelled epic.")


def available_labels(gh: Runner, labels: list[str]) -> tuple[list[str], list[str]]:
    """Omit missing labels before the single create attempt, including labels beyond page one."""
    result = gh(["api", f"repos/{REPO}/labels", "--paginate", "--jq", ".[].name"], None)
    if result.returncode != 0:
        raise PostError(f"Could not check labels; no issue was opened: {result.stderr.strip()}")
    found = {label.casefold() for label in result.stdout.splitlines()}
    return (
        [label for label in labels if label.casefold() in found],
        [label for label in labels if label.casefold() not in found],
    )


def as_user_issue(gh: Runner, request: dict, triage: bool) -> str:
    if request["parent"]:
        validate_parent(gh, request["parent"])
    labels = [request["type"], *request["labels"]]
    omitted_labels = []
    if triage:
        labels, omitted_labels = available_labels(gh, labels)
    path = body_file(fallback_issue_body(request, triage, omitted_labels))
    try:
        args = ["issue", "create", "--repo", REPO, "--title", request["title"], "--body-file", path]
        if triage:
            for label in labels:
                args += ["--label", label]
        created = gh(args, None)
    finally:
        os.unlink(path)
    if created.returncode != 0:
        raise PostError(f"gh issue create failed: {created.stderr.strip()}")
    url = created.stdout.strip().splitlines()[-1] if created.stdout.strip() else ""
    found = re.search(r"/issues/([0-9]+)$", url)
    if not found:
        raise PostError(f"gh issue create named no issue: {created.stdout.strip()}")
    number = found.group(1)
    if triage:
        issue_type = TYPES[request["type"]]
        if issue_type:
            typed = gh(["api", "-X", "PATCH", f"repos/{REPO}/issues/{number}", "-f", f"type={issue_type}"], None)
            if typed.returncode != 0:
                say(f"Opened #{number}, but setting its issue type {issue_type} failed; a maintainer can set it.")
        if request["parent"]:
            link_parent(gh, number, request["parent"])
    return f"OPENED_ISSUE=#{number} {url}"


def link_parent(gh: Runner, number: str, parent: str) -> None:
    issue_id = gh(["api", f"repos/{REPO}/issues/{number}", "--jq", ".id"], None)
    linked = None
    if issue_id.returncode == 0 and issue_id.stdout.strip().isdigit():
        linked = gh(
            ["api", "-X", "POST", f"repos/{REPO}/issues/{parent}/sub_issues", "-F", f"sub_issue_id={issue_id.stdout.strip()}"],
            None,
        )
    if linked is None or linked.returncode != 0:
        say(f"Opened #{number}, but adding it under #{parent} failed; its body says Part of #{parent}.")


def as_user_comment(gh: Runner, request: dict) -> str:
    body = request["body"].rstrip("\n")
    # The note itself, not the phrase alone: a body that quotes the "AI-assisted" rule is not
    # a note, and would post unlabelled.
    if AGENT_NOTE not in body:
        body += f"\n\n{AGENT_NOTE}"
    path = body_file(body + "\n")
    try:
        posted = gh(["pr", "comment", request["pr"], "--repo", REPO, "--body-file", path], None)
    finally:
        os.unlink(path)
    if posted.returncode != 0:
        raise PostError(f"gh pr comment failed: {posted.stderr.strip()}")
    url = posted.stdout.strip().splitlines()[-1] if posted.stdout.strip() else ""
    return f"COMMENTED_PR=#{request['pr']} {url}"


def post(kind: str, raw: dict, gh: Runner = run_gh) -> str:
    request = issue_request(raw) if kind == "issue" else comment_request(raw)
    access = permissions(gh)
    if access.get("push") is True:
        if kind == "issue":
            workflow_request = {**request, "labels": ", ".join(request["labels"])}
            line = via_workflow(gh, "open-issue.yml", workflow_request, "OPENED_ISSUE")
        else:
            line = via_workflow(gh, "comment-pr.yml", request, "COMMENTED_PR")
        if line is not None:
            say("Posted as suhina-bot[bot] through the bot workflow (your account has write access).")
            return line
        say("The bot workflow answered 403, so this is posted from your own account instead.")
    else:
        say("Your account has no write access, so this is posted from your own account, not as suhina-bot[bot].")
    if kind == "issue":
        triage = access.get("triage") is True or access.get("push") is True
        if not triage:
            say("Without triage access the labels and the parent are named in the issue's body for a maintainer.")
        return as_user_issue(gh, request, triage)
    return as_user_comment(gh, request)


def main(argv: list[str]) -> int:
    if len(argv) != 3 or argv[1] not in ("issue", "comment"):
        say("usage: agent_post.py issue|comment request.json")
        return 2
    try:
        with open(argv[2], encoding="utf-8") as file:
            raw = json.load(file)
        if not isinstance(raw, dict):
            raise RequestError("The request must be a JSON object.")
        print(post(argv[1], raw))
    except (OSError, json.JSONDecodeError, RequestError) as error:
        say(str(error))
        return 2
    except PostError as error:
        say(str(error))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
