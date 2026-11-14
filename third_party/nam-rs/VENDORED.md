# nam-rs 0.4.0

Source: crates.io nam-rs 0.4.0, upstream commit 97fba1ea22dc19e68d408adf2abd72252af5e4d6 in
https://github.com/OpenSauce/nam-rs.
License: MIT, preserved in LICENSE, with the upstream projects it ports from listed in NOTICE.

Local change: the WaveNet's `Tanh` activation and the LSTM cell's tanh and sigmoids use the
rational approximations that NeuralAmpModelerCore uses in its fast mode (`NAM/activations.h`
`fast_tanh` and `fast_sigmoid`), in `src/fast_math.rs`, instead of `f32::tanh` and `exp`. The
exact functions are a libm call per activation that the compiler cannot vectorise; a standard
WaveNet makes about 240 per sample. The approximation stays within 5e-4 of `tanh` for |x| <= 10
and within 1 % far beyond (the module's tests). Three of the crate's tests that compared against
the exact `tanh` (`wavenet::gating` and `wavenet::layer`) compare against the approximation now.

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

Nothing else is changed. The crate's tests and benches are otherwise kept as published; the
published crate leaves out `tests/fixtures`, so ten of its tests cannot find their files, before
the change as after it.
