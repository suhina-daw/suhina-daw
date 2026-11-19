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

The script reads IEEE-float WAV directly, including on Python versions before 3.15. It writes
the four variations and leaves the source untouched, so repeated runs do not fade it again.
The shipped variations were made from the original, longer hit; regenerating from the trimmed
`soft.wav` can differ in their tails. It does not reproduce those original files byte for byte.
