# Golden renders of the reverb

`renders_like_the_golden_files` (in `src/golden_tests.rs`) renders every scenario listed there and
compares it, sample for sample (within 1e-6), with the file of the same name here. The other tests
check what the reverb must do (the decay time, the levels, Stereo's correlation); these catch any
change in how it sounds, so a change to the DSP that was not meant to change the sound shows up
here.

Each scenario sets some controls, feeds a signal in blocks of 256 frames and may move a control
part way through (a change splits its block):

- `defaults`: every control at its default (Chorus and Spin moving, Flat and Cut on), at 48 kHz.
- `still`: Chorus and Spin off, fully wet.
- `dense-long`: Density High, a 4 s decay, Size 200, Diffusion 0.9 and a 50 ms predelay, at
  44.1 kHz.
- `sparse-damped`: Density Sparse, a 0.5 s decay in a small room, the low-pass high damping and
  the low damping on, Low Cut on and High Cut off.
- `chorus-spin`: faster and deeper Chorus and Spin, Shape 0.2 and Stereo 120.
- `reflect-diffuse`: the reflections up 4 dB and the tail down 12 dB, Scale 0.9, Stereo 30,
  Flat off, 40 % wet.
- `freeze-cut`: frozen from 0.4 to 1.1 s while the tone goes on; Cut keeps it out.
- `freeze-open`: the same with Cut and Flat off, so the tone keeps feeding the frozen loop.
- `size-change`: Size from 50 to 300 and the predelay from 2.5 to 100 ms at 0.5 s with Smooth
  Slow, then Density Mid at 1 s.

The signals are 50 ms of noise fading out (a repeatable generator, different on each side) or a
tone, 330 Hz on the left and 495 Hz on the right. Scenarios are at 16 kHz unless the list says
otherwise. Each file is little-endian 32-bit floats, left and right interleaved, every n-th
frame as the scenario says, so that no file is over 32 KB.

## Making the renders again

Only when a change to the sound is intended, and the spec says so:

```sh
SUHINA_BLESS=1 cargo test -p suhina-device-reverb renders_like_the_golden_files
```

A new scenario gets its file the same way; `every_scenario_has_its_own_file` fails while a
scenario has no file or a file no scenario.
