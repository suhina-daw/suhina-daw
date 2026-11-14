# nam-rs 0.4.0

Source: crates.io nam-rs 0.4.0, upstream commit 97fba1ea22dc19e68d408adf2abd72252af5e4d6 in
https://github.com/OpenSauce/nam-rs.
License: MIT, preserved in LICENSE, with the upstream projects it ports from listed in NOTICE.

Local changes, all marked in the source as Suhina's and made for speed on the audio thread.

First, the activations (`src/fast_math.rs`). The WaveNet's `Tanh` activation uses the rational
approximation NeuralAmpModelerCore uses in its fast mode (`NAM/activations.h` `fast_tanh`),
within 5e-4 of `tanh` for |x| <= 10 and within 1 % far beyond, instead of `f32::tanh`: a libm
call per activation that the compiler cannot vectorise, about 240 per sample in a standard
WaveNet. Three of the crate's tests that compared against the exact `tanh` (`wavenet::gating`
and `wavenet::layer`) compare against the approximation now. The LSTM keeps near-exact
activations instead (a Cephes-style `exp`, within 3e-7, written to vectorise), because its
recurrence carries an approximation's error forward: with the fast functions a pack model came
out 39 dB below the signal, with these 127 dB below the published crate's output.

A second change: `Model::copy_state_from` (and the same per part, beside each `reset`) copies
another instance's streaming state, the fields `reset` clears, without allocating. Suhina's Amp
Modeler runs one instance while both stereo sides feed it the same frames and hands the other
side this state when they part (docs/spec/nam-amp-and-cab.md in the parent repository).

A third change, the WaveNet's block path (`src/wavenet/conv.rs`, `src/wavenet/activation.rs`,
`src/wavenet/gating.rs`):

- A conv's history stays in the head of each `staged` row between blocks, shifted once per row
  per block (`copy_within`), instead of being staged from the ring with a `%` per element and
  written back to the ring a column per sample. The ring is rebuilt only when the per-sample
  path runs (`history_to_ring`); `reset` and `copy_state_from` keep both in step.
- `kernel_block` computes four outputs over sixteen samples at a time with the sums in
  registers, so each staged load feeds four products. Every output adds its terms in
  `process_sample`'s order, so the block path stays bit-for-bit the per-sample one (the crate's
  `process_block_equals_process_sample_loop` tests).
- The ungated activation runs over a whole block (`apply_block`), the activation chosen once.
- `kernel_block` and `apply_block` are compiled twice through `multiversion` (MIT OR
  Apache-2.0), plain and with AVX, AVX2 and FMA, and the processor picks one at run time; Rust
  never fuses a multiply and an add into one FMA by itself, so both give the same bits.

One standard WaveNet (Tim R JCM2000 Crunch) at 48 kHz on an AMD Ryzen 5 5600X, the share of one
core, built for plain x86-64 as Suhina ships:

| | 64-frame calls | 512-frame calls |
|---|---|---|
| nam-rs 0.4.0 as published | 24.1 % | 16.8 % |
| with the fast tanh | 15.0 % | 9.0 % |
| with the block path above | 4.1 % | 3.5 % |
| NeuralAmpModelerCore, AVX2 and FMA, fast tanh | 3.5 % | 3.1 % |

A fourth change, the LSTM cell (`src/lstm/cell.rs`): the weights are kept transposed, one
column per input, and `gates_matvec` sums the gates in tiles of 32 and 8 held in registers, each
gate in the row-major loop's order (bias, then the inputs in turn), so the sums are the same bits;
the gates and the cell state go through the activations as slices. AVX2 is picked at run time.

A conv span whose length is not a multiple of sixteen ends with one more full chunk that
overlaps the last one (it writes the shared samples again with the same values) instead of a
sample-at-a-time remainder: the resampler legs around a model feed spans such as 69 frames.

The pack's own LSTM models through the published crate and through this one, the same machine
and build, 90 s of guitar at 48 kHz in 512-frame calls: Suhina Clean (40 × 2) 57.6 % → 6.3 % of
one core, Suhina Overdrive (18 × 3) 21.4 % → 3.9 %.

Nothing else is changed. The crate's tests and benches are otherwise kept as published; the
published crate leaves out `tests/fixtures`, so ten of its tests cannot find their files, before
the change as after it.
