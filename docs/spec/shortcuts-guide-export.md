---
description: Source-owned version-one shortcut data for the Suhina website, its coverage boundary, key tokens and review gate
read_when: website shortcuts, shortcuts.json, keyboard guide export, shortcut documentation coverage
type: spec
status: current
last_verified: 2026-10-21
covers:
  - scripts/shortcuts_guide.py
  - scripts/test_shortcuts_guide.py
  - .github/workflows/settings-guide.yml
  - docs/guide-data/shortcuts-help.json
  - docs/guide-data/shortcuts.json
---

# Website shortcut export

The native repository owns explanations and their review. The website consumes the committed
`docs/guide-data/shortcuts.json` from official `main`, validates its structure and publishes it
alongside the existing settings export. There is no second prose approval in the web repository.
AI-assisted prose is proposed in a pull request and read by a maintainer before merge.

## Coverage

This first directory covers every `WINDOW_ACTIONS` and `DETAIL_ACTIONS` row, including actions
without a default key, plus every action named in `DEFAULTS` and `DETAIL_DEFAULTS`. Default-bound
command labels come from the session and project command registries. Parameterless commands
that have no default binding and are not window actions are outside this directory. They remain
available in Settings → Shortcuts; this export must not claim to list every rebindable command.
Mouse gestures and a user's local keymap import are later additions.

## Contract

The envelope is `{schemaVersion: 1, scope: "default-and-window-actions", entries: [...]}`.
An entry has its stable action `id`, native `name` and `category`, `description`, `context`,
`whenToUse`, `platformNotes`, `sourceFingerprint`, `helpStatus`, `references` and `bindings`.
`references` preserves the validated repository-relative implementation/spec paths from the
authored help. Consumers may omit these contributor references from their user-facing display;
the website does not add raw JSON or source links to individual shortcut explanations.
Categories preserve the native Shortcuts catalog: commands appended from the session/project
registries belong to Commands, even when their IDs begin with edit, projects or transport.

`bindings` maps Select-first, Draw-first and Mouse-first to windows, linux and macos. Each OS
value is an array of alternatives, each an array of key tokens in pressing order. An empty
array means intentionally unbound. Custom is not a fourth default preset: its values depend
on the user's changes. Cmd and Option are explicit macOS tokens. Zoom in omits the unavailable
Cmd+Plus variant on macOS and retains Cmd+=. Draw-first gives Duplicate Ctrl/Cmd+B and Browser
Ctrl/Cmd+Shift+B. The exporter encodes these implementation rules; changing the keymap source
invalidates all authored fingerprints so a reviewer must verify those rules before updating.

## Author and check

`docs/guide-data/shortcuts-help.json` is keyed by action ID and contains the authored fields,
implementation/spec `references` and the checked fingerprint. Read `keymap.rs`, action dispatch
in `app.rs`, relevant piano-roll and command implementations, and `docs/shortcuts.md` and the
matching specs. Where implementation differs from older prose, document the implementation.
The fingerprint detects keymap metadata and algorithm changes, not every downstream behavioral
change: implementation changes still require a documentation review.

Run `python scripts/shortcuts_guide.py` after review; never edit the generated JSON by hand.
Run `python scripts/shortcuts_guide.py --check` and
`python -m unittest discover -s scripts -p "test_shortcuts_guide.py"` to verify deterministic
output, coverage, presets, OS mappings and required help. Existing native docs CI runs the tests;
the settings-guide job also checks the committed export. Unknown, missing or stale covered
actions fail before the website can import them. No app code is executed by the website sync.
