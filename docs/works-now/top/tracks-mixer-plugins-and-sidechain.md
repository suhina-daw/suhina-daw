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
  *Sidechain to*. A plug-in hears the key on its sidechain input.
- **Renaming devices:** a device's menu starts with its name in a field; type a new one and
  press Enter. A renamed plug-in still loads as itself.
- **Notes stop at the instrument:** a plug-in effect after a track's instrument no longer gets
  the track's notes, so a compressor that MIDI notes can trigger no longer holds the sound down.
- **The original VST2 first:** a project saved with a VST2 plug-in opens it with the installed
  VST2 itself before a VST3 copy that only shares its id, so its saved sound loads; a VST3 that
  declares it replaces the VST2 still comes first.
- **A plug-in's process from the plug-in manager:** a row's menu has *Process* (one process per
  plug-in file, per vendor, or a process of its own for each), from the plug-in's next start.
- **No tone without an instrument:** a MIDI track with no instrument is silent, unless
  *Tone without instrument* (Settings › Devices) is on; a switched-off or missing instrument is
  always silent.
- **Folders:** Put in a folder in a track's menu gathers tracks (a selection, or one track) under a
  folder track that only groups them in view: what they play and where stays as it was. A folder
  folds, names and colours its tracks like a group, has no mixer controls and is kept in the
  project file.
