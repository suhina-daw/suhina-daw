- **Importing `.flp` projects (partial):** the window, `suhina-cli` and the MCP server open an
  FL Studio project file. Patterns become clips on the instrument or sample tracks they play,
  shared across placements; samplers, audio clips (fades, gain, stretch timing as warp markers),
  wrapped VST2 and VST3 instruments and effects with their saved state, parallel instrument
  patches as device groups and neutral layers come in, with known effects as approximate
  built-in substitutes. Mixer routing becomes groups and sends, a mixer insert fed by one source
  folds into its track, an instrument's own mute switches its device off and a mixer mute stays
  on its track or group. Volume automation of audio clips becomes gain envelopes and supported
  device and mixer automation becomes lanes on its owner; playlist colours and groups become
  track colours and folders. Samples are found beside the project or in the browser's library.
  Everything else (step-sequencer notes, note expression, unsupported plug-ins and curves) is
  kept in the project and named in the import's warnings (`docs/spec/flp-model-mapping.md`).
- **Instruments that are switched off or missing stay silent:** they still take their track's
  notes, so no default tone plays in their place, and play again when switched on or relinked.
- **Groups apply their pan and can take sends:** a group's pan applies once to its sum and its
  post-fader sends; a send can go into a group, and solo follows sends as well as outputs.
- **Linked copies:** Duplicate linked in a clip's menu makes copies that keep one content, so
  editing any of them edits all; they carry a chain mark, and Unlink gives a clip its own content
  again. An ordinary duplicate stays independent.
- **Zipped projects:** a zip holding a project and its samples opens after a bar asks where to
  unpack it (beside the zip by default); `suhina-cli` takes `--extract-to <folder>`.
- **Sidechain:** a compressor or a plug-in can read another track as its key (its detector
  listens to that track while it processes its own). Choose the track with the **SC** button
  in the device's header or *Sidechain* in its menu, with where it is read (before or after
  the track's devices, or after its fader) and its gain; or from the source's track menu,
  *Sidechain to*. A plug-in hears the key on its sidechain input. An imported `.flp` keeps
  its plug-ins' sidechains.
