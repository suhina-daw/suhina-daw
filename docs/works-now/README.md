# What works now, one file per pull request

A pull request that changes what works for users adds its bullet for
[`ROADMAP.md`](../../ROADMAP.md) › *What works now* here, as a file of its own, instead of editing
`ROADMAP.md`:

```text
docs/works-now/<section>/<name>.md
```

- `<section>` is `top` for the list right under *What works now*, or the milestone whose list it
  joins: `m0` to `m8`, as in `### M4 Clip launching`.
- `<name>` is unique: the branch's name will do (`feature-piano-roll-workflow.md`).
- The file holds one or more bullets in the ROADMAP's style: a line starting `- `, continued on
  lines indented by two spaces, no blank lines and no headings.

After the merge, [`docs-generated.yml`](../../.github/workflows/docs-generated.yml) adds each file
to the end of its section on `main` and deletes it (`python scripts/works_now.py fold`). CI checks
the files with `python scripts/works_now.py check`. A change to a bullet already in `ROADMAP.md`
(rewording it, or removing a limit it names) is still made in `ROADMAP.md`.

These files are not documents: `scripts/docs.py` skips this folder, and they carry no front matter.
