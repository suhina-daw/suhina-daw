---
description: The layout of Suhina's own project file (.suhina), field by field, with what a reader accepts
read_when: .suhina layout, markers in the file, project file fields, clip id, lane id, scene id, next_id, file without ids, writing a project file by hand or from an agent, format version, version history, newer file refused, base64 state, muted note, note flags, clip scale, default_scale, scale root and mode
type: spec
status: current
last_verified: 2026-11-11
covers:
  - crates/project-file/src/lib.rs
  - crates/model/src/ids.rs
---

# The .suhina project file

Suhina's own project file ([ADR 0010](../decisions/0010-project-files.md)), version 5. It is
UTF-8 JSON, written pretty-printed. Its parts mirror the project model
([ADR 0001](../decisions/0001-daw-neutral-model.md)); this lists them and what a reader accepts.

```json
{
  "format": "suhina",
  "version": 5,
  "project": {
    "transport": { "tempo_bpm": 120.0, "time_signature": [4, 4] },
    "tracks": [ … ], "channels": [ … ], "contents": [ … ],
    "arrangement": { "lanes": [ … ], "markers": [ … ], "takes": [ … ] },
    "launcher": { "scenes": [ … ], "columns": [ … ] },
    "next_id": 12
  }
}
```

## Rules

- `format` is `"suhina"` and `version` a whole number from 1 to the newest the reader knows (5);
  anything else is refused, and the message names the format the file gave or the newest version
  the reader knows. A writer always writes the newest version (*Versions* below). Files saved
  before 2026-10-13 named another format and are not read
  ([ADR 0010](../decisions/0010-project-files.md) decision 12).
- Ids are whole numbers that fit 32 bits. Numbers are finite (a writer writes 0 for one that is
  not). A list or an optional field may be left out or `null`.
