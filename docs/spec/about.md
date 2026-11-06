---
description: Help › About Suhina, the dialog that says which build this is (logo, version, platform, licence) and copies one line naming it for a bug report; and macOS's own Suhina › About Suhina panel
read_when: about, version, build number, which build, nightly version, bug report, help menu, about dialog
type: spec
status: draft
last_verified: 2026-10-28
covers:
  - crates/ui/src/app/project_dialog.rs
  - scripts/macos_app.sh
---

# About Suhina

Testers report bugs against nightlies that change every night, so the app has to say which build
it is, in a place a user finds without asking.

## Observed in Live

From Ableton's public help article [Installed Live
Version](https://help.ableton.com/hc/en-us/articles/209072309-Installed-Live-Version): the
installed version is found under Help › About Live on Windows and under the application menu
(Live › About Live) on macOS. It opens a splash-like window that lists the edition, the version,
the serial number, the registered account and, in some releases, whether the build is 32 or
64 bit. Support asks for this version in bug reports.

## Suhina

Suhina's menu bar is drawn inside the window on every platform, so the item is in one place
everywhere: **Help › About Suhina**, the last menu. It can also be bound to a key
(`help.about`, Settings › Keys). It opens a dialog over the window, drawn like the other
dialogs, with Suhina's logo at the top (the name with the wave, as on the splash) and under it:

| Row | Shows |
|---|---|
| Version | the workspace version: `0.0.1` for a release, `0.0.1-nightly.20261013+a15ad35` for a nightly, the night and the commit ([releasing](../releasing.md#nightly)) |
| Build | the build string: `20261016+a15ad35`, the checked-out commit's date (UTC) and its hash to seven characters, the form a nightly's version ends in; `unknown` where there is no git, and a release's CI sets the environment instead |
| Platform | the operating system and architecture the build is for: `linux x86_64` |
| Licence | GPL-3.0-or-later, free software |

**Copy version** puts one line on the clipboard, `Suhina 0.0.1 build 20261016+a15ad35 (linux
x86_64)`, and the status bar says what was copied; the dialog stays open. Close, Escape and
Enter close it.

Suhina has no edition, serial number or account, so none is shown.

## On macOS

macOS also gives every app an application menu with its own **About Suhina** item, which opens
macOS's standard About panel rather than Suhina's dialog. It shows the same build:

| Part | Comes from |
|---|---|
| Icon | the bundle's icon, `Suhina.icns`, made from `assets/brand/suhina-icon.png`; run from outside a bundle, the icon the app gives its window, the same image |
| Name and *Version 0.0.1 (0.0.1)* | `CFBundleName` and the version without its suffix, in `Info.plist` |
| The whole version and the licence | `Contents/Resources/Credits.html`, which `scripts/macos_app.sh` writes |
| *© 2026 The Suhina contributors. GPL-3.0-or-later, free software.* | `NSHumanReadableCopyright` in `Info.plist` |

The panel has no Build row and no Copy version button; Help › About Suhina has both.
