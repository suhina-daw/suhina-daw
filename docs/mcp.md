---
description: The MCP server for agents: tools, file roots, errors and how to run it
read_when: MCP, agents, attach, mcp --attach, agents in the app, live agent edits, allow agent, audio output, MIDI input, arm, record, recording, count-in, audio device, buffer size, ASIO, project_open, command_run, file roots, rmcp, suhina-cli mcp, several projects, project id, plug-in commands, device commands, library commands, add samples, track.add, clip.addFiles, preview, preview.noteOn, preview.noteOff, note.split, mute a note, note.setMuted, note.humanize, note.fitToScale, note.invert, note.reverse, note.legato, note.stretch, note.randomizeVelocity, note.rampVelocity, MIDI tools
type: system
status: current
last_verified: 2026-10-29
covers:
  - crates/automation/src/server.rs
  - crates/automation/src/workspace.rs
  - crates/session/src/audio.rs
  - crates/session/src/midi.rs
  - crates/automation/src/render.rs
  - crates/engine/src/cpu.rs
  - apps/suhina-cli/src/main.rs
  - crates/automation/src/attach.rs
  - crates/automation/src/endpoint.rs
---

# MCP server

Sampler uses the common commands: `device.add {"track":1,"id":"sampler"}`,
`device.loadSample {"track":1,"device":0,"path":"/path/to/sample.wav"}` and
`device.setParam` for its stable parameters. Sample paths are confined to the
read root. `device.recordSample {"track":1,"device":0,"on":true}` records the
armed track's selected audio input; `on:false` finishes and loads the take, as one
undo step. Save the project and open an audio output before recording.

Loading a sample initializes `source_bpm` from its metadata, filename or detected
tempo, falling back to the current project/playback tempo; `device.loadSample`
returns `source_bpm` too. Set `warp` to 1 to follow project BPM using that original
tempo. Correct `source_bpm` explicitly when known, or set it to 0 to request fresh
detection. The legacy `warp_beats` parameter remains readable; setting it explicitly
returns to beat-length timing. Source and tempo changes persist and undo together.

