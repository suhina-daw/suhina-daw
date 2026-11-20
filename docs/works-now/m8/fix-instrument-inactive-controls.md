- **Device controls with no effect are dimmed:** a device panel can mark a control inactive while
  the device's other settings leave it nothing to do (`active_when` in the SDK, `.sd` version 7);
  it is drawn dimmed, its tooltip says why, and it still takes edits so a value can be prepared.
  The experimental Instrument's panel and editor dim its modal-body controls in Sample, its sample controls and
  waveform in Modal, and its blend outside Hybrid.
