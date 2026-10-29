# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Read-only test-build lookup for trusted-requests.yml. Never checks out PR code.

Run from the default-branch checkout, with REQUEST, GITHUB_REPOSITORY, GH_TOKEN,
MAINTAINERS and TRUSTED_CONTRIBUTORS. Prints a JSON reuse decision, or {} to build.
Git objects are used only to compare main-only merges with branch_update.py.
API errors stop the request instead of accidentally starting another build.
"""

from __future__ import annotations

from datetime import datetime, timezone
import json
import os
import re
import subprocess
import sys

import branch_update as bu

PLATFORMS = ("linux-x86_64", "windows-x86_64", "macos-universal")
SHA = re.compile(r"[0-9a-f]{40}\Z")
PROOF = re.compile(r"<!-- trusted-build ([0-9a-f]{40}) ([\w-]+) ([0-9]+) -->\s*$")


def members(value):
    return {name.lstrip("@").lower() for name in re.split(r"[,\s]+", value) if name}


def timestamp(value):
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00")).astimezone(timezone.utc)
    except (ValueError, AttributeError):
        return datetime.min.replace(tzinfo=timezone.utc)


class Api:
    def __init__(self, repo):
        self.prefix = f"repos/{repo}/"

    def get(self, path):
        result = subprocess.run(["gh", "api", self.prefix + path], capture_output=True,
                                text=True, encoding="utf-8", timeout=60, check=True)
        return json.loads(result.stdout)

    def pages(self, path, key=None):
        separator = "&" if "?" in path else "?"
        for page in range(1, 101):
            data = self.get(f"{path}{separator}per_page=100&page={page}")
            rows = data[key] if key else data
            yield from rows
            if len(rows) < 100:
                return
        raise ValueError("The lookup exceeds 100 pages; no build was requested.")


def eligible_heads(head, main, cwd=None):
    """At most ten main-only updates, stopping at any change to the PR's own patch."""
    result = [head]
    for _ in range(10):
        parents = bu.git("show", "-s", "--format=%P", result[-1], cwd=cwd).decode().split()
        if len(parents) != 2 or not bu.only_merges_main(parents[0], result[-1], main, cwd)[0]:
            break
        result.append(parents[0])
    return result


def authorized_proofs(api, repo, number, comments, trusted):
    """Only authentic, unchanged human requests may supply a bot build's authority."""
    found = []
    for comment in comments:
        user = comment.get("user", {})
        if user.get("login") != "suhina-bot[bot]" or user.get("type") != "Bot":
            continue
        match = PROOF.search(comment.get("body", ""))
        if not match or match[2].lower() not in trusted:
            continue
        try:
            source = api.get(f"issues/comments/{match[3]}")
        except subprocess.CalledProcessError as error:
            # A deleted authorization cannot be used; other API failures stop the lookup.
            if "(HTTP 404)" in error.stderr:
                continue
            raise
        author = source.get("user", {})
        at = timestamp(comment.get("created_at"))
        if (author.get("type") == "User" and author.get("login", "").lower() == match[2].lower()
                and source.get("body", "").strip() == "/test-build"
                and source.get("issue_url") == f"https://api.github.com/repos/{repo}/issues/{number}"
                and timestamp(source.get("updated_at")) <= at):
            found.append((match[1], at))
    return found


