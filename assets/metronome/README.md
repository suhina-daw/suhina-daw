# Metronome clicks

The recorded click sounds the metronome plays (`docs/spec/metronome.md` › Sounds), carried
inside the executable (`crates/engine/src/metronome.rs`). Mono, 32-bit float, 44.1 kHz.

| File | Author | Source | Licence |
|---|---|---|---|
| `soft.wav` | Suhina | a single hit rendered from a software synthesizer patch of our own, cut to 80 ms | CC0-1.0 |
| `muted.wav`, `deep.wav`, `tick.wav`, `knock.wav` | Suhina | made from `soft.wav` by `make_variations.py` (filters, a repitch and a mix with itself an octave lower) | CC0-1.0 |

To make the variations again:

```sh
python -I assets/metronome/make_variations.py assets/metronome/soft.wav assets/metronome
```

From `soft.wav` it makes the same files again, equal within float rounding (the original hit
was longer, so `muted.wav` and `deep.wav` come out a few silent frames shorter); it rewrites
`soft.wav` unchanged.
