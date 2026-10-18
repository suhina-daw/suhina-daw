---
description: Help › About Suhina, the dialog that says which build this is (version, platform, licence) and copies one line naming it for a bug report
read_when: about, version, build number, which build, nightly version, bug report, help menu, about dialog
type: spec
status: draft
last_verified: 2026-10-14
covers:
  - crates/ui/src/app/project_dialog.rs
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
(`help.about`, Settings › Shortcuts). It opens a dialog over the window, drawn like the other
dialogs:

| Row | Shows |
|---|---|
| Version | the workspace version: `0.0.1` for a release, `0.0.1-nightly.20261013+a15ad35` for a nightly, the night and the commit ([releasing](../releasing.md#nightly)) |
| Build | the build string: `2026_10_16_a15ad35`, the checked-out commit's date and abbreviated hash; `unknown` where there is no git, and a release's CI sets the environment instead |
| Platform | the operating system and architecture the build is for: `linux x86_64` |
| Licence | GPL-3.0-or-later, free software |

**Copy version** puts one line on the clipboard, `Suhina 0.0.1 build 2026_10_16_a15ad35 (linux
x86_64)`, and the status bar says what was copied; the dialog stays open. Close, Escape and
Enter close it.

Suhina has no edition, serial number or account, so none is shown.
