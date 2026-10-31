# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Parity scoring: how much of a reference manual's documented surface Suhina has today.

    python scripts/parity/score.py [--dir target/parity]

Page counts come from scripts/parity/headings.py; the coverage estimates and the prose notes are
ours (see docs/research/parity-live-12.md, Sources and IP). Prints the roll-ups; the tables of the
report are rendered from the data here by update_report.py, and the figures by charts.py.
"""

import argparse
import json
import re
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]

# The measurement's provenance: bumped by hand when scripts/parity/run.py is run against a manual
# and the result is checked against the code. The report's front matter, its headline block and
# both figures carry it, so a stale measurement is visible rather than implied.
MEASURED_AT = "2026-10-13"
MEASURED_COMMIT = "eeb8899"

# Section counts from the last full run (headings.py over the manual). Used when target/parity/ is
# not there, so the report and the figures still render for someone without a copy of the manual -
# including the workflow that regenerates them on main.
SECTIONS = {"total": 344, "zero": 111, "coverage": 0.168}

# The manual the measurement was read from, kept by hand: the manual is not in the repository.
# COMMANDS below is a placeholder that pull requests leave alone: update_report.py replaces it with
# the count of docs/commands.md, generated from the command registry on main. It stays until the
# pull requests opened before that change have merged, so the ones that changed it do not conflict.
MANUAL_CHAPTERS = 43  # chapters 1 to 43, the same chapters CHAPTERS lists
MANUAL_PDF_PAGES = 1039
COMMANDS = 190

# num, chapter title, pages of the manual body, coverage 0..1 (None = not a feature chapter),
# status, note. "coverage" is the share of what the chapter documents that Suhina has today.
CHAPTERS = [
    (1, "Welcome to Live", 1, None, "n/a", "Front matter."),
    (2, "First Steps (installation, Learn/Info view, Settings)", 11, 0.52, "partial",
     "Release and nightly packages for Windows, macOS and Linux; Windows packages include installed ASIO driver support; Help > About names the build "
     "(version, nightly date and commit, platform). Settings pages, grouped by task in headed sections, scrolling and searchable: General, Export (MCP write folders), Arrangement, Session and mixer, Clip editor, Transport, Detail area, Browser, Appearance (zoom, theme, graphics), MIDI, Audio (driver/device/rate/buffer/exclusive/cue; "
     "a change or a device back after unplugging keeps playback and launched clips going; "
     "active ASIO driver's native control panel and separate engine/callback buffer readouts; input device, MIDI inputs on/off), "
     "Shortcuts, Plug-ins; the detail area beside the browser or across the full width. Missing Link, Tempo & MIDI, File & Folder, Library, Record/Warp/Launch, "
     "Learn view, Info view."),
    (3, "Live Concepts", 27, 0.50, "partial",
     "Control bar, browser, sets, arrangement/session, tracks, audio/MIDI clips, devices, mixer "
     "concepts exist. Routing, recording, racks, clip/device view and presets are missing or thin."),
    (4, "Working with the Browser", 60, 0.53, "partial",
     "Search, categories, folders watched on disk, pins, recent, colour tags, preview through the "
     "engine, sample map by sound similarity (smooth with 200 000 items, also while its window is resized) with kinds recognised from the sound and set by hand; samples and MIDI files dragged from the system's file "
     "manager land like a browser drag; dropping one sample on an empty MIDI track adds Sampler (exact placement waits for pointer positions from the "
     "windowing layer); installed plug-ins grouped by vendor in the tree, and in the Plug-ins "
     "category as the instruments or the effects by name and format, with pins and tags on them. "
     "Samples copied out to a folder (Copy to folder…, library.copyTo) or dragged out to the file manager as copies; "
     "samples dragged onto a browser folder moved or copied there (asked, or as a setting says; library.moveTo keeps their pins, recent places and tags). "
     "The arrow keys move through the list after a click on a row, opening and closing folders. "
     "Missing collections/tag editor depth, packs, Splice, "
     "Cloud, Push transfer, filter groups, browser history."),
    (5, "Managing Files and Sets", 43, 0.40, "partial",
     ".suhina save/open, File > Open recent, .als import, templates, several projects in tabs, missing-file locate and "
     "relink, backups on an interval or before critical operations with the offer after a crash. "
     "Export renders the mix to wav (32f, 24 or 16 with dither), flac, ogg or mp3 (through the vendored LAME 3.100 "
     "encoder, at 128/192/256/320 kbps or VBR V0/V2, at 32/44.1/48 kHz), at a sample rate, with normalise and overwrite "
     "refused until asked: File > Export… (format, rate, depth, mp3 rate, normalise, dither, overwrite) and the shared project.export "
     "(a range and tail, a source, or the whole mix), one export at a time, the state the running plug-ins have now "
     "(what their editors changed), and the write rules held again where the bytes land. "
     "Settings > Export grants and revokes MCP write folders, including the project-folder default; "
     "attached MCP exports prepare over frames and render off the window thread. "
     "Missing project folders/collect, unused files, packing, .asd, decoding cache, set "
     "merging, file-reference view, the export dialog's source and range rows."),
    (6, "Arrangement View", 22, 0.66, "partial",
     "Layout, zoom, transport, track headers with a band of the track colour that opens the colour palette, ruler/play start, a click in the tracks setting the next start without moving playback and a ruler seek quantized to the launch grid, play from the selected clip, clip drag+move, "
     "whole tracks cut, copied, pasted (between project tabs too) and duplicated with their devices, automation and clips, "
     "an optional toolbar of tools (Select, Range, Split, Erase, Draw, Mute) with keys while the view has the focus, "
     "track names with # shown as the track's number, an empty track taking the kind of the clip dropped on it, several clips' edges dragged together, a MIDI clip's left edge drawn out before its notes, "
     "overlap replace/stack, fades and fade shapes, the control bar's grid value (bars to "
     "triplets, or none) drawn as lines that adapt to the zoom and snapping moves, drops, fades "
     "and ruler clicks (Alt for off the grid), nested groups folded and unfolded, "
     "tracks dragged into and out of groups, markers (locators: add, move, rename, delete, jump; imported from Live "
     "Sets), the arrangement loop brace (draw, resize, move, on/off with Ctrl+L; playback wraps "
     "sample-accurately; imported from Live Sets), split at the playhead or a clicked beat, consolidate "
     "(MIDI joined, audio rendered to a file), time-signature changes (commands; the ruler counts bars "
     "through them and marks them), time selections painted over clips, one row of stacked clips "
     "consolidated on its own, a waveform zoom that draws audio clips larger without changing their "
     "sound (a slider by the ruler, Ctrl+Alt+Up/Down, a setting). Missing previous/next marker, punch "
     "in/out, editing signature changes on the ruler, crop, crossfades, linked-track editing, the "
     "time commands."),
    (7, "Session View", 13, 0.67, "partial",
     "Slots, scenes, scene launch, stop buttons, track status, column headers filled with the track "
     "colour and a colour palette opened from the channel strip (one palette shared with the "
     "track menu and colour tags), quantised launching, a mixer resized "
     "from its top edge with a channel strip per track that shows more as it grows (volume fader "
     "with meter and dB, pan, sends, name, device list), on audio tracks an input block above "
     "the toggles (Audio From with the source's dropdown and In/Auto/Off), a double-click that "
     "makes a clip of the track's kind (a setting), "
     "selecting a slot, copying clips between the arrangement and slots (keys, menus, a drag over "
     "the view switch); "
     "scenes inserted, duplicated, deleted and renamed from a menu, with a tempo and time signature "
     "each. A launched clip is selected and the detail area shows its follow-action controls. "
     "Missing capture-and-insert scene, moving clips between slots, select-on-launch options."),
    (8, "Clip View", 34, 0.24, "started",
     "Detail area shows clip properties and, for a MIDI clip in the Arrangement, a first piano roll "
     "(docs/spec/ui-clip-editor.md): keys, ruler, note grid, velocity lane, zoom and scroll, a toolbar of "
     "tool icons with the note clipboard and a toolbar size, a MIDI tools column beside the notes, "
     "and the selection described in the clip column (count, span, pitch and velocity range); the "
     "editor keeps a least height with its notes in view and closes by its splitter, an x or View > "
     "Hide editor, opening again when a clip is selected. The clip column edits the clip: loop on "
     "or off, an audio clip's loop start and end, start and end markers, gain (a slot's too), warp on or off, warp mode and transpose. "
     "No clip panels, no sample editor, no groove/scale panels."),
    (9, "Audio Clips, Tempo, and Warping", 18, 0.42, "partial",
     "Real-time time-stretching along warp markers read from Live Sets, clip gain, transpose, "
     "six warp modes in the model, warp on or off, the mode and transpose set from the clip column "
     "(clip.setAudio; warp turned on gives unwarped audio markers at its start and end), tempo "
     "detect for the browser. One stretch algorithm serves "
     "every mode except Re-pitch, so the mode names are not yet different algorithms, and rates "
     "outside 0.25x-4x fall back to Re-pitch. Tempo changes over the song (steps and ramps, a tempo "
     "map every clip follows, read from a set's tempo envelope; a scene tempo overrides it until "
     "transport.followTempoMap or the next start); the tempo field typed or dragged to a thousandth of a BPM by the place grabbed, clips following while it moves. Missing tap tempo, tempo follower/leader, "
     "auto-warp of long samples, warp-marker editing, quantise audio, drawing the tempo as an "
     "automation envelope."),
    (10, "Editing MIDI", 41, 0.44, "started",
     "A first piano roll draws, selects (click, rectangle, Ctrl+A), moves, resizes and deletes notes "
     "on the grid (Alt off-grid), by mouse or by arrow keys, with Select, Draw, Paint, Erase, Slice and Mute "
     "tools (keys 1 to 6, the wheel with the right button held steps through them) and "
     "its own grid value (one grid menu: snap on or off, adaptive widths that follow the zoom, fixed "
     "values, finer and coarser, triplets, Ctrl+1 to Ctrl+4), pulled up to the full height, copies, cuts, pastes and duplicates them, "
     "splits them at a drawn cut line (note.split splits every note the line crosses), plays keys "
     "through the track's instrument, mutes and unmutes notes (a muted note keeps its place and is "
     "never played, in the model, the project file and an imported Live Set), "
     "quantises them (Ctrl+U, and grid, amount and ends in the MIDI tools column) and edits velocity in a lane, "
     "through note.* commands with undo; a duplicated clip's notes are its own once edited. A "
     "workflow menu in its toolbar switches between three presets and a Custom mix of the click, "
     "right-click, double-click and duplicate behaviours. The ruler shows the clip's loop and its "
     "start and end markers, dragged on the grid (clip.setLoop, clip.setStart, clip.setLength), "
     "zooms and scrolls time when dragged, and Z and Shift+Z zoom to the selection or the clip. A note's "
     "left edge moves its start, Ctrl+drag copies, Alt+drag sets velocity, a line drawn in the "
     "velocity lane sets many stems, the lane's height is dragged and an Audition button sounds "
     "drawn and moved notes. Ghost notes of the clips playing together, of the same track or of "
     "one picked clip are drawn faintly behind the notes. An empty "
     "clip of the track's kind (MIDI or audio, the audio one with no file yet) is made with a "
     "double-click on a slot or a lane (clip.create). A clip has a scale "
     "(clip.setScale, a project default new clips take) whose keys the rows mark, and Fold and Fold "
     "to scale show only the keys with notes or in the scale. No "
     "chords, probability or MPE lanes."),
    (11, "MIDI Tools", 37, 0.12, "started",
     "A MIDI tools column runs quantise, transpose, humanise, fit to scale, invert, reverse, legato, "
     "x2 and /2 and the velocity randomise and ramp over the selection or the whole clip, each one "
     "note.* command with undo and a seed for the random ones. No chord/strum/arpeggiate tools, no "
     "generative or shape-based tools."),
    (12, "Editing MPE", 10, 0.00, "absent", "No MPE data, editing or MPE-aware devices."),
    (13, "Converting Audio to MIDI", 5, 0.05, "planned",
     "Spec written (docs/spec/audio-to-midi.md); Sampler slicing, dynamic manual boundaries and waveform zoom/pan are available; audio-to-MIDI detectors remain roadmap."),
    (14, "Using Grooves", 6, 0.00, "absent", "No groove pool, extraction or commit."),
    (15, "Using Tuning Systems", 9, 0.00, "absent", "No .scl/.ascl tuning systems."),
    (16, "Launching Clips", 12, 0.77, "partial",
     "Launch quantisation (none, 8 bars..1/32, triplets), scenes (with their tempo and time "
     "signature), stop, loop, back to arrangement (the arrangement's dimmed lanes and the ruler's "
     "button), warped launched clips, follow actions (A/B by "
     "chance, linked or timed, jump to a slot; by command and in the detail area), samples looping "
     "in slots with or without a known tempo, a slot clip's loop switched on or off (clip.setLoop), "
     "launch modes (trigger, gate, toggle, repeat) with press and release, legato, per-clip launch "
     "quantisation, slots and scenes launched from computer keys. Missing velocity amount, clip "
     "offset/nudge, launching from live MIDI notes, follow-action multiplier and scene follow actions."),
    (17, "Routing and I/O", 22, 0.39, "partial",
     "Track output to main or a group (track.setOutput); MIDI controllers play the armed tracks "
     "and, with auto-arm on, the selected one as well (while a take records, arming "
     "does not follow the selection, Settings > MIDI, #834), with MIDI monitoring Auto / In / Off per track, "
     "each track choosing its MIDI port and channel (track.setMidiInput), with the mod wheel, "
     "pitch bend and aftertouch reaching the instrument alongside notes (#812); an audio input "
     "device with a channel or pair per track and monitoring Auto / In / Off "
     "(track.setAudioInput). No MIDI out, external instrument, resampling, sidechain, "
     "multi-timbral."),
    (18, "Mixing", 18, 0.62, "partial",
     "Faders (vertical in the Session view's channel strips), pan, mute, solo (exclusive/Ctrl), groups as buses nested up to 16 deep (solo and mute "
     "through the levels), master with devices and plug-ins of its own over the whole mix and its volume, pan and mute lanes, latency compensation, "
     "sends pre or post fader into return tracks that run their device chains (track.setSend, "
     "track.setSendMode, return.add/remove/rename, send knobs in the Session mixer), level "
     "meters (peak, RMS, clip) per track, group, return and main, also over MCP, and a cue output "
     "on a second pair of the device (previews, per-track cue, cue volume); return tracks "
     "imported and drawn. Missing crossfader, track delay, send fields in the Arrangement headers."),
    (19, "Recording New Clips", 13, 0.54, "partial",
     "Independent record arm and Auto / In / Off input monitoring (MIDI controllers, audio inputs); MIDI and audio recording into "
     "an empty slot from the next launch boundary (ended on the next one, then played looped) or "
     "into the arrangement, several armed tracks at once, a count-in of 1, 2 or 4 bars, takes lined "
     "up with what was heard, audio written to WAV files off the audio thread, one undo step per "
     "take; the take drawn growing with its notes or waveform while it records, every loop pass or "
     "jump back kept as a take of its own; notes played from the computer keyboard (Ctrl+K). Missing overdub, punch in/out, recording automation, capture MIDI, session record button."),
    (20, "Bounce to Audio", 6, 0.15, "partial",
     "Whole-project offline render to 32-bit float WAV (CLI/MCP), from an open project with its "
     "running plug-ins' state as heard; no per-track or group bounce, no paste."),
    (21, "Comping", 6, 0.85, "partial",
     "Take lanes made by recording into the arrangement (a lane per loop pass or jump back, the latest "
     "copied to the track and playing), drawn under the track with the parts in use lit and the rest dimmed, "
     "folded away from a button or a menu; a click copies a take's clip and a drag a part of it onto the track "
     "(take.comp), the parts named and coloured after their takes; Alt+Up/Down swap a part's take, the cut "
     "between two parts dragged (take.moveCut), Recolor lane and Recolor selected take, splits reaching the "
     "takes, Delete take, take pieces picked with Ctrl and deleted; a take lane soloed in place of its track, "
     "put whole on the track, unused takes deleted; short crossfades where parts of two takes meet; clips "
     "not stacked over takes; saved with the project. Missing take lanes made by hand, renaming and "
     "reordering them."),
    (22, "Stem Separation", 8, 0.00, "planned",
     "Roadmap item (Demucs candidate, weights unaudited). The pipeline around a model (chunking, "
     "resampling) exists in crates/separation with a test backend; no model and nothing a user "
     "can run yet."),
    (23, "Working with Instruments and Effects (Device View)", 34, 0.40, "partial",
     "Device chains per track and bus, parameters through device.* commands and the detail area, "
     "the devices one click from a MIDI clip's editor (a Clip / Devices switch, Ctrl+Shift+D) and "
     "a double-click on a track header or channel strip that shows its devices, "
     "DX7 voices, stock-preset ADR, latency compensation, plug-in state in the panel. "
     "Every built-in device, .sd device and running plug-in has editable controls in its panel "
     "(a layout, or a generated parameter list), and a device may open a larger expanded panel "
     "over the main area. "
     "SDK-authored .sd effects and instruments load onto tracks, embed in projects, expose parameters, "
     "graphs and custom panels, and support explicit revision updates. Missing A/B, "
     "hot-swap, preset browser depth, defaults, device delay review."),
    (24, "Using Plug-Ins", 12, 0.82, "partial",
     "CLAP and VST3 hosted out of process, sandboxed, VST3 bundles on Windows, macOS and Linux, "
     "editors (Windows/X11), scans that try failures again and settle files built for another "
     "processor, a plug-in manager (plug-ins switched off, several picked with Ctrl, Cmd or Shift and turned off or on from the right-click menu, folders per format and switched off), latency "
     "compensation (followed when a plug-in changes its latency while it plays), saved state "
     "(a change in an editor counts as unsaved), VST2 read and matched to VST3, and — behind the "
     "host's vst2 feature, off by default until its release questions are resolved — VST2 hosted "
     "directly: scanned in the system's VST folders and any folder added for the format, a "
     "project's VST2 plug-in that has no VST3 or CLAP copy installed runs through the host with "
     "its saved bank as the state, while a copy that exists still plays it (ADR 0006 decision 8), "
     "parameters set "
     "and automated from the engine and shown as a generated list in the device panel, in step "
     "both ways with a plug-in's own editor (VST3 through the host's handler, CLAP through its "
     "blocks' output parameter events: an edit there becomes the panel's value with the next "
     "block, and the editor follows the panel and automation), the song's tempo, "
     "transport, position, bar, time signature and loop given to every plug-in, ARA plug-ins per audio "
     "clip (Edit with, the clip's gain and fades after it, its track's other clips past it; edits saved, clip changes followed live, the editor in the clip's place, "
     "one document a plug-in and project so its editor shows every track; warped and looped clips not "
     "yet), VST2 editors — a window of its own or embedded where the platform allows (Windows, "
     "X11), the plug-in's own size ask followed by the editor and reported back. Missing AU, "
     "configure mode, VST preset banks, sidechain parameters."),
    (25, "Instrument, Drum and Effect Racks", 20, 0.25, "partial",
     "Device groups: Ctrl+G groups picked devices, parallel branches with volume, pan, mute and "
     "solo in the group's panel, a mix against the dry signal and an output gain, instruments "
     "layered on the same notes, plug-ins inside, latency compensated, groups nested four deep, "
     "moved and duplicated by command, devices inside groups picked and Ctrl+A picking a whole chain. No macros, key/velocity/chain zones, drum pads or group "
     "presets."),
    (26, "Automation and Editing Envelopes", 22, 0.58, "partial",
     "Envelopes for track volume, pan and mute, built-in, .sd and plug-in device parameters: breakpoints "
     "with curved segments and jumps, saved, played in playback and renders (64-frame steps, ramped "
     "mixer gains), a manual change turning a lane off until re-enabled. Lanes under each track in "
     "the Arrangement: a parameter chooser with a search, the automated parameters first, devices and "
     "parameter groups folded under headings, points added, moved on the grid, selected and moved or "
     "deleted together, curves bent by dragging, re-enabled from the lane, a click near the line adding "
     "a point on it, a faint line at the value of a lane without points; a time range painted on a lane "
     "copied, pasted onto any lane, duplicated right after itself and deleted, and shapes inserted into it "
     "(sine, triangle, saws, square, ramps, curves, swell) with a depth and cycles a bar, live while dragged, into a point's segment from its menu or as the "
     "shape new points take; a curve handle half-way along each segment; a click adds a point on the line "
     "(anywhere, as a setting); a range's points scaled live, their corners and jumps rounded into curves or thinned to fewer bent segments, the arrows moving "
     "selected points; the automation under a clip moving, copying and duplicating with it, a box from a track picking its lanes' points. Missing recording, "
     "drawing with a pencil, plug-in lanes in the view, send, master and tempo automation, "
     "locking."),
    (27, "Clip Envelopes", 13, 0.00, "absent", "No clip envelopes."),
    (28, "Working with Video", 6, 0.00, "absent", "No video import or window."),
    (29, "Live Audio Effect Reference", 139, 0.21, "partial",
     "Suhina has 10 effects (utility, auto-pan, eight-band EQ, three-band DJ EQ, vinyl distortion, "
     "overdrive, compressor, limiter, delay, reverb) against 42 documented audio effects; the EQ, "
     "delay and reverb take every parameter of their counterparts, several others are close in kind but far "
     "narrower in parameters and modes."),
    (30, "Live MIDI Effect Reference", 13, 0.10, "started",
     "One built-in MIDI effect, pitch: semitones or scale degrees with the clip's scale, and a key "
     "window that blocks, folds or limits notes; its step buttons are planned. SDK MIDI-effect "
     "graphs run before the instrument too. The other 7 documented MIDI effects are missing."),
    (31, "Live Instrument Reference", 120, 0.12, "started",
     "FM synth and single-source sample/granular instrument against 13 instruments; "
     "graphical envelope stages with cumulative timing and directly draggable points, persistent waveform with grouped granular controls and a shared six-shape pitch/filter/volume/pan LFO with Random, Hz/time/beat-synced rates, independent source and loop-edge fades and tempo-synchronised sample Warp using editable original BPM; five pitch-preserving modes currently share one stretcher; no multisample zones, "
     "drum sampler, wavetable, subtractive or physical modelling yet."),
    (32, "Max for Live", 6, 0.05, "planned",
     "ADR 0021 defines data-only .sd graphs with a Rust SDK, DSP runtime and custom UI nodes; typed sample resources, polyphonic voice/grain subgraphs, slicing, stretch and versioned components are executable through the Rust host API; "
     "visual graph editing is planned. Imported Max devices remain placeholders; no Max runtime."),
    (33, "Max for Live Devices", 35, 0.00, "absent", "Not applicable until Max for Live exists."),
    (34, "MIDI and Key Remote Control", 9, 0.25, "partial",
     "Every command can be bound to a key (keymap.json, Settings > Shortcuts); a key mapping mode "
     "(Ctrl+M) maps keys to clip slots, scenes and stop buttons, saved in the project, and "
     "launcher.addMapping takes MIDI notes too. No live MIDI remote input, CC mapping, takeover "
     "modes or mapping browser."),
    (35, "Using Push 1", 40, 0.00, "absent", "No control-surface support."),
    (36, "Using Push 2", 59, 0.00, "absent", "No control-surface support."),
    (37, "Synchronizing with Link, Tempo Follower, and MIDI", 9, 0.00, "absent",
     "No Link, no Tempo Follower, no MIDI clock sync."),
    (38, "Computer Audio Resources and Strategies", 7, 0.40, "partial",
     "CPU-load meter, the program's RAM in the control bar, buffer-size choice, render-ahead "
     "engine, plug-in deadline reporting, real-time scheduling without setup (MMCSS Pro Audio, "
     "SCHED_FIFO or rtkit), an 80 s / 58-warped-clip set opens in 0.4 s. Missing track freeze, "
     "disk-load meter."),
    (39, "Audio Fact Sheet", 8, None, "n/a",
     "Not a feature list. Suhina renders 32-bit float and does not dither; no neutrality test suite."),
    (40, "MIDI Fact Sheet", 6, None, "n/a",
     "Not a feature list; MIDI timing and quantisation behaviour. Suhina has no MIDI timing tests yet."),
    (41, "Accessibility and Keyboard Navigation", 11, 0.05, "absent",
     "No screen-reader or keyboard-navigation story beyond the shortcut system."),
    (42, "Live Keyboard Shortcuts", 23, 0.30, "partial",
     "Keymap covers command binding, selection actions and window actions; many editing shortcuts "
     "have no command behind them yet."),
    (43, "Credits", 3, None, "n/a", "Front matter."),
]

AREAS = {
    "Views and editing (ch 6-10, 16)": [6, 7, 8, 9, 10, 16],
    "Browser, files, project (ch 4, 5)": [4, 5],
    "Devices (ch 23-25, 29-31)": [23, 24, 25, 29, 30, 31],
    "Mixing, routing, I/O (ch 17, 18, 38)": [17, 18, 38],
    "Automation, envelopes (ch 26, 27)": [26, 27],
    "Generative/transform (ch 11-15)": [11, 12, 13, 14, 15],
    "Recording, bounce, comp (ch 19-22)": [19, 20, 21, 22],
    "Outside scope (ch 28, 32-37, 41)": [28, 32, 33, 34, 35, 36, 37, 41],
    "Setup, shortcuts (ch 2, 3, 42)": [2, 3, 42],
}

# Chapters that describe products or hardware outside a clip-launching DAW's own scope.
# ADR 0016 › Decision 2 lists the same chapters; change both together.
OUT_OF_SCOPE = {28, 32, 33, 35, 36}  # video, Max for Live (+ its devices), Push 1, Push 2


def weighted(rows):
    """Page-weighted coverage of a selection of chapter rows."""
    num = sum(p * c for _, _, p, c, _, _ in rows if c is not None)
    den = sum(p for _, _, p, c, _, _ in rows if c is not None)
    return num / den, den


def section_metrics(work: Path):
    """Coverage weighted by numbered sections instead of pages.

    Falls back to the counts from the last full run (SECTIONS) when the heading map is not there,
    which is the normal case on a machine, or in CI, without a copy of the manual.
    """
    path = work / "headings.json"
    if not path.is_file():
        return dict(SECTIONS)
    headings = json.loads(path.read_text(encoding="utf-8"))

    def squeeze(text):
        prev = None
        while prev != text:
            prev = text
            text = re.sub(r"(?<=\d) (?=\d)", "", text)
        return text

    counts: dict = {}
    for entry in headings:
        if entry["kind"] != "section":
            continue
        chapter = int(squeeze(entry["num"]).split(".")[0])
        counts[chapter] = counts.get(chapter, 0) + 1

    coverage = {n: c for n, _, _, c, _, _ in CHAPTERS}
    total = sum(counts.values())
    zero = sum(v for k, v in counts.items() if not coverage.get(k))
    covered = sum(v * (coverage.get(k) or 0) for k, v in counts.items())
    return {"total": total, "zero": zero, "coverage": covered / total if total else 0.0}


def report(work: Path):
    scored = [r for r in CHAPTERS if r[3] is not None]
    all_cov, all_pages = weighted(scored)
    core = [r for r in scored if r[0] not in OUT_OF_SCOPE]
    core_cov, core_pages = weighted(core)
    scope = [r for r in core if r[0] != 41]
    scope_cov, scope_pages = weighted(scope)

    print(f"all chapters            {all_cov*100:5.1f}%  ({all_pages} pages)")
    print(f"without Push/M4L/video  {core_cov*100:5.1f}%  ({core_pages} pages)")
    print(f"roadmap scope           {scope_cov*100:5.1f}%  ({scope_pages} pages)")
    metrics = section_metrics(work)
    if metrics:
        print(f"by numbered sections    {metrics['coverage']*100:5.1f}%  "
              f"({metrics['total']} sections, {metrics['zero']} in untouched chapters)")
    print()
    for name, nums in AREAS.items():
        sel = [r for r in CHAPTERS if r[0] in nums and r[3] is not None]
        cov, pages = weighted(sel)
        print(f"{cov*100:5.1f}%  {pages:>4}p  {name}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dir", default=str(REPO / "target" / "parity"))
    args = parser.parse_args()

    report(Path(args.dir))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
