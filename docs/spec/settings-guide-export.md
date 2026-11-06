---
description: Source-owned settings help, its versioned website export and the coverage gate reviewed with native changes
read_when: website settings, settings.json export, settings help, web guide, documentation synchronization, settings-guide CI
type: spec
status: current
last_verified: 2026-10-21
covers:
  - scripts/settings_guide.py
  - scripts/test_settings_guide.py
  - .github/workflows/settings-guide.yml
---

# Settings guide export

The settings guide is maintained and reviewed in this repository together with the native
implementation. The website consumes it from this repository's `main` branch. It must not
read a developer's checkout, a feature branch or a pull request, nor ask for another content
approval after the source review.

## Source and review

`docs/guide-data/settings-help.json` contains the English descriptions, choice explanations,
practical when-to-change guidance and implementation references keyed by setting ID. It was
migrated from the Suhina website guide and covers the current native settings registry,
including Arrangement colour and track-size controls, experimental liquid strengths and
the theme editor. References name Suhina's own code and specs, not proprietary material.

When adding or changing a setting, read its control in
`crates/ui/src/settings/settings_rows.rs`, its implementation and the relevant spec, and
update its authored help in the same pull request. Explain individual controls of grouped
panels, dependencies and cases where the setting has no effect. Non-choice controls use an
empty `options` array. Keep text plain, not HTML. AI can prepare text for review; merging
the native pull request is the content approval, not the author's use of `helpStatus`.

The source fingerprint hashes the page, section, label, native note and saved keys. Copy it
into authored help only after checking the setting. It detects metadata changes, not every
implementation change; behavior-only changes still require help review. Current IDs include
the native location, so a page/section move requires migrating the help ID too.

`workInProgress: true` marks an incomplete implementation independently of documentation
completeness. The website renders it as a yellow Work in progress pill. It currently marks
Warp mode's shared stretching backend and the plug-in library's unavailable VST2 hosting.

## Export contract

Run from the repository root:

```sh
python scripts/settings_guide.py
python scripts/settings_guide.py --check
python -m unittest discover -s scripts -p 'test_settings_guide.py'
```

The first command writes `docs/guide-data/settings.json`. Commit this deterministic export
with the authored change. `--check` writes nothing and rejects stale output, missing or
stale help, unknown help IDs, invalid descriptions/options/status and invalid reference
paths. It also fails if a registry row no longer follows the parsed source format, rather
than silently exporting only part of the table. No native executable or audio device is
needed. The source CI workflow checks pull requests, main and merge groups.

Version 1 is an object with `schemaVersion: 1` and `entries`. Each entry contains `id`,
`page`, `section`, `name`, `note`, `keys`, `sourceFingerprint`, `helpStatus`, `description`,
`options`, `whenToChange` and `workInProgress`. Every exported entry has complete help and
`helpStatus: reviewed`. Contributor references remain in the authored input rather than
being rendered as links beside individual settings. An unknown contract version must be
rejected by the consumer.

The tracked JSON cannot contain its own Git commit ID without a circular reference. The
website records the actual native `main` commit it fetched in a separate provenance file.
It reads the committed blob, never uncommitted source files. Unchanged data does not cause
a new website commit merely because unrelated native code changed.

## Delivery

The companion website workflow checks `main` about every 30 minutes and can also be run
manually. It imports only the committed file above, validates the contract, builds and tests
the site, then records the update. When website publication is enabled, this source-approved
update deploys automatically after technical checks. No cross-repository secret or direct
push from native CI to the website is needed. Until a valid export is on native `main`, the
website keeps its previous data.

The existing always-running docs job discovers `test_settings_guide.py`, including its
real-repository test of complete coverage and current output. Its required `ci` aggregate
therefore includes this gate without changing build-selection rules or requiring Rust
builds for documentation-only updates. The dedicated workflow also reports the explicit
Settings guide export / settings-guide result.

Shortcut export is a follow-up contract: include action names, explanations, categories,
contexts, presets and OS-specific bindings from the native action registry. This first
slice exports settings only and does not change keymaps or the UI.
