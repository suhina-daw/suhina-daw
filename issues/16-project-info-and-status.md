# Project info and project status: where a user sees what a project is and what state it is in

**Type:** enhancement · **Area:** `ui` · **Phase:** M8 (the UI milestone — this is UI work, not later work) · **Priority:** P2

Three questions have no home in the interface today: **what is this project** (its name, its author, where it lives
and how it is stored), **what state is it in** (how many versions, whether anything is unmerged, whether anyone else
is connected), and **what changed between two versions**. The epic answers the third ([ADR 0027](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0027-merge-and-versions.md)
decision 7 and issue 07) and part of the first (issue 15), and neither has a surface. ADR 0027 also creates a new
savable state — **a document carrying a reported, unmerged conflict** — which nothing in the interface can show, and
which is the one state that quietly loses work if a user does not notice it.

**Depends on:** issue 15 (the metadata there is what this shows) and issue 07 (the version browser this opens). The
conflict indicator depends on ADR 0027's conflict policy being written into a spec.

## Goal

A user can see what a project is and what state it is in **without knowing what version control is**, the one state
that needs attention cannot be missed or switched off, and nothing added here duplicates an indicator that already
exists.

## Not a reversal of a decision already made

[`docs/spec/ui-session-view.md`](https://github.com/suhina-daw/suhina-daw/blob/main/docs/spec/ui-session-view.md) records that Suhina *"keeps one 20 px status bar for
messages and counts and has no separate info view"*. **That rejected surface is a pointer-help panel** — help text for
whatever is under the pointer — and this issue is not that. It is worth stating in the spec when this lands, so that
the next reader does not think the two contradict each other.

## Scope

1. **A Project Info surface**: the project's name, its author (issue 15), where it lives and how it is stored (a
   single document, a folder, a folder that is also a repository), how much of its audio is collected and what is
   missing, and an entry point to the versions. **It is a place to look and to set metadata, not a place to do
   version control.**
2. **The version browser is a main-area surface, not a sidebar** (issue 07 owns its behaviour). ADR 0027 has it
   listing versions **and auditioning the tracks that changed**, which is not a sidebar job. The precedent to follow
   is the sample map: large in the main area, or in a window of its own with its own shortcut.
