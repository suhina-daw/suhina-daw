- **Importing `.flp` projects (partial):** the window, `suhina-cli` and the MCP server open an FL
  Studio project file. Patterns become clips on the instrument or sample tracks they play, shared
  across placements; samplers, audio clips (fades, gain, stretch timing as warp markers), wrapped
  VST2 and VST3 instruments and effects with their saved state and sidechains, parallel instrument
  patches as device groups and neutral layers come in, with known effects as approximate built-in
  substitutes. Mixer routing becomes groups and sends, a mixer insert fed by one source folds into
  its track, an instrument's own mute switches its device off and a mixer mute stays on its track
  or group. Volume automation of audio clips becomes gain envelopes and supported device and mixer
  automation becomes lanes on its owner; playlist colours and groups become track colours and
  folders. Samples are found beside the project or in the browser's library. Everything else
  (step-sequencer notes, note expression, unsupported plug-ins and curves) is kept in the project
  and named in the import's warnings (`docs/spec/flp-model-mapping.md`).
- **Plug-in instruments at FL's level:** an imported plug-in generator's channel plays 3 dB louder
  than before, as FL plays it; audio clip channels were already right.
