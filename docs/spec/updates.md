---
description: Checking for a newer build of Suhina and installing it from inside the app, the nightly channel and, in developer mode, test builds of pull requests through the GitHub CLI
read_when: update, updates, new version available, check for updates, download update, nightly update, auto update, test build, gh, github cli, developer mode
type: spec
status: draft
last_verified: 2026-11-13
covers:
  - crates/update/src/lib.rs
  - crates/update/src/version.rs
  - crates/update/src/github.rs
  - crates/update/src/net.rs
  - crates/update/src/install.rs
  - crates/ui/src/app/updates.rs
  - crates/ui/src/settings/update_settings.rs
  - scripts/updater_source.py
  - scripts/test_updater_source.py
  - .github/workflows/updater-source.yml
  - .github/CODEOWNERS
---

# Updates

Nightlies change twice a day (see [releasing.md › Nightly](../releasing.md#nightly)), and a
tester who has to visit the releases page to find out is usually a few builds behind. The start
panel says when a newer build is out, and one button downloads it, puts it in place of the
running one and restarts Suhina.

## What is sent

The check only reads. When Settings › Agents and updates › Updates › *Check for updates* is on (the default),
Suhina asks GitHub once per start, in the background, which build is the newest in its channel:

- **Nightly:** `GET https://api.github.com/repos/suhina-daw/suhina-daw/releases/tags/nightly`
- **Test builds:** `GET https://api.github.com/repos/suhina-daw/suhina-daw/actions/artifacts`,
  one request whatever the number of pull requests: what makes a build trusted is in the same
  list (see *Channels*). GitHub allows a computer 60 such requests an hour without a sign-in.

Nothing about the user, the projects or the machine is in the request; it carries a
`User-Agent: Suhina/<version>` header, as GitHub asks every client to. As with any web request,
GitHub sees the computer's IP address and that a request was made. Turned off, Suhina makes no
request unless the user picks Help › *Check for updates…* › *Check now*. No connection (offline, a
firewall, GitHub down) is not an error on the start panel: the check gives up after a few
seconds and shows nothing.

The request goes through the system's own `curl` (in Windows 10 and later, macOS and almost every
Linux), which uses the operating system's certificates. Suhina's binary carries no TLS library of
its own for this one request. Without `curl`, Help › *Check for updates…* says so.

## When a build is newer

Versions are the ones of [releasing.md › Versions](../releasing.md#versions):

| Running | The channel's newest | Newer when |
|---|---|---|
| a nightly, `0.0.1-nightly.20261013+a15ad35` | the nightly | its day is later, or the same day with another commit (the afternoon's build) |
| a release or candidate, `0.1.0`, `0.1.0-rc.1` | the nightly | its version is greater (semver) |
| a test build, `0.0.1-pr.663+4d01080` | test builds | the same pull request has a build of another commit |
| a test build, or a local build `0.0.1` | the nightly | never on its own; *Check now* names the nightly and offers to install it |

A local build (`cargo run`) is never told it is behind: it has no channel, and replacing a
`target/` folder with a nightly helps nobody.

## The start panel

While a newer build is available, a line between the actions and the list says *New nightly
available: Suhina 0.0.1-nightly.20261018+c176a33* with **Download update now** and a link to the
release page. The button:

1. downloads the platform's package (`suhina-nightly-windows-x86_64.zip`,
   `suhina-nightly-macos-universal.tar.gz` or `suhina-nightly-linux-x86_64.tar.gz`, see
   [releasing.md › Packages](../releasing.md#packages)) to a folder beside the running program,
   with a progress bar;
2. checks its SHA-256 against the `digest` GitHub lists for the asset, and stops on a mismatch;
3. unpacks it and puts each file in place of the running one (below);
4. says *Update installed* with **Restart now**, which quits as File › Quit does (asking about
   unsaved projects) and starts the new build.

A failure (no space, a folder Suhina may not write to, a checksum that does not match) is shown on
the same line with the reason, and the running build is left as it was.

When the download cannot start at all (a test build while Suhina is not connected to gh, no folder
the program is in), the button opens Help › Check for updates…, which says why and has *Connect
Suhina to your gh* when a sign-in is what is missing. The button never does nothing.

A test build is offered as *New test build available: the test build of #663 (…)*.

Nothing is downloaded or installed until the user presses the button: the check only tells.

## With a project open

The same line shows as a bar across the top of the workspace, under the toolbar, where the
missing-files bar shows. The prompt to open a project and the missing-files bar go first while
either has something to say. The bar is part of the window, not a dialog, and does not take the
keyboard. Besides **Download update now** and **What's new** it has **Later**, which hides the
offer of that build until a newer one is out or Suhina starts again. The download's progress,
*Restart now* and a failure show in the same bar. While the bar waits on the person it has
**Later**:

- on an installed update, beside *Restart now*: the bar goes, and *Restart now* stays in Help ›
  Check for updates…;
- on a failure, beside *Try again*: the bar goes until the updater has something new to say (a
  new install, a new failure, a newer build).

A download or an install in progress has nothing to put off and shows no **Later**.

## Installing in place

The new files go where the running `suhina` is: the folder the package was unpacked to.

- **Windows** does not let a running program's file be overwritten, but does let it be renamed. Each
  file the package replaces is renamed to `<name>.old` first, and the new one is moved in. The next
  start deletes the `.old` files. If any move fails, the files moved so far are put back.
- **macOS and Linux:** each new file is moved over the old one. The running program keeps the old
  file open until it quits, so nothing breaks while it runs. The executable bit comes from the
  package.
- **macOS, in `Suhina.app`:** the bundle is replaced whole, since its signature covers every file in
  it. The download is staged beside the bundle (in Applications, when it was put there), the
  running bundle is renamed to `Suhina.app.old` and the new one moved in its place; if the move
  fails the old one is put back. The next start deletes `Suhina.app.old`, and nothing else beside
  it. A build run from outside a bundle (one unpacked before the packages had one) takes the
  files from the new package's `Suhina.app/Contents/MacOS` into its own folder.

The packages are not signed (releasing.md › Packages), so the update is as trusted as the
download page: the checksum catches a broken download, not a replaced release.

## Where builds come from

Suhina installs and runs what the updater downloads, so a pull request that changes where it looks
must not pass unnoticed, however large it is. Three things hold, and none of them can be turned off
by the pull request alone:

- **Code owner's review.** `.github/CODEOWNERS` names the repository owner for `crates/update/`, as
  it does for `.github/` and `scripts/`. GitHub reads the file from the base branch, and the main
  ruleset requires that review, so `ready to merge` alone does not merge a change here (checked on
  2026-11-13, CONTRIBUTING.md › Merging many pull requests: the app's approval, although the app is
  a bypass actor of the ruleset, left the pull request blocked until the owner approved). The crate
  holds everything that decides where the updater looks and what it installs: the addresses, which
  test builds are trusted, the checksum, unpacking and putting the build in place. The window's
  files (`crates/ui/src/app/updates.rs`, `crates/ui/src/settings/update_settings.rs`) only show what
  the crate found and hand the pressed button back, so they follow the usual review; a change there
  that starts a download of its own is the case the paragraph below the list names.
- **The required check.** `scripts/updater_source.py` runs in the `docs` job of `ci.yml`, under the
  `ci` check main requires, in the merge queue too. It fails when `crates/update/src/github.rs` no
  longer holds the pinned lines (`REPOSITORY`, `NIGHTLY_URL`, `NIGHTLY_PAGE`, `ARTIFACTS_URL` and the
  names that pick a package), when a manifest (`Cargo.toml`, `crates/ui/Cargo.toml`,
  `apps/suhina/Cargo.toml`) names `suhina-update` on any line but the one that points it at
  `crates/update`, or when a file the updater is built from holds a URL other than the ones it uses,
  whole: a prefix of this repository is not enough, since GitHub serves a fork's commits under this
  repository's address too. Changing the check itself means changing `.github/` or `scripts/`: the
  first point again.
- **The same check from main.** `.github/workflows/updater-source.yml` runs the script as it is on
  main against the pull request's commit, read as text and never run (`pull_request_target`). A pull
  request cannot edit it, so it stays red with the lines that changed even when the pull request also
  rewrites the pins, and the code owner sees it. It cannot be a required check, since
  `pull_request_target` does not run in the merge queue.

The pinned lines are the guard; the URL scan is a tripwire for the plain form of a changed address.
It reads text, so it does not see a URL put together at run time, or a host written with no scheme,
which curl takes as well; and code outside the watched files can start a download of its own. The
code owner's review covers the first two, and ordinary review the last. Moving the project to
another repository changes the pinned lines and the constants together; the check from main is red
on that pull request by design.

## Channels

Settings › Agents and updates › Updates › *Channel*:

- **Nightly** (the default): the `nightly` release.
- **Test builds:** only listed in developer mode (*Developer mode* on, or when the running build is
  itself a test build). The test builds of pull requests, newest per pull request, for this
  platform (artifacts named `suhina-pr<number>-<platform>`; builds of several pull requests
  together are not listed). Help › *Check for updates…* lists them and installs the one picked;
  the start panel offers a newer build of the pull request the running build is from.
  Only trusted builds are listed. An artifact's name says nothing about who made it: a pull request
  from a fork runs the workflow files as the fork has them, so it can upload a build under any
  pull request's name. A build is listed when its run is of a branch in this repository, which only
  people who can write to it can push, or when the same list holds its marker: an artifact named
  `suhina-linked-pr<number>-run<run id>`, for that pull request and the build's run, uploaded by a
  run of this repository. `test-build-links.yml` uploads it from `main` when it links a fork's
  build, which it does only after a `/test-build` request for its commit ([releasing.md › Test
  builds](../releasing.md#test-builds)); no fork can upload into a run of this repository. A
  marker from a fork's run, or for another pull request or run, marks nothing. An artifact without
  its run's repository ids is not used, and an untrusted build never replaces a trusted one as the
  newest.

### Connecting to the GitHub CLI

Listing test builds needs no sign-in, but downloading one does
([releasing.md › Test builds](../releasing.md#test-builds)). Suhina never signs in by itself and
never reads a token it was not handed:

- When the GitHub CLI (`gh`) is on the `PATH`, Help › *Check for updates…* shows **Connect Suhina
  to your gh**. Only that click runs `gh auth token`, once, and keeps the token in memory until
  Suhina quits or *Disconnect* is pressed. It is never written to a file, and the next start asks
  again.
- Without `gh`, the dialog says that downloading a test build needs the GitHub CLI, signed in with
  `gh auth login`.

The token is passed to `curl` on its standard input, not on the command line, so other programs
listing processes do not see it, and `curl` drops it when GitHub redirects the download to its
storage host.

## Settings

| Setting | Key | Default |
|---|---|---|
| Check for updates (once per start) | `check_updates = on \| off` | on |
| Channel | `update_channel = nightly \| test_builds` | nightly |
| Developer mode | `developer_mode = on \| off` | off |

Help › *Check for updates…* checks now whatever the setting says.