3. **One place for project status, and it is the bottom-left corner of the status bar.** The indicators form a
   **fixed prefix** at the far left and the last message follows them, so the message always begins at the same
   x position whether or not an indicator is present.
   - **A status is not a control, and the control bar is for controls.** No *readout* of project state is added
     there. But **one entry point is**: a button that opens the Project Info surface, placed with the bar's other
     view-changing control, the Session/Arrangement switch. That is the precedent — a control that changes what you
     are looking at rather than what plays — and a button is exactly that kind of thing, which is why it belongs
     where a status readout does not. **One button, not two**: it opens Project Info, and Project Info is the way to
     the versions (Scope 1), so the bar does not grow a second entry for them.
   - **Three routes, each for its own purpose, so none of them is redundant.** The **menu** (`Projects`, which
     already exists) is the canonical one and the only one a user has to find once; the **control-bar button** is the
     fast route for something reached often; the **status prefix** is what tells a user whether there is any reason
     to reach for either.
   - **The status bar is always drawn**, so an indicator that needs action is visible while playing without being
     moved to a different bar. Its severity is carried by **color and weight**, which the bar already does:
     [`docs/design/components.md`](https://github.com/suhina-daw/suhina-daw/blob/main/docs/design/components.md) records errors drawn in the recording red.
   - **A fixed prefix means a fixed budget.** The prefix is capped (the issue records the number and the measurement);
     beyond it, the least important indicator collapses into a count that opens Project Info. **The message keeps the
     rest of the width and truncates**, as it must, rather than the indicators being pushed around by it.
   - **Detail lives in the Project Info surface above**, which is where a user goes when the prefix tells them there
     is something to look at.
   - **On the button's mark.** ADR 0027 decision 7 decided the interface is **not** a version-control interface, and
     Scope 8 below says the words are about versions, changes, a conflict and a connection — never commits, branches
     or hashes. A mark borrowed from version-control tooling is therefore **not forbidden as a shape but it is
     forbidden as a justification**: what settles it is the rule in Scope 4. If the button names itself in words
     ("Versions — 3, one unmerged") the mark only has to be legible; if the mark has to teach a metaphor the user
     does not have, it is the wrong mark. Its exact form is a design decision under `theme` and
     [`docs/design/brief.md`](https://github.com/suhina-daw/suhina-daw/blob/main/docs/design/brief.md), not a borrowing.
4. **The rule that makes an indicator honest: the icon is a state, the words are the message.** An indicator with
   something to say names it in words on hover or on click, and offers the action if there is one; **an indicator
   with nothing to say is not drawn at all.** An unlabelled glyph in a bar is unreadable to anyone who does not
   already know what it means, and a permanently lit icon teaches the user to stop looking at the bar.
5. **Do not duplicate an indicator that already exists.** The list, so that a later change does not add a fourth:
   - unsaved changes — already a dot in the tab and the window title;
   - missing audio — already a bar under the control bar ([`missing-files.md`](https://github.com/suhina-daw/suhina-daw/blob/main/docs/spec/missing-files.md));
   - a plug-in that is loading, missing or crashed — already the device panel (ADR 0006).
   Anything in this issue must add a state that has none.
6. **Configurable, with one thing that is not.** The user chooses which **quiet** indicators are shown, as a setting
   written like every other one (rule 6: a command, and a key in `settings.txt`). **An indicator that needs action
   cannot be hidden**, for the same reason a missing file is shown rather than configured away: hiding it is how work
   is lost without anyone seeing it happen.
7. **Every indicator is a read command and every control is an edit command** (rule 6). The window dispatches
   commands through the same funnel as the CLI and MCP and never touches the model.
8. **The words are about versions, changes, a conflict and a connection — never commits, branches or hashes.**
   ADR 0027 decision 7 already decided the interface is not a version-control interface; the visual language follows
   that, and the design brief governs the look (ADR 0009).
9. **Tests and documents** (AGENTS.md rules 6 and 7): the surfaces are drawn from commands and monitors, so they are
   testable headless like the rest of the window; `docs/design/components.md` gains or amends the control bar and
   status bar rows with measurements, and `docs/spec/ui-session-view.md` gains the region and the sentence above.

## Acceptance criteria

- [ ] A Project Info surface exists, shows the project's name, author, storage and asset state, and is reachable by
      a shortcut and from the menu; it is listed in `docs/design/components.md` with its measurements.
- [ ] The version browser opens from it and is a main-area surface (issue 07), not a sidebar.
- [ ] **The state indicators sit in the status bar's left prefix, and the control bar gains exactly one control** —
      the entry point, placed with the view switch. **No status readout is added to the control bar**, and the rest of
      its measurements in `docs/design/components.md` are unchanged.
- [ ] The entry point is reachable three ways — the menu, the control-bar button and its shortcut — and all three open
      the same surface, so a user who has not found the button is not blocked.
- [ ] The button's mark names itself in words on hover or on click, per Scope 4; a test or a written check shows it
      does not depend on the user knowing a metaphor from another tool.
- [ ] The last message begins at the same x position whether or not any indicator is present, and a long message
      truncates rather than moving the indicators.
- [ ] An unmerged conflict is visible **whenever one exists**, cannot be hidden by a setting, and names itself in
      words when the pointer is over it or it is clicked.
- [ ] No state in the list in Scope 5 gains a second indicator: a test or a written check shows each is already
      reported once.
- [ ] Indicators that do not need action can be switched off and the choice survives a restart; an indicator that
      needs action cannot.
- [ ] No indicator is drawn when it has nothing to say, so the status bar's left prefix is empty in a project with no
      versions, no repository and no peers.
- [ ] The prefix has a stated cap, and going beyond it collapses the least important indicator into a count that opens
      Project Info rather than letting the bar fill up.
- [ ] Nothing here is reachable from the audio callback (rule 3), and the window still runs headless in tests.
- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` pass.
- [ ] Documents updated: `docs/design/components.md`, `docs/spec/ui-session-view.md`, `docs/shortcuts.md` for any new
      key, `ROADMAP.md`, and `docs/commands.md` regenerated on `main` for any new command.

- [ ] The pull request's description says **how to check the change** — numbered steps a
      non-musician can follow from a test build — or, when nothing a user can notice changed, one line
      saying so.
## Not in scope

- **The version browser's behaviour** (issue 07) and **the musical diff** (issue 08). This issue gives them a place
  to appear.
- **Any version-control operation.** Committing, merging and resolving live in their own issues and commands.
- **A commit graph, a branch diagram, or any glyph borrowed from version-control tooling** (ADR 0027 decision 7).
- **A pointer-help info view**, which the session-view spec already decided against.
- **Re-drawing the control bar.** Existing controls keep their place and their measurements; the one entry point is
  added beside the view switch and nothing else moves.

## Notes

- **Where the code goes** (`docs/architecture.md` › Where new code goes): `ui`, drawn from the project and from
  monitors, dispatching commands — the same shape as the existing control bar, which already draws the tempo, the
  transport and the CPU load. Measurements and colors come from `theme`, `docs/design/brief.md` and
  `docs/design/components.md`, never from a value written in a view.
- **A design constraint worth repeating here because it is the easiest one to break:** the control bar is for
  **controls** — including the control that changes what you are looking at. **How full another product's bar is, is
  not a reason to put something in this one** (ADR 0009, and the brief's "Make our own" table), and neither is it a
  reason to refuse something: what decides is whether the thing is a control. **A status readout is not, so it goes to
  the status bar; an entry point is, so it goes to the control bar.** The distinction is worth keeping in writing
  because the two look alike in a mock-up and behave differently in use.
- **Related:** [ADR 0027](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0027-merge-and-versions.md) decision 7 (the author column, and the reported
  conflict that becomes a savable state), ADR 0009 (the look), issue 07 (the version browser), issue 08 (the musical
  diff), issue 15 (the metadata this shows), issue 09 (a message that says what was removed),
  [`docs/design/components.md`](https://github.com/suhina-daw/suhina-daw/blob/main/docs/design/components.md) › the control bar, status bar and menu bar rows,
  [`docs/spec/missing-files.md`](https://github.com/suhina-daw/suhina-daw/blob/main/docs/spec/missing-files.md) (the existing "show it, do not hide it" precedent).
- **Decisions that belong in a spec or the design documents, not here:** the exact measurements and the placement of
  each indicator; whether the indicators are drawn at all when the project has no versions and no repository; whether
  Project Info is a panel, a dialog or a tab; and whether the conflict state can be dismissed as well as resolved.
- **One measurement to take before the layout is drawn, and it is smaller than it first looked.** The place is
  settled — the status bar's left prefix — so what is left is **how wide the prefix may be**: at 20 px, how many
  indicators fit before the message becomes useless, and what the message's remaining width is for a realistic worst
  case (a long path in a message, the full prefix, the counts on the right). That number is the prefix's cap. **The
  answer cannot move the indicators to another bar**, only cap them, because the placement follows from the control
  bar's purpose rather than from the measurement.
- **Board:** `enhancement`; Area `ui`; Phase M8; Priority P2.
