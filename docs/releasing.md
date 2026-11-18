---
description: How versions are numbered, how release builds, the nightly and test builds of pull requests are made, and how pull requests are merged (squash, no merge queue)
read_when: release, version, release candidate, tag, binaries, release workflow, nightly, past nightlies, development build, download, test build, pull request build, try a pull request, merge queue, squash merge, ready to merge label, auto-merge, update branch, merging many pull requests, batch build, Discord announcement, Discord bot, testing forum, merged thread, release notes, changelog, pull_request_target, Actions event policy, slow CI tests, nextest, test partitions
type: howto
status: current
last_verified: 2026-11-18
covers:
  - .github/workflows/ci.yml
  - .config/nextest.toml
  - scripts/install_nextest.sh
  - .github/workflows/release.yml
  - .github/workflows/nightly-notes.yml
  - .github/workflows/discord.yml
  - .github/workflows/test-build.yml
  - .github/workflows/test-build-links.yml
  - .github/workflows/ready-to-merge.yml
  - .github/workflows/overlap.yml
  - .github/workflows/pr-labels.yml
  - .github/workflows/updater-source.yml
  - scripts/build_reuse.py
  - scripts/release_packages_check.sh
  - scripts/test_build_reuse.py
  - scripts/test_merge_approval.py
  - scripts/branch_update.py
  - scripts/macos_app.sh
  - assets/brand/suhina-icon.png
  - scripts/release_notes.py
  - scripts/discord_post.py
  - scripts/discord_merged.py
  - scripts/discord_test_build.py
  - scripts/discord_test_build_backfill.py
  - scripts/discord_api.py
  - scripts/nightly_notes.py
  - scripts/test_nightly_notes.py
  - apps/suhina/build.rs
  - apps/suhina-cli/build.rs
  - crates/plugin-host/build.rs
---

# Releasing

Releases are built by [`.github/workflows/release.yml`](../.github/workflows/release.yml).

Windows release, nightly and test packages enable `suhina/asio,suhina-cli/asio`, using
the runner's LLVM installation for libclang and the vendored ASIO SDK. Both the desktop
app and CLI can use installed ASIO drivers alongside WASAPI; no driver is bundled.
Local builds need the same features explicitly (README › ASIO on Windows).
The Windows package smoke test checks that `suhina-cli audio devices` reports the ASIO
host even when the runner has no ASIO drivers installed.

## Versions

