---
name: suhina-engine
description: How to test and change Suhina's audio engine (`crates/engine`) - offline renders as the oracle, rendering launches, onset and golden-file helpers, test projects and audio fixtures, and where engine tests live. Use when writing or fixing an engine test, changing playback, the clip launcher, tempo, fades, loops or mixing, or investigating something that plays late, early or wrong.
---

Offline renders and real-time playback run the same `Renderer::process` (AGENTS.md rule 4), so
an engine change is tested by rendering offline and comparing the samples. The engine reads no
files; tests hand it projects and audio. Before finishing, the checks of
[`working-on-suhina`](../working-on-suhina/SKILL.md) apply.

## An engine test

1. **Where:** in `crates/engine/src/<module>_tests.rs`, the test file of the module it tests
   (`launcher_tests.rs`, `fade_tests.rs`, `tempo_tests.rs`, `loop_tests.rs` ...), included by
   `#[cfg(test)] #[path = "<module>_tests.rs"] mod tests;`. CI fails on an inline test module
   over 150 lines (`scripts/inline_tests.py`).
2. **A project:** `suhina_test_support::ProjectBuilder` (`midi_track`, `audio_track`, `notes`,
   `audio`, `clip`, `scene`, `slot`, `tempo`, `with` for anything else, then `build`). A test
   signal: `suhina_test_support::sine`; measures: `peak`, `rms`.
3. **Render:**
   - the arrangement: `suhina_engine::render_offline(&project, sample_rate, block_frames)`, or
     `render_offline_from` with `RenderInputs` (audio, plug-ins) and `RenderOptions` (one node of
     the track tree);
   - the launcher: `suhina_engine::render_launches(&project, &audio, sample_rate, &[(seconds,
     LaunchCommand::..)], seconds)`, the same path a live `Controller` drives;
   - step by step: `Renderer::stopped(&project, &audio, sample_rate)`, then
     `renderer.handle(LaunchCommand::..)` and `renderer.process(&mut block)` a fixed number of
     times. Never against the clock or an audio device.
   A low sample rate (8 kHz) keeps renders small.
4. **Check:**
   - when sound starts: `onsets` and `assert_onsets` in `launcher_tests.rs` (frames where sound
     starts after silence, within 2 frames); `tempo_tests.rs` and `follow_tests.rs` have their
     own variants;
   - against a file: `suhina_test_support::assert_golden_f32(path, &render, tolerance)` with the
     file in `crates/engine/tests/golden/` and a line for it in that folder's `README.md`.
     `SUHINA_BLESS=1 cargo test -p suhina-engine <name>` writes the file; look at what it wrote
     before committing it.
5. **Audio fixtures:** build an `AudioPool` in the test from samples (a constant, a ramp, a
   sine) rather than reading a file; a WAV a test must read is written with
   `suhina_test_support::write_wav` into a temporary folder.

## Changing what plays

- A change that a user can hear changes a golden render on purpose: bless it, compare it to the
  old file, and say in the pull request what changed and why.
- Code reachable from the audio callback never allocates, frees, locks, logs or does I/O
  (AGENTS.md rule 3).
- Session-level tests of commands against an offline renderer are in
  `crates/session/tests/launcher.rs` (`project()`, `playing()`, `advance`; commands run through `support::run`).
