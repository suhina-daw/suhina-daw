# Golden renders of the delay

`renders_like_the_golden_files` (in `src/golden_tests.rs`) renders every scenario listed there and
compares it, sample for sample (within 1e-6), with the file of the same name here. The other tests
check what the delay must do (where a repeat lands, how loud it is); these catch any change in how
it sounds, so a change to the DSP that was not meant to change the sound shows up here.

Each scenario sets some controls, feeds a signal in blocks of 256 frames at 120 BPM with the
transport running, and may move a control part way through (a change splits its block):

- `defaults`: every control at its default (synced to 3 and 4 sixteenths, the band-pass in the
  loop, half wet), at 48 kHz.
- `free-times`: free times of 130 and 210 ms, feedback 0.7, a 2-octave band at 2 kHz.
- `synced-offsets`: unlinked sync steps of 2 and 6 sixteenths with offsets of +20 % and -30 %,
  at 44.1 kHz.
- `ping-pong`: a signal on the left only, bouncing between the sides.
- `lfo`: a sine LFO at 3 Hz on the time and the band.
- `lfo-synced-wander`: the Wander shape synced to a quarter note, with Morph.
- `repitch`, `fade`, `jump`: the time going from 200 to 50 ms at 0.4 s under each transition.
- `freeze`: frozen from 0.5 to 1 s while the tone goes on.
- `equal-loudness`: the equal-loudness mix, Dry/Wet going from 30 to 80 % at 0.5 s.

The signals are 50 ms of noise fading out (a repeatable generator, different on each side) or a
tone, 330 Hz on the left and 495 Hz on the right. Scenarios are at 16 kHz unless the list says
otherwise. Each file is little-endian 32-bit floats, left and right interleaved, every n-th
frame as the scenario says, so that no file is over 32 KB.

## Making the renders again

Only when a change to the sound is intended, and the spec says so:

```sh
SUHINA_BLESS=1 cargo test -p suhina-device-delay renders_like_the_golden_files
```

A new scenario gets its file the same way; `every_scenario_has_its_own_file` fails while a
scenario has no file or a file no scenario.