- Binary data (`state`, extension `data`) is standard base64.
- Arrangement lanes, clips (on the arrangement and in slots) and scenes have an `id`, a whole
  number from 1 to 4294967294, that stays the same when the object is moved or edited
  ([ADR 0025](../decisions/0025-object-identity.md)). The three kinds take their ids from one
  counter, the project's `next_id`: no two of them share a value, and a value once used is never
  given again, not even the id of an object that was deleted. Two objects with the same `id` are
  refused, whether of one kind or of two (a clip with a lane's id), as are `id` 0, the largest
  32-bit value and anything that is not such a number.
- An object without an `id` (a file written before ids, or an object added by hand) gets one when
  the file is read: the next values of the counter, in document order — each arrangement lane and
  then its clips, lane by lane; then the scenes; then the slots' clips, column by column. The same
  file therefore always reads with the same ids. A `next_id` that is missing or not above every
  `id` in the file is moved past them; one beyond 32 bits is refused, and so is a file whose
  counter runs out before every object has an id. Writing ids always, so a reader never derives
  them, is the version after 3 ([ADR 0026](../decisions/0026-project-folder-and-package.md) › 10).
- Markers, automation points, tempo changes and notes have ids only while the project is open,
  and the file holds none: nothing refers to one of them across a save
  ([ADR 0025](../decisions/0025-object-identity.md) › Decided since). A reader gives them new
  ones and ignores an `id` a file has on one.
- `extensions` (on the project, a track, channel, device, content or clip) is a list of
  `{ "format", "key", "data" }`: what an importer kept from its format; it is left out when empty.
- The project must hold together as an imported one must: ids unique, every reference resolves,
  fades fit their clip. A file that does not is refused, naming the first problems.
- A reader refuses a file over 512 MiB.

## Versions

The version rises with every change to what a file holds, additions included, and every file is
written as the newest ([ADR 0010](../decisions/0010-project-files.md) › Amendment: format
versions). A reader refuses a newer file, so an older Suhina never opens one and drops what it
does not know when it saves. Every older version is still read, with one exception: a start,
scene or change time signature with a denominator other than 1, 2, 4, 8 or 16 (/32, /64 and the
like) is refused in every version. None are known to exist and Suhina is pre-alpha, so nothing
reads them.

A change to what is written:

1. raises `VERSION` in `crates/project-file/src/lib.rs`;
2. adds `crates/project-file/tests/versions/v<N>.suhina` (`SUHINA_BLESS=1 cargo test -p
   suhina-project-file` writes it; the file of a version that shipped is never rewritten) and
   gives the new field a value in the test's `every_feature` project;
3. reads the earlier versions as before, and adds a row here.

Before 1.0 the number may rise often. A file saved by a pull request's test build is disposable.

| Version | What it added | Pull request |
|---|---|---|
| 1 | The project as JSON ([ADR 0010](../decisions/0010-project-files.md)); ids, markers, automation, follow actions, launch settings, cue and launch mappings came later without a rise | #212 and earlier |
| 2 | Device groups, written only when a project held one ([device groups](device-groups.md) › Project file) | #147 |
| 3 | Written by every file: the first version that rises with every change | #893 |
| 4 | An automation point's `curve` as two control points, `[x1, y1, x2, y2]`, each from -0.5 to 1.5 (a split's halves lie outside 0 to 1); a bend from earlier versions reads as the mirrored curve; take lanes, `arrangement.takes` ([recording](recording.md#take-lanes)) | #914, #916 |
| 5 | Versioned sampler source corrections in the `suhina/sample_edits` device extension ([sampler](sampler.md#source-correction-storage)) | Issue #1026 |
| 6 | A device's `id`, from the project's object counter ([ADR 0025](../decisions/0025-object-identity.md)); an earlier file's devices get theirs in document order | #763 › 1 |
| 7 | A macro mapping's `device` is the target device's `id`, not its path in the group; an earlier file's path is read as the device at that place | #763 › 1 |
| 8 | A launch mapping's `scene` counts from 0, as the commands do, where earlier versions counted from 1 | #763 › 1 |
| 9 | The project's `controller_mappings`, in place of the launcher's `mappings`; an earlier file's launch mappings are read into it ([MIDI mapping › Launch keys](midi-mapping.md#launch-keys)) | #1162 |
| 10 | An audio clip's own `audio` state (gain, gain envelope, transpose, warp, warp markers, tempo source) on the clip, the region keeping the file only; an earlier file's region state is read into every clip of its content ([ADR 0035](../decisions/0035-clip-gain-envelope.md)) | #1190 |
| 11 | The project's `lane_layouts`: how each channel's automation lanes are laid out in the Arrangement view ([automation](automation.md#in-the-arrangement-view)) | #1322 |

## Parts

| Part | Fields |
|---|---|
| `transport` | `tempo_bpm` (1–999, default 120); `time_signature` `[numerator, denominator]`, the numerator 1–255 and the denominator 1, 2, 4, 8 or 16 (default `[4, 4]`); `loop` (optional, left out without a loop region): `{ "start", "end", "enabled" }`, beats with `start` from 0 and `end` after it, `enabled` true or false (default false) ([loop region](arrangement-editing.md#loop-region)); `tempo_changes` `[{ beat, bpm, ramp }]` (beat after 0 and at most 120 000, bpm 1–999, `ramp` default false) and `time_signature_changes` `[{ bar, time_signature }]` (bar from 1, counted from 0; numerator 1–99, denominator 1, 2, 4, 8 or 16), each at most 10 000 and left out when empty ([tempo map](tempo-map.md); its *Ranges* compares these with what the commands set)) |
| track | `id`; `name`; `color` `"#RRGGBB"` or `null`; `notes`, `audio` (what it plays); `channel` (its mixer channel) and `parent` (its group) or `null`; `midi_input` `{ port, channel, monitor }`, the MIDI it hears ([ADR 0017](../decisions/0017-inputs-and-latency.md)): `port` `"all"` (the default), `"none"` or `{ "name" }`, `channel` 1–16 or `null` for every one, `monitor` `"auto"` (heard while armed, the default, also when `monitor` is missing as in files written before it), `"in"` (always) or `"off"` (never; an armed track still records); left out when it is every port and channel, monitored automatically; `audio_input` `{ channels, monitor }`, what it hears of the audio input device: `channels` `[]`, `[n]` or `[n, n + 1]` (from 1, at most 256), `monitor` `"auto"` (while armed, the default), `"in"` or `"off"`; left out when it is no input and auto |
| channel | `id`; `name`; `role` `regular`, `master`, `effect` (a return) or `submix`; `volume` (linear, not negative, default 1), `pan` (−1 to 1); `mute`, `solo`; `cue` (also heard on the cue output; written only when true, [spec](cue-output.md)); `output` (a channel, `null` for the audio output); `sends` `[{ target, level, pre_fader }]`; `devices`; `automation` (lanes of `volume`, `pan`, `mute`, as a device's; left out when the channel has none) |
| device | `id` (from version 6: the project's object counter, ADR 0025; an earlier file's devices get theirs in document order); `name`; `kind` `{ "type": "builtin", "id" }`, `sd`, `clap`, `vst3` (32 hex digits), `vst2` (a whole number), `foreign` `{ format, id }` or `group` `{ branches, macros }` (below); a `type` the reader does not know is refused with "this project uses a device a newer Suhina knows", never dropped; `enabled` (default true); `state` (base64: plug-in state or embedded UTF-8 .sd graph); `params` `[{ id, value }]` (a built-in effect or instrument's, or an .sd device's, by the parameter's stable id; a plug-in's by its parameter id in decimal, in the plug-in's own range, set over its `state`); `automation` `[{ param, points: [{ beat, value, curve }], enabled }]` (left out when the device has none; points in beat order, two at one beat a jump; `curve` `[x1, y1, x2, y2]`, the two control points of the segment to the next point (each from -0.5 to 1.5: 0 to 1 but for the halves of a split, which can lie outside), left out for a straight line, one bend from -1 to 1 before version 4; `enabled` left out when true; see [automation.md](automation.md)). A VST3 controller's state is the extension `Vst3ControllerState`; `clip_host` (`true`, written only when set): the channel's ARA plug-in its clips play through, always its first device (#990) |
| device group | the `kind` of a device group ([spec](device-groups.md)): `branches` (1 to 16), each `{ name, volume, pan, mute, solo, devices }` (`volume` linear, not negative, default 1; `pan` −1 to 1, default 0; `mute`, `solo` default false; `devices` as a channel's, groups inside nested at most four deep, at most 64 devices on a channel counting every depth; `extensions`); `macros`, exactly eight, each `{ name, mappings }`, a mapping `{ device, param, min, max }` with `device` the `id` of a device inside the group at any depth (from version 7; before, a path of whole numbers relative to the group: branch, index, then again for each nested group; a version 7 file still writes the path for a device that has no id yet, and every version reads it), `param` a parameter id, `min` and `max` 0 to 1 (default 0 and 1); no parameter mapped twice. The group's `mix`, `gain` and `macro.1` … `macro.8` are its device's `params` |
| content | `id`; `name`; `length` (beats); `lanes`, each `{ target, notes }` or `{ target, audio }` (`target` a track for a shared pattern, else `null`); `scale` (written only when the content has one, missing or `null` means none; a scale below) |
| scale | `{ "root", "mode" }`: `root` a whole number 0 (C) to 11 (B); `mode` `major`, `minor`, `harmonicMinor`, `melodicMinor`, `dorian`, `phrygian`, `lydian`, `mixolydian`, `locrian`, `majorPentatonic`, `minorPentatonic` or `blues`. A root outside 0 to 11, a mode not in this list or a scale that is not such an object is an error naming where it is, as other bad values are ([ADR 0001](../decisions/0001-daw-neutral-model.md) › Amendment: clip scales) |
| `next_id` | in `project`: the next id a lane, clip or scene gets; left out while nothing has an id |
| `default_scale` | in `project`, beside `transport`: the scale new MIDI clips take, written only when the project has one; missing or `null` means none |
| note | `time`, `duration` (beats); `key` 0–127; `velocity`, `release_velocity` 0–1; `channel` 0–15; `muted` (written only when the note is muted, missing means it plays; [ADR 0001](../decisions/0001-daw-neutral-model.md) › Amendment: muted notes) |
| audio | `file` (relative to the project file, `/` between folders); `absolute_file`; `sample_rate`, `channels`, `duration_seconds` (if known); before version 10 also the state its clips play it with (`warped`, `stretch`, `warp_markers`, `gain`, `transpose`; see clip audio), read into each clip of the content; `stream`: which audio track of a file with several plays (a Stems file: 0 the master, 1 and up the stems; missing or `null` for an ordinary file) |
| arrangement | `lanes`; `markers` `[{ beat, name }]`: named positions on the timeline, `beat` a number from 0 on; read in any order and kept in beat order; left out when there are none ([arrangement editing](arrangement-editing.md#markers)); `takes` `[{ track, name, clips, color? }]`: the take lanes, recorded passes kept under their tracks in the order recorded, their clips written as a lane's are but without ids, `color` (`"#RRGGBB"`) the take's own colour, left out while it takes the palette's; left out when there are none ([recording](recording.md#take-lanes)) |
| arrangement lane | `id`; `name`; `track` or `null`; `clips` |
| clip | `id`; `content`; `start`, `length`, `offset` (beats); `loop` `[start, end]` or `null`; `enabled`; `fade_in`, `fade_out` (beats); `fade_in_shape`, `fade_out_shape` (`linear`, `equal_power`, `fast`, `slow` or `s_curve`; default `linear`) and `fade_in_curve`, `fade_out_curve` (−1 to 1, default 0), see [arrangement-editing](arrangement-editing.md#fades); `follow` (written only when it differs from the default, missing means off) `{ enabled, time, linked, a, b, chance_a, chance_b }`: time in beats (more than 0, at most 100000), actions `nothing`, `stop`, `again`, `previous`, `next`, `first`, `last`, `any`, `other` or `jump N` (N a scene, from 1), chances 0–100, see [clip-launching](clip-launching.md#follow-actions); `launch` `{mode, legato, quantization}`, written only when it differs from the defaults (`trigger`, `false`, `global`), each key optional, see [clip-launching](clip-launching.md#launch-settings); `ara` (`true`, written only when set): the clip plays through its channel's clip host (#990); `audio` (written only when it differs from the starting state: a clip audio, below) |
| clip audio | an audio clip's own state, version 10 on ([clip panel](clip-panel.md)): `gain` (linear, default 1); `gain_envelope` `[{ beat, db, curve? }]` (content beats; dB from the gain, −70 to +24; `curve` the segment to the next point as an automation point's, `[x1, y1, x2, y2]` or a bend, left out when straight; at most 10 000; left out when empty); `transpose` (semitones, cents as hundredths; default 0); `warped` (default false); `stretch` `repitch`, `rhythmic`, `tonal`, `texture`, `smooth` or `high_quality` (default `smooth`); `warp_markers` `[{ time, audio_time }]` (content beats and file seconds); `tempo_source` `metadata`, `name`, `detected` or `manual` (left out when not known). A `tempo_source` not in this list, or a point without a `beat` or a `db`, is an error naming where it is |
| scene | `id`; `name`; `tempo_bpm` or `null`; `time_signature` `[numerator, denominator]` (left out when the scene has none; the numerator a whole number from 1 to 255, the denominator 1, 2, 4, 8 or 16) |
| launcher column | `track`; `slots` `[{ clip, has_stop }]`, one per scene |
| controller mapping | in the project's `controller_mappings` (from version 9), left out when empty: `source` (`key:<name>` or `note:<channel 1-16>:<note 0-127>`) and `target`, `{ "command", "track", "scene" }`: `clip.launch` with `track` and `scene`, `scene.launch` with `scene`, or `track.stop` with `track`, the scene from 0. A source appears once. The track and scene need not exist: a mapping whose target is gone is kept. A mapping that does not read refuses the file, as every part does. See [MIDI mapping › Launch keys](midi-mapping.md#launch-keys) |
| lane layout | in the project's `lane_layouts` (from version 11), left out when empty: per channel `{ channel, lanes, groups }`. `lanes` lists the lanes mounted under the track's top lane, top to bottom, each `{ device?, param }`: `device` is the device's id (left out for the channel's own `volume`, `pan` or `mute`) and `param` the parameter's id as its automation lane names it. `groups` lists groups of those lanes, each `{ name, lanes, folded?, device? }`: its lanes are in `lanes` and in no other group, `folded` (default false) and `device`, the device a group was made for. At most 1024 lanes and 256 groups a channel, a name of 1 to 128 characters, no lane twice. A lane whose device or channel is gone is kept. A layout that does not read refuses the file |
| launch mapping | before version 9, in the launcher's `mappings`: `source`, `target` (`slot`, `scene` or `stop`), `track` (slot, stop) and `scene` (slot, scene; from 0 from version 8, from 1 before). Read into `controller_mappings` (`slot` → `clip.launch`, `scene` → `scene.launch`, `stop` → `track.stop`) and never written; a version 9 file's launcher `mappings` is not read |

## Audio paths

A project saved to another folder gets each audio path written relative to its new file (with
`..` where needed, on the same drive) and the file's absolute path; opening it looks for the
relative path first, then the absolute one, and reads it only within the
[project file boundaries](missing-files.md#project-file-boundaries)
([ADR 0011](../decisions/0011-preview-and-added-audio.md) › 11). The audio is not copied.
