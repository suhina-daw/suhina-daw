---
description: How edits reach an engine that is already playing — a new graph built off the audio thread and swapped in at an exact frame
read_when: edit while playing, graph swap, renderer rebuild, Controller::update, live edits, volume change not heard, LOOP-26
type: decision
status: current
last_verified: 2026-10-23
covers:
  - crates/engine/src/lib.rs
  - crates/engine/src/swap.rs
  - crates/engine/src/ahead.rs
  - crates/session/src/lib.rs
  - crates/engine/src/chain.rs
  - crates/engine/src/launcher.rs
  - crates/engine/src/plugin.rs
---

# 0005: Edits while playing

## Context

A `Renderer` is built from a project once, then moved to the audio thread. Edit commands
(`transport.setTempo`, `track.setVolume`, `track.setPan`, `track.setMute`, `clip.move`, undo and
redo) change the session's project, but a playing renderer did not hear them until the project
was opened again. AGENTS.md rule 3 says how a change has to reach the audio thread: swap whole
graphs atomically, never allocate, lock or free there.

Most of what an edit changes is baked into the renderer's flat lists: a track's volume and pan
are multiplied into every note and audio event of the track, the tempo into every frame
position, and every warped clip placement owns a stretcher. Parameter messages for "cheap"
changes would need a second path for each kind of edit, and would not cover the tempo or a moved
clip; devices (ADR 0004) will add many more parameters.

## Decision

1. **The renderer is split into a graph and its state.** The *graph* is everything derived from
   the project: note and audio events, stretched clip placements and their stretch voices, the
   launcher's prepared clips, lengths and the tempo. The *state* is what playing has produced:
   the playhead and the play start marker, the transport, sounding note voices, playing audio
   clips, and the launcher's
   runtime (what each column plays, fades and has queued, the beat, the tempo, quantisation).
2. **An edit builds a new graph off the audio thread.** `Controller::update(project)` builds it
   on the calling thread with the audio pool and sample rate the renderer was built with (or
   the pool given since with `Controller::set_audio`, when files were added; ADR 0011), and
   sends it to the audio thread through a lock-free queue (`rtrb`).
3. **The swap happens at an exact frame.** The controller picks a frame a little ahead of the
   playhead it reads from the monitor (`SWAP_AHEAD_SECONDS`), moves the new graph's stretch voices there
   and fills their rings before sending it. `process` splits its block at that frame, so the
   block before it is the old graph and the frames from it the new one.
