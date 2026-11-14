- **A note's velocity range** ([#1282](https://github.com/suhina-daw/suhina-daw/issues/1282)): each
  note can have a range, -127 to 127, rolled every time it starts, so a looped part varies pass by
  pass without its notes changing. The roll is a pure function of the note and where it plays, so
  an offline render repeats and matches playback (ADR 0037). The velocity lane shows the range as
  a fainter bar, Ctrl+drag on a stem sets it, Alt+Shift+Up and Down step it, the clip panel shows
  it beside the velocities, and `note.setVelocityRange` sets it; project files are version 13.