`suhina-cli mcp` serves the [Model Context Protocol](https://modelcontextprotocol.io) on
stdin/stdout (the official `rmcp` SDK), so an agent can open, inspect, edit and render projects.
It follows the same conventions as [PhotoCraft](https://github.com/storytold/photocraft)'s MCP server.

```sh
suhina-cli mcp --read-root ./projects --write-root ./renders [--audio]
```

- **Files.** Paths are relative to a root, or absolute paths beneath it. `..` and symlinks that
  lead outside are refused. Without `--read-root` the agent cannot open files; without
  `--write-root` it cannot write any. Existing files are replaced only with `overwrite: true`.
  `project.save` writes below the write root only: with `path`, or over the project's own file
  when that file is there (`overwrite: true` needed, as for any existing file; `overwrite` is an
  MCP-only parameter, removed before the command runs). `project.export` is checked the same
  way: the first of `path` and `output` that names a file decides, untrimmed as the command
  reads it, and the checked file is what both aliases keep, so neither alias can slip past the
  check the other went through.
  A launch script's steps are confined like the tool calls', and the file-writing commands
  (`project.export`, `project.save`) do not run in one, whatever a caller's permission: the
  render's own output is the only file a script writes, at the path the render was given.
  Commands that read files take the same paths and are checked the same way before they run:
  `clip.addFiles` (`paths`), `preview.play` (`path`), `device.add` and `track.add` (`preset`),
  `library.list` (`folder`), `library.search` (a folder `place`), and a file `item` of
  `library.info`, `library.pin`, `library.unpin` and `library.tag`; `soundmap.analyse`
  (`folder`), `soundmap.layout` (a folder `place`), and `soundmap.similar` (`item`, a folder `place`) and
  `soundmap.info` (`item`); `soundmap.setKind` (`item`); `library.findFiles` (`folder`) and `project.relinkFiles` (every
  `to` file).
- **Missing files.** `project.missingFiles` lists the audio files a project names but cannot
  find; `library.findFiles {names, folder?}` finds candidates by name (the browser's index and
  the project's folder, or a folder below the read root); `project.relinkFiles {files, to}`
  points the clips at them, one undo step ([spec](spec/missing-files.md)).
- **Several projects.** The server keeps every opened project with its own undo history, like
  tabs ([ADR 0007](decisions/0007-several-open-projects.md)). `project_open` adds one and makes
  it active; `project.list`, `project.switch`, `project.next`, `project.previous` and
  `project.close` (through `command_run`) manage them. Tools work on the active project, or on
  the one their `project` argument names, without switching. At most 32 are open.
- **Audio output.** With `--audio`, each opened project also plays on the chosen output device
  (the default one until `audio.setOutput` chooses another) with the transport stopped, and transport commands (`scene.launch`, `clip.launch`, `clip.release`, `track.stop`,
  `launcher.stopAll`, `launcher.setQuantization`, `launcher.backToArrangement`, `transport.play`, `transport.stop`,
  `transport.setPosition`, `transport.setMetronome`,
  `launcher.state`) are heard. Without it they return an error, except `launcher.setQuantization`,
  which the project's session keeps and hands to an engine attached later; launch scripts in
  `project_render` work either way. Edits and undo/redo are heard by the playing engine from its
  next audio block (0.15 s later when stretched arrangement clips must be prepared first), with
  launched clips playing on
  ([ADR 0005](decisions/0005-edits-while-playing.md)). To sweep a track's volume or pan by
  ear as one undo step, send `track.setVolume` or `track.setPan` repeatedly with
  `continue: true` after the first ([ADR 0002](decisions/0002-commands.md)). Only the active project plays: switching
  closes the previous project's output and starts the new one (its result has `audio_output`),
  so transport commands aimed at another project fail.
- **Plug-ins between commands.** The server takes in the plug-ins that started every 50 ms, as
  the window does every frame, so a project plays with them while the agent sends nothing or only
  `project.*` commands that list, switch and close projects.
- **Choosing the output.** `audio.devices` lists the audio hosts (WASAPI, and ASIO in a build
  with the `asio` feature; ALSA, JACK or CoreAudio elsewhere) and each one's output devices with
  `sample_rates`, `buffer_sizes` (what the driver allows), `buffer_range` and whether WASAPI
  exclusive mode is possible, plus the output `chosen`. `audio.setOutput {host?, device?,
  sample_rate?, buffer_frames?, exclusive?}` changes the parts given (an empty text or 0 goes back
  to the default; a new host forgets the device) and reopens the active project's output there,
  the transport where it was (the play start marker stays; while playing, playback goes on from
  the playhead with the launched clips playing on and the launches still waiting, at the launch
  quantisation and scene tempo there were) and the
  plug-ins running on when the sample rate stays the same; a device that is not there plays the
  default output and the result's `notice` says so. The choice is kept: while the default plays
  in its place, Suhina looks every few seconds whether the chosen device is back and moves the
  output there, the same way. The engine renders in blocks of
  `bufferFrames` (64 to 4096), which sets the plug-ins' deadline. A size is fitted to what the
  device's driver declared, and a driver is never asked for one outside that range: a range
  holding no size the engine renders in leaves the driver its own size, and `notice` says which
  size plays. On WASAPI shared mode Windows sets the device's buffer, so there `bufferFrames` is
  the engine's block alone and `buffer_sizes` the engine's own sizes. `audio.info` says what plays
  now: `host`, `device`, `sample_rate`, `buffer_frames` (the device's, measured),
  `engine_block`, `exclusive`, `latency_ms` (engine to speaker, measured), `live` (a track is
  armed, so the engine keeps one block ahead instead of two), `live_latency_ms` (what a key on a
  MIDI controller takes to be heard: the measured latency and the fixed delay notes are placed
  at, ADR 0017), `underruns`, `failed` and `fallback` (it plays in
  place of a chosen host or device that is not there), `channels` (opened) and `cue_pair`.
  Without `--audio`, `audio.devices` returns an error.
  `audio.openControlPanel {}` requests the active ASIO stream's own native panel
  on its owner thread and returns `requested: true`. It does not reopen the output
  or load plugins. Other hosts or a missing output return an error; close a modal
  driver panel before changing output or closing the project.

- **The cue (headphones).** `output.setCuePair {pair}` plays previews and the tracks whose cue
  is on (`track.setCue {track, cue}`, an edit) on another pair of the same device's outputs
  (1 is outputs 3/4; 0 plays them with the main output), reopening the output like
  `audio.setOutput`; a pair the device lacks is refused. `output.setCueVolume {db}` sets their
  level without reopening. `suhina-cli render --cue cue.wav` writes the cue beside the main mix
  ([spec](spec/cue-output.md)).

- **Tempo map.** `tempo.points` lists the start tempo, the tempo changes and the time-signature
  changes; `tempo.addPoint {beat, bpm, ramp?}`, `tempo.movePoint` and `tempo.deletePoint` edit the
  tempo changes (numbered from 1), `timeSignature.add {bar, numerator, denominator}`,
  `timeSignature.move` and `timeSignature.delete` the signature changes (bars from 1). Renders and
  the playing engine follow them ([tempo map](spec/tempo-map.md)). A launched scene's tempo or
  time signature overrides the map for the launcher clock (`launcher.state` reports
  `follows_tempo_map: false`) until `transport.followTempoMap`, the next `transport.play` after a
  stop, or an edit to the map ([scenes and the tempo map](spec/tempo-map.md#scenes-and-the-tempo-map)).

- **MIDI controllers.** With `--audio`, the server also opens every MIDI input port
  ([ADR 0017](decisions/0017-inputs-and-latency.md)). `midi.inputs` lists them with `enabled`,
  `open` and the `error` a port could not be opened with, and whether the playing engine takes
  their notes (`connected`); `midi.setInput {port, enabled}` turns one off or on again.
  `track.setArm {track, armed, exclusive?}` arms a track: its instrument plays what the ports
  play, transport running or not. `track.setMidiInput {track, port?, channel?}` chooses which
  controllers it hears (`port` `"all"`, `"none"` or a port's name from `midi.inputs`; `channel`
  1 to 16, or 0 for every one), so two controllers can play two armed tracks; it is an edit,
  saved with the project, and `project.tracks` reports each track's `midi_input`.

- **Audio input.** `audio.devices` lists each host's `inputs` (name, channels, sample rates).
  `audio.setOutput {input}` opens one beside the output (`"off"`, the default; `"default"`; or
  a name), at the output's sample rate. `track.setAudioInput {track, input?, monitor?}` chooses
  what a track hears: `input` `"none"`, a channel (`"3"`) or a pair (`"1/2"`), `monitor`
  `auto` (while armed), `in` or `off`; it is an edit, saved with the project. Arm several
  tracks with `track.setArm` and `exclusive: false` to hear a band on separate tracks.
  `audio.info` adds `input`, `input_channels` and `input_underruns`. Arming is not an edit (no undo step, not saved) and works
  before an engine plays; `project.tracks` says which tracks are `armed`. Without `--audio`,
  `midi.*` returns an error.

- **Recording** ([spec](spec/recording.md)). With an engine playing (`--audio`), arm tracks with
  `track.setArm`, then `clip.record {track, scene}` records into an empty slot from the next
  launch boundary, and `clip.launch` on that slot ends the take at the next boundary and plays
  it, looped; or `transport.record {on: true}` records every armed track into the arrangement
  until `transport.record {on: false}` or `transport.stop`. A track with an audio input records
  its input into a WAV file in `Recorded/` next to the project file (save the project first); a
  track that plays notes records the MIDI controllers it hears. `transport.setCountIn {bars}`
  (0, 1, 2 or 4) counts in when recording starts the transport. `record.state` lists the takes
  running (`track`, `into`, `kind`, `start_beat`, `end_beat`, `notes`, `frames`, `file`),
  `count_in_bars`, `count_in_left` and `notices`. A take becomes a clip, one undo step, once its
  end has passed; the server looks around every command, so call `record.state` to collect it.

- **Play position.** `transport.setPosition {beats}` moves the play start marker, where
  `transport.play` starts (a jump while playing). `transport.play {from: "clip", track, index}`
  moves the marker to that arrangement clip's start and plays from there. `transport.stop` returns the playhead to the
  marker; with `return: "here"` the marker moves to where it stopped; stopping while stopped
  moves the marker to beat 0. `launcher.state` reports `start_beat`. In a `project_render`
  launch script, `transport.setPosition` then `transport.play` renders from that beat
  ([arrangement editing](spec/arrangement-editing.md) › Play position).

- **Saving.** `project.save { path?, copy? }` (through `command_run`) writes the project as a
  Suhina project (`.suhina`, [spec](spec/suhina-format.md)) with its plug-ins' state as it is now;
  without a path, to the project file it came from; `copy: true` writes a copy (a template) and the
  project keeps its own file. `project.info` reports `unsaved_changes`, which a change made only in
  a plug-in's editor sets too, within about a minute (two state reads that agree). `project_open` opens
  `.suhina` files as well as Live Sets.
- **Backups.** `project.autosave { now?, interval?, clean_up? }` reports where the active
  project's backups go, its last one and whether one is due ([spec](spec/backups.md));
  `now: true` writes one into `.backup` beside the project's file, which must be below the write
  root. Agents cannot choose `interval: "critical"`, which would write there unasked; the server
  writes no timed backups.

- **Clips by id.** Every clip has an `id`, a whole number that stays the same when the clip is
  moved, trimmed or edited, and never comes back once the clip is deleted
  ([ADR 0025](decisions/0025-object-identity.md)). `project.clips` and `project.launcher` report
  it, and every command that acts on a clip takes it as `clip` (or a name only one clip has),
  `{ "clip": id }` in a `clips` list, so an agent can read a project, change it, and still name
  the clip it meant: `clip.move {clip, start}`, `note.add {clip, notes}`. Commands that make
  clips (`clip.paste`, `clip.duplicate`, `clip.split`, `clip.create`, `clip.consolidate`,
  `clip.addFiles`) return the new clips' ids. The older form, `track` with `index` (an
  arrangement clip's place on its track) or `scene`, still works and is **deprecated**: an index
  changes whenever a clip before it moves, and it is accepted only until the project format's
  next version. The examples below show the positional form where it is still the only way to
  name a slot (`scene`).

- **Clips.** `project.clips` lists each arrangement clip with its id, start, length and fades
  (lengths, shapes and curves); `clip.move`, `clip.duplicate`, `clip.delete`, `clip.copy`,
  `clip.cut`, `clip.paste`, `clip.setGain` and `clip.setFades` (fade-in and fade-out in beats,
  `inShape`/`outShape` and `inCurve`/`outCurve`) edit them through
  `command_run` ([arrangement editing](spec/arrangement-editing.md)). `clip.copy {track, scene}`
  copies a clip slot's clip and `clip.paste {track, scene}` puts the copied clip into a slot, so a
  clip goes from the arrangement into a slot and back
  ([clip launching](spec/clip-launching.md) › Clips between the arrangement and the slots). `clip.setFades` with
  `scene` instead of `index` sets a launcher slot clip's fades
  ([clip launching](spec/clip-launching.md)). `clip.create {track, scene? | start?, length?,
  stack?}` makes an empty clip — MIDI on a notes track, audio with no file yet on an audio
  track — a bar long by default, in an empty slot or at a beat
  ([arrangement editing](spec/arrangement-editing.md#making-an-empty-clip)).
  `clip.split {track, index, at}` cuts a clip in two at a song position; `clip.consolidate
  {track, start, end}` turns a range of one track into one clip (audio is rendered to a file next
  to the project, which must have been saved).

- **Clip loops, lengths and start markers.** `clip.setLoop {track, scene, loop}` switches the
  loop of the clip in a slot: on, it repeats its whole length; off, it plays once. A file placed
  in a slot with `clip.addFiles` loops, a sample with no known tempo too
  ([clip launching](spec/clip-launching.md) › Suhina decisions). It takes an arrangement clip by
  `index` instead of `scene`, and `start` and `end` (content beats) to set the loop itself, which
  turns it on. `clip.setLength {track, index | scene, length}` sets how long a clip plays (an
  arrangement clip may not run into the next clip on its lane), and `clip.setStart {track, index
  | scene, offset}` its start marker (a clip that does not loop keeps its end, so its length
  changes by as much). These are what the piano roll's ruler runs when its loop handles and
  markers are dragged ([clip editor](spec/ui-clip-editor.md) › Commands).

- **Notes.** `note.list {track, index | scene}` lists a MIDI clip's notes (index, time and length
  in beats, key with 60 as C3, velocity 1 to 127, and `muted`); `note.add {..., notes: [{time, length, key,
  velocity?}]}` appends notes and returns their indices; `note.delete`, `note.move {beats?,
  semitones?}`, `note.resize {length? | by? | startBy?}` (`startBy` moves each start and keeps
  each end) and `note.setVelocity {velocity? | by? | velocities?}` (`velocities`: one value per
  entry of `notes`) take
  `notes`, a list of those indices. Edits keep the notes' order, so the indices of notes not
  deleted stay valid; each is one undo step and fails as a whole on a bad value
  ([clip editor spec](spec/ui-clip-editor.md) › Commands). An edit to a clip whose content
  other clips also play (a duplicate) first gives that clip its own copy of the content, so the
  others do not change; the result's indices are the same.
- **Muting notes.** `note.setMuted {track, index | scene, notes, muted?}` sets the named notes'
  mute (`true` mutes, `false` unmutes); with `muted` left out each named note is flipped, which is
  what a click with the piano roll's Mute tool does. A muted note keeps its place, length, key and
  velocity and is never played (the arrangement, the launcher and an offline render all skip it);
  it is saved as `"muted": true` in the project file and survives a save and reopen. One undo step;
  it returns the notes' indices and their state.
- **Quantise.** `note.quantize {track, index | scene, notes?, grid, amount?, ends?}` moves the
  starts of the given notes, or of every note, towards the nearest line of `grid` (`"1/16"`,
  `"1/8T"`, `"1 bar"`, as `launcher.setQuantization` takes it) by `amount` percent (default 100,
  onto the line); with `ends: true` the ends move too. One undo step; a bad grid, an amount
  outside 0 to 100 or a note that does not exist fails and changes nothing.
- **MIDI tools.** The piano roll's MIDI tools column runs the same commands an agent can:
  `note.humanize {notes?, time?, velocity?, seed?}` moves each start and velocity a small random
  amount, `note.fitToScale {notes?, root?, scale?}` moves each note to the nearest key of a scale
  (a key name such as `"C"` or `"F#"`, and `major`, `minor`, `dorian`, `blues` and the rest),
  `note.invert {notes?}` mirrors the pitches, `note.reverse {notes?}` turns them round in time,
  `note.legato {notes?}` makes each note last until the next of its key, `note.stretch {notes?,
  factor}` stretches them in time (`2` is the **×2** button, `0.5` **÷2**),
  `note.randomizeVelocity {notes?, amount?, seed?}` and `note.rampVelocity {notes?, from, to}`
  change velocities. `notes` left out means every note of the clip. The two random tools take a
  `seed`, so the same call gives the same result twice and a script can repeat it; the result says
  which seed it used when none was given. Each is one undo step and fails as a whole, and none of
  them changes a note's mute.
- **Scales.** `clip.setScale {track, index | scene, root?, mode?, off?}` gives a MIDI clip a scale
  (a root such as `"D"`, `"F#"` or 0 to 11, and a mode such as `major`, `minor`, `dorian` or
  `blues`, the names `note.fitToScale` takes), keeping the part left out, or clears it with
  `off: true`; the piano roll then marks the scale's keys. `clip.scale {track, index | scene}`
  reads it back with the project's default, and `project.setDefaultScale {root?, mode?, off?}`
  sets the scale new MIDI clips take (`clip.create`, recorded and dropped MIDI clips; clips that
  exist keep theirs; `project.info` shows it as `default_scale`). A scale is the clip content's,
  so a clip whose content others play gets its own copy first, as a note edit does; each is one
  undo step and changes nothing on a bad root or mode. `note.fitToScale` with neither `root` nor
  `scale` fits to the clip's own scale.
- **Copying notes.** `note.copy {..., notes}` copies notes to the session's note clipboard (it
  changes nothing); `note.cut` copies and deletes them. `note.paste {track, index | scene, time?}`
  adds the copied notes to any MIDI clip with their spacing kept, the first at `time` (by default
  right after the copied notes), and returns their indices; `note.duplicate {..., notes, beats?,
  semitones?}` adds a copy right after the notes' end, or `beats` and `semitones` from them (the
  piano roll's Ctrl+drag). The note clipboard is apart from the clip clipboard.
- **Splitting notes.** `note.split {track, index | scene, notes, at}` splits each named note that
  spans the beat `at` into two at `at` (the first keeps the start, the second the end); a note
  that starts or ends exactly at `at` is left alone, so a second split at the same beat changes
  nothing. One undo step; it returns the indices of the notes it made, and `at` is a beat from 0
  to 120 000.
- **Follow actions.** `clip.setFollowAction {track, scene, enabled?, time?, linked?, actionA?,
  actionB?, chanceA?, chanceB?}` sets what a slot clip does after its follow time (one undo
  step); `project.launcher` lists each slot clip's `follow_action`. A `project_render` script
  picks the same random actions every time ([clip launching](spec/clip-launching.md#follow-actions)).

- **Tracks and files.** `track.add {kind?, name?, after?, device?, preset?, voice?, sd?}` adds an audio
  or MIDI track (with a device, when one is named). `clip.addFiles {paths, track?, start? |
  scene?, separate?, stack?, warp?}` places samples and MIDI files as clips, on a track or on new
  tracks (a Stems file as a group of its master, muted, and its stems, with its mastering on the
  group, [spec](spec/stems-format.md)), one after another from a beat or down the clip slots from a scene; files that cannot be
  read are listed in `skipped`. A sample whose tempo is known (metadata, name, detection) is
  warped to the project's tempo. Each is one undo step, and a playing engine hears an added
  sample from its next graph ([browser spec](spec/browser.md) › Files,
  [ADR 0011](decisions/0011-preview-and-added-audio.md)).

- **Previews.** With `--audio`, `preview.play {path, from?, track?, gain?}` previews a sample or
  a MIDI file after the main channel (a MIDI file through `track`'s instrument, or the FM synth);
  `preview.setGain {gain}` changes the level of the one playing (in dB, gliding there without a
  click); `preview.stop` stops it; `launcher.state` reports `preview`. With no project open they still
  work, through an output of their own that closes before a project opens one (ADR 0011).
  `preview.noteOn {track, key | keys, velocity?}` plays one key (or the keys of `keys` together,
  as the piano roll's audition does) through the track's instrument and holds it, as the piano
  roll's key column does, until `preview.noteOff {track, key?}` lets that
  key go, or every held preview note when `key` is left out. Both are transport commands: the
  project, its undo history and the transport are untouched.

- **Launch settings.** `clip.setLaunchMode {track, scene, mode}` (trigger, gate, toggle,
  repeat), `clip.setLegato {track, scene, on}` and `clip.setQuantization {track, scene, value}`
  (global, none or a grid value) set how a slot's clip launches, one undo step each;
  `project.launcher` shows them. `clip.launch` is a press of the slot's launch button and
  `clip.release` its release, which stops a gate clip and ends a repeat clip's repeats; in a
  `project_render` script the two can be steps at different times
  ([spec](spec/clip-launching.md#launch-settings)).
- **Launch keys and notes.** `launcher.addMapping {source, target, track?, scene?}` maps a key
  (`key:Q`) or a MIDI note (`note:1:36`) to a slot, a scene or a track's stop button;
  `launcher.removeMapping` and `launcher.mappings` remove and list them. Agents launch with
  `clip.launch` and friends as before; the mappings are for the window and controllers
  ([spec](spec/clip-launching.md#launching-from-keys-and-notes)).
- **CPU load.** `launcher.state` reports `cpu`: the audio engine's share of wall time spent
  rendering, in percent, measured over windows of 0.2 s and smoothed so it settles within about
  a second (`0.0` before anything is measured, `null` without an audio output). The control bar shows the same number.

- **Level meters.** With `--audio`, `mixer.levels` reads the main output's and every track's
  peak and RMS level per side in dB (`null` for silence) and whether it clipped;
  `mixer.resetClip {track?, main?}` turns clip indicators off
  ([spec](spec/level-meters.md)). An agent can play a project and check its levels this way.

- **The browser's library.** The server opens the user's browser library (`browser.json` and the
  folder indexes in Suhina's settings folder) and keeps it up to date while it runs
  ([ADR 0012](decisions/0012-file-library.md)). `library.search` finds samples, MIDI files,
  presets and projects in it, the built-in effects and instruments, and installed plug-ins; `library.list`,
  `library.info`, `library.folders`, `library.refresh`, `library.recent`, `library.pinned`,
  `library.tags` read it; `library.pin`, `library.unpin`, `library.tag`, `library.setTag` and
  `library.removeTag` change pins and colour tags. `library.addFolder` and
  `library.removeFolder` are refused for agents: the user chooses the browser's folders.

- **The sample map.** The server also analyses the library's samples in the background, as the
  desktop app does ([sample map spec](spec/sample-map.md), [ADR 0013](decisions/0013-sample-map-analysis.md)).
  `soundmap.status` tells how far it is per folder; `soundmap.layout {place?, text?, tags?,
  grid?, aspect?, maxSeconds?}` lays a place's analysed samples out by how they sound and their kind
  (position 0–1, kind such as kick, snare or hat, whether it was guessed from the sound and whether it
  was set by hand, whether it matches `text` and `tags`,
  and with `grid` a cell in a grid shaped close to `aspect`, width over height); `soundmap.similar {item, count?, place?}` gives the samples that sound
  most like one; `soundmap.info {item}` its kind, where the kind comes from (`hand`, `name`,
  `guess` or `none`) and its features; `soundmap.setKind {item, kind}` sets a sample's kind by
  hand (`auto` gives it back to its name and sound); `soundmap.analyse {folder?}` analyses again.

- **Devices.** `device.list` lists the built-in effects and instruments with every parameter (id, range,
  default, unit, options); `track.devices` lists a track's devices with their values;
  `device.add`, `device.addSd {track, path, sample?, position?}`, `device.remove`, `device.setEnabled` and `device.setParam` (through
  `command_run`) edit a track's chain, one undo step each ([ADR 0004](decisions/0004-built-in-devices.md)).
  `device.setParam` sets a plug-in device's parameter too, by the id `device.pluginParams` lists
  (a running plug-in's parameters with their ranges, default, value and the plug-in's own
  text), heard from the next block; `device.setAutomation {param, points}` automates one over the
  arrangement, `points` a flat list of beat and value (`[0, 0.2, 4, 0.8]`), `[]` removing it.
  Values are kept in the parameter's range once `device.pluginParams` has listed them.
  `track.devices` gives a plug-in's set values and any device's lanes (`automation`).

- **Device groups.** A device inside a device group is named by a *device path*
  ([spec](spec/device-groups.md)): a whole number is a device on the track, as before, and text
  such as `"2/1/0"` is the device at index 2 (a group), its branch 1, device 0 there. Every
  `device.*` command takes a path for `device`, and `device.add`, `device.addSd`,
  `device.addPlugin` and `device.paste` a path for `position` (`"2/1/3"`: branch 1 of the group
  at 2, before its device 3). `track.devices` gives each device its `path`, and a group its
  `params` (`mix`, `gain`, `macro.1` … `macro.8`, set with `device.setParam`), `branches` (name,
  volume, pan, mute, solo and their `devices`) and `macros`. `device.group {track, device, count?,
  name?}` puts consecutive devices into a new group of one branch without changing the sound,
  `device.ungroup` takes a group of one branch apart, `device.addGroup {track, position?}` adds an
  empty one, `device.move {track, device, to}` moves a device along the chain, between branches or
  into and out of a group, and `device.duplicate` copies a device or a whole group next to itself.
  `device.duplicateMany {track, devices}` and `device.paste {track, position?}` refuse the edit and
  leave the project unchanged when the resulting chain would have a second instrument, a MIDI effect
  after an instrument or an audio effect, an instrument or MIDI effect on a track that plays no
  notes, or a MIDI effect in a group.
  `branch.add`, `branch.remove`, `branch.duplicate`, `branch.rename`, `branch.setVolume {db}`,
  `branch.setPan`, `branch.setMute` and `branch.setSolo` take the group's path as `device` and the
  `branch` index. Each is one undo step. A group whose branches start with an instrument is the
  track's instrument and goes first; an effect group goes anywhere an effect can. Automation of a
  device inside a group is refused until the engine plays it.

- **Automation.** `automation.addPoint`, `automation.movePoint`, `automation.deletePoint`,
  `automation.movePoints`, `automation.deletePoints` (several points of one lane in one undo
  step), `automation.clear`, `automation.setEnabled` and `automation.list` (through `command_run`) edit
  the envelopes of a track's `volume`, `pan` and `mute` (no `device`) and of any device
  parameter (`device` and its `param` id), point by point, each one undo step; values in the
  set command's unit (the volume in dB). `device.setAutomation` replaces a whole device lane, for
  built-in devices too. Setting an automated parameter by hand turns its lane off (the answer
  says `"automation": "off"`) until `automation.setEnabled` turns it on again
  ([automation spec](spec/automation.md)).
  MIDI effects (`pitch`) go first, on a MIDI track, and change the notes on their way to the
  instrument (`fm-synth`), which goes right after them and plays the notes; audio effects
  follow. `track.devices` gives each built-in and `.sd` device's `role` (`effect`,
  `instrument`, `midi_effect`). `device.add {preset, voice?}` adds the FM synth playing a DX7 voice from a `.syx` file.
  `device.sdNodes` returns every SDK node contract, ports, units, defaults, configuration examples
  limits and custom canvas capabilities (including equality expressions and literal-value selector buttons, conditional controls, choice menus and text alignment) for effect authoring by agents ([guide](device-sdk.md)).
  `device.setParam` applies declared conditional peer links as one undo gesture and sends all affected values to the player.
  `device.addSd` validates and embeds a composable effect graph in the project; its supported nodes
  render in both real-time playback and offline rendering. Renders and
  the playing engine run the chains.
  `device.setParam` accepts an optional bounded text `gesture` id: send `continuous: true` for
  intermediate updates across both axes, then `continuous: false` for the last axis on release.
  This gives an XY edit one undo step. A final unchanged value also ends the gesture.

- **Plug-ins.** `plugin.scan`, `plugin.list` and `plugin.folders` (through `command_run`)
  find and list installed plug-ins; a scan loads each new or changed plug-in file in a process
  of its own and tries files that failed again ([ADR 0006](decisions/0006-plug-in-hosting.md)).
  `plugin.list` with `ignored: true` also lists the files that hold no plug-ins; it always lists
  the files built for another processor (`incompatible`), and each plug-in's type and whether it
  is switched on. `plugin.setEnabled` switches a plug-in off (not offered, not loaded) or on,
  `plugin.scanFile` reads one listed file again, and `plugin.setFolderEnabled` switches a
  plug-in folder off or on for scans. `plugin.check` lists an open
  project's plug-in devices with the installed plug-in found for each (also a VST3 version
  standing in for a VST2 one), how it was found and whether its settings carry over. `plugin.addFolder` and
  `plugin.removeFolder` are refused for agents, since a folder decides whose code a scan runs;
  the user adds folders with `suhina-cli plugins add-folder` or in the desktop app's Settings ›
  Plug-ins. `plugin.setHosting` chooses
  how a plug-in's instances share host processes (`binary`, the default, `vendor` or
  `instance`), from their next start.

- **Plug-in devices.** `device.addPlugin` adds an installed plug-in (by name, id or file, from
  `plugin.list`) to a track, an instrument first; one undo step. With an audio engine
  (`--audio`) the active project's plug-ins run: each is found in the catalog and started with
  its saved settings in the background, between commands; `device.plugin` (and `plugin` in
  `track.devices`) says where one stands (`loading`, `running`, `missing`, `crashed`, `failed`,
  or `stopped` while the project does not play) with the plug-in found, its latency, whether
  its editor is open, and how it keeps up (`blocks` given and `late_blocks` not answered in
  time, which the chain went on without; the device panel shows the share). `device.openEditor` and `device.closeEditor` open its editor in a window
  on the user's screen (above Suhina's window when the desktop app runs it, or inside its
  device panel when its settings say so). When the desktop app runs it, the editor opens in
  the background: the answer is `"editor_open": "opening"` and `device.plugin` says when it is
  open; otherwise it opens at once and the answer gives its `width` and `height`. And `device.restart`
  starts a crashed one again with its
  last settings. Renders run the installed plug-ins with their saved settings; their result's
  `plugins` lists those that are missing or did not start.

## Tools

| Tool | What it does |
|---|---|
| `project_open` | Open a Suhina project (`.suhina`) or a `.als` Live Set below the read root as a new, active project; returns its `project` id, a summary and import warnings. |
| `project_inspect` | Summary, tracks (volume, pan, mute, devices) and arrangement clips as JSON; optional `project`. |
| `command_list` | Command ids with description, kind (`read`, `edit`, `history`, `transport`, `projects`, `plugins`, `plugin`, `library`, `audio`) and parameter schema; optional `filter`. |
| `command_run` | Run one command: `{"id": "track.setPan", "params": {"tracks": ["Bass"], "pan": -1}}`; optional `project`. |
| `command_batch` | Run `steps` in order; each editing command is its own undo step; `stop_on_error` (default true); optional `project` for every step. |
| `project_render` | Render the active project (or `project`): the arrangement, or `scene` (1-based), to a WAV below the write root. Audio files are loaded from the opened set's folder; missing ones are listed in `warnings`. Its plug-ins start with the state the project's running plug-ins have now (what their editors changed included), as `project.save` writes it. With `launches` (and `seconds`), render a launch script instead: transport commands run at times in seconds from a stopped transport, e.g. `[{"at": 0, "id": "scene.launch", "params": {"scene": 1}}, {"at": 4, "id": "track.stop", "params": {"track": "Bass"}}]`; each step's result is returned. The steps are confined like the tool calls' and never carry the file-writing commands (`project.export`, `project.save`); the render's output is the only file a script writes. |

Commands are defined once in `suhina-session` (ADR 0002); the MCP server, the CLI and the
desktop app all run the same ones. [`commands.md`](commands.md) lists every command with its
parameters; it is generated from the registry, so a new command needs no edit there.

## Attached to the app

`suhina-cli mcp --attach` works on the projects open in the running desktop app instead of
projects of its own ([ADR 0028](decisions/0028-mcp-in-the-running-app.md)): the person sees each
change as it is made, and each editing command is one step in the undo history of its tab
(`edit.undo` and the window's Ctrl+Z walk the same history).

```sh
suhina-cli mcp --attach
```

- **Which app.** The app of the same build: both executables sit in one folder, and the app
  listens on a named pipe (Windows) or a Unix domain socket named from that folder, never a
  network port. Test builds in different folders do not meet. Starting a build's `suhina` while it
  runs opens the project as a tab in the running window instead (`suhina --new-instance` starts
  another window, which serves no agents).
- **The person lets agents in.** Nothing listens until **Settings › Agents and updates › Agents › Let agents
  work on open projects** is on. Each connection then waits: the window asks whether to allow the
  client (by the name it gives at `initialize`), and tool calls wait for the answer; Deny ends the
  connection. The control bar shows each agent let in with a **Disconnect** button.
- **Tabs.** The agent works on the window's tabs: `project.list`, `project.switch` and the
  `project` argument act on them. `project_open` is refused (the person opens projects), and so is
  `project.close`: closing loses a tab's unsaved changes and its undo history.
- **Files.** Settings › Agents and updates controls where attached agents may write (`project.save`,
  `project.export`, `project_render`, `project.autosave` with `now`). The project's folder is
  allowed by default; the person can turn that grant off and add or remove other folders.
  Each grant includes subfolders and persists across restarts. Relative write paths start in
  the project's folder when allowed, otherwise the first available allowed folder. An unsaved
  project can export into an explicitly allowed folder. Reads still use the project's folder
  and browser library. Agents cannot grant themselves access. Revocation applies to subsequent
  operations and exports still gathering state; it does not cancel a render already started.
- **Settings.** Commands that change the app's settings or the person's library rather than a
  project are refused: `audio.setOutput`, `output.setCuePair`, `output.setCueVolume`,
  `midi.setInput`, `plugin.setHosting`, `library.setPluginsView`, `library.pin`, `library.unpin`,
  `library.tag`, `library.setTag`, `library.removeTag`, `soundmap.setKind`, and
  `project.autosave` with `interval` or `cleanUp`. Transport commands play on the window's output.
- **Plug-ins.** `plugin.setEnabled` and `plugin.setFolderEnabled` change the person's plug-in
  library and are refused. So are `plugin.scan` and `plugin.scanFile`: a scan would run on the
  window's thread while plug-ins load; the person scans from the plug-in manager. `project_render`
  takes its snapshot with the window's projects and renders on a thread of its own, so the window
  keeps drawing.
- **Export.** `project.export`, including a step in `command_batch`, gathers plug-in states
  across window frames with a deadline, then renders and encodes on a worker. The export stays
  with its original project when the person switches tabs. Closing that project or cancelling
  the request during preparation releases its state gather. A failed or late plug-in state
  read names the plug-in in the result's warnings and uses its last known state. Batch steps
  remain sequential, but the person and other calls can work while an export runs.
- **Errors.** With no app of the build running, or with agents not let in, `--attach` says which
  and exits.

## Errors

- Unknown tool arguments return JSON-RPC error `-32602` naming the key, before anything runs.
- A command that fails returns a tool result with `isError: true` and leaves the project
  unchanged. Unknown command parameters are errors too.
- Every tool has a title and read-only/destructive hints.

## Using it from Claude Code

```sh
claude mcp add suhina -- /path/to/suhina-cli mcp --read-root /path/to/projects --write-root /path/to/renders
```

Or, to work on the projects open in the running app:

```sh
claude mcp add suhina-app -- /path/to/suhina-cli mcp --attach
```

Sampler manual boundaries are edited with `device.setSampleSlices` (`track`,
`device`, `points`: normalized source positions strictly between 0 and 1). The
command validates, sorts and deduplicates the list and supports undo/redo; an
empty list clears the map. Optional `continue` groups consecutive drag edits.

`track.devices` includes `sample_slices` when a Sampler has an explicit manual map.

`device.updateSd` applies the same read-root confinement as `device.addSd` and
`device.loadSample`, including symlink resolution.

The optional `sample` path on `device.addSd` and `track.add {sd, sample}` binds one audio
file to the SDK resource key `sample`. Both paths are checked against the caller's read roots,
including symbolic links. Missing required bindings fail insertion before a project edit.
The graph remains embedded as device state; its sample reference is a separate model extension,
so saving, project relocation and missing-file relinking preserve both. Custom Rust hosts can
supply several named resources through `PreparedSamples`; the desktop command currently binds
one source. Graphs needing other keys fail clearly rather than loading an arbitrary path.

`device.loadSample` can replace the `sample` source of an SDK graph that already has
that binding, preserving embedded graph state and parameter values. The replacement
uses the same read-root confinement and undo path as the built-in sampler.

`device.setNamFile` reads its `path` (a `.nam` model or an impulse response) under the same
read-root confinement. `project.collect` writes below the write root: its `folder` (made if it
is new, inside an existing folder of the root) or, without one, the folder of the project's own
file; a blank `folder` is refused, and so is a `Samples` or `Samples/Collected` in it that leads
outside. Which parameters are places on disk is their type (`ParamType::Path`, with what the command
does with it: reads a file, files or a folder, writes one, a library item or place), and
`confine()` checks every one by that type; the files commands write have each their own rule. A
test gives every `Path` parameter a path outside the roots and fails when `confine()` lets it
through, and fails for a text parameter named like a file or a folder (`path`, `paths`, `folder`,
`file`, `to`, `item`, `preset`, `sd`, `output`, `place`) that is not a `Path`, so a new command
cannot take a path unchecked. Which commands an agent may not run is on each command's spec
(`Policy::agents`: denied, or not to an agent attached to the app).