4. **State carries over at the swap, without allocation.** The playhead stays in beats: if the
   tempo map changed, the transport's origin is moved so the same beat plays on (the old map's
   beat at the playhead, placed through the new map; between two constant tempos the old
   rescaling, to the frame). The controller moves the new graph's stretch voices to that beat
   through the map of the last graph it sent. Cursors into the
   new event lists are found by binary search; playing audio clips are found again by their
   time; an unwarped audio clip plays at its own speed, not the beat's, so it carries on from
   the file position it was at (after a tempo edit its beat position moves, as in other DAWs).
   Sounding note voices ring on, on their track's chain in the new graph. A track's device
   chain keeps its running devices when the new graph has the same devices on the same track,
   with the new parameter values, so instrument notes and effect tails carry on (ADR 0004
   decision 9), and its sends keep the sound in their delay lines when they go to the same
   returns with the same latency (decision 12). The launcher's runtime is copied column by column (a
   column or slot that no longer exists is dropped). Slots carry over by index, except when the
   edit (or its undo) inserted or removed one scene: the session finds that with
   `SceneShift::between` and sends it with the graph (`Controller::update_with_scenes`), so a
   playing or queued clip moves with its row and a removed row's clips stop. A time signature a
   launched scene set stays across the swap, like its tempo, unless the project's own changed. A slot
   whose stretched audio is unchanged keeps its built stretch voice (moved from the old graph to
   the new, whether its clip plays or not; ADR 0003 › decision 7); any other playing clip's new
   voice is moved to the clip's position. Level meters keep their levels and clip
   indicators while each meter slot plays the same track as before; a slot that now holds
   another track (tracks moved in the project's order) starts from silence
   ([level meters](../spec/level-meters.md)).
5. **Nothing is freed on the audio thread.** The old graph goes back to the controller through
   a second queue and is dropped there, on the next update or when the controller is dropped. If
   that queue is full, the audio thread keeps the old graph and takes no new one until it can.
6. **If the playhead moved unexpectedly** (the transport stopped, started or jumped between the
   build and the swap), the swap moves the stretch voices to where the playhead really is
   (`AheadReader::move_to`); with the transport stopped, to the play start marker, where the
   next start begins. Until the worker has done so, stretched clips are silent; they
   never play out of time. That silence is the move's, so it is not counted as underruns.
7. **The stretch worker thread serves every graph.** It owns a list of workers; a new graph's
   worker joins it before the graph is sent, and a worker leaves when its graph has been dropped.
   The launcher's preparers (ADR 0003 › decision 7) join the same way and leave once their graph
   has been swapped out.
8. **The session notifies the player.** After an edit or undo/redo command changes the project,
   `Session::execute` calls `Player::update`, so the CLI, MCP and the window all get the same
   behaviour. The arrangement's loop region is part of the graph too, so a loop edit takes
   effect at the swap, as any other edit does.

## Amendment: running plug-ins across graphs (2026-10-12)

A plug-in's audio half is not part of any graph's project: it is started by the session and
sent to the audio thread as a message (`Controller::send_plugin`), placed in its slot in the
playing graph or kept until a graph with its slot takes over. A swap hands every running
plug-in to its slot in the new graph, by key, even where the chain around it changed, and the
delay lines of latency compensation carry their sound over when their length stays the same.
What a graph knows about plug-in devices (`Controller::set_plugin_info`: which have a slot,
which play notes, their latency) is set before the graph is built
([ADR 0006](0006-plug-in-hosting.md)).

A renderer that ends because another audio output was chosen (`audio.setOutput`) gives its
plug-ins back once it no longer runs (`Renderer::into_plugins`): from its graph, the graph not
yet taken back, the ones parked or still queued, the newest for each device. The next renderer
is built with them (`Renderer::build` with `RenderInputs::with_plugins`) when the sample rate stays the same, so the
plug-ins keep running and their state across the reopen.

## Amendment: edits in the next block (2026-10-12)

In a DAW an edit should be heard at once; with a graph per edit taken one per audio block and
0.15 s ahead, two edits in a row took two blocks, and a fader moved only the next note. As in
other real-time audio graphs (structural changes built off the audio thread and swapped
atomically, parameter changes as lock-free messages applied at a block boundary, as Firewheel,
klingt and JUCE's graph do), now:

1. **Every graph due in a block is taken in that block,** at its frame, in order; the swap
   queue holds 16 graphs.
2. **`SWAP_AHEAD_SECONDS` only when it is needed:** a graph with stretched arrangement clips still
   takes over 0.15 s ahead, so their stretch voices are moved and their rings filled first; any
   other graph takes over at the start of the next block.
3. **Device parameters are messages, not graphs.** `Controller::set_device_param(track,
   device, param, value)` sends a numbered message through a lock-free queue; the renderer
   applies it at the start of the next block. `device.setParam` uses it when an engine plays
   (the project is updated and an undo step recorded as for any edit). Each graph records the
   number of the last message its project already held; when a graph built before newer
   messages takes over, the renderer applies those again (it keeps the last 256), so an older
   graph never undoes a newer value. A plug-in device's parameter takes the same message, with
   its parameter id for `param`; its slot sends the value to the plug-in at the start of the
   next block (ADR 0006, decision 3), and a graph built after it sends only the values that
   changed.
   The same message path updates parameters on prepared .sd effects (ADR 0021); changing the
   graph structure still rebuilds the renderer.
4. **Sounding notes follow their track's mix.** Each sine voice remembers its track and
   velocity; at a swap it takes the new graph's volume, pan and mute (and bus, if the track's
   chain changed) at once instead of keeping them until the note ends.

## Consequences

- An edit is rendered from the next engine block after the command returns, plus the time to
  build the graph (with stretched arrangement clips, 0.15 s after), and heard once that block
  leaves the output's ring, which runs up to two blocks ahead of the device (`crates/output`). A
  device parameter is rendered from the next block without building anything.
- Structural edits still rebuild the whole graph. Building did become too slow for knob drags in
  a large project (60 to 80 ms a step on the window's thread), so a track's volume, pan and
  sends take the device parameters' message path (`MIXER_VOLUME`, `MIXER_PAN`, `MIXER_SEND`):
  the chain remembers the volume and send levels its gains were built at and scales them by the
  new value's ratio; a track's pan is applied live over its whole range (its gains, and its
  post-fader sends', are built centred and the pan's equal-power factors multiply them); each is
  smoothed like a fader (a one-pole low pass at about 25 Hz). Where that cannot do it (a volume
  or pan on a lane, a group, a send that starts from silence), the message is refused and the
  edit builds a graph as before. A knob of a track that is not heard (muted, outside a solo)
  changes nothing played, and keeps the playing graph.
- Offline rendering stays the oracle (rule 4): tests swap graphs inside an offline render and
  compare against renders of the edited project.

## Sources

- Firewheel design document (events flushed together, schedules recompiled only on graph changes): https://docs.rs/crate/firewheel/0.12.1/source/DESIGN_DOC.md
- klingt (parameter messages through ring buffers, applied at block boundaries): https://docs.rs/klingt
- Stateful audio graphs and node lifetime (graphs swapped atomically, old graphs freed off the audio thread, node state kept across rebuilds): https://omr.it.com/blog/stateful-audio-graphs-dsp-node-lifetime-management/
