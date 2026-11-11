- A hosted plug-in's parameters (CLAP, VST3, VST2) are automated from lanes in the Arrangement
  view: the lane's parameter list shows each plug-in on the track under its name, with its own
  groups and the same search as a built-in device; a lane shows the plug-in's own text for its
  values, a move in the plug-in's editor turns the lane off, and the editor follows the lane
  playing and stopped. A lane whose plug-in is missing keeps its points.
- A track's top automation lane shows the parameter being worked on: a control moved by hand (a
  track's volume, pan or mute, a device's knob, a knob in a plug-in's own editor) shows there,
  ready to draw on, and its + mounts it as a lane of its own. Mounted lanes are reordered, picked,
  grouped (each plug-in's lanes in a group of their own, a setting) and deleted, with undo, and
  their layout is saved with the project.
- A time range painted over several automation lanes, or over a track where no clip plays, is
  copied, duplicated and deleted with its automation on every lane.