def find_reuse(api, repo, pr, head, candidates, maintainers, trusted, now=None):
    now = now or datetime.now(timezone.utc)
    number = pr["number"]
    labels = {label["name"] for label in pr.get("labels", [])}
    platforms = ("windows-x86_64",) if "test-build-windows" in labels else PLATFORMS
    expected_jobs = {f"test build ({platform})" for platform in platforms}
    expected_artifacts = {f"suhina-pr{number}-{platform}" for platform in platforms}
    comments = list(api.pages(f"issues/{number}/comments"))
    proofs = authorized_proofs(api, repo, number, comments, trusted | maintainers)
    latest_current_run = datetime.min.replace(tzinfo=timezone.utc)
    running = None
    for source in candidates:
        # A bounded recent page: overlooking an older run requests a new build, never reuses
        # incomplete or unrelated evidence. The workflow endpoint binds the workflow identity.
        runs = api.get(f"actions/workflows/test-build.yml/runs?event=pull_request&head_sha={source}&per_page=100")["workflow_runs"]
        for run in runs:
            if (run.get("head_sha") != source or run.get("event") != "pull_request"
                    or run.get("head_branch") != pr["head"]["ref"]
                    or run.get("head_repository", {}).get("full_name") != pr["head"]["repo"]["full_name"]):
                continue
            actor = run.get("actor", {}).get("login", "").lower()
            allowed = actor in maintainers or (actor == "suhina-bot[bot]" and any(
                sha == source and at <= timestamp(run.get("created_at")) for sha, at in proofs))
            if not allowed:
                continue
            run_id = int(run["id"])
            jobs = list(api.pages(f"actions/runs/{run_id}/jobs", "jobs"))
            build_jobs = {job["name"]: job for job in jobs if job["name"] in expected_jobs}
            # A queued workflow may be visible before its matrix jobs exist. It must not
            # cancel the dispatch window; a completed skipped run, however, permits retry.
            if source == head and (run["status"] == "completed" or build_jobs):
                latest_current_run = max(latest_current_run, timestamp(run.get("created_at")))
            if set(build_jobs) != expected_jobs:
                continue
            state = None
            if run["status"] != "completed":
                # A previous-head build still running can be superseded by a new push. Only
                # reuse an in-flight run for the exact current head, never an ancestor.
                if source == head and all(job["status"] != "completed" or job.get("conclusion") == "success"
                                          for job in build_jobs.values()):
                    state = "running"
            elif run.get("conclusion") == "success" and all(
                    job.get("conclusion") == "success" for job in build_jobs.values()):
                artifacts = list(api.pages(f"actions/runs/{run_id}/artifacts", "artifacts"))
                available = {a["name"] for a in artifacts if not a.get("expired", True)
                             and a.get("size_in_bytes", 0) > 0 and timestamp(a.get("expires_at")) > now}
                if expected_artifacts <= available:
                    state = "ready"
            if state == "ready":
                return {"head": head, "source": source, "state": state,
                        "url": f"https://github.com/{repo}/actions/runs/{run_id}"}
            if state and running is None:
                # Kept until every eligible head is checked: a finished build of an earlier
                # head is better than waiting for this one.
                running = {"head": head, "source": source, "state": state,
                           "url": f"https://github.com/{repo}/actions/runs/{run_id}"}
    if running:
        return running
    # Actions may not expose a newly dispatched run yet. Serialize requests per PR and allow
    # a five-minute dispatch window when an authenticated proof and its label already exist.
    if "test-build" in labels and any(
            sha == head and at > latest_current_run and 0 <= (now - at).total_seconds() < 300
            for sha, at in proofs):
        return {"head": head, "source": head, "state": "requested",
                "url": f"https://github.com/{repo}/pull/{number}/checks"}
    return {}


def build_paths(workflow):
    """The `paths` filter of test-build.yml's pull_request trigger, as written there."""
    block = workflow.split("  pull_request:", 1)[1].split("  workflow_dispatch:", 1)[0]
    return [m for m in re.findall(r'^\s+- "([^"]+)"\s*$', block, flags=re.M)]


def builds_binary(files, paths):
    """Whether test-build.yml runs for a pull request changing these files."""
    def match(name, pattern):
        if pattern.endswith("/**"):
            return name.startswith(pattern[:-2])
        return name == pattern
    return any(match(f, p) for f in files for p in paths)


def main():
    request = json.loads(os.environ["REQUEST"])
    if request["command"] != "test-build":
        return {}
    repo, head, number = os.environ["GITHUB_REPOSITORY"], request["sha"], request["number"]
    if not SHA.fullmatch(head) or not isinstance(number, int) or number <= 0:
        raise ValueError("Invalid pull request or commit.")
    api = Api(repo)
    pr = api.get(f"pulls/{number}")
    if pr["state"] != "open" or pr["head"]["sha"] != head:
        raise ValueError("The pull request changed; post a new request.")
    # test-build.yml does not run for a pull request that changes nothing in the binaries, so
    # there is no build to wait for: the decision says so instead of asking for one.
    with open(os.path.join(os.path.dirname(__file__), "..", ".github", "workflows", "test-build.yml"),
              encoding="utf-8") as workflow:
        paths = build_paths(workflow.read())
    if not paths:
        raise ValueError("test-build.yml has no paths filter to read.")
    files = [f["filename"] for f in api.pages(f"pulls/{number}/files")]
    files += [f["previous_filename"] for f in api.pages(f"pulls/{number}/files") if f.get("previous_filename")]
    if not builds_binary(files, paths):
        return {"head": head, "source": head, "state": "no-binary", "url": ""}
    candidates = [head]
    if pr["base"]["ref"] == "main":
        # Fetch objects only: do not check out files, submodules, build scripts or hooks from PRs.
        bu.git("fetch", "--no-tags", "origin", "+refs/heads/main:refs/remotes/origin/main", f"refs/pull/{number}/head")
        candidates = eligible_heads(head, "origin/main")
    return find_reuse(api, repo, pr, head, candidates, members(os.environ.get("MAINTAINERS", "")),
                      members(os.environ.get("TRUSTED_CONTRIBUTORS", "")))


if __name__ == "__main__":
    try:
        print(json.dumps(main()))
    except (KeyError, ValueError, bu.GitError, subprocess.SubprocessError) as error:
        print(f"Build lookup failed; no new build was requested: {error}", file=sys.stderr)
        sys.exit(1)
