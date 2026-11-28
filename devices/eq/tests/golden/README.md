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
- `every-type`: the first eight bands, one of each of the first eight types, at 44.1 kHz.
- `adaptive-q-and-scale`: three bells with gain, Adaptive Q on, Scale at -150 %.
- `left-right`: bells on bands 1 to 8 placed Left and 9 to 16 placed Right, as a project saved
  in Left/Right loads.
- `mid-side`: a boost on the mid and a narrow one on the side (bands 1 to 8 Mid, 9 to 16 Side),
  Output at -6 dB.
- `changes`: a bell swept from 1 to 4 kHz, turned into a 48 dB high cut, a band switched off and
  the bands moved to the mid while playing.
- `tilts-and-placements`: a tilt shelf, a flat tilt, a bell on the left and one on the side in
  slots past the eighth, and a boost in a slot not in use, which is not heard, at 44.1 kHz.

Scenarios are at 16 kHz unless the list says otherwise. Each file is little-endian 32-bit floats,
left and right interleaved, every n-th frame as the scenario says, so that no file is over 32 KB.

## Making the renders again

Only when a change to the sound is intended, and the spec says so:

```sh
SUHINA_BLESS=1 cargo test -p suhina-device-eq renders_like_the_golden_files
```

A new scenario gets its file the same way; `every_scenario_has_its_own_file` fails while a
scenario has no file or a file no scenario.
