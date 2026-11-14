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

Measured on an AMD Ryzen 5 5600X, one standard WaveNet (Tim R JCM2000 Crunch) at 48 kHz in
512-frame blocks, the share of one core: 16.8 % before, 9.8 % after. NeuralAmpModelerCore
compiled the same way (SSE2) takes 8.2 % with its fast tanh and 13.8 % without.

A second change: `Model::copy_state_from` (and the same per part, beside each `reset`) copies
another instance's streaming state, the fields `reset` clears, without allocating. Suhina's Amp
Modeler runs one instance while both stereo sides feed it the same frames and hands the other
side this state when they part (docs/spec/nam-amp-and-cab.md in the parent repository).

Nothing else is changed. The crate's tests and benches are otherwise kept as published; the
published crate leaves out `tests/fixtures`, so ten of its tests cannot find their files, before
the change as after it.
