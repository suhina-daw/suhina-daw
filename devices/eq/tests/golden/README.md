# Golden renders of the EQ

`renders_like_the_golden_files` (in `src/golden_tests.rs`) renders every scenario listed there and
compares it, sample for sample (within 1e-6), with the file of the same name here. The other tests
check what the EQ must do (each type's response against the measured one, Adaptive Q, Scale, the
modes); these catch any change in how it sounds, so a change to the DSP that was not meant to
change the sound shows up here.

Each scenario sets some controls, feeds noise at a quarter of full scale (a repeatable generator,
different on each side) in blocks of 256 frames, and may move a control part way through (a change
splits its block):

- `defaults`: every control at its default (four bands on, none with gain), at 48 kHz.
- `every-type`: the eight bands of curve A, one of each type, at 44.1 kHz.
- `adaptive-q-and-scale`: three bells with gain, Adaptive Q on, Scale at -150 %.
- `left-right`: different bells on the two curves in Left/Right.
- `mid-side`: a boost on the mid and a narrow one on the side in Mid/Side, Output at -6 dB.
- `changes`: a bell swept from 1 to 4 kHz, turned into a 48 dB high cut, a band switched off and
  the mode changed to Mid/Side while playing.

Scenarios are at 16 kHz unless the list says otherwise. Each file is little-endian 32-bit floats,
left and right interleaved, every n-th frame as the scenario says, so that no file is over 32 KB.

## Making the renders again

Only when a change to the sound is intended, and the spec says so:

```sh
SUHINA_BLESS=1 cargo test -p suhina-device-eq renders_like_the_golden_files
```

A new scenario gets its file the same way; `every_scenario_has_its_own_file` fails while a
scenario has no file or a file no scenario.
