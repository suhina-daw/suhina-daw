# Golden renders of the engine

Tests in `src/fade_tests.rs` render small projects offline and compare the left channel, sample
for sample (within 1e-6), with the files here. Offline renders are the oracle for real-time
playback: both run the same `Renderer::process` (AGENTS.md › One render path).

- `fade-in.f32`: a 2-beat audio clip at 120 bpm, 8 kHz, playing a constant 0.25, with a
  1-beat fade-in.
- `fade-out.f32`: the same clip with a half-beat fade-out.
- `loop-1-2.f32` (`src/loop_tests.rs`): a 4-beat audio clip playing a rising ramp, with the loop
  region from beat 1 to beat 2 on, played for 3 seconds: two beats straight, then the second beat
  four times. The test also checks each pass against a render without the loop.
- `launched-loop-fade-in.f32`: the same 2-beat clip in a launcher slot, looping, with a 1-beat
  fade-in and a half-beat fade-out, launched at once and rendered for three passes through
  `render_launches`: it fades in once and never fades out (`docs/spec/clip-launching.md` › Clip
  fades).
- `launched-stretched.f32` (`src/launcher/prepare/tests.rs`): at 240 bpm, a looping 440 Hz sine
  stretched to 0.9 times its speed launched at once, a legato 330 Hz sine at 1.25 times taking
  over at the next bar from where the first was, and the first launched again from its start a
  bar later; 2.75 seconds. Made with the stretchers built when the graph was built, before they
  moved to the worker thread (ADR 0003 › decision 7), so it shows that launches sound the same.
  Its test also checks each clip's pitch; it has no envelope written out. It is compared within
  1e-4, not 1e-6: the stretcher's floating-point maths differs in the last digits between
  platforms.

Each file is little-endian 32-bit floats, one channel, one per frame. The tests also check every
frame against the envelope written out in the test (linear fades times the 2 ms click guard,
`docs/spec/arrangement-editing.md` › Fades), so a golden file cannot hold a wrong envelope.

## Making the renders again

Only when a change to the envelope is intended, and the spec says so:

```sh
SUHINA_BLESS=1 cargo test -p suhina-engine fade_tests
```

`launched-stretched.f32` only when a change to the stretcher's sound is intended (ADR 0003):

```sh
SUHINA_BLESS=1 cargo test -p suhina-engine launched_stretched_clips_render_like_their_golden_file
```
