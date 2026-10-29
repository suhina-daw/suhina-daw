---
description: Checking for a newer build of Suhina and installing it from inside the app, the nightly channel and, in developer mode, test builds of pull requests through the GitHub CLI
read_when: update, updates, new version available, check for updates, download update, nightly update, auto update, test build, gh, github cli, developer mode
type: spec
status: draft
last_verified: 2026-10-28
covers:
  - crates/update/src/lib.rs
  - crates/update/src/version.rs
  - crates/update/src/github.rs
  - crates/update/src/net.rs
  - crates/update/src/install.rs
  - crates/ui/src/app/updates.rs
  - crates/ui/src/settings/update_settings.rs
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
- **Test builds:** `GET https://api.github.com/repos/suhina-daw/suhina-daw/actions/artifacts`

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

While a newer build is available, a line between the actions and the list says *New version
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

## Channels

Settings › Agents and updates › Updates › *Channel*:

- **Nightly** (the default): the `nightly` release.
- **Test builds:** only listed in developer mode (*Developer mode* on, or when the running build is
  itself a test build). The test builds of pull requests, newest per pull request, for this
  platform (artifacts named `suhina-pr<number>-<platform>`; builds of several pull requests
  together are not listed). Help › *Check for updates…* lists them and installs the one picked;
  the start panel offers a newer build of the pull request the running build is from.

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
