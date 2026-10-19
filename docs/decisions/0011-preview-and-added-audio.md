---
description: How a sample added while the project plays reaches the engine, and how the browser and the piano roll's key column preview sound through the engine without locks or allocation on the audio thread
read_when: preview, audition, preview voice, add a sample while playing, AudioPool, set_audio, clip.addFiles, preview.play, preview.noteOn, preview.noteOff, held preview note, key column, MIDI preview, browser sound
type: decision
status: current
last_verified: 2026-10-15
covers:
  - crates/engine/src/preview.rs
  - crates/engine/src/swap.rs
  - crates/session/src/preview.rs
  - crates/session/src/projects.rs
  - crates/session/src/files.rs
  - crates/session/src/lib.rs
  - crates/media/src/lib.rs
---

# 0011: Previewing, and audio added to a playing project

- **Status:** accepted
- **Date:** 2026-10-12

## Context

The browser ([spec](../spec/browser.md)) adds samples to a project that may be playing, and
previews samples and MIDI files. Until now the audio a project uses was decoded once when it
opened: the front end (window, MCP server, CLI) kept the `AudioPool` and handed it to the
renderer, whose graphs are all built with that pool (ADR 0005). A clip whose file is not in the
pool is silent. Nothing could play a sound outside the project.

AGENTS.md rule 3 rules out decoding, allocating, locking or freeing on the audio thread; rule 6
asks that previewing and adding files be commands; rule 4 that offline renders and playback
run the same code.

## Decision

1. **The session owns its project's audio.** `Session` keeps the `AudioPool` (front ends load it
   with `media::load_project_audio` and give it to the session, `Session::set_audio`) and the
   folder the project's relative paths are read from. Front ends start players with the
   session's pool.
2. **A new file is decoded off the audio thread and handed over with the next graph.**
   `clip.addFiles` decodes each sample on the calling thread, unless it is already decoded
   (`Session::provide_audio`, which the window uses after decoding on a thread of its own while
   the drop is prepared, so the window never waits). It adds the data to the session's pool,
   gives the player a new pool (`Controller::set_audio`: a new `Arc<AudioPool>` whose map holds
   the same `Arc<AudioData>`s plus the new ones, so nothing is copied), and the graph the edit
   builds is built with it (ADR 0005). The audio thread sees only a new graph, as for any edit.
   The pool's key for a dropped file is its absolute path; saving rewrites it relative to the
   project file (ADR 0010). An empty audio clip (`clip.create` on an audio track) names no
   file: nothing is decoded for it until a sample lands on its slot or lane.
3. **Undo leaves the decoded file in the pool.** It costs memory until the project closes and
   means redo needs no decoding.
4. **Previewing is a voice of its own in the engine, after the main channel.** The renderer has
   one preview voice. The controller sends it what to play over a lock-free queue (`rtrb`,
   `PreviewCommand`): a sample (`Arc<AudioData>`, a start in seconds, a gain), a MIDI preview
   (notes already turned into frames at the engine's rate, in a `Box`, with the track whose
   instrument plays them, or a fallback instrument), or stop. Whatever the voice drops (the
   `Arc` it played, the notes, a fallback instrument) goes back over a second queue and is freed
   on the controller's thread, as retired graphs are; when that queue is full the voice keeps
   the old one and takes nothing new until it can. The voice is mixed into the output after the
   track chains, so the project's main volume does not change it, and it plays whether the
   transport runs or not. *Amended 2026-10-15:* the voice plays into the cue bus, at the cue
   volume; when the cue has an output pair of its own the preview is heard only there, and
   otherwise also on the main output, as before ([cue spec](../spec/cue-output.md)). `PreviewMonitor` (`Controller::preview_monitor`) publishes whether it plays and its position.
5. **A sample preview reads the file at its own rate,** stepping by file rate / engine rate with
   linear interpolation, as an unwarped clip does, with the click guard at its start and end.
6. **A MIDI preview plays through the chosen track's chain** when that chain starts with an
   instrument (built-in or plug-in): the voice gives the chain note-ons and note-offs at their
   frames, so the track's effects and mix apply. It holds its own notes apart from the
   arrangement's and, when it stops, a graph swap replaces the chain, or the transport stops,
   it sends note-offs for the keys it holds, on the first frame of the block the stop applies
   in (preview commands are taken once the chains have begun the block). A track without an
   instrument plays through a
   **fallback instrument**: an FM synth made and prepared on the controller's thread (in
   `Controller::preview_instrument`, packed with the notes by `preview_notes`) and sent with the notes, processed into a buffer of its own and
   mixed with the sample preview.
7. **Offline renders include the preview voice** like everything else, so tests render previews
   through the same `process`.
8. **A note can be held until it is let go**, which is how the piano roll's key column plays
   ([spec](../spec/ui-clip-editor.md) › Playing the keys): the controller sends the preview voice
   a note whose end is "until `preview.noteOff`" (its own marker in the voice's held keys, so the
   block loop never releases it), and a release command names one key or every held key. A key
   the voice holds through a track's chain is held there too, so the chain does not release it at
   the frame it started with; letting the last key go ends the preview and sends what it holds
   back to the controller's thread, as a stop does.
9. **Commands:** `preview.play {path, from?, gain?, track?}`, `preview.noteOn {track, key,
   velocity?}`, `preview.noteOff {track, key?}` and `preview.stop` are Transport commands;
   they fail when no player is attached, like `transport.play`. The window decodes a preview
   on a thread and passes it with `Session::provide_audio` first, so the command finds it
   decoded.
9. **Previews with no project open** (#234): `Projects` keeps an empty session of the previews'
   own while no project is open (`Projects::preview_session_mut`), and `preview.play` and
   `preview.stop` run on it. Its player opens on the chosen output at the first preview
   (`preview.stop` opens nothing); it is closed before a project opens its output
   (`Projects::close_preview_output`, which the desktop app calls first and `Projects::add`
   calls too) and when another output is chosen, so there is still one stream at a time.

## Consequences

- A sample dropped while playing is heard from the next graph on; the window never waits for
  decoding.
- Previews use the same output device as the project (no second stream to open, which some
  drivers refuse), and are heard after the main channel, or on the cue pair of that device when
  one is chosen. With no project open they have the output to themselves, closed again before a
  project's opens.
- The key column's notes are previews like any other: they never touch the project, the undo
  history or the transport, and an agent can play a track's instrument over MCP with the same
  commands the UI uses.
- Undo of an added clip does not free its audio until the project closes.
- `Session` gains the pool, so the window no longer keeps one per project.

## Alternatives considered

- **A second audio stream for previews** (in `output`): simpler for the engine, but two streams
  on one device fail with exclusive-mode drivers, the preview could not play through a track's
  instrument, and offline renders could not test it.
- **Sending decoded audio to the audio thread as a message** for the graph to pick up: the
  graph already carries its pool; a new pool with the next graph needs no new path.

Project-open decoding confines both clip and Sampler references to the project folder
(without following symlinks or junctions); external references are reported as missing and require
explicit relinking. Direct sample loads and relink commands still decode user-selected
paths. Shared decoders reject formats outside 1000..384000 Hz and 1..32 channels before
creating a decoder, and cap decoded PCM at 1 GiB per file, checked before copying
each packet (all streams combined for Stems).
