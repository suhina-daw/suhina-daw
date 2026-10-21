---
description: How versions are numbered, how release builds, the nightly and test builds of pull requests are made, and how many pull requests are merged through the merge queue
read_when: release, version, release candidate, tag, binaries, release workflow, nightly, past nightlies, development build, download, test build, pull request build, try a pull request, merge queue, merge group, ready to merge label, auto-merge, update branch, merging many pull requests, batch build, Discord announcement, Discord bot, testing forum, merged thread, release notes, changelog
type: howto
status: current
last_verified: 2026-10-20
covers:
  - .github/workflows/ci.yml
  - .github/workflows/release.yml
  - .github/workflows/discord.yml
  - .github/workflows/test-build.yml
  - .github/workflows/test-build-links.yml
  - .github/workflows/ready-to-merge.yml
  - scripts/branch_update.py
  - scripts/release_notes.py
  - scripts/discord_post.py
  - scripts/discord_merged.py
  - scripts/discord_test_build.py
  - scripts/discord_test_build_backfill.py
  - scripts/discord_api.py
  - apps/suhina/build.rs
  - apps/suhina-cli/build.rs
  - crates/plugin-host/build.rs
---

# Releasing

Releases are built by [`.github/workflows/release.yml`](../.github/workflows/release.yml).

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
workflow artifacts without creating a release, to test the pipeline.

