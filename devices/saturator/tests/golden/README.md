# Golden renders of the saturator

`renders_like_the_golden_files` (in `src/golden_tests.rs`) renders every scenario listed there and
compares it, sample for sample (within 1e-6), with the file of the same name here. The other tests
check what the saturator must do (each curve's gain, the Color filters cancelling, the post
clip's ceiling); these catch any change in how it sounds, so a change to the DSP that was not
meant to change the sound shows up here.

Each scenario sets some controls, feeds a signal in blocks of 256 frames, and may move a control
part way through (a change splits its block):

- `defaults`: every control at its default (Analog Clip, clean at this level), at 48 kHz.
- `analog-clip`, `soft-sine`, `medium-curve`, `hard-curve`, `digital-clip`: each curve driven
  12 dB; `sinoid-fold` the same at 48 kHz; `bass-shaper` with its threshold at -20 dB.
- `waveshaper`: every waveshaper amount away from its default.
- `color`: noise into a hard clip with Color Low at -70 % and Color High at +80 % around 2 kHz,
  at 48 kHz.
- `post-clip-soft`, `post-clip-hard`: a steep waveshaper caught by each post clip at -6 dB.
- `changes`: the curve, the drive, Color Low, Dry/Wet and the DC filter changed while a tone
  with a DC offset plays.

The signals are a tone, 110 Hz on the left and 165 Hz on the right at 0.4, or noise at a quarter
of full scale (a repeatable generator, different on each side). Scenarios are at 16 kHz unless
the list says otherwise. Each file is little-endian 32-bit floats, left and right interleaved,
every n-th frame as the scenario says, so that no file is over 32 KB.

## Making the renders again

Only when a change to the sound is intended, and the spec says so:

```sh
SUHINA_BLESS=1 cargo test -p suhina-device-saturator renders_like_the_golden_files
```

A new scenario gets its file the same way; `every_scenario_has_its_own_file` fails while a
scenario has no file or a file no scenario.
