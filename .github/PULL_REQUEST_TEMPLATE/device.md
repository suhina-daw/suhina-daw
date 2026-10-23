## The effect or instrument

<!-- What it does and who it is for, its parameters in a line each, and the issue it closes
     ("Closes #123"). -->

## Where the knowledge and the DSP come from

<!-- Own design, or the open-source project the DSP is ported from (name, licence, version). If it
     is meant to stand in for another product's device on import, the row in
     docs/spec/device-import.md. -->

## How I tested it

<!-- What you played through it by hand, on which OS, audio device and buffer size, and what you
     heard. -->

## Checklist

**Clean-room and licences**
- [ ] Nothing comes from disassembled, decompiled or leaked material, or from a proprietary
      product's content (samples, presets, artwork, interface text, visual design).
- [ ] Ported DSP has a GPLv3-compatible licence, keeps its copyright notices and has a row in
      [`ATTRIBUTION.md`](https://github.com/suhina-daw/suhina-daw/blob/main/ATTRIBUTION.md).
- [ ] Every commit ends with a `Signed-off-by:` line (`git commit -s`).

**Built like the others** ([ADR 0004](https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0004-built-in-devices.md))
- [ ] A crate `devices/<name>` named `suhina-device-<name>`, implementing the `Device` trait,
      added to `device-registry` (so `device.list`, `device.add` and `device.setParam` reach it
      from the UI, CLI and MCP) and to the workspace.
- [ ] Every parameter has a stable id, a range, a default and a unit; ids never change once merged.
- [ ] The audio path never allocates, locks, blocks, logs or does I/O, and nothing can panic; it
      handles any sample rate and block size, and reports its latency.

**Tested**
- [ ] The `device-api` conformance tests run for it, plus golden renders of its sound.
- [ ] I played it by hand and it sounds as intended.
- [ ] `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`
      and `python scripts/rename_suhina.py --check` pass.

**Documented**
- [ ] AGENTS.md's crate map, ROADMAP › Built-in effects and instruments, and
      `docs/spec/device-import.md` if it imports another product's device.