On Windows the three executables carry version information, which the file's Properties ›
Details and Task Manager show: the product name Suhina, what each program is (Suhina, Suhina
command line, Suhina plug-in host), the workspace version and the copyright. Their build scripts
(`build.rs` in `apps/suhina`, `apps/suhina-cli` and `crates/plugin-host`) write it with
`winresource`, so every build has it, nightlies and test builds too. Code signing checks the
product name and version later ([#180](https://github.com/suhina-daw/suhina-daw/issues/180)); the
binaries are not signed yet.

## Nightly

The nightly is how people try the current state between releases: a build of `main`, not a
release. Every 12 hours (02:17 and 14:17 UTC) the workflow builds `main` and publishes it as the release
**`nightly`**, replacing the previous one: its release and tag are deleted and made again on the
new commit, published at once without a draft. A run with no new commit on `main` since
the last nightly builds nothing. Running the workflow by hand from `main` with **nightly** checked publishes one now.

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
to a build that opens their project. The day's second scheduled build is kept as
**`nightly-YYYYMMDD-2`** beside the first; one run by hand replaces the day's first. The nightly job deletes the dated nightlies older than `NIGHTLY_KEEP_DAYS` (30) with
their tags; it touches no other release, and a failure there leaves the new nightly published.

Each new nightly is posted to Discord's releases channel (see [Discord](#discord)): the version,
a link to its dated release, so the link still leads to the build it announced after the next
night, and the changes since the previous nightly (at most 15, as described under
[Release notes](#release-notes)). A failed post leaves the nightly published.

Until Suhina has a release of its own (published, not a release candidate), the nightly is the
repository's **latest** release, so the Releases box on the repository page offers its downloads.
From the first release on, the nightly is published as a pre-release that never becomes the
latest. Nothing is promoted from it: a release is made from a version tag as above.

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
| anyone else's, from a fork or a branch | when a maintainer adds the `test-build` label, of the commit the label was added on |

A pull request with the `test-build-windows` label is built for Windows only, which keeps the
shared runners free while only that platform is being tried; its comment lists the other two as
not built.

Only a pull request that changes what goes into the binaries is built: `apps/`, `crates/`,
`devices/`, `presets/`, `third_party/`, `Cargo.toml`, `Cargo.lock`, `.cargo/` or `test-build.yml`
itself. One that changes only documentation, scripts, other workflows or agent files has no new
binary to try.

The build is of the pull request's head merged with `main`, as the event saw it, so a push after
the label is not what gets built. A push by anyone but a maintainer takes the label off, and the
new commits are built when a maintainer has read them and adds the label again; the comment on such
a build says it is a contributor's. A first-time contributor's run also waits for a maintainer's
*Approve and run*.

The version says which pull request and commit it is: `0.0.1-pr.302+abc1234` in `--version` and
bug reports. Test builds use the release profile without LTO and with 16 codegen units, so they
build faster and run slightly slower than the nightly.

**A branch update is not built.** A push that only merges `main` into the pull request, as
*Update branch* does, leaves the pull request's own changes as they were, so the build already
linked from it is still the one to try. A small first job, *what to build*, asks
[`scripts/branch_update.py`](../scripts/branch_update.py) whether the new head is a merge commit
whose first parent is the previous head, whose second parent is on `main`, and whose own diff is
the same patch as the previous head's (a resolved conflict, or `main` changing lines next to the
pull request's own, makes it a new patch). It also checks that the previous head's run succeeded,
so a failed or cancelled build is never kept as the last one. When all hold, the platforms are
skipped, and `test-build-links.yml`, finding nothing built, leaves the comment linking the last
real build. Anything else, a new commit, a rewritten history or an unknown previous head, is built
as before. The merge queue's branches (below) are never built.

**Several pull requests together:** Actions › Test build › Run workflow with their numbers
(`302 303 304`) merges them onto `main` in that order and builds the result, named
`suhina-prs-302-303-304-<platform>`; each platform's summary links its package. A pull request that does not
merge cleanly fails the run and is named in the error.

The build has a read-only token and no secrets, and for a fork GitHub runs `test-build.yml` as the
fork has it; so `test-build-links.yml`, which runs from `main` and never runs pull request code,
links only runs a maintainer started. The maintainers are listed in both workflows.

Each linked build is also posted to the pull request's thread in the Discord testing forum
(see [Discord](#discord)).

## Merging many pull requests

With many pull requests waiting, merging them one by one through *Update branch* costs a CI run per
pull request per merge, and every merge makes the rest out of date again. Two things help.

**The merge queue.** [`ci.yml`](../.github/workflows/ci.yml) runs on `merge_group`, so the queue's
temporary branch (`gh-readonly-queue/main/...`: `main` with the queued pull requests merged onto it
in order) gets the same jobs as a pull request, and the `ci` check that `main` requires reports
there. Code checks compare the group with the `main` it is built on; the steps that need a pull
request (the sign-off check for forks, the list of documents to update, the guard on generated
files) were run on each pull request before it was queued and skip in the queue. With the queue on,
a maintainer presses *Merge when ready* on each green pull request: the queue tests several of them
in one CI run, merges them in order, and takes out a pull request that fails, with no *Update
branch* by hand. The test build never runs on the queue.

**Which changes build Rust.** In a pull request, the merge queue and on `main`, `ci.yml`'s
`changes` job skips fmt, clippy and the tests when every changed file is documentation (`docs/`,
any `.md`, `LICENSE`, `NOTICE`), a script under `scripts/` or a file under `.github/`: no Rust
build or test reads them. A change to `ci.yml` itself still runs every job. The `docs` job runs on
every change, so the script tests and the tests of the claim bot and the Depends on check still run; in the Actions tab the
skipped jobs show as skipped and `ci` passes.

The queue is a setting, not code: Settings › Rules (or Branches) › the rule for `main` › **Require
merge queue**, with the merge method used today, a group size of about 5 and *Only merge
non-failing pull requests*. With a queue, *Require branches to be up to date before merging* is not
needed. Every check the rule requires must come from a workflow that runs on `merge_group`; today
those are `ci` and `Depends on` ([`claim-bot.md`](claim-bot.md) › The Depends on check), and a check
required from another workflow would keep the queue waiting.

Without a queue, turn off *Require branches to be up to date before merging* and merge each green
pull request without updating it; CI on `main`, which runs on every push, catches the rare
conflict in meaning between two of them.

**The `ready to merge` label.** A maintainer who has read a pull request adds the label instead of
coming back to press *Merge when ready* once CI is green.
[`ready-to-merge.yml`](../.github/workflows/ready-to-merge.yml) (`pull_request_target`, never
checking out or running the pull request's code) then:

| Event | What happens |
|---|---|
| a maintainer adds the label | the app approves the head commit as the event saw it (the rule for `main` needs one approval, and a maintainer cannot approve their own pull request) and turns on auto-merge with that commit as the expected head; the queue takes the pull request once `ci` is green and every review thread is resolved |
| the label on a contributor's pull request without `test-build` | queued as usual when a maintainer has approved that same head commit in a review; otherwise the label comes off, with a comment: a maintainer approves the commit they read, or tries its build first |
| the label on a Dependabot Cargo update (branch `dependabot/cargo/…`) | as for a maintainer's own pull request: no `test-build` needed, the queue's build and tests check the new crate; an update of the Actions still needs `test-build`, since `ci` runs no Rust jobs for a change under `.github/` |
| anyone else adds the label | it comes off |
| a push by a maintainer | the label stays; the app approves the new head and turns auto-merge on for it |
| a push by anyone else | the label comes off, so the new commits wait to be read (the rule for `main` dismisses the older approval) |
| the label comes off | auto-merge off, out of the queue, and the app's approvals dismissed; a call that fails fails the run. When the label was added again meanwhile, the run fails and asks for the label to be taken off and added again, so the pull request waits rather than merging |

The merge method is the queue's (squash). Agents never add the label (`AGENTS.md` › Tasks).

It acts with a short-lived token of the GitHub App that also pushes the generated docs
(`docs-generated.yml`), not with `GITHUB_TOKEN`: GitHub starts no workflows for a merge made through
`GITHUB_TOKEN`, and those on `main` must run after every merge. Setup, once: the app has Repository
permissions › **Contents** and **Pull requests**: Read and write; its public Client ID is the
`DOCS_BOT_CLIENT_ID` variable and its private key the `DOCS_BOT_PRIVATE_KEY` secret in the
`docs-bot` environment, which `pull_request_target` may use
because it runs on `main`; and Settings › General › **Allow auto-merge** is on. Without that setting the
*approve and queue* job fails, and nothing merges. The maintainers are listed in the workflow as in
the test build's.

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
network, no AI service). A change is one commit on `main`'s first-parent line, so one squash-merged
pull request, listed by its title with a link to the pull request. Changes that touch only
documentation, CI, scripts and tests are counted, not listed. Pull request titles are the release
notes, so they say what changed for a user: "Clip fades: shapes and an adjustable curve".

- **Nightly:** the changes since the previous nightly, on its release page and in the Discord post.
- **Draft of a tagged release:** first *New and changed*, the bullets of ROADMAP.md › What works now
  that are new or reworded since the previous release, by milestone, then *All changes*. A
  release candidate counts from the tag before it, a release from the previous release, the first
  from the beginning. Edit the draft before publishing: the ROADMAP bullets describe what works,
  not what changed, and a reworded bullet may be a fix of its text.

Its tests run in CI: `python3 -m unittest discover -s scripts -p 'test_*.py'`.

## Discord

Three channels get posts from GitHub Actions, each through a webhook (Discord: channel settings ›
Integrations › Webhooks) whose URL is a repository secret (GitHub: Settings › Secrets and
variables › Actions). A channel without its secret gets nothing, and a failed post fails nothing
else. No mention in a post pings anyone.

| Channel | Secret | Posts |
|---|---|---|
| releases | `DISCORD_WEBHOOK_RELEASES` | each nightly (`release.yml`); each release when a person publishes it, with its *New and changed* section (`discord.yml`) |
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

## Packages

| Package | Built on | Contents |
|---|---|---|
| `suhina-<version>-linux-x86_64.tar.gz` | Ubuntu 22.04 (glibc 2.35) | `suhina`, `suhina-cli`, `suhina-plugin-host`; needs `libasound2` |
| `suhina-<version>-windows-x86_64.zip` | Windows, MSVC, static C runtime | `suhina.exe`, `suhina-cli.exe`, `suhina-plugin-host.exe`; no Visual C++ redistributable needed |
| `suhina-<version>-macos-universal.tar.gz` | macOS, arm64 + x86_64 joined with `lipo` | `suhina`, `suhina-cli`, `suhina-plugin-host`; macOS 11 or later |

The nightly's packages are named `suhina-nightly-<platform>` instead. `suhina` is the desktop app,
`suhina-cli` the command-line tool. `suhina-plugin-host` is the process plug-ins run in
([ADR 0006](decisions/0006-plug-in-hosting.md)); both look for it in their own folder, so the
three stay side by side. The smoke test runs the VST3 test plug-in built into the host through the
packaged CLI; the desktop app needs a window, so only its libraries are checked. Each archive also holds
`LICENSE`, `NOTICE`, `README.md`, `ATTRIBUTION.md` and the licences of the ported code,
`LICENSE-APACHE` (the FM synth) and `LICENSE-signalsmith.txt` (the time stretcher).

## Reminders

Secrets that expire, so a release checks them before they stop a bot in the middle of the next one:

| Secret | Used by | Expires | Renew |
|---|---|---|---|
| `PROJECT_TOKEN` | the claim bot and Project sync ([`claim-bot.md`](claim-bot.md)) | fill in | [`claim-bot.md`](claim-bot.md) › Setting it up |

## Not yet

The binaries are not signed: macOS Gatekeeper and Windows SmartScreen warn about them. The
desktop app is a bare executable, not an installer or a macOS app bundle. Installers (AppImage,
deb, rpm, Flatpak, MSI), a macOS `.app`, code signing and notarisation, and Linux aarch64 and
Windows arm64 builds come with M8 (ROADMAP).
