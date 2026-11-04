---
description: What of ARA hosting is built and how it behaves - scanning, the clip host, the bindings, binding an instance, the document and its edit batches, audio sources, the archive, the editor, notes on clips, playback control, carrying edits between tracks, and where it still differs from ADR 0033
read_when: ARA, ARA plug-in, clip host, Edit with, clip.setAra, araEdits, ara_update, ara_notes, ara_playback, ara_export, ara_import, test:ara, ARA archive, audio source region, suspended region, ARA editor
type: spec
status: draft
last_verified: 2026-11-02
covers:
  - crates/plugin-host/src/ara.rs
  - crates/session/src/ara.rs
  - crates/session/src/clip_ara.rs
  - crates/ipc/src/source.rs
---

# ARA hosting

What runs of [ADR 0033](../decisions/0033-ara-hosting.md) (proposed), stages 2 to 6 of its list
in a first form. Where the code differs from the ADR's decisions it is marked *interim*; the
decision stays the goal. Own design; the ARA API's own documents are the ADR's sources.

## Scanning

the scan marks each VST3 plug-in that supports ARA (`PluginInfo::ara`, shown
by `plugin.list`): its module declares a class in the category `ARA Main Factory Class`
with the plug-in's class name, as `ARAVST3.h` pairs them, or the module has one of each. It
is read from `moduleinfo.json` without loading anything, or from the factory's class list in
the scan worker without making any object. The catalog keeps it (version 4; an older
catalog's VST3 files are read again so installed ARA plug-ins are found). `plugin.setHosting
... instance` is refused for such a plug-in, and an `instance` choice saved before its
support was known starts it in its file's process. The factory's API generations are checked
when an instance is bound.

## The plug-in belongs to the clip

[#990](https://github.com/suhina-daw/suhina-daw/issues/990). An ARA edit is the clip's, not the
track's, so the plug-in is no longer one of the track's effects:

- **A clip names its ARA plug-in** (`Clip::ara`, `clip.setAra {clips, on, plugin?}`, the clip
  menu's **Edit with** and **Stop editing with**); a clip holds no other effect. Only clips that
  name it are its playback regions (ADR 0033 › 5). A track's clips use one ARA plug-in in the first
  version.
- **Signal flow:** the clip's raw audio → its ARA plug-in → the clip's gain and fades → the track's
  devices → Main. The plug-in reads the source itself (ADR 0033 › 4), so gain never reaches its
  analysis; the host applies each clip's gain and fades to the renderer's output, as the ARA API
  describes for per-region processing in the host. The track's other clips go past the plug-in.
- **Hosting:** the plug-in is the channel's first device, marked as its clip host
  (`Device::clip_host`), which the device lists leave out and a device command never puts behind
  another device. The engine runs it on its own (never in a joint block with the plug-ins after
  it), fed with nothing, its output scaled frame by frame by the playing ARA clip's gain and fade
  (1 while stopped, so the editor's auditions sound). Overlaps of ARA clips stay refused (decision
  6), so one renderer per track is enough; overlaps would need one per concurrently sounding
  region, round robin. A disabled or missing host leaves the clips playing as they are.
- **The editor** opens with a double-click on an ARA clip, the host's view selection set to it.
- **Copies and moves** keep the clip's flag and its edits.
- **Old projects:** a track's ARA device from before keeps editing every audio clip of its track;
  the first **Edit with** or `clip.setAra` there makes it the clip host and names those clips. An
  ARA plug-in dropped on a track becomes its clip host for all its audio clips, as before.

## What runs

What runs now, in [#667](https://github.com/suhina-daw/suhina-daw/pull/667) (stages 3 to 6 of
the list below, in a first form). Where it differs from the decisions above it is marked
*interim*; the decision stays the goal.

- **Bindings.** The ARA headers (2.3, Apache-2.0) are vendored in `third_party/ara-api`
  (`VENDORED.md` names the commit). `scripts/ara_bindings.sh` generates
  `crates/plugin-host/src/ara_sys/` from them with bindgen, with its layout checks, so a
  struct that does not match the C headers fails to compile. The three VST3 interfaces of
  `ARAVST3.h` are declared by hand in `crates/plugin-host/src/ara.rs`. The headers pack structs
  to 1 byte on x86 and to 8 on ARM, so the bindings are generated once for each (`x86.rs`,
  `arm.rs`) and the host builds with the one for its CPU.
- **Model updates.** The host calls the document controller's `notifyModelUpdates` every 250 ms
  while it is not editing, right after every edit and restore, and before storing the edits, as
  the API asks ("periodically by the host whenever not editing"). Without it a plug-in sends what
  an edit set off only on its own timer, which showed as a clip resize drawn about 2 s late in
  the editor (#718).
- **Musical context.** The document carries the song's tempo map as ARA tempo entries (seconds
  and quarter notes; a tempo ramp gets one a beat, since ARA takes the tempo as even between two)
  and its time signatures as bar signatures, and the host's content access controller hands them
  to the plug-in's musical context readers, so its timeline counts bars as the song does (it
  counted from 1 at the clip). A change in either reaches it as an edit
  (`updateMusicalContextContent`). A document without them, from an older engine, is an even
  tempo in 4/4.
- **Transport requests.** The plug-in's own play and stop buttons and a click in its editor's
  timeline reach Suhina's transport: the host's playback controller queues them and sends
  `ara_playback` (protocol version 12), and the window plays, stops or moves the play position
  through the tempo map. A newer position replaces one not sent yet. The app takes these requests
  in on the thread that reads the host's lines, before it wakes the window, so the frame it wakes
  moves the transport. Until the engine reports the position asked for (or a second has passed),
  the host shows the plug-in that position in its process context while the transport is stopped:
  the request goes to the engine and back, and the plug-in otherwise drew the old position from the
  next block, its playhead flickering back for a moment. A loop made or switched in the
  editor is the arrangement's loop (`ara_playback` `cycle` and `cycle_on`, protocol version 14):
  its start and length in seconds become beats through the tempo map and `transport.setLoop`, its
  switch `transport.setLoopEnabled`, and a newer loop replaces one not taken yet, as a dragged
  loop asks on every move. The plug-in sees the arrangement's loop, however it was set, in each
  block's process context (`cycleStartMusic`, `cycleEndMusic`, `kCycleActive`). A click on a clip in the arrangement
  moves the play position there, so the editor follows the clip it shows (#717). The rules that play
  from a selected clip's start (Settings › Play from, and a clip picked) give way
  on a track with an ARA plug-in, except that a click on a clip's name bar moves the marker to its start: there the editor's timeline and a click set where
  play starts.
- **Copies keep their edits** (ADR 0033 › 5). A clip that gets its own content (a paste, a split's
  second part, notes copied on edit) names in the update the modification it is a copy of
  (`clone_of`): the same clip's earlier one, or for a new clip the narrowest region of the same
  file whose span holds its own. The host makes the new modification with
  `cloneAudioModification`, so it starts with those edits; a plug-in that cannot clone gets a new
  one, as before.
- **Edits taken to another track** (stage 6 in part, #722). A clip an ARA plug-in edits that is
  moved or copied to another track asks first: **Keep edits** or **Copy as new** (or Cancel).
  `clip.moveMany` takes it as `araEdits` (`keep`, or `new`, the default). With keep, the track it
  goes to gets the same plug-in first in its chain when it has none (with no state of its own),
  and the clip's edits go along: the first track's plug-in stores the clip's modification and its
  source on their own (`storeObjectsToArchive` with a filter, a partial archive: `ara_export`),
  and once the other track's plug-in runs with the clip, it restores them for the clip's
  modification there (`restoreObjectsFromArchive` with a filter mapping the old persistent id to
  the new one, `ara_import`; protocol version 13). The session keeps each carry until it is done,
  for at most 60 s, and says when it could not be. A clip that keeps its content on the other
  track keeps its modification in the shared document (below), so its edits are already there;
  the archive carries them where it gets a content of its own.
- **Loading and analysis on the clips.** While an ARA plug-in loads, its track's audio clips say
  so (dimmed, "<plug-in> loading"); while it analyses them, how far (the least done of the
  track's sources, from the plug-in's analysis progress, sent as `ara_analysis` when it moves by
  a percent), with a bar filling along the bottom. The clips cannot be edited in it until then.
  An analysis ends when the plug-in reports it completed, or when, asked whether the source's
  notes are still incomplete (each time the notes are read, and every second while a clip shows
  the analysis), it says they are not: a plug-in that restores its analysis from an archive
  reports only that it started (#816). One progress covers all of a source's analyses, so
  while the plug-in says another content type it lists as analysable is incomplete, the
  analysis shows until that one is done too, or until it has not reported for 3 s (a type it
  never analyses unasked). A track that leaves the document forgets its sources'
  analysis, so the next track that plays one asks for it again. After an archive is restored
  every source's analysis is asked for again, as one asked for before may have been dropped for
  the archive's without a *completed* report; and a plug-in that keeps saying the notes are
  incomplete while every region of the track has them is taken as done once it has not
  reported for 3 s (Melodyne after a project is opened again, #816).
- **Notes on the clips** (#720). After a model update in which the plug-in said a region's,
  modification's or source's notes may have changed (not with "note scope remains unchanged"),
  and after the host's own edits, the host reads every region's notes with a playback-region
  content reader, outside the update as the API asks, and sends those that changed as
  `ara_notes` (times from the region's start, pitch, volume; at most 16 384 a region). The
  session keeps them by clip while the plug-in runs; the arrangement draws them as bars over
  the clip's waveform, which dims under them, placed through the tempo map. A bypassed
  plug-in's notes are kept and not shown. When the factory lists notes among what it analyses, the host asks it once
  for each source whose notes are not found yet (again after the tracks that played it left), so they show without the editor being opened. A
  finished analysis (its progress Completed) has the notes read too, and while a region has none
  yet they are looked for again every second, as a plug-in may finish without saying the content
  changed.
- **The document** (ADR 0033 › 2). `load` carries an optional `ara` document (protocol version 9):
  the track's name and place, the project's document key, the tempo, the sources (an id, a name
  and a shared-memory address each) and the regions (clip id, content id, start, length and
  offset in seconds). The host queries the component for `IPlugInEntryPoint`, requires ARA 2.0
  Final and initialises the factory once per process. The instances of one plug-in whose
  documents carry the same key (one per project: the session's rack makes it) share one document
  controller in their host process: the first makes it, with the host interfaces and the
  musical context, and each adds its track as a region sequence (named and ordered as the
  track), the sources and modifications it lacks (tracks that play the same content share its
  modification, and so its edits) and one playback region per clip, in one edit, then binds in
  every role (`IPlugInEntryPoint2` when there is one) before processing is set up. Its renderers
  get its own track's regions; its editor shows every track, so the plug-in's editor lists the
  project's tracks and shows the others' notes. On unload the track's regions leave its
  renderers and go with its sequence in one edit; the last track's unload takes the sources,
  modifications and context, then the document controller, as ARA prescribes. An instance let
  go of later (just loaded) leaves the document at once. An offline render's plug-ins share
  documents the same way, under a key of their own; a document without a key is the instance's
  own. A plug-in that cannot be bound
  does not load, and the reason is the device's (`... could not start: the plug-in does not
  support ARA`).
- **Persistent ids.** A source's id is its audio pool key, a modification's is `content-<id>`, a
  region's is `clip-<id>` ([#568](https://github.com/suhina-daw/suhina-daw/issues/568)).
- **Loops.** A looped clip is a region for each pass through its loop, all of one modification,
  so an edit plays in every repeat: the first pass's region is `clip-<id>`, the later ones
  `clip-<id>-<pass>`.
- **Audio** (ADR 0033 › 4). `suhina_ipc::AudioSource` writes a file's decoded samples once, planar
  32-bit floats after a checked header, into a region the host maps read only; `readAudioSamples`
  copies from it in chunks, as 32- or 64-bit samples, with silence outside the source.
- **Transport.** An ARA plug-in renders by the song position, so the engine sends a `Transport`
  event (playing, position in samples) and a `Tempo` event with every block to a plug-in loaded
  with a document, and the VST3 host turns them into the block's process context.
- **The project keeps the edits** (ADR 0033 › 7). The device's state is the component's state and
  its track's part of the document archive (`storeObjectsToArchive`, with the factory's
  `documentArchiveID`, filtered to the sources and modifications its clips played, and the
  document's own data) in one blob, so save, autosave, backups and a restart keep them as they keep any plug-in's state. On
  load the archive goes back with `restoreObjectsFromArchive` in an edit, once the document is
  built; an archive of a kind the plug-in does not read is refused with the reason.
- **Clip edits** (ADR 0033 › 3). When the track's clips change (moved, trimmed, added, removed,
  muted, a tempo change, an undo of any of these), the session sends the plug-in the whole new
  document at once (`ara_update`, answered `ara_updated`): the samples of files already shared
  are not shared again, only a new file's. The host matches it with what it holds by persistent
  id and changes it in one edit: a new source or modification is made, none goes (their edits
  stay in the archive, so an undo brings a deleted clip back with its edits), a region that moved
  or changed length is updated in place (`updatePlaybackRegionProperties`), a new one made, and
  one that is gone, or plays another content now, is taken out of the renderers and destroyed.
  Moving and trimming change nothing else, so the plug-in plays on and its editor stays open.
  Adding or taking out regions changes the renderers, which ARA allows only while the plug-in
  does not render: its processing stops for that and starts again, as for a latency change, a
  gap of a few blocks. Where an update fails, and for a plug-in loaded without a document (its
  track had no audio clips), the change instead loads it again with the new document once the
  clips have stayed put for 0.75 s, the edits of the clips still there coming back by their ids
  and an editor that was open opening again.
- **The editor in the clip's place** (stage 7, first part,
  [#594](https://github.com/suhina-daw/suhina-daw/issues/594)). An arrangement audio clip on a
  track whose plug-in is bound shows that plug-in's editor in the detail area's Clip view, where a
  MIDI clip has its piano roll ([clip editor](ui-clip-editor.md) › Where it opens). The
  session tells the plug-in which clip is selected with `ara_select` (a message that is not
  answered), and the host passes it to the editor view's `notifySelection`. The editor is
  embedded whatever the embedding setting says for other editors; **Float** takes it out to a
  window of its own and **Dock** back (`Session::set_ara_floating`). On Windows an editor's
  window passes the keyboard focus on to the plug-in's view, so its keys (Ctrl+Z) are its own. The clip's menu
  (**Open in** the plug-in) and Alt+M open it. The overlap marks and fixes, and the
  question when the plug-in is removed, come with the overlap guard.
- **Found when it loads, too.** The host tells the engine with `loaded` whether the plug-in
  answers `IPlugInEntryPoint` (`edits_clips`). One the scan did not mark (the module's ARA class
  named differently, say, or a catalog from before the scan read ARA) is then loaded again with
  its track's clips, with a notice, and is treated as an ARA plug-in from then on: the catalog
  remembers it, so its next device, in any project, starts with its clips at once.
- **Opened as a host with a pitch editor built in does.** A clip's menu (**Open in** the plug-in)
  and Alt+M work on any audio clip in the Arrangement: when the clip's track has no ARA plug-in
  running, the installed one (the scan's ARA plug-ins, Melodyne first, else Melodyne by name) is
  added to the track, and its editor opens in the clip's place once it runs with the clips.
- **An ARA plug-in goes first in its chain**, wherever it is dropped or `device.addPlugin` puts
  it (the result says `moved_first`): it plays the clips from their files, so a device before it
  would not be heard. Observed in another host, which moves it there too.
- **Other interim limits.** Warped clips are described at their file's speed (their start and end follow the song), and the overlap guard and suspension (ADR 0033 › 6) are not built: overlapping
  clips are both in the document. Offline renders load the plug-in with the same document. The
  `notifyDocumentDataChanged` flag is kept but not read yet: the edits are noticed when the
  state is read. `test:ara` (ADR 0033 › 10) does not exist yet; the test VST3 plug-in, listed as an
  ARA one, checks that a document reaches the host and that a refusal is reported.