Versions follow [Semantic Versioning](https://semver.org): `MAJOR.MINOR.PATCH`, with release
candidates as `MAJOR.MINOR.PATCH-rc.N`. A release goes out as one or more release candidates first;
the final release is the last candidate unchanged, under the plain version:

```text
0.1.0-rc.1  →  testers find a bug, fixed on main  →  0.1.0-rc.2  →  no new bugs  →  0.1.0
```

The workflow accepts only these two forms. Release candidates are published as GitHub
pre-releases and never become the repository's "latest" release; a release, `0.x` ones included,
becomes the latest.

## Making a release

1. Set the version in the root `Cargo.toml` (`[workspace.package] version`), for example
   `0.1.0-rc.1`, run `cargo check` so `Cargo.lock` follows, and merge that through a pull request.
2. Tag the merge commit on `main` with the same version and push the tag:

   ```sh
   git tag v0.1.0-rc.1
   git push origin v0.1.0-rc.1
   ```

3. The workflow checks the version's form and that the tag matches it, builds the packages, runs
   each binary's `--version` and checks its library dependencies, and creates a **draft** release
   with the archives and their SHA-256 sums attached.
4. Read the draft on GitHub, edit the notes (see [Release notes](#release-notes)) and publish it.
5. For the next candidate, or the final release, repeat from step 1 with `0.1.0-rc.2` or `0.1.0`.

Running the workflow by hand (Actions › Release › Run workflow) builds the same packages as
workflow artifacts without creating a release, to test the pipeline. Every run's artifacts are
kept for 7 days; a nightly or a release keeps its packages on its release page. A release or a
nightly is published only with all three platforms' packages
(`scripts/release_packages_check.sh`): when one build failed, re-run the whole workflow, since a
build re-run after the others' artifacts expired brings only its own.

On Windows the three executables carry version information, which the file's Properties ›
Details and Task Manager show: the product name Suhina, what each program is (Suhina, Suhina
command line, Suhina plug-in host), the workspace version and the copyright. Their build scripts
(`build.rs` in `apps/suhina`, `apps/suhina-cli` and `crates/plugin-host`) write it with
`winresource`, so every build has it, nightlies and test builds too. Code signing checks the
product name and version later ([#180](https://github.com/suhina-daw/suhina-daw/issues/180)); the
binaries are not signed yet.

## Nightly

The nightly is how people try the current state between releases: a build of `next`, where pull
requests merge, not a release. Running the workflow from `next` with **nightly** checked builds
`next` and publishes it as the release **`nightly`**, replacing the previous one: its release and tag are deleted and made
again on the new commit, published at once without a draft. The workflow has no schedule of its
own: a scheduled agent starts it twice a day (`gh workflow run release.yml -f nightly=true`) when
`next` has moved since the last nightly, or a maintainer does by hand; once every package is
published, `main` follows ([The next branch](#the-next-branch)). A second agent then rewrites
its notes and posts it to Discord (see [Nightly notes](#nightly-notes)).

The nightly's version is the workspace version without a pre-release suffix, the date and the
commit: `0.0.1-nightly.20261013+a15ad35`. The workflow writes it into `Cargo.toml` and
`Cargo.lock` before building (nothing is committed), so `suhina-cli --version`, the desktop
app's Help › About Suhina ([spec](spec/about.md)) and bug reports say which build it is. Its
packages keep one name, `suhina-nightly-<platform>`, so a link to the latest one never changes:

```text
https://github.com/suhina-daw/suhina-daw/releases/download/nightly/suhina-nightly-linux-x86_64.tar.gz
```

Each night is also kept as a pre-release of its own under the tag **`nightly-YYYYMMDD`**
(`nightly-20261013`), with the same packages and notes, published on the same commit and never
the latest. A tester can go back to an earlier night to check when something broke, or return
to a build that opens their project. A second nightly on the same day is kept as
**`nightly-YYYYMMDD-2`** beside the first, and so on; the name is chosen when the nightly is
published, so a rerun of a failed run replaces only a dated release of its own commit and never
another build's. The nightly job deletes the dated nightlies older than `NIGHTLY_KEEP_DAYS` (30) with
their tags; it touches no other release, and a failure there leaves the new nightly published.

### Nightly notes

The workflow publishes the nightly with the list from
[`scripts/release_notes.py`](../scripts/release_notes.py) (see [Release notes](#release-notes)) and
ends the notes with a hidden line, `<!-- nightly-range: <previous nightly>..<this commit> -->`.
A scheduled Claude agent (a routine on the maintainer's account, not in this repository) runs about
an hour after each build. It takes every dated nightly of the last three days whose notes do not
yet say `<!-- discord: posted -->`: without `<!-- notes: composed -->` it reads the pull requests
merged in that range (their titles and descriptions) and writes notes for users from them; with it
(the notes are published but the Discord post failed) it hands over the published notes again. A
build that is still running is waited for, and a nightly the agent missed is caught up on its next
run. Until then the nightly keeps the
script's list, and it is published either way.

The agent runs in a Claude cloud session, which may start workflows but may not edit releases, so
it hands the notes to **Actions › Nightly notes** ([`nightly-notes.yml`](../.github/workflows/nightly-notes.yml))
as its `tag` and `notes` inputs. [`scripts/nightly_notes.py`](../scripts/nightly_notes.py) checks
them before anything changes: the tag is `nightly-YYYYMMDD` or `nightly-YYYYMMDD-N`, the notes
start with `### New since the previous nightly`, carry no HTML, images, `@`, character references
(`&#58;`) or `]:` (which starts a reference-style link definition, `[label]: …`), and link only
to this repository, also once Markdown's backslash escapes are undone. It then replaces the notes section of that dated release (and of `nightly` when it
is the same build) and adds `<!-- notes: composed -->`, and posts the notes to Discord's releases
channel (see [Discord](#discord)) as one embed whose title links to the dated release, so the link
still leads to the build it announced after the next night. Only after Discord took the post is
`<!-- discord: posted -->` added, so a failed post is tried again by the next run and a nightly is
never posted twice. A maintainer can run the workflow by hand with the same inputs.

Until Suhina has a release of its own (published, not a release candidate), the nightly is the
repository's **latest** release, so the Releases box on the repository page offers its downloads.
From the first release on, the nightly is published as a pre-release that never becomes the
latest. A release is never made from it: it is made from a version tag as above.

## The next branch

In use since 2026-11-18. Pull requests merge into an integration branch, `next`, and `main` holds
only code that built and packaged on Linux, Windows and macOS, so that every nightly is green and a
pull request need not wait for a test build of its own.

1. Pull requests target `next`, the repository's default branch, and are squashed into it once
   `ci` is green. New branches start from `next`. Test builds stay as they are:
   a maintainer's own pull request is built on every push, others on `/test-build`.
2. The scheduled agent starts the nightly from `next` (`--ref next`) when `next` has moved since the
   last nightly.
3. When every system's package is published, the release workflow's job `promote` moves `main`
   to the commit the nightly was built from. It is a fast-forward only: if `main` is not an
   ancestor of that commit, GitHub refuses and `main` stays. A failed build or publish never
   reaches the job, and `main` waits for the next night; the fix merges into `next` like any other.
   Nothing is cherry-picked and no commit is made: `main` gets the very commits of `next`, one
   squashed commit per pull request, with the same hashes, and only lags behind it.

The rulesets `main` and `main merge queue` keep their names but target `~DEFAULT_BRANCH`, so they
apply to `next`; `main follows the nightly` keeps everyone but the app `suhina-docs-bot` from
moving `main`.

The workflows follow the repository's default branch, `next`: `ci.yml`, `test-build.yml`
(`cache`) and `docs-generated.yml` run on a push to the default branch and save its caches, and do
nothing on a push to `main` (the nightly moving it); a pull request's test build compares it with
its own base branch (`scripts/build_reuse.py`, `branch_update.py`), and `ready to merge` queues a pull
request into `next` without waiting for a test build of it (`ready-to-merge.yml`), since the
nightly builds `next` on every system before `main` moves. `docs-generated.yml` keeps one run at
a time per branch, so a push to `main` never drops a waiting run of `next`. `nightly-notes.yml`
still runs from `main`.

How it was turned on, in this order, kept for setting it up again. `main` must gain nothing that
`next` lacks: a single commit on `main` that is not on `next` keeps
`promote` from ever moving `main` again (it is a fast-forward only), until someone moves `next` or
`main` by hand.

1. Stop everything that writes to `main`: let nothing into the merge queue (take `ready to merge`
   off the queued pull requests), and wait until no run of `docs-generated.yml` (it commits to
   `main` after a merge), `ci.yml` or the merge queue is queued or in progress on `main`.
2. Create `next` from `main` as it is then.
3. Settings › Rules: the rulesets `main` (an approval, the code owner's review, the required `ci`
   check, no force push or deletion) and `main merge queue` target `~DEFAULT_BRANCH`, not the name
   `main`, so they move to `next` with the default branch in step 5 by themselves, and leave `main`
   with no rule at all: anyone who can write could push it and break `promote`'s fast-forward. So
   first add a ruleset for `refs/heads/main` with the rules *Restrict updates*, *Restrict
   deletions* and *Block force pushes*, and only the app `suhina-docs-bot` (id 5307338) on its
   bypass list (Always): `promote` pushes with that app's token, and nothing else moves `main`.
4. Settings › Environments: add `next` to the deployment branches of every environment that
   allows `main` today, keeping `main` (and `discord`'s `v*` tags): `docs-bot`, `discord`,
   `project-board` and `suhina-bot`. GitHub runs `issue_comment`, `issues` and `workflow_run` on
   the default branch, and `pull_request_target` on the pull request's base branch, so once `next`
   is the default their jobs run on `next`: without it, linking test builds and posting to
   Discord (`test-build-links.yml`, `discord.yml`), `/claim` and the board (`claim.yml`,
   `project-sync.yml`), `/test-build` and the bot's issues and comments (`trusted-requests.yml`,
   `open-issue.yml`, `comment-pr.yml`), the generated docs, `ready to merge` and `promote` stop
   at the environment. `next` is as protected as `main` (step 3), so allowing it lets no other
   branch read their secrets.
5. Check that `next` and `main` are still the same commit
   (`gh api repos/suhina-daw/suhina-daw/compare/main...next --jq .behind_by` prints 0); if `main`
   moved, move `next` to it first. Then Settings › General: make `next` the default branch.
   From then on a run of `docs-generated.yml` writes only to `next`, a run by hand on `main`
   included. Dependabot and `ready to merge` follow the default branch.
6. Point the open pull requests at `next` (`gh pr edit N --base next`), and let the merge queue
   take pull requests again.
7. The scheduled agents: the nightly's starts it with `--ref next` and compares `next` with the
   tag `nightly`; the notes' fetches `next` as well as `main`.
8. Check the automation on the new default branch: a finished test build is linked on its pull
   request, `/claim` answers on an issue, and `ready to merge` queues a pull request into `next`.
9. CONTRIBUTING.md, docs/workflow.md and AGENTS.md: pull requests target `next`; docs/claim-bot.md
   and this document's Discord section: the environments allow `next` as well as `main`.

## Test builds

A test build is a pull request packaged for download, so it can be tried without building it
locally. [`.github/workflows/test-build.yml`](../.github/workflows/test-build.yml) builds the
same three packages as the nightly, for Linux, Windows and macOS (see [Packages](#packages)), and
uploads each as a workflow artifact kept for 7 days.
[`.github/workflows/test-build-links.yml`](../.github/workflows/test-build-links.yml) then links
them from the pull request in one comment that each new build updates, with a platform that failed
linked to its log. Downloading an artifact needs a GitHub sign-in, and an artifact is always a
zip: on Windows it holds the package folder, on Linux and macOS the package's `.tar.gz`, because a
zip drops the executable bit.

The linking run belongs to the build it links, and says so in what it starts: only a run that built
something is answered, so a run that was skipped (its *what to build* job decided not to build),
cancelled or superseded by a newer push links nothing and takes no runner from the builds that do
have something to link. A failed build is answered like a successful one, with the platform that
failed linked to its log. The runs that do link wait as one run per pull request rather than one per
platform, a newer build's link replacing one that is still waiting; a run from a fork lists no pull
request there, and is grouped by its head repository and branch instead.

| Pull request | Built |
|---|---|
| opened and pushed by a maintainer | on every push; a newer push cancels a build still running |
| anyone else's, from a branch in this repository | when a maintainer adds `test-build`, or a trusted contributor comments `/test-build`, for the authorized commit |
| from a fork | when a maintainer or trusted contributor comments `/test-build`, for that commit; the label alone, a maintainer's too, is refused |

A pull request with the `test-build-windows` label is built for Windows only, which keeps the
shared runners free while only that platform is being tried; its comment lists the other two as
not built.

Only a pull request that changes what goes into the binaries is built: `apps/`, `crates/`,
`devices/`, `presets/`, `third_party/`, `Cargo.toml`, `Cargo.lock`, `.cargo/` or `test-build.yml`
itself. One that changes only documentation, scripts, other workflows or agent files has no new
binary to try.

The build is of the pull request's head merged with `main`, as the event saw it, so a push after
the label is not what gets built. A push by anyone but a maintainer takes the label off, and the
new commits are built when a maintainer adds the label again or a trusted contributor posts a new
`/test-build` after reading the diff; the comment on such
a build says it is a contributor's. A first-time contributor's run also waits for a maintainer's
*Approve and run*.

The version says which pull request and commit it is: `0.0.1-pr.302+abc1234` in `--version` and
bug reports. Test builds use the release profile without LTO and with 16 codegen units, so they
build faster and run slightly slower than the nightly.

**Repeated build requests reuse available work.** `/test-build` first checks the current head
and up to ten preceding main-only updates using `scripts/build_reuse.py` from the default branch.
It reuses a current-head run already building the requested platforms, or a successful run with
all requested, nonempty, unexpired artifacts. Failed, cancelled, skipped or incomplete builds
are not reused. Windows-only packages satisfy a Windows-only request, but not an all-platform
request. Reuse replies link the existing run without changing the `test-build` label. Build lookup
errors stop the request instead of assuming that no build exists. Main-only reuse is conservative:
a changed patch or a resolved conflict requires a new build. PR files are never checked out or
executed during this lookup; only git objects are fetched for comparison.

**A branch update is not built.** A push that only merges `main` into the pull request, as
*Update branch* does, leaves the pull request's own changes as they were, so the build already
linked from it is still the one to try. A small first job, *what to build*, asks
[`scripts/branch_update.py`](../scripts/branch_update.py) whether the new head is a merge commit
whose first parent is the previous head, whose second parent is on `main`, and whose own diff is
the same patch as the previous head's, ignoring only line numbers and blob hashes, never whitespace
or file modes (a resolved conflict, or `main` changing lines next to the
pull request's own, makes it a new patch). It also checks that the previous head's run succeeded,
so a failed or cancelled build is never kept as the last one. When all hold, the platforms are
skipped, and `test-build-links.yml`, finding nothing built, leaves the comment linking the last
real build. Anything else, a new commit, a rewritten history or an unknown previous head, is built
as before.

**Several pull requests together:** Actions › Test build › Run workflow with their numbers
(`302 303 304`) merges them onto `main` in that order and builds the result, named
`suhina-prs-302-303-304-<platform>`; each platform's summary links its package. A pull request that does not
merge cleanly fails the run and is named in the error.

The build has a read-only token and no secrets, and for a fork GitHub runs `test-build.yml` as the
fork has it; so `test-build-links.yml`, which runs from `main` and never runs pull request code,
links only a maintainer's run of a branch in this repository, or a run with a bot-authored
authorization for that exact commit, written before the run started. A fork's run needs that
authorization even when a maintainer started it: adding a label, *Update branch*, closing and
reopening all start one, and the fork's own workflow files decide what it builds, so who started it
says nothing about its code. `scripts/build_reuse.py` counts a fork's earlier build under the same
rule. It rechecks the original human comment
and current `TRUSTED_CONTRIBUTORS` membership. The build checks the App's proof without relying
on repository variables, which may be unavailable in fork PR runs; the default-branch gateway
and linker check membership. A rerun retains the original run actor's authorization, not the
rerunner's. Both workflows verify the commit-bound proof;
a forged or stale proof, removed trusted user, edited/deleted request or another PR's comment
cannot authorize a link. All role checks read the repository variable `MAINTAINERS`, using
comma/whitespace-separated logins, optional `@` prefixes and case-insensitive exact matching.
An empty list grants no maintainer authority. Set it at repository level, not as an environment
override. For fork PRs, maintainers also use `/test-build`: a label is refused there, since it
binds no commit the maintainer read, and the build may not receive repository variables anyway. Automatic builds
of maintainer PRs continue for branches in this repository.

Suhina's updater cannot tell from an artifact's name who made it, since a fork's run names its
artifacts as it likes. So when `test-build-links.yml` links a fork's build, it links only the
artifacts named for that pull request (`suhina-pr<number>-<platform>`) and uploads a marker from its
own run, an artifact named `suhina-linked-pr<number>-run<run id>` kept for 7 days like the build.
The updater lists a fork's build only when the artifacts list holds that marker from a run of this
repository ([updates.md › Channels](spec/updates.md#channels)).

Trusted contributors use `/test-build` on an open PR, as described in
[Trusted bot requests](claim-bot.md#trusted-bot-requests). The bot attributes the request, records
the head SHA and applies `test-build` using its App token. No extra GitHub label permissions are
needed. A new push requires a new request; merge approval remains maintainer-only. The first
fork-run approval required by GitHub is still a separate maintainer action.

Each linked build is also posted to the pull request's thread in the Discord testing forum
(see [Discord](#discord)).

## Merging many pull requests

Pull requests are squashed into `next` as soon as their own `ci` is green: one commit per pull
request, and no merge queue (since 2026-11-18). A queue ran the whole of CI once more for every
merge, about 20 minutes, to test the pull requests together on top of the branch; with `next`
that protection is there already, since a conflict in meaning between two merged pull requests
turns `next`'s own CI red, and the nightly builds `next` on every system before `main` moves
([The next branch](#the-next-branch)). *Require branches to be up to date before merging* is off,
so no one presses *Update branch* either. The rule for `next` (the ruleset **main**) allows squash
merges only, and Settings › General allows no merge commits; the queue merged with merge commits
(`Merge pull request #N from …`).

**Which changes build Rust.** In a pull request and on the default branch, `ci.yml`'s
`changes` job skips fmt, clippy and the tests when every changed file is documentation (`docs/`,
any `.md`, `LICENSE`, `NOTICE`), a script under `scripts/` or a file under `.github/`: no Rust
build or test reads them. A change to `ci.yml` itself still runs every job. The `docs` job runs on
every change, so the script tests and the tests of the claim bot and the Depends on check still run,
and so do the Depends on check itself and the settings guide export; `cargo deny` skips with the
build jobs, since only a Rust change can change a dependency. In the Actions tab the skipped jobs
show as skipped and `ci` passes.

**The build cache.** Only the default branch saves `ci.yml`'s Rust cache; pull requests restore
it. The repository's caches share GitHub's 10 GB, and when every pull request push
saved its own (about 0.8 GB per system), `main`'s were evicted and about half the runs compiled
every dependency again: 11 to 12 minutes of compiling on Linux instead of 3½ to 4½. A run on
`main` is never cancelled, since a cancelled run saves no cache: when the generated-docs commit
that follows every merge cancelled it, Windows (the slowest) never saved one and every pull
request compiled every dependency there, 23 to 29 minutes instead of about 10. The pushes to
`main` share one concurrency group without cancelling: the running run goes to the end, only the
newest push waits, and the ones in between get no run (GitHub keeps one waiting run a group).
When every push had a group of its own, five merges in twenty minutes filled the organisation's
runners (about 20 jobs at once, 5 on macOS) for half an hour with the runs of superseded commits,
nine jobs each, and the pull requests' runs queued behind them. The generated-docs commit says
`[skip ci]` and starts no workflow.

rust-cache saves only when the cache's key changes (`Cargo.lock`, the toolchain), and what a job
builds is not part of the key. When a job comes to build less, a new `prefix-key` is what makes
`main` save a smaller cache: `check` has `v1-rust` since the tests moved to `test` and its old
cache, which still held the test build, took 1.1 to 1.3 GB a system of the 10 GB. A cache nothing
restores any more is the first GitHub evicts. `gh cache list --sort last_accessed_at` shows what
the 10 GB holds.

The test build follows the same rule: its `cache` job builds the test build's binaries on every
push to `main` that changes them, only to save the cache, and pull requests' test builds restore it
and save none. Their own caches, 0.6 to 0.8 GB per system on every build, had filled the 10 GB and
evicted the others.

**What the tests compile.** `cargo test` builds all of the workspace's crates in every run, with
the `test` profile, and about 90% of that time is LLVM at `opt-level = 1`; linking is 1 to 3% on
each system, so a faster linker has nothing to win. The crates that process no sound are built at
`opt-level = 0` in that profile (`[profile.test.package]` in `Cargo.toml`). Measured on one machine,
that takes the compile from 236 to 114 seconds on Linux, 600 to 300 on Windows and 380 to 183 on
macOS, and the tests' run from 108 to 147, 138 to 203 and 148 to 192, about three quarters of it in
the window tests (`crates/ui/tests/main.rs`). The crates that process sound stay at 1: with them
at 0, `suhina-engine`'s tests take 93 seconds instead of 7. A new crate starts at 1, and joins the
list if it processes no sound and takes long to compile.

The style engine, `suhina-style`, stays at 1 although it processes no sound: the window tests and
theme-render's drawings run it thousands of times, and it compiles quickly. At 0, theme-render's
`painted::every_built_in_theme_paints_each_scene_as_recorded` took 510 seconds on a Linux runner
and held one test partition for most of its run; with it at 1, that partition's tests took 133
seconds instead of 535 on Linux and 111 instead of 393 on Windows, and the compile did not grow
(2026-11-18, #1553). `suhina-ui`, `suhina-ui-gpu` and `suhina-theme-package` at 1 as well made the
tests a little faster still, but `suhina-ui` took 3½ to 6 minutes more to compile on Windows, and
the slowest Windows partition went from 13½ to 19½ minutes.

**How CI runs the tests.** `ci.yml`'s `test` job runs the workspace's tests with
[cargo-nextest](https://nexte.st), split into three partitions on Linux and three on Windows, six
runners at once; `check` runs clippy, the documentation and the vendored code's tests beside them,
and macOS runs everything on one runner. `cargo test` ran the test binaries one after another
(about 140 of them, 5,000 tests): the run took 11 minutes on Windows and 19 on Linux of a 16 and
22 minute step, much of it waiting for three binaries, the window tests
(`crates/ui/tests/main.rs`), the theme renders (`apps/theme-render/tests/main.rs`) and the packs
(`crates/device-registry/tests/pack.rs`), while the runner's other cores had nothing to do.
nextest runs every test in a process of its own and the tests of every binary side by side, and
`--partition slice:N/3` gives each runner every third test in order, so each runner gets a third of
each binary. Each partition compiles the tests itself from `main`'s cache, which the first
partition saves; the doc tests, which nextest does not run, run in the first partition with
`cargo test --doc`, and the engine's tests again on four render threads
(`SUHINA_RENDER_THREADS=4`, ADR 0038 › 16) are split between the partitions the same way. They are
picked from the workspace build with `-E "package(suhina-engine)"`, not built with `-p`: cargo
resolves the dependencies' features for the one package then, and compiled a share of them again.

nextest's settings are in [`.config/nextest.toml`](../.config/nextest.toml): every failing test
is reported rather than the run stopping at the first (locally, `cargo test --workspace
--no-fail-fast` does the same), a test that runs a minute or more is named while it runs, and one
that runs ten minutes is stopped and fails: it has hung, and would otherwise hold its runner until
the job's six-hour limit, as `crates/engine`'s `an_offline_pools_workers_never_join_real_time_priority`
did once on Linux (it takes milliseconds).
nextest spreads tests, not the work inside one: a test that loops over many independent cases
holds one core for all of them, and sets how short a partition can get. A test binary whose tests
take turns through a lock inside the binary (a `static Mutex`) needs a test group of one thread
there, since in processes of their own they no
longer wait for each other; `crates/ipc/tests/leftovers.rs` is one. A test that fails only under
nextest has a race of that kind, not a timing problem: there are no retries. The version is pinned
with its archives' checksums in [`scripts/install_nextest.sh`](../scripts/install_nextest.sh);
locally `cargo test --workspace` runs the same tests, and so does `cargo nextest run --workspace`
once nextest is installed (`cargo install cargo-nextest --locked`).

**The pack's tests.** The shipped pack's tests (`crates/device-registry/tests/pack.rs`) render NAM
models, and NAM ran at two to four times real time in a test build until `nam-rs` was built at
`opt-level = 3` without debug assertions or overflow checks there (`[profile.test.package]` in
`Cargo.toml`; its own tests still run with them in the `check` job): five to six times faster,
the pack's tests from 207 s on one core to 26 s (measured 2026-11-18). The own models' compares run
over the first 20 s of their phrases, every amp model over two 1 s passages; the comments there
give the measurements. Two of them audit the pack's data, every amp model and every stock preset
that names a file: a pull request that changes none of what they read (`ci.yml`'s `changes` job
lists it: the pack, the presets, the NAM devices and the crates they are built from, the vendored
`nam-rs`, the workspace manifest and lock file) leaves them out through nextest's `ci-pr` profile,
and the default branch always runs them. In the 300 merges to `main` before 2026-11-18, 61
changed what they read.

The ruleset **main** (it targets the default branch, so `next`) asks for an approval, the code
owner's review, the required `ci` check and resolved threads; a role added to its bypass list skips
all of them. The checks that have workflows of their own (`cargo deny`, the Depends on check,
[`claim-bot.md`](claim-bot.md) › The Depends on check, and the settings guide export) are called
from `ci.yml` as its jobs, so `ci` goes red with them: a check main does not require can fail
without stopping a merge, and the `ready to merge` label merges a pull request on `ci` alone.
`ci.yml` still runs on `merge_group`, so a queue could be turned on again (Settings › Rules, a
ruleset with *Require merge queue* and the merge method *Squash*) without changing a workflow.

**The `ready to merge` label.** A maintainer who has read a pull request adds the label instead of
coming back to press *Squash and merge* once CI is green. That label is their approval of the
current head, given with a finished test build of it: nothing merges without one, but a build that
already exists is reused, so a stale review never forces a new packaging run.
[`ready-to-merge.yml`](../.github/workflows/ready-to-merge.yml) (`pull_request_target`, checking out
only `main`'s automation for the build lookup, never running the pull request's code) then:

| Event | What happens |
|---|---|
| a maintainer adds the label and a finished test build of the head exists | the app approves the head commit as the event saw it (the rule for `main` needs one approval, and a maintainer cannot approve their own pull request) and turns on auto-merge (squash) with that commit as the expected head; GitHub squashes the pull request into its base once `ci` is green and every review thread is resolved |
| a maintainer adds the label and no finished test build of the head exists | the label comes off with a comment asking for `/test-build`; nothing is approved or queued. A build counts when it is a successful, authorized `test build` run of the head, or of an earlier head the branch only merged `main` into, with every requested package still available (`scripts/build_reuse.py`); a build still running does not count yet. This applies to every author, maintainers and dependency updates included. A pull request that changes nothing `test-build.yml` builds (only documentation, scripts or other workflows) needs no build, and neither does one whose built files are all user interface code (`UI_PATHS` in `scripts/build_reuse.py`: `crates/ui`, `crates/ui-gpu`, `crates/style`, `crates/theme-package`, `apps/theme-render`), which its tests cover in CI; `/test-build` still builds one |
| anyone else adds the label | it comes off |
| a push by a maintainer | the label stays and the new head goes through the same check: with a finished test build of it (a push that only merged `main` reuses the earlier one) the app approves it and turns auto-merge on for it; without one the label comes off with the same comment |
| a push by anyone else | the label comes off, so the new commits wait to be read (the rule for `main` dismisses the older approval) |
| the label comes off | auto-merge off, and the app's approvals dismissed; a call that fails fails the run. When the label was added again meanwhile, the run fails and asks for the label to be taken off and added again, so the pull request waits rather than merging |

The merge method is squash, the only one allowed. Agents never add the label (`docs/workflow.md` › Tasks).

It acts with a short-lived token of the GitHub App that also pushes the generated docs
(`docs-generated.yml`), not with `GITHUB_TOKEN`: GitHub starts no workflows for a merge made through
`GITHUB_TOKEN`, and those on `main` must run after every merge. Setup, once: the app has Repository
permissions › **Contents**, **Pull requests** and **Issues**: Read and write (auto-merge merges as the
app, and GitHub closes the issues a description closes, `Closes #N`, as the one who merged: without
Issues they stay open, and nothing reports it). A permission added to the app reaches its token only
once the installation accepts it: Organization settings › GitHub Apps › suhina-docs-bot ›
**Configure**, then **Review request** › **Accept new permissions**. Its public Client ID is the
`DOCS_BOT_CLIENT_ID` variable and its private key the `DOCS_BOT_PRIVATE_KEY` secret in the
`docs-bot` environment, which `pull_request_target` may use
because it runs on `main`; and Settings › General › **Allow auto-merge** is on. Without that setting the
*approve and queue* job fails, and nothing merges. Maintain the shared `MAINTAINERS` repository
variable in Settings → Secrets and variables → Actions → Variables. Changing that list controls
who may authorize a merge; there is no maintainer name embedded in the workflows.

Use the Client ID from the app's GitHub settings (or the public `GET /apps/{app_slug}` response),
not its numeric App ID. `actions/create-github-app-token` v3 deprecates the `app-id` input.
The issue and PR-comment bot uses the `SUHINA_BOT_CLIENT_ID` variable and the existing
`SUHINA_BOT_PRIVATE_KEY` secret in the `suhina-bot` environment. Both bots still create tokens
for this repository only. Client IDs are public; private keys remain environment secrets.

**One build of several pull requests.** To try a batch before merging it, run Actions › Test build
› Run workflow with their numbers (`302 303 304`), as described under [Test builds](#test-builds):
one build of `main` with all of them merged on, in that order.

## Release notes

[`scripts/release_notes.py`](../scripts/release_notes.py) writes the notes from git alone (no
network, no AI service); the nightly's are rewritten afterwards by an agent, as described under
[Nightly notes](#nightly-notes). A change is one commit on `main`'s first-parent line, so one merged pull
request, listed by its title with a link to the pull request: a squash merge's subject, or for a
merge commit ("Merge pull request #N from …") the first line of its message, which is the pull
request's title. Changes that touch only documentation, CI, scripts and tests are counted, not
listed, and so are changes whose title's area word says they are code structure, tests,
dependencies or repository work ("Architecture: …", "Code structure: …", "Tests: …", "Deps: …",
"CI: …", "Docs: …", "Refactor: …"), which touch the programs without a user noticing. Pull
request titles are the release notes, so they say what changed for a user: "Clip fades: shapes
and an adjustable curve".

- **Nightly:** the changes since the previous nightly, on its release page until the agent rewrites them.
- **Draft of a tagged release:** first *New and changed*, the bullets of ROADMAP.md › What works now
  that are new or reworded since the previous release, by milestone, then *All changes*. A
  release candidate counts from the tag before it, a release from the previous release, the first
  from the beginning. Edit the draft before publishing: the ROADMAP bullets describe what works,
  not what changed, and a reworded bullet may be a fix of its text. A pull request writes a new
  bullet as a file under `docs/works-now/`, which `docs-generated.yml` folds into ROADMAP.md on
  `main` right after the merge ([`works-now/README.md`](works-now/README.md)), so the draft sees it
  like any other.

Its tests run in CI: `python3 -m unittest discover -s scripts -p 'test_*.py'`.

## Discord

Three channels get posts from GitHub Actions, each through a webhook (Discord: channel settings ›
Integrations › Webhooks) whose URL is a secret in the environment **`discord`** (GitHub: Settings ›
Environments › `discord`). A channel without its secret gets nothing, and a failed post fails nothing
else. No mention in a post pings anyone.

The Discord secrets, `DISCORD_BOT_TOKEN` among them, are environment secrets rather than repository
secrets, so a workflow pushed to another branch cannot read them. The environment's *Deployment
branches and tags* allow `main`, `next` and tags matching `v*`: `discord.yml` runs on its tag for a
release event, the other jobs run on `next`, the default branch. Only maintainers may create `v*`
tags (the `release tags` ruleset restricts their creation), so no one else can make a ref that reaches them. The jobs say
`deployment: false`, so the environment adds no deployment to a pull request's timeline.

| Channel | Secret | Posts |
|---|---|---|
| releases | `DISCORD_WEBHOOK_RELEASES` | each nightly, with the notes the agent wrote (`nightly-notes.yml`, see [Nightly notes](#nightly-notes)); each release when a person publishes it, with its *New and changed* section (`discord.yml`) |
| events | `DISCORD_WEBHOOK_EVENTS` | pull requests opened, merged, closed and reopened; issues opened, closed and reopened (`discord.yml`) |
| testing (a forum) | `DISCORD_WEBHOOK_TESTING` | each linked test build, in its pull request's thread (`test-build-links.yml`) |

[`scripts/discord_post.py`](../scripts/discord_post.py) writes the posts of `discord.yml` from
the event. The nightly never reaches `discord.yml`: it is created with the workflow's own token,
which starts no other workflow. `discord.yml` runs on `pull_request_target`, so pull requests from
forks are posted too; it checks out the base branch and never runs the pull request's code. No
repository webhook (Settings › Webhooks) posts to Discord; one would post the nightly twice. Two
events about one item (an issue opened and then closed, a pull request pushed and then merged) share
a `concurrency` group keyed on that item, so a newer event replaces one that is still waiting and a
burst waits as one run per item instead of one per event; a post that is already running is never
cut off, because a half-posted notification is one nobody sees.

The testing forum has a thread per pull request, named `#<number> <title>`; the number always
comes first, because the bot *Suhina GitHub* finds a pull request's thread by it, among the
server's active threads and then the forum's archived ones
([`scripts/discord_api.py`](../scripts/discord_api.py)). The bot only finds and marks threads; the
webhook posts. Both need the secret `DISCORD_BOT_TOKEN` and the variables `DISCORD_GUILD_ID` and
`DISCORD_TESTING_FORUM_ID`, and the bot may manage posts and add reactions in that forum only.

Each test build that `test-build-links.yml` links is posted there
([`scripts/discord_test_build.py`](../scripts/discord_test_build.py)), from the same data as the
pull request comment: the commit, each platform's download and size or its state and log, a link
to the comment, and that downloading needs a GitHub sign-in. The first build opens the thread
(only if some platform built); later builds post *New build is available for testing* in it,
which unarchives it. An existing thread is never renamed. Discord refuses a post in a locked
thread; that is logged.

**Builds linked before.** A build linked before the forum was set up, or while a setting was
missing, reaches Discord only by hand: Actions › Test build links › Run workflow (a maintainer
only), with pull request numbers (`302 303`) or empty for every open pull request.
[`scripts/discord_test_build_backfill.py`](../scripts/discord_test_build_backfill.py) reads each
pull request's test build comment into the same data and posts it the same way, opening the thread
or posting in it. It skips a pull request with no comment, a build whose artifacts have expired,
and a thread whose latest 100 messages already name the commit, so running it twice posts nothing
twice. The run's summary lists what happened to each pull request.

A webhook secret must be the whole URL Discord shows (*Copy Webhook URL*,
`https://discord.com/api/webhooks/…`). One that is not is reported as *the URL does not start with
https://*, never with the URL itself, and nothing is posted.

When a pull request is merged, its thread in the testing forum is marked by the bot
*Suhina GitHub* (`discord.yml`, [`scripts/discord_merged.py`](../scripts/discord_merged.py)). The
bot finds the thread, adds ✅ to its first post, renames it `#<number> [Merged] <title>` (the
number stays first) and archives it. A thread already marked is not renamed again. A merged pull request
cannot be reopened, so a marked thread gets no more builds. No thread, a missing setting or a
Discord error is logged and fails nothing. Discord allows two renames of a thread in ten minutes,
so a rate limit is waited out.

**Commands.** The same Discord application also answers commands: renamed **Suhina** (it was
*Suhina GitHub*), it is a Cloudflare Worker in its own repository,
[suhina-daw/suhina-bot](https://github.com/suhina-daw/suhina-bot), that Discord calls for each
command and that writes to GitHub as the App `suhina-bot` (`suhina-bot[bot]`). It never reads
messages on its own. Its commands are listed in
[docs/commands.md](https://github.com/suhina-daw/suhina-bot/blob/main/docs/commands.md), its
settings, secrets and deployment in
[docs/operations.md](https://github.com/suhina-daw/suhina-bot/blob/main/docs/operations.md); the
plan is suhina-daw/suhina-bot#1. The scripts above keep using the bot token for marking threads.

## The pull_request_target policy

From December 2, 2026, GitHub blocks workflows triggered by `pull_request_target` in public
repositories unless an Actions event policy allows them
([changelog](https://github.blog/changelog/2026-11-02-pull_request_target-blocked-by-default-december-2/)).
The repository has one such policy, *Allow pull_request_target for the safe workflows*
(Settings › Actions › **Policies**), with enforcement **Active**. It targets these workflow files
and no others:

| Workflow | What it does on `pull_request_target` |
|---|---|
| [`ready-to-merge.yml`](../.github/workflows/ready-to-merge.yml) | the `ready to merge` label flow ([Merging many pull requests](#merging-many-pull-requests)) |
| [`test-build-links.yml`](../.github/workflows/test-build-links.yml) | its `synchronize` part; the `workflow_run` and `workflow_dispatch` parts are allowed too |
| [`overlap.yml`](../.github/workflows/overlap.yml) | the Overlap notice ([`workflow.md`](workflow.md) › Claiming) |
| [`pr-labels.yml`](../.github/workflows/pr-labels.yml) | the kind and area labels ([`workflow.md`](workflow.md) › Branch names) |
| [`updater-source.yml`](../.github/workflows/updater-source.yml) | the updater-source check ([`spec/updates.md`](spec/updates.md)) |
| [`discord.yml`](../.github/workflows/discord.yml) | the pull request posts; its `issues` and `release` parts are allowed too ([Discord](#discord)) |

Its *Restrict events* rule keeps every event in the list, so a trigger added to one of these files
later is not blocked silently. Actors are not restricted, so pull requests from forks still start
them. All six follow the safe pattern: they check out the default branch, never the pull request's
head, with `persist-credentials: false`, and read the pull request only as data
([Securely using pull_request_target](https://docs.github.com/en/actions/reference/security/securely-using-pull_request_target)).

A new workflow on `pull_request_target` follows the same pattern and gets its path added to the
policy; without that it never runs. A workflow that needs no write token and no secret uses
`pull_request` instead. Settings › Actions › **Policy insights** lists runs the policy blocked.

## Packages

| Package | Built on | Contents |
|---|---|---|
| `suhina-<version>-linux-x86_64.tar.gz` | Ubuntu 22.04 (glibc 2.35) | `suhina`, `suhina-cli`, `suhina-plugin-host`; needs `libasound2` |
| `suhina-<version>-windows-x86_64.zip` | Windows, MSVC, static C runtime | `suhina.exe`, `suhina-cli.exe`, `suhina-plugin-host.exe`; no Visual C++ redistributable needed |
| `suhina-<version>-macos-universal.tar.gz` | macOS, arm64 + x86_64 joined with `lipo` | `Suhina.app` (with `suhina`, `suhina-cli` and `suhina-plugin-host` in `Contents/MacOS`) and a `suhina-cli` link into it; macOS 11 or later |

The nightly's packages are named `suhina-nightly-<platform>` instead. `suhina` is the desktop app,
`suhina-cli` the command-line tool. `suhina-plugin-host` is the process plug-ins run in
([ADR 0006](decisions/0006-plug-in-hosting.md)); both look for it in their own folder (past a
link), so the three stay side by side.

On macOS the desktop app is an app bundle, `Suhina.app`, which Finder opens with a double-click
and which can be dragged to Applications. [`scripts/macos_app.sh`](../scripts/macos_app.sh), run by
both `release.yml` and `test-build.yml`, moves the three binaries into `Suhina.app/Contents/MacOS`,
writes its `Info.plist` (the bundle id `io.github.suhina-daw.suhina`, the version without its
suffix, macOS 11, the copyright and licence line, and the text macOS shows when Suhina first asks
for the microphone, for the tracks' audio inputs) and `Credits.html`, which puts the whole version
and the licence into Suhina › About Suhina, macOS's own panel ([spec](spec/about.md#on-macos)), signs the bundle ad hoc, and puts a `suhina-cli` link in the package
folder, so `./suhina-cli` still works from it. The app's icon, `Suhina.icns`, is made from
[`assets/brand/suhina-icon.png`](../assets/brand/suhina-icon.png) with `sips` and `iconutil`. While
the app runs, eframe would put its own icon in the Dock over it, so on macOS the desktop app tells
eframe to leave the Dock alone (`apps/suhina/src/main.rs`, `app_icon`): the Dock keeps the bundle's
icon, which macOS draws with rounded corners, where an image set at run time is drawn square. Run
from outside a bundle (`cargo run`), and on the other platforms, the app gives its window the same
PNG, so the Dock and the About panel still show Suhina's icon. The ad hoc signature names no developer; it is
there because Apple silicon runs only signed code and because macOS calls an unsigned downloaded
bundle damaged rather than offering to open it. The icon of the Windows executable file,
the Linux desktop entry and the `.suhina` file type are still to come ([#607](https://github.com/suhina-daw/suhina-daw/issues/607)). The smoke test runs the VST3 test plug-in built into the host through the
packaged CLI; the desktop app needs a window, so only its libraries are checked. Each archive also holds
`LICENSE`, `NOTICE`, `README.md`, `ATTRIBUTION.md` and the licences of the ported code,
`LICENSE-APACHE` (the FM synth) and `LICENSE-signalsmith.txt` (the time stretcher).

## Reminders

Secrets that expire, so a release checks them before they stop a bot in the middle of the next one:

| Secret | Used by | Expires | Renew |
|---|---|---|---|
| `PROJECT_TOKEN` (environment `project-board`) | the claim bot and Project sync ([`claim-bot.md`](claim-bot.md)) | fill in | [`claim-bot.md`](claim-bot.md) › Setting it up |

## Not yet

The binaries are not signed with a developer identity: macOS Gatekeeper and Windows SmartScreen
warn about them. On Windows and Linux the desktop app is a bare executable, not an installer, and
on macOS `Suhina.app` comes in a `.tar.gz`, not a disk image. Installers (AppImage, deb, rpm,
Flatpak, MSI, a macOS disk image), code signing and notarisation, and Linux aarch64 and Windows
arm64 builds come with M8 (ROADMAP).
