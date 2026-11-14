- Partial `.flp` project import places clips on their instrument/sample tracks, with working
  mixer controls, per-source shared patterns, shared mixer routing and source-level Utilities.
  Repeated sources merge across playlist rows; compatible mixed rows form functional groups.
  Samples use existing paths, project-local files and unambiguous matches in the browser library. Stored audio
  stretch timing becomes warp markers. Known effects have approximate native substitutes;
  wrapped VST2/VST3 instruments and effects retain their identities and supported saved states
  for the existing plugin host. Unavailable instruments remain silent, including when soloed. Supported audio
  volume automation becomes gain envelopes; unsupported automation stays in retained source data. Import warnings
  explain the limits and the source is retained.
- In an imported `.flp` project, supported automation links appear on their target devices or mixer
  channels. Neutral layered notes fan out to existing source instruments. Supported parallel
  instrument patches become device groups, with each plug-in state and output connection level
  retained. Single-output mixer routing preserves shared buses and their Solo behaviour.
  Unsupported cases are reported. Source instrument mute switches the retained instrument device
  off; mixer mute stays on its track or bus. Disabled instruments stay silent and can be switched
  back on independently.
- In an imported `.flp` project, dedicated mixer inserts fold into their source track; shared
  primary routes form groups, and parallel routes remain sends. Unused mixer strips are omitted.
  Output labels show the actual destination and its mute state.
