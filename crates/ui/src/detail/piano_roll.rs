// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll: the MIDI clip editor in the detail area (`docs/spec/ui-clip-editor.md`). A key
//! column, a ruler, the note grid and a velocity lane. Every edit is a `note.*` command; the view
//! keeps only what is on screen: zoom, scroll, the selected notes and a gesture in progress.

use std::collections::BTreeSet;

use egui::{Align2, Color32, CursorIcon, Id, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use serde_json::{Value, json};
use suhina_engine::Quantization;
use suhina_engine::model::{Clip, ClipId, LaneData, Note, Project, Scale, TrackId};

use crate::detail::midi_tools::ToolSettings;
use crate::detail::piano_roll_ghosts::{self as ghost_view, GhostSource, Ghosts};
use crate::detail::piano_roll_grid::{FIXED_VALUES, GridMode, PianoGrid, Width};
use crate::detail::piano_roll_ruler::{self as ruler_view, ClipShape, RulerState};
use crate::detail::piano_roll_scale::{self as scale_view, Fold, KeyRows};
use crate::interaction::grid::default_track_colour;
use crate::kit::theme;
use crate::kit::widgets::{self, Action};
use crate::settings::ToolbarSize;
use crate::settings::workflow::Workflow;

/// The gestures of [#351](https://github.com/suhina-daw/suhina-daw/issues/351): the left edge,
/// copying by dragging, velocity on a note and as a line, the lane's height and the audition.
#[path = "piano_roll_gestures.rs"]
mod gestures;
use gestures::Part;
pub use gestures::start_by;

/// Height of the toolbar, width of the key column, height of the ruler and the default height of
/// the velocity lane.
const TOOLBAR: f32 = 22.0;
const KEYS: f32 = 36.0;
const RULER: f32 = 24.0;
const VELOCITY_LANE: f32 = 72.0;
/// The velocity lane is left out under this height, and leaves at least this many key rows.
const LEAST_LANE: f32 = 12.0;
const LEAST_KEY_ROWS: f32 = 5.0;
/// The least height of the detail area while it shows the piano roll: the toolbar, the ruler and
/// five key rows at the default row height, and beside them the clip column's title and its five
/// rows (`docs/spec/ui-clip-editor.md` › Least height and closing).
pub const LEAST_HEIGHT: f32 = 120.0;
/// The toolbar's icon size and the space between its buttons, in points.
const TOOL_ICON: f32 = 14.0;
const TOOL_GAP: f32 = 3.0;
/// Key row heights and points per beat: the default and the zoom ranges.
const DEFAULT_KEY_HEIGHT: f32 = 12.0;
const KEY_HEIGHT_RANGE: (f32, f32) = (6.0, 32.0);
const BEAT_WIDTH_RANGE: (f32, f32) = (4.0, 400.0);
/// How much one point of Alt and the wheel scales key rows.
const KEY_ZOOM_PER_POINT: f32 = 0.004;
/// Grid lines closer than this, in points, are thinned out.
const MIN_LINE_SPACING: f32 = 6.0;
/// The most grid lines drawn in a frame, so a bad zoom can never draw without end.
const MAX_LINES: usize = 4000;
/// The most notes one command takes, so a stroke never builds one it cannot run.
const MAX_NOTES_PER_COMMAND: usize = suhina_session::MAX_NOTES_PER_COMMAND;
/// How close to a note's start or end, in points, the pointer resizes it instead of moving it.
const EDGE: f32 = 5.0;
/// How close to a velocity stem, in points, the pointer grabs it.
const STEM_REACH: f32 = 4.0;
/// The shortest note a resize leaves, in beats.
const MIN_LENGTH: f64 = 0.01;
/// The length of a note drawn without a grid, in beats, and its velocity.
const FREE_LENGTH: f64 = 0.25;
const NEW_VELOCITY: u8 = 100;
const EPSILON: f64 = 1e-9;
/// Two clicks this close in time (seconds) and space (points) are a double-click
/// (› Double-click in `docs/spec/ui-clip-editor.md`).
const DOUBLE_CLICK_TIME: f64 = 0.3;
const DOUBLE_CLICK_DISTANCE: f32 = 6.0;

const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// The name of MIDI key `key`, with 60 as C3.
pub fn key_name(key: u8) -> String {
    let name = NAMES.get(usize::from(key % 12)).copied().unwrap_or("?");
    format!("{name}{}", i32::from(key / 12) - 2)
}

fn is_black(key: u8) -> bool {
    matches!(key % 12, 1 | 3 | 6 | 8 | 10)
}

/// A MIDI clip as the piano roll shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct PianoClip {
    /// The params that name the clip in a `note.*` command: `track` and `index` or `scene`.
    pub address: Value,
    pub notes: Vec<Note>,
    /// Where the part of the content the clip plays ends, in beats (its loop's end when it
    /// loops): the grid past it is shaded.
    pub length: f64,
    pub color: Color32,
    /// Beats in a bar.
    pub bar_beats: f64,
    /// The fixed grid step in beats (the piano roll's own value or the control bar's); 0 is none
    /// or an adaptive grid, whose step [`grid_menu`](Self::grid_menu) gives at each zoom.
    pub grid: f64,
    /// Where the clip is on the arrangement, to find the playhead in its content.
    pub placement: Placement,
    /// The grid value as the toolbar's grid menu shows it and ticks it, and whether things snap.
    pub grid_menu: PianoGrid,
    /// The grid is the control bar's (Settings › Piano roll grid): the menu offers only what the
    /// control bar can hold.
    pub grid_follows: bool,
    /// The clip's start marker, length and loop, as the ruler shows and changes them.
    pub shape: ClipShape,
    /// The notes of other clips drawn faintly behind this clip's own, and the clips the ghost
    /// menu offers (› Ghost notes); the window fills them in.
    pub ghosts: Ghosts,
    /// The scale of the clip's content, whose keys the rows mark (› Scale and key).
    pub scale: Option<Scale>,
}

/// Where a clip plays its content on the arrangement.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Placement {
    /// The arrangement beat it starts at, and how long it is.
    pub start: f64,
    pub length: f64,
    /// Where in the content it starts playing, and the part it loops (start, end), if any.
    pub offset: f64,
    pub loop_range: Option<(f64, f64)>,
}

impl Placement {
    /// The content beat played at arrangement beat `at`; `None` outside the clip.
    pub fn content_beat(&self, at: f64) -> Option<f64> {
        let elapsed = at - self.start;
        if !(elapsed >= 0.0 && elapsed < self.length) {
            return None;
        }
        let mut beat = self.offset + elapsed;
        if let Some((start, end)) = self.loop_range
            && end > start
            && beat >= end
        {
            beat = start + (beat - start) % (end - start);
        }
        beat.is_finite().then_some(beat.max(0.0))
    }

    /// The arrangement beat that plays content beat `beat`; `None` when the clip never plays it.
    /// The inverse of [`Placement::content_beat`], used to play from the point under the pointer.
    pub fn arrangement_beat(&self, beat: f64) -> Option<f64> {
        if !(beat.is_finite() && self.start.is_finite() && self.length.is_finite() && self.offset.is_finite()) {
            return None;
        }
        let mut content = beat.max(0.0);
        if let Some((start, end)) = self.loop_range
            && end > start
            && content >= end
        {
            content = start + (content - start) % (end - start);
        }
        let elapsed = content - self.offset;
        // A beat before the clip's offset is not played by it; one past its end lands on the end.
        if elapsed < 0.0 {
            return None;
        }
        let at = self.start + elapsed;
        (at < self.start + self.length).then_some(at.max(self.start))
    }
}

/// The piano roll for the arrangement clip `id` on `track`, with the grid `quantization`;
/// `None` when the clip is missing or plays audio.
pub fn arrangement_clip(
    project: &Project,
    track: TrackId,
    id: ClipId,
    quantization: Quantization,
) -> Option<PianoClip> {
    let lane = project.arrangement.lanes.iter().find(|l| l.track == Some(track))?;
    let clip = lane.clips.iter().find(|c| c.id == id)?;
    let mut piano = piano_clip_from(project, track, clip, quantization)?;
    // The note commands name the clip by its id; the track is there for what reads it.
    piano.address = json!({ "track": track.0, "clip": id.0 });
    // Up to the next clip on the lane, as `clip.setLength` allows.
    let next =
        lane.clips.iter().filter(|c| c.id != id && c.start > clip.start).map(|c| c.start).fold(f64::INFINITY, f64::min);
    piano.shape.room = next.is_finite().then_some(next - clip.start);
    Some(piano)
}

/// The piano roll for a launcher slot's clip: `scene` is 0-based, as the session's selection
/// carries it; the `note.*` commands name the clip by `scene` (1-based). A slot's clip plays
/// from its launch, not from an arrangement position: its placement starts at the launch and
/// never ends, so the beats since the launch map through it — the playhead and paste at the
/// playhead take those beats.
pub fn launcher_clip(project: &Project, track: TrackId, scene: usize, quantization: Quantization) -> Option<PianoClip> {
    let clip = project.launcher.columns.iter().find(|c| c.track == track)?.slots.get(scene)?.clip.as_ref()?;
    let mut piano = piano_clip_from(project, track, clip, quantization)?;
    piano.placement = Placement {
        start: 0.0,
        length: f64::INFINITY,
        offset: clip.offset,
        loop_range: clip.loop_range.map(|l| (l.start, l.end)),
    };
    piano.address = json!({ "track": track.0, "scene": scene + 1 });
    Some(piano)
}

/// What `arrangement_clip` and `launcher_clip` share: the content's notes and shape as the
/// piano roll shows them.
fn piano_clip_from(project: &Project, track: TrackId, clip: &Clip, quantization: Quantization) -> Option<PianoClip> {
    let content = project.content(clip.content)?;
    let mut notes = None;
    for lane in content.lanes.iter().filter(|l| l.target.is_none_or(|t| t == track)) {
        match &lane.data {
            LaneData::Audio(_) => return None,
            LaneData::Notes(n) if notes.is_none() => notes = Some(n.clone()),
            LaneData::Notes(_) => {}
        }
    }
    if content.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_))) {
        return None;
    }
    let position = project.tracks.iter().position(|t| t.id == track)?;
    let color = project.tracks.get(position).and_then(|t| t.color).unwrap_or_else(|| default_track_colour(position));
    let ts = project.transport.time_signature;
    let bar_beats =
        if ts.denominator == 0 { 4.0 } else { f64::from(ts.numerator.max(1)) * 4.0 / f64::from(ts.denominator) };
    Some(PianoClip {
        address: json!({ "track": track.0 }),
        notes: notes.unwrap_or_default(),
        length: content_end(clip.loop_range.map(|l| l.end), clip.offset, clip.length, content.length),
        color: Color32::from_rgb(color.0, color.1, color.2),
        bar_beats,
        grid: grid_step(quantization, bar_beats),
        placement: Placement {
            start: clip.start,
            length: clip.length,
            offset: clip.offset,
            loop_range: clip.loop_range.map(|l| (l.start, l.end)),
        },
        grid_menu: PianoGrid::default(),
        grid_follows: false,
        shape: ClipShape {
            offset: clip.offset,
            length: clip.length,
            loop_range: clip.loop_range.map(|l| (l.start, l.end)),
            room: None,
        },
        ghosts: Ghosts::default(),
        scale: content.scale,
    })
}

/// Where the part of the content a clip plays ends, in content beats: its loop's end when it
/// loops, else its offset and length; at least a beat.
fn content_end(loop_end: Option<f64>, offset: f64, length: f64, content_length: f64) -> f64 {
    let end = loop_end.unwrap_or(offset + length);
    let end = if end.is_finite() && end > 0.0 { end } else { content_length };
    if end.is_finite() { end.clamp(1.0, f64::from(BEAT_LIMIT)) } else { 4.0 }
}

/// The furthest the piano roll reaches, in beats: the engine's furthest start.
const BEAT_LIMIT: f32 = 120_000.0;

/// The grid step in beats for the control bar's value; 0 for none.
pub fn grid_step(quantization: Quantization, bar_beats: f64) -> f64 {
    let step = match quantization {
        Quantization::None => 0.0,
        Quantization::Bars(bars) => f64::from(bars) * bar_beats,
        Quantization::Beats(beats) => beats,
    };
    if step.is_finite() && step > 0.0 { step } else { 0.0 }
}

/// The grids quantise offers, as `note.quantize` takes them.
pub const QUANTIZE_GRIDS: [&str; 13] =
    ["1 bar", "1/2", "1/2T", "1/4", "1/4T", "1/8", "1/8T", "1/16", "1/16T", "1/32", "2 bars", "4 bars", "8 bars"];

/// What Ctrl+U and the Quantise panel quantise with; kept until changed, for the whole session.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuantizeSettings {
    /// One of [`QUANTIZE_GRIDS`].
    pub grid: &'static str,
    /// How far towards the grid, 0 to 100 percent.
    pub amount: f64,
    /// Whether the notes' ends move to the grid too.
    pub ends: bool,
}

impl Default for QuantizeSettings {
    fn default() -> Self {
        Self { grid: "1/16", amount: 100.0, ends: false }
    }
}

/// What a press in the note grid does: the toolbar's tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    /// Selects, moves and resizes notes; a double-click adds or deletes one.
    #[default]
    Select,
    /// A click adds a note; dragging sideways while holding sets its length. With Shift held,
    /// dragging draws notes back to back, each the last note's length.
    Draw,
    /// Dragging paints notes one note length apart, each in the row under the pointer.
    Paint,
    /// A click deletes a note; dragging deletes every note the pointer passes over.
    Erase,
    /// Dragging draws a cut line; the notes it crosses are split where it crosses them.
    Slice,
    /// A click mutes a note, or unmutes it; a drag sets every note it passes over to the state the
    /// first note the stroke touched takes (› Muting in `docs/spec/ui-clip-editor.md`).
    Mute,
}

impl Tool {
    pub const ALL: [(Self, &'static str); 6] = [
        (Self::Select, "Select"),
        (Self::Draw, "Draw"),
        (Self::Paint, "Paint"),
        (Self::Erase, "Erase"),
        (Self::Slice, "Slice"),
        (Self::Mute, "Mute"),
    ];
}

/// A drag in progress, from where it started.
#[derive(Debug, Clone, PartialEq)]
enum Gesture {
    /// The selection follows the grabbed note; with `copy` (Ctrl held when it began) a copy of
    /// it does, and the notes stay.
    Move { from: Pos2, grabbed: usize, copy: bool },
    /// The selection's lengths follow the grabbed note's right end.
    Resize { from: Pos2, grabbed: usize },
    /// The selection's starts follow the grabbed note's left edge; their ends stay.
    ResizeStart { from: Pos2, grabbed: usize },
    /// Alt and a drag on a note: the selection's velocities follow the pointer up and down.
    NoteVelocity { from: Pos2, grabbed: usize },
    /// A line drawn in the velocity lane: each stem it spans takes its height.
    VelocityLine { from: Pos2 },
    /// A rectangle selects the notes it touches; `add` keeps the selection.
    Select { from: Pos2, add: bool },
    /// The selection's velocities follow the grabbed stem.
    Velocity { from: Pos2, grabbed: usize },
    /// A note drawn where the press was, as long as the drag reaches.
    Draw { from: Pos2 },
    /// Notes painted along the drag ([`PianoRollState::stroke`]).
    Paint,
    /// Notes the drag passes over are deleted ([`PianoRollState::erasing`]).
    Erase,
    /// A cut line from where the press was; the notes it crosses are split when it is let go.
    Slice { from: Pos2 },
    /// A mute stroke: every note it passes over is set to [`PianoRollState::mute_target`], which
    /// is the state the first note the stroke touched takes, so a stroke that starts on a muted
    /// note unmutes the run.
    Mute,
    /// Shift and Draw: notes back to back along the row the drag started in
    /// ([`Gestures::draw_run`]).
    DrawRun,
    /// The selection follows the keys the drag passes over in the key column; `add` keeps the
    /// rest of the selection, as it was when the click began.
    KeySelect { add: bool },
}

/// What the piano roll keeps between frames.
#[derive(Debug, Clone, PartialEq)]
pub struct PianoRollState {
    /// The clip shown last, as its address: another clip resets the view.
    shown: Option<Value>,
    /// Points scrolled from beat 0 and from the top of the highest key.
    pub scroll: Vec2,
    pub beat_width: f32,
    pub key_height: f32,
    /// Indices of the selected notes.
    pub selected: BTreeSet<usize>,
    /// The last press was in the piano roll, so Delete deletes notes there.
    pub focused: bool,
    /// A key this frame selected notes its command is still to add (paste, duplicate): they
    /// are kept selected until the command has run.
    selecting_new: bool,
    gesture: Option<Gesture>,
    panning: bool,
    /// The right button and the wheel are choosing tools: the wheel scrolls nothing until the
    /// button is up and the wheel's smoothing has run out.
    wheel_tools: bool,
    /// The settings quantise uses, and whether the quantise block in the MIDI tools column is open.
    pub quantize: QuantizeSettings,
    pub quantize_open: bool,
    /// The MIDI tools column's own settings, kept until Suhina closes, and whether the column is
    /// shown at all (the toolbar's three-slider button toggles it; window state, not a command).
    pub tools: ToolSettings,
    pub tools_open: bool,
    /// The toolbar's tool.
    pub tool: Tool,
    /// Notes moved onto others of the same key and start replace them (Settings; the window sets
    /// it each frame).
    pub notes_replace: bool,
    /// The length (0: one grid step) and velocity a new note takes: those of the note last
    /// drawn or clicked.
    last_length: f64,
    last_velocity: u8,
    /// The notes a paint stroke has painted so far, and the notes an erase stroke has passed over.
    stroke: Vec<Note>,
    erasing: BTreeSet<usize>,
    /// The notes a mute stroke has passed over; they are drawn faded until the stroke ends, and
    /// `mute_target` is the state the stroke gives them.
    muting: BTreeSet<usize>,
    mute_target: bool,
    /// The right button is playing the clip from the point under the pointer (the Mouse-first
    /// switch): letting it go stops the transport.
    right_playing: bool,
    /// The wheel stepped the tools while the right button was held: letting it go over a note
    /// then deletes nothing (› Zoom and scroll).
    right_stepped: bool,
    /// A grid value picked in the toolbar's grid menu or with its keys this frame, for the window
    /// to keep.
    pub grid_picked: Option<PianoGrid>,
    /// The toolbar's Hide editor button was clicked this frame: the window hides the editor.
    pub hide_requested: bool,
    /// The key rows were scrolled by hand (the wheel, a pan, zooming the key height) since the
    /// clip was shown: until then a change in the editor's height centres the notes again.
    scrolled_by_hand: bool,
    /// The grid's height the notes were last centred for.
    centred_for: f32,
    /// The key the key column is playing, if any, and where the pointer last was on it: the
    /// window turns a change into `preview.noteOn` and `preview.noteOff`.
    playing_key: Option<u8>,
    hover_key: Option<u8>,
    /// When and where the last click in the grid was, while a second click could still make it a
    /// double-click; the piano roll counts its own clicks (› Double-click).
    last_click: Option<(f64, Pos2)>,
    /// The index the note the last click added will have, so the second click of a double-click
    /// selects that note rather than deleting it.
    click_added: Option<usize>,
    /// A workflow mode or setting picked in the toolbar's menu this frame, for the window to keep
    /// in the keymap.
    pub workflow_picked: Option<crate::settings::workflow::Pick>,
    /// A drag on the ruler in progress.
    pub(crate) ruler: RulerState,
    /// How wide the note grid was last drawn, for the zoom keys (Z, Shift+Z).
    view_width: f32,
    /// The velocity lane's height dragged by its top edge, kept until Suhina closes; `None`: the
    /// default height (› Layout).
    lane_wanted: Option<f32>,
    /// The toolbar's Audition button: drawn and moved notes sound once.
    pub audition: bool,
    /// Keys to sound at the end of this frame, and the track and time the sounding ones go off.
    audition_pending: Vec<u8>,
    audition_off: Option<(u64, f64)>,
    audition_sounding: Vec<u8>,
    /// Which other clips show as ghost notes, picked in the toolbar's ghost menu; kept until
    /// Suhina closes (› Ghost notes).
    pub ghost_source: GhostSource,
    /// Which key rows show, picked in the toolbar's Scale menu (Fold, Fold to scale); kept until
    /// Suhina closes (› Zoom and scroll).
    pub fold: Fold,
}

impl Default for PianoRollState {
    fn default() -> Self {
        Self {
            shown: None,
            scroll: Vec2::ZERO,
            beat_width: 40.0,
            key_height: DEFAULT_KEY_HEIGHT,
            selected: BTreeSet::new(),
            focused: false,
            selecting_new: false,
            gesture: None,
            panning: false,
            wheel_tools: false,
            quantize: QuantizeSettings::default(),
            quantize_open: false,
            tools: ToolSettings::default(),
            tools_open: true,
            tool: Tool::Select,
            notes_replace: true,
            last_length: 0.0,
            last_velocity: NEW_VELOCITY,
            stroke: Vec::new(),
            erasing: BTreeSet::new(),
            muting: BTreeSet::new(),
            mute_target: true,
            right_playing: false,
            right_stepped: false,
            grid_picked: None,
            hide_requested: false,
            scrolled_by_hand: false,
            centred_for: 0.0,
            playing_key: None,
            hover_key: None,
            last_click: None,
            click_added: None,
            workflow_picked: None,
            ruler: RulerState::default(),
            view_width: 0.0,
            lane_wanted: None,
            audition: false,
            audition_pending: Vec::new(),
            audition_off: None,
            audition_sounding: Vec::new(),
            ghost_source: GhostSource::default(),
            fold: Fold::default(),
        }
    }
}

impl PianoRollState {
    /// What the toolbar's Cut, Copy, Paste and Duplicate buttons do: the same as Ctrl+X, Ctrl+C,
    /// Ctrl+V and the duplicate key, on the selected notes (› Copy, cut, paste and duplicate).
    /// `None` when there is nothing to act on: no note selected, or nothing copied for Paste.
    fn clipboard_action(&mut self, button: IconButton, clip: &PianoClip, copied: Option<usize>) -> Option<Action> {
        let action = match button {
            IconButton::Cut if !self.selected.is_empty() => "selection.cut",
            IconButton::Copy if !self.selected.is_empty() => "selection.copy",
            IconButton::Paste if copied.is_some_and(|n| n > 0) => "selection.paste",
            IconButton::Duplicate if !self.selected.is_empty() => "selection.duplicate",
            _ => return None,
        };
        // What the keys run: the same command with the same params.
        self.key_action(action, clip, 0.0, copied)
    }

    /// `note.delete` for the selected notes, when the piano roll has the keyboard and notes are
    /// selected; the selection is then cleared.
    pub fn delete_selected(&mut self) -> Option<Action> {
        if !self.focused || self.selected.is_empty() {
            return None;
        }
        let mut params = self.shown.clone()?;
        params["notes"] = json!(self.selected.iter().collect::<Vec<_>>());
        self.selected.clear();
        Some(Action::new("note.delete", params))
    }

    /// `note.quantize` for the selected notes, or every note when none is selected, with the
    /// last settings; `None` when no clip is shown.
    pub fn quantize_selected(&self) -> Option<Action> {
        let mut params = self.shown.clone()?;
        if !self.selected.is_empty() {
            params["notes"] = json!(self.selected.iter().collect::<Vec<_>>());
        }
        params["grid"] = json!(self.quantize.grid);
        params["amount"] = json!(self.quantize.amount);
        params["ends"] = json!(self.quantize.ends);
        Some(Action::new("note.quantize", params))
    }

    /// What a key does to the piano roll showing `clip`, when it has the keyboard: the selection
    /// actions change the selection here, the others become a `note.*` command. `playhead` is the
    /// arrangement's playhead in beats, `copied` how many notes `note.paste` would add. `None`
    /// when the key does nothing here, so copy, cut, paste and duplicate act on the clip instead.
    pub fn key_action(
        &mut self,
        action: &str,
        clip: &PianoClip,
        playhead: f64,
        copied: Option<usize>,
    ) -> Option<Action> {
        if !self.focused || self.shown.as_ref() != Some(&clip.address) {
            return None;
        }
        self.selected.retain(|i| *i < clip.notes.len());
        let chosen: Vec<Note> = self.selected.iter().filter_map(|i| clip.notes.get(*i).copied()).collect();
        let count = clip.notes.len();
        let mut params = clip.address.clone();
        if action == "selection.paste" {
            // After the selection, or at the playhead when it is inside the clip; else where the
            // command puts it (right after the copied notes).
            let copied = copied?;
            let end = chosen.iter().map(|n| n.time + n.duration).fold(f64::NEG_INFINITY, f64::max);
            let time = if end.is_finite() { Some(end) } else { clip.placement.content_beat(playhead) };
            if let Some(time) = time {
                params["time"] = json!(round_beat(time));
            }
            self.selected = (count..count.saturating_add(copied)).collect();
            self.selecting_new = true;
            return Some(Action::new("note.paste", params));
        }
        match action {
            "pianoRoll.selectAll" => {
                self.selected = (0..count).collect();
                return None;
            }
            "pianoRoll.deselect" => {
                self.selected.clear();
                return None;
            }
            "pianoRoll.draw" => {
                self.tool = if self.tool == Tool::Draw { Tool::Select } else { Tool::Draw };
                return None;
            }
            "pianoRoll.tool1" | "pianoRoll.tool2" | "pianoRoll.tool3" | "pianoRoll.tool4" | "pianoRoll.tool5"
            | "pianoRoll.tool6" => {
                if let Some(tool) = tool_of(action) {
                    self.tool = tool;
                }
                return None;
            }
            "pianoRoll.gridNarrow" | "pianoRoll.gridWiden" | "pianoRoll.gridTriplet" | "pianoRoll.gridSnap" => {
                if let Some(picked) = grid_key(action, clip) {
                    self.grid_picked = Some(picked);
                }
                return None;
            }
            // Z: the selected notes fill the width (the whole clip when none is selected); Shift+Z:
            // the whole clip does. Window state, not a command.
            "pianoRoll.zoomSelection" => {
                let start = chosen.iter().map(|n| n.time).fold(f64::INFINITY, f64::min);
                let end = chosen.iter().map(|n| n.time + n.duration).fold(f64::NEG_INFINITY, f64::max);
                if start.is_finite() && end > start {
                    self.zoom_to(start, end);
                } else {
                    self.zoom_to(0.0, clip.length);
                }
                return None;
            }
            "pianoRoll.zoomClip" => {
                self.zoom_to(0.0, clip.length);
                return None;
            }
            _ if chosen.is_empty() => return None,
            _ => {}
        }
        params["notes"] = json!(self.selected.iter().collect::<Vec<_>>());
        let grid = note_grid(clip, self.beat_width);
        let moves = |beats: f64, keys: i32| {
            let (beats, moved) = move_delta(beats, keys, &chosen, grid, false);
            // A pitch move that does not fit leaves the pitch alone, so no note changes its name.
            (beats, if moved == keys { keys } else { 0 })
        };
        let (id, beats, keys) = match action {
            "selection.copy" => return Some(Action::new("note.copy", params)),
            "selection.cut" => {
                self.selected.clear();
                return Some(Action::new("note.cut", params));
            }
            "selection.duplicate" => {
                self.selected = (count..count.saturating_add(chosen.len())).collect();
                self.selecting_new = true;
                return Some(Action::new("note.duplicate", params));
            }
            "pianoRoll.shorter" | "pianoRoll.longer" => {
                // By a step; the keys never make a note shorter than one step.
                let shortest = chosen.iter().map(|n| n.duration).fold(f64::INFINITY, f64::min);
                let by = if action == "pianoRoll.longer" {
                    grid.step
                } else {
                    (-grid.step).max((grid.step - shortest).min(0.0))
                };
                if by == 0.0 || !by.is_finite() {
                    return None;
                }
                params["by"] = json!(round_beat(by));
                return Some(Action::new("note.resize", params));
            }
            "pianoRoll.earlier" => ("note.move", moves(-grid.step, 0).0, 0),
            "pianoRoll.later" => ("note.move", moves(grid.step, 0).0, 0),
            "pianoRoll.up" => ("note.move", 0.0, moves(0.0, 1).1),
            "pianoRoll.down" => ("note.move", 0.0, moves(0.0, -1).1),
            "pianoRoll.octaveUp" => ("note.move", 0.0, moves(0.0, 12).1),
            "pianoRoll.octaveDown" => ("note.move", 0.0, moves(0.0, -12).1),
            _ => return None,
        };
        if beats == 0.0 && keys == 0 {
            return None;
        }
        self.audition_moved(&chosen, keys);
        params["beats"] = json!(round_beat(beats));
        params["semitones"] = json!(keys);
        Some(Action::new(id, params))
    }

    /// Zooms and scrolls time so beats `start` to `end` fill the note grid's width (as far as the
    /// zoom range allows), keys untouched.
    fn zoom_to(&mut self, start: f64, end: f64) {
        let width = self.view_width.max(1.0);
        let span = (end - start).max(EPSILON) as f32;
        self.beat_width = (width / span).clamp(BEAT_WIDTH_RANGE.0, BEAT_WIDTH_RANGE.1);
        self.scroll.x = (start.max(0.0) as f32 * self.beat_width).max(0.0);
    }

    /// Forgets the clip, so the next one shown is fitted to the view again.
    pub fn hide(&mut self) {
        self.shown = None;
        self.selected.clear();
        self.gesture = None;
        self.focused = false;
        self.quantize_open = false;
        self.selecting_new = false;
        self.stroke.clear();
        self.erasing.clear();
        self.audition_pending.clear();
    }

    /// Keeps `note`'s length and velocity for the next note drawn, and for the key column's
    /// preview.
    fn remember(&mut self, note: &Note) {
        if note.duration.is_finite() && note.duration > 0.0 {
            self.last_length = note.duration;
        }
        self.last_velocity = suhina_session::midi_velocity(note.velocity);
    }

    /// Presses the key column's `key`: `preview.noteOn` through the clip's track. The key that
    /// was sounding before goes off in the same call. `None` when that key already sounds.
    pub fn key_press(&mut self, clip: &PianoClip, key: u8) -> Option<Action> {
        if self.playing_key == Some(key) {
            return None;
        }
        let track = clip.address.get("track").and_then(Value::as_u64)?;
        self.playing_key = Some(key);
        Some(Action::new("preview.noteOn", json!({ "track": track, "key": key, "velocity": self.last_velocity })))
    }

    /// Lets go of the key the key column is playing, `preview.noteOff`; `None` when none sounds.
    pub fn key_release(&mut self, clip: &PianoClip) -> Option<Action> {
        let key = self.playing_key.take()?;
        let track = clip.address.get("track").and_then(Value::as_u64)?;
        Some(Action::new("preview.noteOff", json!({ "track": track, "key": key })))
    }

    /// Every note of the keys in `pitches` is selected; `add` keeps the rest of the selection,
    /// else it is replaced. Ctrl+click and Ctrl+drag on the key column use it.
    pub fn select_pitches(&mut self, clip: &PianoClip, pitches: &[u8], add: bool) {
        if !add {
            self.selected.clear();
        }
        for (index, note) in clip.notes.iter().enumerate() {
            if pitches.contains(&note.key) {
                self.selected.insert(index);
            }
        }
    }

    /// The key the pointer is over, drawn lit.
    pub fn hover_key(&self) -> Option<u8> {
        self.hover_key
    }

    /// The key that is sounding, drawn lit.
    pub fn playing_key(&self) -> Option<u8> {
        self.playing_key
    }

    /// The length and velocity a new note takes, for the key column's preview.
    pub fn set_preview_velocity(&mut self, midi: u8) {
        self.last_velocity = midi.clamp(1, 127);
    }

    /// The note a press at `pos` would draw: its start on the grid line before the pointer
    /// (`free`: where it is), the row's key, the length and velocity last used.
    fn new_note(&self, layout: &Layout, clip: &PianoClip, grid: NoteGrid, pos: Pos2, free: bool) -> Option<Note> {
        let key = layout.key_at(pos.y)?;
        let time = grid.place(layout.beat_at(pos.x).max(0.0), free, true).max(0.0);
        let length = if self.last_length > 0.0 {
            self.last_length
        } else if grid.snaps && !free {
            chosen_step(clip, self.beat_width).max(MIN_LENGTH)
        } else {
            FREE_LENGTH
        };
        let velocity = f32::from(self.last_velocity) / 127.0;
        Some(Note { time, duration: length, key, velocity, release_velocity: 0.5, channel: 0, muted: false })
    }
}

/// The `note.add` params for `notes`.
fn add_params(clip: &PianoClip, notes: &[Note]) -> Value {
    let mut params = clip.address.clone();
    params["notes"] = notes
        .iter()
        .map(|n| {
            json!({
                "time": round_beat(n.time),
                "length": round_beat(n.duration),
                "key": n.key,
                "velocity": suhina_session::midi_velocity(n.velocity),
            })
        })
        .collect();
    params
}

/// The tool a `pianoRoll.tool*` action chooses, if it is one.
fn tool_of(action: &str) -> Option<Tool> {
    let index = action.strip_prefix("pianoRoll.tool")?;
    let index: usize = index.parse().ok()?;
    index.checked_sub(1).and_then(|i| Tool::ALL.get(i)).map(|(tool, _)| *tool)
}

/// What the window tells the piano roll about the settings that change what it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Options {
    /// How large the toolbar is drawn: Settings › Clip editor › Piano roll › Toolbar size.
    pub toolbar_size: ToolbarSize,
    /// The workflow settings: what clicks, right-clicks and double-clicks do (› Workflow modes in
    /// `docs/spec/ui-clip-editor.md`).
    pub workflow: Workflow,
}

/// What the piano roll needs from the window besides its own state, for one frame: the settings
/// that change what it does, the keys its buttons and tooltips show, and how many notes a paste
/// would add (the session's note clipboard).
pub struct Context<'a> {
    pub options: Options,
    pub keymap: &'a crate::settings::keymap::Keymap,
    pub copied: Option<usize>,
    /// The arrangement beat being played while the transport runs, for the playhead; `None`
    /// while it is stopped.
    pub playhead: Option<f64>,
}

/// The toolbar's buttons: one shape each, drawn in code with our own strokes, as
/// [`crate::browser::browser_view`]'s icons are. `active` marks the note clipboard's buttons while there
/// is something to act on, and the quantise block while it is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IconButton {
    Select,
    Draw,
    Paint,
    Erase,
    Slice,
    Mute,
    Cut,
    Copy,
    Paste,
    Duplicate,
    Quantise,
    MidiTools,
    /// Headphones: drawn and moved notes sound once (› Playing the keys).
    Audition,
    /// The × at the right end: hides the editor.
    Hide,
}

impl IconButton {
    /// What the button is called, for its tooltip and its accessible label.
    fn name(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Draw => "Draw",
            Self::Paint => "Paint",
            Self::Erase => "Erase",
            Self::Slice => "Slice",
            Self::Mute => "Mute",
            Self::Cut => "Cut",
            Self::Copy => "Copy",
            Self::Paste => "Paste",
            Self::Duplicate => "Duplicate",
            Self::Quantise => "Quantise",
            Self::MidiTools => "MIDI tools",
            Self::Audition => "Audition",
            Self::Hide => "Hide editor",
        }
    }

    /// The action id whose key the tooltip shows ("Draw (2)"), when the button has one.
    fn action(self) -> Option<&'static str> {
        Some(match self {
            Self::Select => "pianoRoll.tool1",
            Self::Draw => "pianoRoll.tool2",
            Self::Paint => "pianoRoll.tool3",
            Self::Erase => "pianoRoll.tool4",
            Self::Slice => "pianoRoll.tool5",
            Self::Mute => "pianoRoll.tool6",
            Self::Cut => "selection.cut",
            Self::Copy => "selection.copy",
            Self::Paste => "selection.paste",
            Self::Duplicate => "selection.duplicate",
            Self::Hide => "view.editor",
            Self::Quantise | Self::MidiTools | Self::Audition => return None,
        })
    }

    /// Our own strokes for the shape, in a 14 px box around `rect`'s centre, scaled with the
    /// toolbar (Toolbar size): `scale` multiplies the shape and its strokes together.
    fn draw(self, painter: &egui::Painter, rect: Rect, color: Color32, scale: f32) {
        let scale = if scale.is_finite() { scale.clamp(0.5, 2.5) } else { 1.0 };
        let c = rect.center();
        let stroke = Stroke::new(1.1 * scale, color);
        let at = |x: f32, y: f32| c + Vec2::new(x * scale, y * scale);
        let size = |w: f32, h: f32| Vec2::new(w * scale, h * scale);
        let line = |a: (f32, f32), b: (f32, f32)| painter.line_segment([at(a.0, a.1), at(b.0, b.1)], stroke);
        match self {
            // An arrow pointing up and to the left, its head filled.
            Self::Select => {
                line((-1.5, 6.0), (-1.5, -4.0));
                line((-1.5, -4.0), (-1.5, -6.0));
                painter.add(egui::Shape::convex_polygon(
                    vec![at(-4.0, -5.0), at(5.0, 0.5), at(-4.0, 5.0)],
                    color,
                    Stroke::NONE,
                ));
            }
            // A pencil, lying along the diagonal.
            Self::Draw => {
                line((-6.0, 6.0), (3.5, -3.5));
                line((3.5, -3.5), (6.0, -1.0));
                line((6.0, -1.0), (-3.5, 8.5));
                line((-3.5, 8.5), (-6.0, 6.0));
                line((-6.0, 6.0), (-3.0, 6.5));
                line((-3.0, 6.5), (-3.5, 8.5));
            }
            // A brush: a short handle and a wide head.
            Self::Paint => {
                line((-6.5, 6.5), (-2.5, 2.5));
                line((-2.5, 2.5), (1.5, 6.5));
                line((1.5, 6.5), (6.5, 1.5));
                line((6.5, 1.5), (2.5, -2.5));
                line((2.5, -2.5), (-2.5, 2.5));
                line((2.5, -2.5), (6.0, -6.0));
            }
            // An eraser, tilted, with its rubbing end lighter.
            Self::Erase => {
                line((-6.0, 2.0), (2.0, -6.0));
                line((2.0, -6.0), (6.0, -2.0));
                line((6.0, -2.0), (-2.0, 6.0));
                line((-2.0, 6.0), (-6.0, 2.0));
                painter.add(egui::Shape::convex_polygon(
                    vec![at(-6.0, 2.0), at(-2.0, 6.0), at(-0.5, 4.5), at(-4.5, 0.5)],
                    color.gamma_multiply(0.35),
                    Stroke::NONE,
                ));
                line((-1.0, 3.0), (3.0, 7.0));
            }
            // A blade: a handle with a straight edge under it.
            Self::Slice => {
                line((-5.5, -5.5), (1.5, 1.5));
                line((1.5, 1.5), (5.5, 1.5));
                line((5.5, 1.5), (5.5, 5.5));
                line((5.5, 5.5), (-5.5, 5.5));
                line((-5.5, 5.5), (-5.5, 1.5));
            }
            // A speaker with a cross beside it: mute.
            Self::Mute => {
                painter.add(egui::Shape::convex_polygon(
                    vec![at(-6.5, -2.0), at(-3.0, -2.0), at(0.5, -6.0), at(0.5, 6.0), at(-3.0, 2.0), at(-6.5, 2.0)],
                    color.gamma_multiply(0.45),
                    stroke,
                ));
                line((3.0, -3.5), (6.5, 0.0));
                line((3.0, 3.5), (6.5, 0.0));
                line((3.0, 0.0), (6.5, -3.5));
                line((3.0, 0.0), (6.5, 3.5));
            }
            // A sheet with a tab: copy.
            Self::Copy => {
                let page = Rect::from_center_size(at(0.5, 1.0), size(9.0, 11.0));
                painter.rect_stroke(page, 0.0, stroke, StrokeKind::Inside);
                painter.line_segment([at(-2.5, -6.0), at(-2.5, -4.5)], stroke);
                painter.line_segment([at(-2.5, -6.0), at(3.5, -6.0)], stroke);
                painter.line_segment([at(3.5, -6.0), at(3.5, -4.5)], stroke);
            }
            // Two sheets, one behind the other: duplicate.
            Self::Duplicate => {
                painter.rect_stroke(
                    Rect::from_center_size(at(-1.5, -1.5), size(8.0, 8.0)),
                    0.0,
                    stroke,
                    StrokeKind::Inside,
                );
                let front = Rect::from_center_size(at(1.5, 1.5), size(8.0, 8.0));
                painter.rect_filled(front, 0.0, theme::panel_background());
                painter.rect_stroke(front, 0.0, stroke, StrokeKind::Inside);
            }
            // Scissors: two rings and the blades crossing over them.
            Self::Cut => {
                painter.circle_stroke(at(-2.0, 4.0), 2.0 * scale, stroke);
                painter.circle_stroke(at(2.0, 4.0), 2.0 * scale, stroke);
                line((-3.0, 2.5), (4.0, -5.0));
                line((3.0, 2.5), (-4.0, -5.0));
            }
            // A clipboard with a page being laid onto it: paste.
            Self::Paste => {
                let board = Rect::from_center_size(at(0.0, 1.0), size(10.0, 12.0));
                painter.rect_stroke(board, 0.0, stroke, StrokeKind::Inside);
                painter.rect_filled(Rect::from_center_size(at(0.0, -5.5), size(5.0, 2.5)), 0.0, color);
                line((-3.0, 0.0), (3.0, 0.0));
                line((-3.0, 3.0), (3.0, 3.0));
            }
            // Notes snapping onto a line: the quantise button.
            Self::Quantise => {
                line((-6.0, 5.5), (6.0, 5.5));
                for (x, y) in [(-3.5, -1.0), (0.0, -4.0), (3.5, -1.5)] {
                    painter.rect_filled(Rect::from_center_size(at(x, y), size(4.0, 3.0)), 0.0, color);
                    line((x, y + 1.5), (x, 5.5));
                }
            }
            // A cross: hides the editor.
            Self::Hide => {
                line((-4.0, -4.0), (4.0, 4.0));
                line((-4.0, 4.0), (4.0, -4.0));
            }
            // Headphones: a band over the head and a cup at each end.
            Self::Audition => {
                let band: Vec<Pos2> = (0..=12)
                    .map(|i| {
                        let angle = std::f32::consts::PI * (1.0 + i as f32 / 12.0);
                        at(5.0 * angle.cos(), 1.5 + 5.5 * angle.sin())
                    })
                    .collect();
                painter.add(egui::Shape::line(band, stroke));
                painter.rect_filled(Rect::from_center_size(at(-5.0, 3.5), size(3.0, 5.0)), 0.0, color);
                painter.rect_filled(Rect::from_center_size(at(5.0, 3.5), size(3.0, 5.0)), 0.0, color);
            }
            // Three horizontal sliders: the MIDI tools column's toggle.
            Self::MidiTools => {
                for (y, knob) in [(-4.0, -2.0), (0.0, 3.0), (4.0, -1.0)] {
                    line((-6.0, y), (6.0, y));
                    painter.rect_filled(Rect::from_center_size(at(knob, y), size(3.0, 5.0)), 0.0, color);
                }
            }
        }
    }
}

/// Where things are this frame.
#[derive(Debug, Clone, Copy)]
struct Layout {
    keys: Rect,
    ruler: Rect,
    grid: Rect,
    lane: Rect,
    scroll: Vec2,
    beat_width: f32,
    key_height: f32,
    /// The key rows shown, from the top: every key, or the keys a fold leaves (› Scale and key).
    rows: KeyRows,
}

impl Layout {
    fn x_of(&self, beat: f64) -> f32 {
        self.grid.left() - self.scroll.x + beat as f32 * self.beat_width
    }

    fn beat_at(&self, x: f32) -> f64 {
        f64::from((x - self.grid.left() + self.scroll.x) / self.beat_width)
    }

    /// The top of `key`'s row; far above the grid for a key the fold hides, so nothing on it is
    /// drawn or hit.
    fn y_of(&self, key: u8) -> f32 {
        match self.rows.row_of(key) {
            Some(row) => self.grid.top() - self.scroll.y + row as f32 * self.key_height,
            None => self.grid.top() - 1.0e6,
        }
    }

    /// The key whose row is at `y`, if any.
    fn key_at(&self, y: f32) -> Option<u8> {
        let row = ((y - self.grid.top() + self.scroll.y) / self.key_height).floor();
        if !(row >= 0.0 && row < self.rows.len() as f32) {
            return None;
        }
        self.rows.key_of(row as usize)
    }

    fn note_rect(&self, note: &Note) -> Rect {
        let left = self.x_of(note.time);
        let right = self.x_of(note.time + note.duration.max(0.0)).max(left + 2.0);
        let top = self.y_of(note.key);
        Rect::from_min_max(Pos2::new(left, top + 1.0), Pos2::new(right - 1.0, top + self.key_height))
    }
}

/// The grid step chosen for `clip` at `beat_width` points per beat, before it is made coarser to
/// keep lines apart: the fixed value, or the adaptive grid's step at this zoom.
fn chosen_step(clip: &PianoClip, beat_width: f32) -> f64 {
    if clip.grid_menu.is_adaptive() { clip.grid_menu.step(beat_width, clip.bar_beats) } else { clip.grid }
}

/// The grid `clip` is edited on at `beat_width`: snapping only while the menu's Snap to grid is on.
pub(crate) fn note_grid(clip: &PianoClip, beat_width: f32) -> NoteGrid {
    let grid = NoteGrid::new(chosen_step(clip, beat_width), beat_width);
    NoteGrid { snaps: grid.snaps && clip.grid_menu.snap, ..grid }
}

/// The grid a grid key (Ctrl+1 to Ctrl+4 by default) makes of `clip`'s; `None` when the grid
/// follows the control bar and it has no such value.
fn grid_key(action: &str, clip: &PianoClip) -> Option<PianoGrid> {
    let grid = clip.grid_menu;
    let picked = match action {
        "pianoRoll.gridNarrow" => grid.narrower(),
        "pianoRoll.gridWiden" => grid.wider(),
        "pianoRoll.gridTriplet" => grid.toggle_triplet(),
        "pianoRoll.gridSnap" => grid.toggle_snap(),
        _ => return None,
    };
    (!clip.grid_follows || picked.control_bar_value().is_some()).then_some(picked)
}

/// The grid as drawn at this zoom: its step (the chosen one, doubled until lines are far
/// enough apart) and whether things snap to it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteGrid {
    pub step: f64,
    pub snaps: bool,
}

impl NoteGrid {
    pub fn new(chosen: f64, beat_width: f32) -> Self {
        let snaps = chosen.is_finite() && chosen > 0.0;
        let mut step = if snaps { chosen } else { 1.0 };
        let points = f64::from(beat_width.max(f32::MIN_POSITIVE));
        while step * points < f64::from(MIN_LINE_SPACING) && step < 1e6 {
            step *= 2.0;
        }
        Self { step, snaps }
    }

    /// The grid line nearest to `beat`: what a slice snaps its cut to.
    pub fn nearest_line(&self, beat: f64) -> f64 {
        if !beat.is_finite() {
            return 0.0;
        }
        let snapped = self.place(beat.max(0.0), false, false);
        snapped.clamp(0.0, f64::from(BEAT_LIMIT))
    }

    /// `beat` on the nearest line (or the line before it with `down`), unless `free` or there is
    /// no snapping.
    pub(crate) fn place(&self, beat: f64, free: bool, down: bool) -> f64 {
        if free || !self.snaps {
            return beat;
        }
        let lines = beat / self.step;
        (if down { (lines + EPSILON).floor() } else { lines.round() }) * self.step
    }
}

/// How far a drag of `raw` beats and `raw_keys` semitones moves the selection: by whole grid
/// steps unless `free`, and never past the content's start or the keys 0 to 127.
pub fn move_delta(raw: f64, raw_keys: i32, moved: &[Note], grid: NoteGrid, free: bool) -> (f64, i32) {
    let mut beats = if free || !grid.snaps { raw } else { (raw / grid.step).round() * grid.step };
    let earliest = moved.iter().map(|n| n.time).fold(f64::INFINITY, f64::min);
    if earliest.is_finite() {
        beats = beats.max(-earliest);
    }
    let lowest = moved.iter().map(|n| i32::from(n.key)).min().unwrap_or(0);
    let highest = moved.iter().map(|n| i32::from(n.key)).max().unwrap_or(127);
    let keys = raw_keys.clamp(-lowest, 127 - highest);
    (if beats.is_finite() { beats } else { 0.0 }, keys)
}

/// How much a drag of the grabbed note's right end by `raw` beats changes the selection's
/// lengths: the end on the grid unless `free`, and no note shorter than [`MIN_LENGTH`].
pub fn resize_by(raw: f64, grabbed: &Note, resized: &[Note], grid: NoteGrid, free: bool) -> f64 {
    let end = grabbed.time + grabbed.duration;
    let mut new_end = grid.place(end + raw, free, false);
    if grid.snaps && !free && new_end <= grabbed.time + EPSILON {
        // Snapped onto or before its start: the end goes to the first line after the start.
        new_end = ((grabbed.time / grid.step + EPSILON).floor() + 1.0) * grid.step;
    }
    let shortest = resized.iter().map(|n| n.duration).fold(f64::INFINITY, f64::min);
    let by = new_end - end;
    let by = if shortest.is_finite() { by.max(MIN_LENGTH - shortest) } else { by };
    if by.is_finite() { by } else { 0.0 }
}

/// The note at `pos`, the topmost (latest) first, and which part of it `pos` is on.
fn note_at(layout: &Layout, notes: &[Note], pos: Pos2) -> Option<(usize, Part)> {
    notes.iter().enumerate().rev().find_map(|(i, n)| {
        let rect = layout.note_rect(n).expand2(Vec2::new(0.0, 0.5));
        rect.contains(pos).then(|| (i, Part::of(rect, pos.x)))
    })
}

/// The velocity bar under `pos`: a note's bar can be grabbed anywhere over the note's width in
/// the lane (and a little before its start, where the stem is drawn), the nearest start first,
/// a selected note before the others.
fn stem_at(layout: &Layout, notes: &[Note], selected: &BTreeSet<usize>, pos: Pos2) -> Option<usize> {
    let near = |(i, n): &(usize, &Note)| {
        let start = layout.x_of(n.time);
        let end = layout.x_of(n.time + n.duration.max(0.0)).max(start + STEM_REACH);
        (pos.x >= start - STEM_REACH && pos.x <= end).then_some((*i, (start - pos.x).abs()))
    };
    let candidates: Vec<(usize, f32)> = notes.iter().enumerate().filter_map(|e| near(&e)).collect();
    let best = |list: &mut dyn Iterator<Item = (usize, f32)>| list.min_by(|a, b| a.1.total_cmp(&b.1)).map(|c| c.0);
    best(&mut candidates.iter().copied().filter(|(i, _)| selected.contains(i)))
        .or_else(|| best(&mut candidates.iter().copied()))
}

/// The velocity, 1 to 127, at height `y` in the velocity lane: the inverse of [`velocity_height`].
fn velocity_at(lane: Rect, y: f32) -> f64 {
    let height = (lane.height() - 4.0).max(1.0);
    let from_bottom = lane.bottom() - 2.0 - y;
    (f64::from(from_bottom / height) * 127.0).round().clamp(1.0, 127.0)
}

fn velocity_height(velocity: f32, lane: Rect) -> f32 {
    let v = f32::from(suhina_session::midi_velocity(velocity));
    (lane.height() - 4.0).max(0.0) * v / 127.0
}

/// The modifiers and buttons this frame, read once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Inputs {
    shift: bool,
    alt: bool,
    command: bool,
    /// The right mouse button is held: with the wheel it chooses the tools.
    right: bool,
}

/// What gestures in one clip work with: where things are, the clip and its grid.
struct Gestures<'a> {
    layout: &'a Layout,
    clip: &'a PianoClip,
    grid: NoteGrid,
}

impl Gestures<'_> {
    /// `note.delete` for one note, clearing the selection as the erase click does.
    fn delete_note(&self, state: &mut PianoRollState, actions: &mut Vec<Action>, index: usize) {
        state.selected.clear();
        let mut params = self.clip.address.clone();
        params["notes"] = json!([index]);
        actions.push(Action::new("note.delete", params));
    }

    /// Adds one note where `pos` is, `note.add` for it, and selects it.
    fn add_note(&self, state: &mut PianoRollState, actions: &mut Vec<Action>, pos: Pos2, free: bool) {
        if let Some(note) = state.new_note(self.layout, self.clip, self.grid, pos, free) {
            actions.push(Action::new("note.add", add_params(self.clip, &[note])));
            // `note.add` appends: the new note is the next index.
            state.selected = BTreeSet::from([self.clip.notes.len()]);
            state.audition_keys([note.key]);
        }
    }

    /// The notes a Shift+Draw stroke has drawn so far, one step on from the last, up to where
    /// the pointer is: the first starts where the press was, in the row it was in, and each is as
    /// long as the note last drawn or clicked. A stroke adds at most as many notes as one command
    /// takes, and never past the furthest beat.
    fn draw_run(&self, state: &mut PianoRollState, now: Pos2, free: bool) {
        let Some(first) = state.stroke.first().copied() else { return };
        let length = if first.duration > 0.0 { first.duration } else { self.grid.step.max(MIN_LENGTH) };
        let end =
            if free { self.layout.beat_at(now.x).max(0.0) } else { self.grid.nearest_line(self.layout.beat_at(now.x)) };
        let mut time = first.time;
        while time + length <= end + EPSILON {
            let taken = state.stroke.iter().any(|n| (n.time - time).abs() < EPSILON)
                || self.clip.notes.iter().any(|n| n.key == first.key && (n.time - time).abs() < EPSILON);
            if !taken && state.stroke.len() < MAX_NOTES_PER_COMMAND {
                state.stroke.push(Note { time, ..first });
            }
            time += length;
            if time > f64::from(BEAT_LIMIT) {
                break;
            }
        }
    }
}

/// The key column: the keys it draws, what a press plays and what Ctrl+click or Ctrl+drag
/// selects. A plain press and drag up or down plays the keys the pointer passes over, one at a
/// time; Ctrl+click selects every note of a pitch and Ctrl+drag the notes of every pitch the
/// drag passes over, without playing anything.
fn key_column(
    ui: &mut Ui,
    keys: &Rect,
    layout: &Layout,
    clip: &PianoClip,
    state: &mut PianoRollState,
    inputs: Inputs,
    actions: &mut Vec<Action>,
) {
    if keys.height() <= 1.0 {
        return;
    }
    let response = ui.interact(*keys, Id::new("piano-roll-keys"), Sense::click_and_drag());
    let hovered = ui.ctx().pointer_hover_pos().and_then(|p| keys.contains(p).then(|| layout.key_at(p.y)).flatten());
    let pressed = response.interact_pointer_pos().filter(|p| keys.contains(*p)).and_then(|p| layout.key_at(p.y));
    let selecting = inputs.command || inputs.shift;
    // The pointer is down in the column: the key under it plays, or is selected with Ctrl.
    if let Some(key) = pressed
        && ui.input(|i| i.pointer.primary_down())
    {
        state.focused = true;
        match state.gesture {
            // A Ctrl or Shift drag that began here keeps selecting the keys it passes over; the
            // selection is only replaced once, when it began.
            Some(Gesture::KeySelect { add }) => state.select_pitches(clip, &[key], add),
            _ if selecting => {
                let add = inputs.shift;
                state.gesture = Some(Gesture::KeySelect { add });
                state.select_pitches(clip, &[key], add);
            }
            _ => play_key(state, clip, actions, key),
        }
    }
    // Let go, or the pointer off the column: the note goes off. A gesture that belongs to
    // another part of the piano roll (the grid's) is left alone.
    let ours = matches!(state.gesture, Some(Gesture::KeySelect { .. })) || state.playing_key().is_some();
    if ours && (ui.input(|i| i.pointer.any_released()) || (state.playing_key().is_some() && hovered.is_none())) {
        state.gesture = None;
        if let Some(action) = state.key_release(clip) {
            actions.push(action);
        }
    }
    state.hover_key = hovered;
}

/// Plays `key`: the key that was sounding goes off and `key` goes on, so a drag up and down the
/// column sounds one key at a time.
fn play_key(state: &mut PianoRollState, clip: &PianoClip, actions: &mut Vec<Action>, key: u8) {
    if state.playing_key() == Some(key) {
        return;
    }
    if let Some(action) = state.key_release(clip) {
        actions.push(action);
    }
    if let Some(action) = state.key_press(clip, key) {
        actions.push(action);
    }
}

/// Draws the piano roll for `clip` in `area` and adds the edits it makes, and the key column's
/// `preview.*` notes, to `actions`.
pub fn show(
    ui: &mut Ui,
    area: Rect,
    clip: &PianoClip,
    state: &mut PianoRollState,
    cx: Context<'_>,
    actions: &mut Vec<Action>,
) {
    let Context { options, keymap, copied, playhead } = cx;
    let scale = options.toolbar_size.scale();
    let fresh = state.shown.as_ref() != Some(&clip.address);
    if fresh {
        state.shown = Some(clip.address.clone());
        state.selected.clear();
        state.gesture = None;
        state.playing_key = None;
    }
    // Keys run before the piano roll is drawn, and their commands after it: notes a paste or a
    // duplicate selected this frame exist from the next frame on.
    if !std::mem::take(&mut state.selecting_new) {
        state.selected.retain(|i| *i < clip.notes.len());
    }
    // The toolbar along the top; the editor under it. The toolbar's height follows Settings ›
    // Clip editor › Piano roll › Toolbar size, and never takes more than the room the editor has.
    let band = (TOOLBAR * scale).min(area.height());
    let toolbar_rect = Rect::from_min_max(area.min, Pos2::new(area.right(), area.top() + band));
    let whole = area;
    let area = Rect::from_min_max(Pos2::new(area.left(), toolbar_rect.bottom()), area.max);
    let lane_height = velocity_lane_height(area.height() - RULER, state.key_height, state.lane_wanted);
    let keys = Rect::from_min_max(
        Pos2::new(area.left(), area.top() + RULER),
        Pos2::new(area.left() + KEYS, area.bottom() - lane_height),
    );
    let ruler = Rect::from_min_max(Pos2::new(keys.right(), area.top()), Pos2::new(area.right(), keys.top()));
    let grid_rect = Rect::from_min_max(Pos2::new(keys.right(), keys.top()), Pos2::new(area.right(), keys.bottom()));
    let lane = Rect::from_min_max(Pos2::new(keys.right(), keys.bottom()), area.max);
    toolbar(ui, toolbar_rect, ToolbarCx { clip, keymap, copied, scale }, state, actions);
    if grid_rect.width() <= 1.0 || grid_rect.height() <= 1.0 {
        return;
    }
    // A newly shown clip fits the width with its notes in the middle; until the rows are scrolled
    // by hand, a change in height centres the notes again, so they are in view at any height.
    let rows = scale_view::rows(clip, state.fold);
    if fresh {
        fit(state, grid_rect, clip, &rows);
    } else if !state.scrolled_by_hand && (grid_rect.height() - state.centred_for).abs() > 0.5 {
        centre_notes(state, grid_rect.height(), clip, &rows);
    }

    let inputs = Inputs {
        alt: ui.input(|i| i.modifiers.alt),
        shift: ui.input(|i| i.modifiers.shift),
        command: ui.input(|i| i.modifiers.command || i.modifiers.ctrl),
        right: ui.input(|i| i.pointer.button_down(PointerButton::Secondary)),
    };
    if ui.input(|i| i.pointer.any_pressed()) {
        state.focused = widgets::pointer_over(ui, whole).is_some();
    }
    // A fresh press of the right button: whether the last hold stepped the tools with the wheel
    // says nothing about this one.
    if ui.input(|i| i.pointer.button_pressed(PointerButton::Secondary)) {
        state.right_stepped = false;
    }
    scroll_and_zoom(ui, state, area, grid_rect, inputs);
    // The ruler: its loop handles and markers, and dragging it to zoom and scroll time.
    let shape = ruler_view::interact(ui, ruler, grid_rect.left(), clip, state, inputs.alt, actions);
    state.view_width = grid_rect.width();
    let rows_height = rows.len() as f32 * state.key_height;
    state.scroll.y = state.scroll.y.clamp(0.0, (rows_height - grid_rect.height()).max(0.0));
    let width = ((clip.length + clip.bar_beats * 4.0) as f32 * state.beat_width).max(grid_rect.width());
    state.scroll.x = state.scroll.x.clamp(0.0, (width - grid_rect.width()).max(0.0));
    let layout = Layout {
        keys,
        ruler,
        grid: grid_rect,
        lane,
        scroll: state.scroll,
        beat_width: state.beat_width,
        key_height: state.key_height,
        rows,
    };
    let grid = note_grid(clip, state.beat_width);
    let gesture = Gestures { layout: &layout, clip, grid };

    // The grid's gestures: one widget for the whole grid, so a note under the pointer and the
    // empty grid around it are told apart by where the press was.
    let response = ui.interact(grid_rect, Id::new("piano-roll-grid"), Sense::click_and_drag());
    let pointer = response.interact_pointer_pos().or_else(|| ui.ctx().pointer_hover_pos());
    let mut eraser = None;
    if let Some(pos) = ui.ctx().pointer_hover_pos().filter(|p| grid_rect.contains(*p)) {
        let over = note_at(&layout, &clip.notes, pos);
        let cursor = match (&state.gesture, over, state.tool) {
            (Some(Gesture::Resize { .. } | Gesture::ResizeStart { .. }), _, _) => Some(CursorIcon::ResizeHorizontal),
            (Some(Gesture::Move { copy: true, .. }), _, _) => Some(CursorIcon::Copy),
            (Some(Gesture::Move { .. }), _, _) => Some(CursorIcon::Grabbing),
            (Some(Gesture::NoteVelocity { .. }), _, _) => Some(CursorIcon::ResizeVertical),
            (Some(Gesture::Slice { .. }), _, _) | (None, _, Tool::Slice) => Some(CursorIcon::Text),
            // There is no eraser cursor in the windowing layer, so the pointer is hidden and the
            // eraser button's shape is drawn where the pointer is.
            (Some(Gesture::Erase), _, _) | (None, _, Tool::Erase) => {
                eraser = Some(pos);
                Some(CursorIcon::None)
            }
            (None, Some((_, Part::Start | Part::End)), _) => Some(CursorIcon::ResizeHorizontal),
            (None, Some((_, Part::Body)), Tool::Select | Tool::Draw) if ui.input(|i| i.modifiers.command) => {
                Some(CursorIcon::Copy)
            }
            (Some(Gesture::Draw { .. } | Gesture::Paint), _, _) | (None, None, Tool::Draw | Tool::Paint) => {
                Some(CursorIcon::Crosshair)
            }
            _ => None,
        };
        if let Some(cursor) = cursor {
            ui.ctx().set_cursor_icon(cursor);
        }
    }
    if std::env::var("SUHINA_TRACE_CLICK").is_ok() {
        eprintln!(
            "click? {} dbl {} pos {pointer:?} over {:?} tool {:?} selected {:?}",
            response.clicked(),
            response.double_clicked(),
            pointer.map(|p| note_at(&layout, &clip.notes, p)),
            state.tool,
            state.selected
        );
    }
    let workflow = options.workflow;
    if response.clicked()
        && let Some(pos) = pointer
    {
        // The piano roll counts its own double-clicks: two clicks close in time and space, and a
        // third starts a new pair (› Double-click).
        let time = ui.input(|i| i.time);
        let double = state
            .last_click
            .is_some_and(|(at, from)| time - at < DOUBLE_CLICK_TIME && from.distance(pos) < DOUBLE_CLICK_DISTANCE);
        state.last_click = if double { None } else { Some((time, pos)) };
        let added = state.click_added.take();
        let over = note_at(&layout, &clip.notes, pos);
        if double {
            second_click(&gesture, state, actions, (over.map(|(i, _)| i), added), pos, inputs, workflow);
        } else {
            single_click(&gesture, state, actions, over.map(|(i, _)| i), pos, inputs, workflow);
        }
    }
    // The right button: the Mouse-first switches first, then the Draw-first preset's delete in
    // Draw and Paint (› Keymap presets).
    let right_deletes = workflow.right_click_deletes(matches!(state.tool, Tool::Draw | Tool::Paint));
    if response.clicked_by(PointerButton::Secondary)
        && !state.right_stepped
        && !state.right_playing
        && !(workflow.alt_right_plays && inputs.alt)
        && let Some(pos) = pointer
        && let Some((index, _)) = note_at(&layout, &clip.notes, pos)
        && right_deletes
    {
        gesture.delete_note(state, actions, index);
    }
    // The right button: the Mouse-first switches, on the press, so a press that never moves (a
    // click) does the same job as a stroke (› Keymap presets).
    let right_pressed = ui.input(|i| i.pointer.button_pressed(PointerButton::Secondary))
        && widgets::pointer_over(ui, grid_rect).is_some();
    if right_pressed {
        let from = pointer.unwrap_or(grid_rect.center());
        state.erasing.clear();
        state.muting.clear();
        if workflow.alt_right_plays && inputs.alt {
            // Alt (Option) and the right button: the clip plays from the point under the pointer
            // and stops when the button is let go. It is the transport, not an edit.
            if let Some(at) = clip.placement.arrangement_beat(layout.beat_at(from.x)) {
                actions.push(Action::new("transport.setPosition", json!({ "beats": round_beat(at) })));
                actions.push(Action::new("transport.play", json!({})));
                state.right_playing = true;
            }
        } else if workflow.shift_right_splits && inputs.shift {
            state.gesture = Some(Gesture::Slice { from });
        } else if right_deletes {
            state.gesture = Some(Gesture::Erase);
            if let Some((index, _)) = note_at(&layout, &clip.notes, from) {
                state.erasing.insert(index);
            }
        }
    }
    if response.drag_started_by(PointerButton::Primary) {
        let from = ui.input(|i| i.pointer.press_origin()).or(pointer).unwrap_or(grid_rect.center());
        state.stroke.clear();
        state.erasing.clear();
        state.muting.clear();
        state.gesture = Some(match (note_at(&layout, &clip.notes, from), state.tool) {
            (_, Tool::Erase) => Gesture::Erase,
            (_, Tool::Slice) => Gesture::Slice { from },
            // Mute: the state the stroke gives every note it touches is the first note's, flipped.
            (Some((index, _)), Tool::Mute) => {
                state.mute_target = clip.notes.get(index).is_some_and(|note| !note.muted);
                state.muting.insert(index);
                Gesture::Mute
            }
            (None, Tool::Mute) => Gesture::Mute,
            // Shift and Draw repeat the last note's length along the row the drag started in:
            // the first note now, the rest as the drag reaches ([`Gestures::draw_run`]); the
            // whole stroke is one `note.add` when the drag ends.
            (None, Tool::Draw) if inputs.shift => {
                if let Some(note) = state.new_note(&layout, clip, grid, from, inputs.alt) {
                    state.stroke.push(note);
                }
                Gesture::DrawRun
            }
            (None, Tool::Draw) => Gesture::Draw { from },
            (_, Tool::Paint) => {
                state.stroke.extend(state.new_note(&layout, clip, grid, from, inputs.alt));
                Gesture::Paint
            }
            // Left click draws: with the Mouse-first switch on, the Select tool draws a note on an
            // empty spot the way Draw does (› Keymap presets).
            (None, Tool::Select) if workflow.empty_click_draws => Gesture::Draw { from },
            (Some((grabbed, part)), _) => {
                if !state.selected.contains(&grabbed) {
                    if !(inputs.shift || inputs.command) {
                        state.selected.clear();
                    }
                    state.selected.insert(grabbed);
                }
                // The ends resize; on the body Alt changes the velocity and Ctrl copies
                // (› Moving, › Resizing, › Velocity).
                match part {
                    Part::End => Gesture::Resize { from, grabbed },
                    Part::Start => Gesture::ResizeStart { from, grabbed },
                    Part::Body if inputs.alt => Gesture::NoteVelocity { from, grabbed },
                    Part::Body => Gesture::Move { from, grabbed, copy: inputs.command },
                }
            }
            (None, Tool::Select) => Gesture::Select { from, add: inputs.shift || inputs.command },
        });
    }

    // The velocity lane's gestures.
    let lane_response =
        (lane.height() > 4.0).then(|| ui.interact(lane, Id::new("piano-roll-velocity"), Sense::click_and_drag()));
    if let Some(lane_response) = &lane_response {
        let at = lane_response.interact_pointer_pos();
        if lane_response.clicked()
            && let Some(pos) = at
        {
            match stem_at(&layout, &clip.notes, &BTreeSet::new(), pos) {
                Some(index) if inputs.shift || inputs.command => {
                    if !state.selected.remove(&index) {
                        state.selected.insert(index);
                    }
                }
                Some(index) => state.selected = BTreeSet::from([index]),
                None => {}
            }
        }
        if lane_response.drag_started_by(PointerButton::Primary) {
            let from = ui.input(|i| i.pointer.press_origin()).or(at).unwrap_or(lane.center());
            // A drag that grabs no bar (or any drag with Alt) draws a line (› Velocity).
            let grabbed = if inputs.alt { None } else { stem_at(&layout, &clip.notes, &state.selected, from) };
            if let Some(grabbed) = grabbed {
                if !state.selected.contains(&grabbed) {
                    state.selected = BTreeSet::from([grabbed]);
                }
                state.gesture = Some(Gesture::Velocity { from, grabbed });
            } else {
                state.gesture = Some(Gesture::VelocityLine { from });
            }
        }
    }
    // The lane's top edge: dragging it makes the lane taller or shorter (› Layout).
    let room = area.height() - RULER;
    gestures::lane_edge(ui, &layout, room, state);

    // The key column: pressing a key plays it through the track's instrument while it is held,
    // and Ctrl+click or Ctrl+drag selects the notes of that pitch or those pitches.
    key_column(ui, &keys, &layout, clip, state, inputs, actions);

    // The drag's effect, shown while it lasts and made a command when it ends. The pointer
    // position is the interaction's own, so the frame a drag ends still has one.
    let now = ui.input(|i| i.pointer.latest_pos()).or_else(|| ui.ctx().pointer_hover_pos()).or(pointer);
    let dragging = response.dragged_by(PointerButton::Primary)
        || response.dragged_by(PointerButton::Secondary)
        || lane_response.as_ref().is_some_and(|r| r.dragged_by(PointerButton::Primary));
    let stopped = response.drag_stopped_by(PointerButton::Primary)
        || response.drag_stopped_by(PointerButton::Secondary)
        || lane_response.as_ref().is_some_and(|r| r.drag_stopped_by(PointerButton::Primary));
    if dragging && let Some(now) = now {
        match state.gesture {
            Some(Gesture::Paint) => paint(state, &layout, clip, now),
            Some(Gesture::DrawRun) => gesture.draw_run(state, now, inputs.alt),
            Some(Gesture::Erase) => {
                if let Some((index, _)) = note_at(&layout, &clip.notes, now) {
                    state.erasing.insert(index);
                }
            }
            Some(Gesture::Mute) => {
                if let Some((index, _)) = note_at(&layout, &clip.notes, now) {
                    if state.muting.is_empty() {
                        // The first note the stroke touches decides what the whole stroke does.
                        state.mute_target = clip.notes.get(index).is_some_and(|note| !note.muted);
                    }
                    state.muting.insert(index);
                }
            }
            _ => {}
        }
    }
    let chosen: Vec<Note> = state.selected.iter().filter_map(|i| clip.notes.get(*i).copied()).collect();
    let preview = match (state.gesture.as_ref(), now) {
        (Some(Gesture::Draw { from }), Some(now)) if dragging || stopped => {
            Some(Effect::Draw(drawn_note(state, &layout, clip, grid, *from, now, inputs.alt)))
        }
        (Some(Gesture::VelocityLine { from }), Some(now)) if dragging || stopped => {
            Some(gestures::velocity_line(&layout, &clip.notes, *from, now))
        }
        (Some(gesture), Some(now)) if dragging || stopped => {
            Some(effect(gesture, now, &layout, clip, &chosen, grid, inputs.alt))
        }
        _ => None,
    };
    // Letting the right button go stops the clip the Mouse-first switch was playing. It watches
    // the button itself, so a press that never moved stops it too.
    if ui.input(|i| i.pointer.button_released(PointerButton::Secondary)) && std::mem::take(&mut state.right_playing) {
        actions.push(Action::new("transport.stop", json!({})));
    }
    let velocity_label = match (&state.gesture, &preview) {
        (Some(Gesture::NoteVelocity { grabbed, .. }), Some(Effect::Velocity(by))) => Some((*grabbed, *by)),
        _ => None,
    };
    if stopped {
        if let (Some(effect), Some(gesture)) = (preview.clone(), state.gesture.take()) {
            commit(&gesture, effect, clip, state, actions, &layout);
        }
        state.gesture = None;
    } else if !dragging && !ui.input(|i| i.pointer.any_down()) {
        state.gesture = None;
        state.stroke.clear();
        state.erasing.clear();
        state.muting.clear();
    }

    let shape = shape.unwrap_or(clip.shape);
    draw(ui, &layout, clip, state, grid, preview, shape.played());
    if let Some((grabbed, by)) = velocity_label {
        gestures::draw_velocity_label(ui, &layout, clip, grabbed, by);
    }
    let axis = ruler_view::Axis { origin: grid_rect.left() - state.scroll.x, beat_width: state.beat_width };
    ruler_view::draw(ui, ruler, &axis, &shape);
    // The eraser under the pointer: the windowing layer has no eraser cursor, so the pointer is
    // hidden and the Erase button's shape is drawn where it is.
    if let Some(pos) = eraser {
        let painter = ui.painter_at(whole);
        IconButton::Erase.draw(
            &painter,
            Rect::from_center_size(pos, Vec2::splat(TOOL_ICON * scale)),
            theme::text_primary(),
            scale,
        );
    }
    // The playhead: where playback is in the clip's content (through its start, offset and loop),
    // over the ruler, the grid and the velocity lane, as the other views show it.
    if let Some(beat) = playhead.and_then(|at| clip.placement.content_beat(at)) {
        draw_playhead(ui, &layout, beat);
    }
    for_readers(ui, &layout, clip, state);
    scale_view::key_readers(ui, layout.keys, clip, &layout.rows, |key| layout.y_of(key), layout.key_height);
    // Notes the audition sounds go on now and off a moment later.
    let track = clip.address.get("track").and_then(Value::as_u64);
    state.audition_frame(ui, track, actions);
}

/// A single click in the grid, on note `over` or an empty spot at `pos`: what the tool does with
/// it (› Drawing notes, › Selecting, › Muting).
fn single_click(
    gesture: &Gestures<'_>,
    state: &mut PianoRollState,
    actions: &mut Vec<Action>,
    over: Option<usize>,
    pos: Pos2,
    inputs: Inputs,
    workflow: Workflow,
) {
    let clip = gesture.clip;
    match (over, state.tool) {
        (Some(index), Tool::Erase) => gesture.delete_note(state, actions, index),
        // A click with Mute mutes the note, or unmutes it when it is already muted: one
        // `note.setMuted`, one undo step (› Muting).
        (Some(index), Tool::Mute) => {
            if let Some(note) = clip.notes.get(index) {
                let mut params = clip.address.clone();
                params["notes"] = json!([index]);
                params["muted"] = json!(!note.muted);
                actions.push(Action::new("note.setMuted", params));
            }
        }
        (None, Tool::Erase | Tool::Mute) => {}
        (Some(index), _) if inputs.shift || inputs.command => {
            if !state.selected.remove(&index) {
                state.selected.insert(index);
            }
        }
        (Some(index), _) => {
            state.selected = BTreeSet::from([index]);
            if let Some(note) = clip.notes.get(index) {
                state.remember(note);
            }
        }
        // Draw and Paint add a note; so does Select when a click on empty grid draws (the
        // Mouse-first setting, › Workflow modes).
        (None, Tool::Draw | Tool::Paint) => click_adds(gesture, state, actions, pos, inputs),
        (None, Tool::Select) if workflow.empty_click_draws => click_adds(gesture, state, actions, pos, inputs),
        (None, Tool::Slice) => {}
        (None, Tool::Select) => state.selected.clear(),
    }
}

/// A click that adds a note at `pos`, remembering which index it will have, so the second click
/// of a double-click selects it instead of deleting it.
fn click_adds(
    gesture: &Gestures<'_>,
    state: &mut PianoRollState,
    actions: &mut Vec<Action>,
    pos: Pos2,
    inputs: Inputs,
) {
    let before = actions.len();
    gesture.add_note(state, actions, pos, inputs.alt);
    if actions.len() > before {
        state.click_added = Some(gesture.clip.notes.len());
    }
}

/// The second click of a double-click, on note `over` (with `added` the note the first click
/// added, if it did) or an empty spot at `pos` (› Double-click in `docs/spec/ui-clip-editor.md`).
fn second_click(
    gesture: &Gestures<'_>,
    state: &mut PianoRollState,
    actions: &mut Vec<Action>,
    (over, added): (Option<usize>, Option<usize>),
    pos: Pos2,
    inputs: Inputs,
    workflow: Workflow,
) {
    match (state.tool, over) {
        // The first click already erased or muted the note: the second adds nothing, so a
        // double-click is one note and one undo step.
        (Tool::Erase | Tool::Mute, _) => {}
        // The first click added a note: the second selects it and never deletes it.
        (_, Some(index)) if added.is_some() => state.selected = BTreeSet::from([index]),
        (_, None) if added.is_some() => {}
        (Tool::Select, Some(index)) if workflow.double_click_deletes => gesture.delete_note(state, actions, index),
        (Tool::Select, None) if workflow.double_click_adds => click_adds(gesture, state, actions, pos, inputs),
        _ => single_click(gesture, state, actions, over, pos, inputs, workflow),
    }
}

fn draw_playhead(ui: &Ui, layout: &Layout, beat: f64) {
    let x = layout.x_of(beat).round() + 0.5;
    let area = layout.ruler.union(layout.grid).union(layout.lane);
    if !(layout.grid.left()..=layout.grid.right()).contains(&x) {
        return;
    }
    let painter = ui.painter_at(area);
    painter.vline(x, area.y_range(), Stroke::new(1.0, theme::accent()));
    let response = ui.interact(
        Rect::from_center_size(Pos2::new(x, area.center().y), Vec2::new(3.0, area.height())),
        Id::new("piano-roll-playhead"),
        Sense::hover(),
    );
    let label = format!("Playhead at beat {}", round_beat(beat));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &label));
}

/// What the toolbar's buttons draw from: the clip they act on, the keys they show, how many notes
/// a paste would add, and the Toolbar size they are drawn at.
#[derive(Clone, Copy)]
struct ToolbarCx<'a> {
    clip: &'a PianoClip,
    keymap: &'a crate::settings::keymap::Keymap,
    copied: Option<usize>,
    scale: f32,
}

/// The toolbar along the top of the piano roll: the tools, the note clipboard and the quantise
/// button as icons drawn in code, and the grid value at the right end. Returns nothing; a button
/// that runs something adds to `actions`.
fn toolbar(ui: &mut Ui, rect: Rect, cx: ToolbarCx<'_>, state: &mut PianoRollState, actions: &mut Vec<Action>) {
    let ToolbarCx { clip, keymap, copied, scale } = cx;
    if rect.height() < 4.0 {
        return;
    }
    let painter = ui.painter_at(rect);
    let paint = ButtonPaint { ui, painter: &painter, keymap, scale };
    painter.rect_filled(rect, 0.0, theme::panel_background());
    painter.hline(rect.x_range(), rect.bottom() - 0.5, theme::hairline(theme::border()));
    let row = rect.shrink2(Vec2::new(4.0, 3.0));
    // The icon box, the gap and the strokes scale with Toolbar size; the box never outgrows the
    // band, so a small detail area shows the icons and not a clipped toolbar.
    let icon = (TOOL_ICON * scale).min(row.height());
    let gap = TOOL_GAP * scale;
    let side = row.height().min(icon);
    let x = std::cell::Cell::new(row.left() + KEYS - 4.0);
    let place = || {
        let area = Rect::from_min_size(Pos2::new(x.get(), row.top() + (row.height() - side) / 2.0), Vec2::splat(side));
        x.set(area.right() + gap);
        area
    };
    for (tool, button) in [
        (Some(Tool::Select), IconButton::Select),
        (Some(Tool::Draw), IconButton::Draw),
        (Some(Tool::Paint), IconButton::Paint),
        (Some(Tool::Erase), IconButton::Erase),
        (Some(Tool::Slice), IconButton::Slice),
        (Some(Tool::Mute), IconButton::Mute),
    ] {
        let area = place();
        let chosen = tool == Some(state.tool);
        if paint.place(area, button, chosen, true) {
            state.tool = tool.unwrap_or(state.tool);
        }
    }
    // A divider, then the note clipboard's buttons.
    x.set(x.get() + 2.0);
    if x.get() < row.right() {
        painter.vline(x.get(), row.top() + 2.0..=row.bottom() - 2.0, theme::hairline(theme::border_strong()));
    }
    x.set(x.get() + 5.0);
    let selected = !state.selected.is_empty();
    for button in [IconButton::Cut, IconButton::Copy, IconButton::Paste, IconButton::Duplicate] {
        let area = place();
        let active = matches!(button, IconButton::Paste) || selected;
        if paint.place(area, button, false, active)
            && let Some(command) = state.clipboard_action(button, clip, copied)
        {
            actions.push(command);
        }
    }
    // A divider, then the quantise button (which shows the MIDI tools column and its quantise
    // block) and the column's own toggle.
    x.set(x.get() + 2.0);
    if x.get() < row.right() {
        painter.vline(x.get(), row.top() + 2.0..=row.bottom() - 2.0, theme::hairline(theme::border_strong()));
    }
    x.set(x.get() + 5.0);
    let quantise = place();
    if paint.place(quantise, IconButton::Quantise, state.quantize_open, true) {
        state.quantize_open = !state.quantize_open;
        if state.quantize_open {
            state.tools_open = true;
        }
    }
    let tools = place();
    if paint.place(tools, IconButton::MidiTools, state.tools_open, true) {
        state.tools_open = !state.tools_open;
    }
    let audition = place();
    if paint.place(audition, IconButton::Audition, state.audition, true) {
        state.audition = !state.audition;
        if !state.audition {
            state.audition_pending.clear();
        }
    }
    // The workflow menu: the mode's name, the four modes and every workflow setting
    // (› Workflow modes).
    let hide_left = row.right() - side;
    let workflow = Rect::from_min_size(Pos2::new(x.get() + 4.0, row.top()), Vec2::new(WORKFLOW_MENU, row.height()));
    if workflow.right() + 4.0 + 104.0 < hide_left {
        // Each menu takes the width its button needs, so the next one starts right after it.
        let shown = workflow_menu(ui, workflow, keymap, state);
        x.set(shown.right().min(workflow.right()) + gap);
        // The ghost menu after it, when there is room for both (› Ghost notes).
        let ghosts =
            Rect::from_min_size(Pos2::new(x.get() + 4.0, row.top()), Vec2::new(ghost_view::MENU_WIDTH, row.height()));
        if ghosts.right() + 4.0 + 104.0 < hide_left {
            let shown = ghost_view::menu(ui, ghosts, &clip.ghosts, &mut state.ghost_source);
            x.set(shown.right().min(ghosts.right()) + gap);
            // The Scale menu after it, when the grid value still fits after it (› Scale and key);
            // the grid value's caption gives way to it.
            let menu = Rect::from_min_size(
                Pos2::new(x.get() + 4.0, row.top()),
                Vec2::new(scale_view::MENU_WIDTH, row.height()),
            );
            if menu.right() + gap + 2.0 + GRID_MENU + 6.0 <= hide_left - gap {
                let before = state.fold;
                let shown = scale_view::menu(ui, menu, clip, &mut state.fold, actions);
                if state.fold != before {
                    // Another set of rows: the notes are centred again in them.
                    state.scrolled_by_hand = false;
                    state.centred_for = 0.0;
                }
                x.set(shown.right().min(menu.right()) + gap);
            }
        }
    }
    let paint = ButtonPaint { ui, painter: &painter, keymap, scale };
    let x = x.get();
    // At the very end the × that hides the editor, then the grid value before it.
    let hide =
        Rect::from_min_size(Pos2::new(row.right() - side, row.top() + (row.height() - side) / 2.0), Vec2::splat(side));
    if hide.left() >= x && paint.place(hide, IconButton::Hide, false, true) {
        state.hide_requested = true;
    }
    let row = Rect::from_min_max(row.min, Pos2::new(hide.left() - gap - 2.0, row.bottom()));
    let menu = Rect::from_min_max(Pos2::new(row.right() - GRID_MENU, row.top()), row.max);
    if menu.left() < x + 6.0 {
        return;
    }
    // The caption, when there is room for it too.
    if menu.left() >= x + 40.0 {
        let label =
            Rect::from_min_max(Pos2::new(menu.left() - 34.0, row.top()), Pos2::new(menu.left() - 4.0, row.bottom()));
        widgets::text_in(
            &painter,
            label,
            Align2::RIGHT_CENTER,
            "Grid",
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_hint(),
        );
    }
    if let Some(picked) = grid_menu(ui, menu, clip, keymap) {
        state.grid_picked = Some(picked);
    }
}

/// How wide the toolbar's grid menu button is, in points.
const GRID_MENU: f32 = 76.0;

/// The toolbar's grid menu (› Grid and snapping): its button shows the grid value; its menu holds
/// Snap to grid, the adaptive widths, the fixed values, Narrow grid, Widen grid and Triplet grid,
/// each with its key, the chosen ones ticked. While the grid follows the control bar, an entry the
/// control bar cannot hold is greyed out. Returns the grid picked this frame.
fn grid_menu(ui: &mut Ui, rect: Rect, clip: &PianoClip, keymap: &crate::settings::keymap::Keymap) -> Option<PianoGrid> {
    let grid = clip.grid_menu;
    let font = theme::ui_font(theme::TEXT_SMALL);
    let mono = theme::mono_font(theme::TEXT_SMALL);
    let shown = grid.label();
    let mut picked = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        let text = egui::RichText::new(format!("{shown}   ")).font(mono.clone()).color(theme::text_value());
        let menu = ui.menu_button(text, |ui| {
            // One entry: its text, the key its action has, ticked when `ticked` says so.
            let mut entry = |ui: &mut Ui, name: &str, action: Option<&str>, ticked: Option<bool>, result: PianoGrid| {
                let enabled = !clip.grid_follows || result.control_bar_value().is_some();
                let key = action.and_then(|a| keymap.label(a)).unwrap_or_default();
                let text = egui::RichText::new(name).font(font.clone());
                let button = egui::Button::selectable(ticked.unwrap_or(false), text)
                    .shortcut_text(egui::RichText::new(key.as_str()).font(mono.clone()).color(theme::text_hint()));
                let response = ui.add_enabled(enabled, button);
                // Its accessible label is its name and its key, as the icon buttons' are.
                let label = if key.is_empty() { name.to_string() } else { format!("{name} ({key})") };
                response.widget_info(|| match ticked {
                    Some(on) => egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, on, &label),
                    None => egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &label),
                });
                if response.clicked() {
                    picked = Some(result);
                    ui.close();
                }
            };
            let heading = |ui: &mut Ui, text: &str| {
                ui.label(egui::RichText::new(text).font(font.clone()).color(theme::text_hint()));
            };
            entry(ui, "Snap to grid", Some("pianoRoll.gridSnap"), Some(grid.snap), grid.toggle_snap());
            ui.separator();
            heading(ui, "Adaptive grid");
            for width in Width::ALL {
                let name = format!("Adaptive {}", width.name().to_lowercase());
                let result = PianoGrid { mode: GridMode::Adaptive(width), ..grid };
                entry(ui, &name, None, Some(grid.mode == GridMode::Adaptive(width)), result);
            }
            ui.separator();
            heading(ui, "Fixed grid");
            for (index, value) in FIXED_VALUES.iter().enumerate() {
                // Picking a value while following the control bar turns snapping back on: its
                // none is snap off.
                let snap = grid.snap || clip.grid_follows;
                let result = PianoGrid { mode: GridMode::Fixed(index), snap, ..grid };
                entry(ui, value, None, Some(grid.mode == GridMode::Fixed(index)), result);
            }
            ui.separator();
            entry(ui, "Narrow grid", Some("pianoRoll.gridNarrow"), None, grid.narrower());
            entry(ui, "Widen grid", Some("pianoRoll.gridWiden"), None, grid.wider());
            ui.separator();
            entry(ui, "Triplet grid", Some("pianoRoll.gridTriplet"), Some(grid.triplet), grid.toggle_triplet());
        });
        let label = format!("Grid value {shown}");
        menu_arrow(ui, menu.response.rect, theme::text_value());
        menu.response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        menu.response.on_hover_text("The grid: snapping, adaptive or fixed lines, triplets");
    });
    picked.filter(|p| *p != grid)
}

/// The small triangle at the right end of a toolbar menu's button, drawn as a shape: the UI font
/// has no glyph for it.
pub(crate) fn menu_arrow(ui: &Ui, button: Rect, color: Color32) {
    let c = Pos2::new(button.right() - 7.0, button.center().y);
    ui.painter().add(egui::Shape::convex_polygon(
        vec![c + Vec2::new(-3.0, -1.5), c + Vec2::new(3.0, -1.5), c + Vec2::new(0.0, 2.0)],
        color,
        Stroke::NONE,
    ));
}

/// How wide the toolbar's workflow menu is, in points.
const WORKFLOW_MENU: f32 = 96.0;

/// The toolbar's workflow menu: its button shows the mode, its menu offers the four modes and,
/// under a divider, a submenu per workflow setting with its choices. A pick is left in `state`
/// for the window, which keeps it in the keymap.
fn workflow_menu(
    ui: &mut Ui,
    rect: Rect,
    keymap: &crate::settings::keymap::Keymap,
    state: &mut PianoRollState,
) -> Rect {
    use crate::settings::keymap::Preset;
    use crate::settings::workflow::{Pick, Setting};
    let mode = keymap.preset();
    let current = keymap.workflow();
    let font = theme::ui_font(theme::TEXT_SMALL);
    let mut picked = None;
    let mut shown = rect;
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        let text = egui::RichText::new(format!("{}   ", mode.name())).font(font.clone()).color(theme::text_value());
        let menu = ui.menu_button(text, |ui| {
            for choice in Preset::MODES {
                let response =
                    ui.selectable_label(choice == mode, egui::RichText::new(choice.name()).font(font.clone()));
                if response.clicked() {
                    if choice != mode {
                        picked = Some(Pick::Mode(choice));
                    }
                    ui.close();
                }
            }
            ui.separator();
            for setting in Setting::ALL {
                let chosen = current.get(setting);
                let shown = setting.choices().get(chosen).copied().unwrap_or("");
                let title = egui::RichText::new(format!("{}: {shown}", setting.name())).font(font.clone());
                ui.menu_button(title, |ui| {
                    for (index, choice) in setting.choices().iter().enumerate() {
                        let response =
                            ui.selectable_label(index == chosen, egui::RichText::new(*choice).font(font.clone()));
                        let name = format!("{}: {choice}", setting.name());
                        response.widget_info(|| {
                            egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, index == chosen, &name)
                        });
                        if response.clicked() {
                            if index != chosen {
                                picked = Some(Pick::Setting(setting, index));
                            }
                            ui.close();
                        }
                    }
                });
            }
        });
        let label = format!("Workflow {}", mode.name());
        menu_arrow(ui, menu.response.rect, theme::text_value());
        menu.response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        shown = menu.response.rect;
        menu.response.on_hover_text("How clicks, right-clicks and double-clicks work: a preset, or Custom");
    });
    if picked.is_some() {
        state.workflow_picked = picked;
    }
    shown
}

/// The toolbar's icon buttons as they are painted this frame: the surface they are drawn on, the
/// keys their tooltips show and the Toolbar size their shapes are drawn at.
struct ButtonPaint<'a> {
    ui: &'a Ui,
    painter: &'a egui::Painter,
    keymap: &'a crate::settings::keymap::Keymap,
    scale: f32,
}

impl ButtonPaint<'_> {
    /// One icon button: the shape in the tool's colours, a tooltip with its name and the key it
    /// has, and the same name as its accessible label. Returns whether it was clicked.
    fn place(&self, area: Rect, button: IconButton, chosen: bool, active: bool) -> bool {
        let ui = self.ui;
        let painter = self.painter;
        let response = widgets::button_area(ui, area, Id::new(("piano-roll-icon", button.name())), button.name());
        let hint = icon_hint(button, self.keymap);
        let response = response.on_hover_text(hint.clone());
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, active, chosen, hint.clone()));
        if active {
            let fill = if chosen { theme::control_active() } else { theme::control_background() };
            painter.rect_filled(
                area,
                0.0,
                crate::kit::motion::mix(fill, theme::control_hover(), widgets::hover(ui, &response)),
            );
        }
        let color = if !active {
            theme::text_hint()
        } else if chosen {
            theme::text_primary()
        } else {
            theme::text_secondary()
        };
        button.draw(painter, area, color, self.scale);
        active && response.clicked()
    }
}

/// The name and key an icon button shows, such as "Draw (2)": the key is the one the keymap has
/// bound to the button's action now, so a rebound key (or one a preset moved) shows as it is.
fn icon_hint(button: IconButton, keymap: &crate::settings::keymap::Keymap) -> String {
    match button.action().and_then(|id| keymap.label(id)) {
        Some(key) => format!("{} ({key})", button.name()),
        None => button.name().to_string(),
    }
}

/// What a gesture does with the pointer at `now`.
#[derive(Debug, Clone, PartialEq)]
enum Effect {
    Move(f64, i32),
    Resize(f64),
    /// Beats the selection's starts move by, their ends kept.
    ResizeStart(f64),
    Select(Rect),
    Velocity(f64),
    /// A line in the velocity lane from `from` to `to`, and the velocity each stem it spans takes.
    VelocityLine {
        from: Pos2,
        to: Pos2,
        set: Vec<(usize, f64)>,
    },
    /// The note a draw drag makes.
    Draw(Option<Note>),
    /// A slice: the beat the cut is at, and the notes it crosses.
    Slice(f64, Vec<usize>),
    /// A paint or erase stroke: what it does is in the state.
    Stroke,
}

fn effect(
    gesture: &Gesture,
    now: Pos2,
    layout: &Layout,
    clip: &PianoClip,
    chosen: &[Note],
    grid: NoteGrid,
    free: bool,
) -> Effect {
    match gesture {
        Gesture::Move { from, grabbed, .. } => {
            let raw = f64::from((now.x - from.x) / layout.beat_width);
            let rows = (-(now.y - from.y) / layout.key_height).round() as i32;
            // Folded, the grabbed note steps through the shown rows; the rest move as far.
            let keys = clip.notes.get(*grabbed).map_or(rows, |n| layout.rows.semitones(n.key, rows));
            let (beats, keys) = move_delta(raw, keys, chosen, grid, free);
            Effect::Move(beats, keys)
        }
        Gesture::Resize { from, grabbed } => {
            let raw = f64::from((now.x - from.x) / layout.beat_width);
            clip.notes
                .get(*grabbed)
                .map_or(Effect::Resize(0.0), |g| Effect::Resize(resize_by(raw, g, chosen, grid, free)))
        }
        Gesture::ResizeStart { from, grabbed } => {
            let raw = f64::from((now.x - from.x) / layout.beat_width);
            clip.notes
                .get(*grabbed)
                .map_or(Effect::ResizeStart(0.0), |g| Effect::ResizeStart(start_by(raw, g, chosen, grid, free)))
        }
        Gesture::NoteVelocity { from, grabbed } => {
            Effect::Velocity(gestures::velocity_by(*from, now, clip.notes.get(*grabbed)))
        }
        Gesture::VelocityLine { from } => gestures::velocity_line(layout, &clip.notes, *from, now),
        Gesture::Select { from, .. } => Effect::Select(Rect::from_two_pos(*from, now)),
        Gesture::Velocity { grabbed, .. } => {
            // The grabbed bar follows the pointer's height; the other selected notes change by
            // the same amount (each kept within 1 to 127 by the command).
            let target = velocity_at(layout.lane, now.y);
            let current =
                clip.notes.get(*grabbed).map_or(target, |n| f64::from(suhina_session::midi_velocity(n.velocity)));
            Effect::Velocity(target - current)
        }
        Gesture::Slice { .. } => match slice_at(clip, layout, grid, now.x, free) {
            Some((at, cut)) => Effect::Slice(at, cut),
            None => Effect::Stroke,
        },
        Gesture::Draw { .. }
        | Gesture::DrawRun
        | Gesture::Paint
        | Gesture::Erase
        | Gesture::Mute
        | Gesture::KeySelect { .. } => Effect::Stroke,
    }
}

/// The beat a cut at `x` lands on, and the notes it crosses; `None` when it misses every note.
/// The line's own x is used, so the cut is where the pointer is, not where the drag started.
fn slice_at(clip: &PianoClip, layout: &Layout, grid: NoteGrid, x: f32, free: bool) -> Option<(f64, Vec<usize>)> {
    let at = if free { layout.beat_at(x).max(0.0) } else { grid.nearest_line(layout.beat_at(x)) };
    if !at.is_finite() {
        return None;
    }
    let crossed: Vec<usize> = clip
        .notes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.time < at - EPSILON && n.time + n.duration > at + EPSILON)
        .map(|(i, _)| i)
        .collect();
    if crossed.is_empty() { None } else { Some((at, crossed)) }
}

/// The note a draw drag from `from` to `now` makes: it starts where the press was, and its end
/// follows the pointer on the grid (`free`: as it is), at least one step long.
fn drawn_note(
    state: &PianoRollState,
    layout: &Layout,
    clip: &PianoClip,
    grid: NoteGrid,
    from: Pos2,
    now: Pos2,
    free: bool,
) -> Option<Note> {
    let mut note = state.new_note(layout, clip, grid, from, free)?;
    let end = grid.place(layout.beat_at(now.x), free, false);
    let least = if grid.snaps && !free { grid.step.min(note.duration) } else { MIN_LENGTH };
    if end - note.time >= least {
        note.duration = end - note.time;
    }
    Some(note)
}

/// A paint stroke at `now`: the slot of the stroke's note length under the pointer gets a note
/// in the row under it, unless the stroke has one there already.
fn paint(state: &mut PianoRollState, layout: &Layout, clip: &PianoClip, now: Pos2) {
    let Some(first) = state.stroke.first().copied() else { return };
    let Some(key) = layout.key_at(now.y) else { return };
    let length = first.duration.max(MIN_LENGTH);
    let slots = ((layout.beat_at(now.x) - first.time) / length).floor();
    let time = first.time + slots * length;
    // A stroke never paints before the content's start or past the furthest beat.
    if !(time.is_finite() && time >= -EPSILON && time <= f64::from(BEAT_LIMIT)) || state.stroke.len() >= 10_000 {
        return;
    }
    let time = time.max(0.0);
    if state.stroke.iter().any(|n| (n.time - time).abs() < EPSILON) {
        return;
    }
    let taken = clip.notes.iter().any(|n| n.key == key && (n.time - time).abs() < EPSILON);
    if !taken {
        state.stroke.push(Note { time, key, ..first });
    }
}

fn commit(
    gesture: &Gesture,
    effect: Effect,
    clip: &PianoClip,
    state: &mut PianoRollState,
    actions: &mut Vec<Action>,
    layout: &Layout,
) {
    let notes: Vec<usize> = state.selected.iter().copied().collect();
    let mut params = clip.address.clone();
    params["notes"] = json!(notes);
    match (gesture, effect) {
        (Gesture::Move { copy, .. }, Effect::Move(beats, keys)) if !notes.is_empty() && (beats != 0.0 || keys != 0) => {
            let moved: Vec<Note> = notes.iter().filter_map(|i| clip.notes.get(*i).copied()).collect();
            state.audition_moved(&moved, keys);
            params["beats"] = json!(round_beat(beats));
            params["semitones"] = json!(keys);
            if *copy {
                // Ctrl+drag: one `note.duplicate` puts the copies where the drag let go; the
                // copies are appended, so they are the next indices, and they are selected.
                actions.push(Action::new("note.duplicate", params));
                state.selected = (clip.notes.len()..clip.notes.len() + notes.len()).collect();
            } else {
                // Notes moved onto others of the same key and start replace them (Settings ›
                // Piano roll › Moved notes replace the notes under them, on by default); the
                // selection stays on the moved notes as the indices close up.
                let replace = state.notes_replace;
                if replace {
                    params["replace"] = json!(true);
                    let landed: Vec<Note> = notes
                        .iter()
                        .filter_map(|i| clip.notes.get(*i))
                        .map(|n| Note {
                            time: n.time + round_beat(beats),
                            key: u8::try_from(i32::from(n.key) + keys).unwrap_or(n.key),
                            ..*n
                        })
                        .collect();
                    let covered: Vec<usize> = (0..clip.notes.len())
                        .filter(|i| !notes.contains(i))
                        .filter(|i| {
                            landed
                                .iter()
                                .any(|m| m.key == clip.notes[*i].key && (m.time - clip.notes[*i].time).abs() < 1e-9)
                        })
                        .collect();
                    if !covered.is_empty() {
                        state.selected =
                            state.selected.iter().map(|i| i - covered.iter().filter(|c| *c < i).count()).collect();
                    }
                }
                actions.push(Action::new("note.move", params));
            }
        }
        (Gesture::Resize { .. }, Effect::Resize(by)) if !notes.is_empty() && by != 0.0 => {
            params["by"] = json!(round_beat(by));
            actions.push(Action::new("note.resize", params));
        }
        (Gesture::ResizeStart { .. }, Effect::ResizeStart(by)) if !notes.is_empty() && by != 0.0 => {
            params["startBy"] = json!(round_beat(by));
            actions.push(Action::new("note.resize", params));
        }
        (Gesture::Velocity { .. } | Gesture::NoteVelocity { .. }, Effect::Velocity(by))
            if !notes.is_empty() && by != 0.0 =>
        {
            params["by"] = json!(by);
            actions.push(Action::new("note.setVelocity", params));
        }
        // The velocity line: one `note.setVelocity` with a value for each stem it spans.
        (Gesture::VelocityLine { .. }, Effect::VelocityLine { set, .. }) if !set.is_empty() => {
            let mut params = clip.address.clone();
            params["notes"] = set.iter().map(|(i, _)| json!(i)).collect();
            params["velocities"] = set.iter().map(|(_, v)| json!(v)).collect();
            actions.push(Action::new("note.setVelocity", params));
        }
        (Gesture::Draw { .. }, Effect::Draw(Some(note))) => {
            actions.push(Action::new("note.add", add_params(clip, &[note])));
            state.selected = BTreeSet::from([clip.notes.len()]);
            state.remember(&note);
            state.audition_keys([note.key]);
        }
        // Shift and Draw: one `note.add` for the whole stroke, the notes it drew.
        (Gesture::DrawRun, _) => {
            let stroke: Vec<Note> = std::mem::take(&mut state.stroke)
                .into_iter()
                .filter(|p| !clip.notes.iter().any(|n| n.key == p.key && (n.time - p.time).abs() < EPSILON))
                .collect();
            if !stroke.is_empty() {
                actions.push(Action::new("note.add", add_params(clip, &stroke)));
                state.selected = (clip.notes.len()..clip.notes.len() + stroke.len()).collect();
                state.audition_keys(stroke.iter().map(|n| n.key));
                if let Some(last) = stroke.last() {
                    state.remember(last);
                }
            }
        }
        // Slice: one `note.split` for the whole stroke, at the beat the line ends at.
        (Gesture::Slice { .. }, Effect::Slice(at, cut)) if !cut.is_empty() => {
            let mut params = clip.address.clone();
            params["notes"] = json!(cut);
            params["at"] = json!(round_beat(at));
            // `note.split` appends the second halves: they are the notes after the ones there.
            state.selected = (clip.notes.len()..clip.notes.len() + cut.len()).collect();
            actions.push(Action::new("note.split", params));
        }
        (Gesture::Paint, _) => {
            // A painted note on top of one already there (same key, same start) is left out.
            let stroke: Vec<Note> = std::mem::take(&mut state.stroke)
                .into_iter()
                .filter(|p| !clip.notes.iter().any(|n| n.key == p.key && (n.time - p.time).abs() < EPSILON))
                .collect();
            if !stroke.is_empty() {
                actions.push(Action::new("note.add", add_params(clip, &stroke)));
                state.selected = (clip.notes.len()..clip.notes.len() + stroke.len()).collect();
                state.audition_keys(stroke.iter().map(|n| n.key));
            }
        }
        (Gesture::Erase, _) if !state.erasing.is_empty() => {
            let mut params = clip.address.clone();
            params["notes"] = json!(std::mem::take(&mut state.erasing).into_iter().collect::<Vec<_>>());
            state.selected.clear();
            actions.push(Action::new("note.delete", params));
        }
        // Mute: one `note.setMuted` for the whole stroke, every note it passed over set to the
        // state the first note it touched takes (› Muting).
        (Gesture::Mute, _) if !state.muting.is_empty() => {
            let mut params = clip.address.clone();
            params["notes"] = json!(std::mem::take(&mut state.muting).into_iter().collect::<Vec<_>>());
            params["muted"] = json!(state.mute_target);
            actions.push(Action::new("note.setMuted", params));
        }
        (Gesture::Select { add, .. }, Effect::Select(rect)) => {
            if !*add {
                state.selected.clear();
            }
            for (i, note) in clip.notes.iter().enumerate() {
                if layout.note_rect(note).intersects(rect) {
                    state.selected.insert(i);
                }
            }
        }
        _ => {}
    }
}

/// The tool `step` places on from `tool`, stopping at the first and the last tool: what the
/// right button and the wheel choose.
fn step_tool(tool: Tool, step: i32) -> Tool {
    let last = Tool::ALL.len().saturating_sub(1) as i32;
    let index = Tool::ALL.iter().position(|(t, _)| *t == tool).unwrap_or(0) as i32;
    let next = (index + step).clamp(0, last) as usize;
    Tool::ALL.get(next).map_or(tool, |(tool, _)| *tool)
}

/// The cursor a tool shows over the grid. Erase hides the pointer and draws the eraser button's
/// shape in its place (`show`), since the windowing layer has no eraser cursor.
fn cursor_for(tool: Tool) -> Option<CursorIcon> {
    match tool {
        Tool::Select | Tool::Mute => None,
        Tool::Draw | Tool::Paint => Some(CursorIcon::Crosshair),
        Tool::Erase => Some(CursorIcon::None),
        Tool::Slice => Some(CursorIcon::Text),
    }
}

/// Beats rounded to a millionth, so a value made of steps such as thirds sends no float noise.
fn round_beat(beat: f64) -> f64 {
    (beat * 1e6).round() / 1e6
}

/// How tall the velocity lane is with `room` under the ruler: 72 px, at most 30 % of the room
/// (`wanted`, the height its top edge was dragged to, instead of both), never so tall that fewer
/// than five key rows of `key_height` are left, and left out when that leaves it under 12 px
/// (› Layout in `docs/spec/ui-clip-editor.md`).
fn velocity_lane_height(room: f32, key_height: f32, wanted: Option<f32>) -> f32 {
    let most = room - LEAST_KEY_ROWS * key_height;
    let lane = wanted.unwrap_or(VELOCITY_LANE.min(room * 0.3)).min(most);
    if lane.is_finite() && lane >= LEAST_LANE { lane } else { 0.0 }
}

/// A newly shown clip: the whole clip fits the `grid`'s width, and its notes are in the middle.
fn fit(state: &mut PianoRollState, grid: Rect, clip: &PianoClip, rows: &KeyRows) {
    let width = grid.width().max(1.0);
    state.beat_width = (width / clip.length.max(1.0) as f32).clamp(BEAT_WIDTH_RANGE.0, BEAT_WIDTH_RANGE.1);
    state.scroll.x = 0.0;
    state.scrolled_by_hand = false;
    centre_notes(state, grid.height(), clip, rows);
}

/// Scrolls the key rows so the middle of the clip's lowest and highest note (C3 for an empty
/// clip) is in the middle of a grid `height` tall, counting the rows `rows` shows.
fn centre_notes(state: &mut PianoRollState, height: f32, clip: &PianoClip, rows: &KeyRows) {
    let (low, high) = clip.notes.iter().fold((u8::MAX, 0u8), |(lo, hi), n| (lo.min(n.key), hi.max(n.key)));
    let middle = if low <= high { rows.middle_row(low, high) } else { rows.middle_row(60, 60) };
    let height = height.max(1.0);
    state.scroll.y = middle * state.key_height + state.key_height / 2.0 - height / 2.0;
    state.centred_for = height;
}

/// The wheel scrolls, with Ctrl it zooms time and with Ctrl+Shift or Alt key height, around
/// the pointer; the middle button pans. While the right button is held over the grid, a wheel
/// step chooses the next or the previous tool instead, and the toolbar and the cursor follow.
fn scroll_and_zoom(ui: &Ui, state: &mut PianoRollState, area: Rect, grid: Rect, inputs: Inputs) {
    let (before, key_height) = (state.scroll.y, state.key_height);
    scroll_and_zoom_by_hand(ui, state, area, grid, inputs);
    if state.scroll.y != before || state.key_height != key_height {
        state.scrolled_by_hand = true;
    }
}

fn scroll_and_zoom_by_hand(ui: &Ui, state: &mut PianoRollState, area: Rect, grid: Rect, inputs: Inputs) {
    let pointer = widgets::pointer_over(ui, area);
    let (zoom, wheel) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta()));
    // The right button and the wheel: the next tool for a step one way, the previous for the
    // other. Held over the grid only, and the step scrolls and zooms nothing.
    // One step per wheel event, its own direction, not the smoothed delta: that goes on for a few
    // frames after the wheel stops and would scroll once the button is up.
    let wheel_steps: Vec<f32> = ui.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::MouseWheel { delta, .. } => Some(if delta.y == 0.0 { delta.x } else { delta.y }),
                _ => None,
            })
            .filter(|step| *step != 0.0)
            .collect()
    });
    if inputs.right && (state.wheel_tools || pointer.is_some_and(|p| grid.contains(p))) {
        state.wheel_tools = true;
        for step in wheel_steps {
            state.right_stepped = true;
            state.tool = step_tool(state.tool, if step > 0.0 { 1 } else { -1 });
        }
        if let Some(cursor) = cursor_for(state.tool) {
            ui.ctx().set_cursor_icon(cursor);
        }
        return;
    }
    if state.wheel_tools {
        // The button is up: the wheel scrolls again once its smoothing has run out.
        if wheel != Vec2::ZERO || zoom != 1.0 {
            return;
        }
        state.wheel_tools = false;
    }
    if let Some(pointer) = pointer {
        let zoom_keys = |state: &mut PianoRollState, factor: f32| {
            let key = (pointer.y - grid.top() + state.scroll.y) / state.key_height;
            state.key_height = (state.key_height * factor).clamp(KEY_HEIGHT_RANGE.0, KEY_HEIGHT_RANGE.1);
            state.scroll.y = key * state.key_height - (pointer.y - grid.top());
        };
        if inputs.alt && zoom == 1.0 {
            // Alt (Option on macOS) and the wheel; some systems turn it into a sideways scroll,
            // so either axis zooms.
            let steps = if wheel.y == 0.0 { wheel.x } else { wheel.y };
            if steps != 0.0 {
                zoom_keys(state, (steps * KEY_ZOOM_PER_POINT).exp());
            }
        } else if zoom != 1.0 {
            if inputs.shift {
                zoom_keys(state, zoom);
            } else {
                let beat = (pointer.x - grid.left() + state.scroll.x) / state.beat_width;
                state.beat_width = (state.beat_width * zoom).clamp(BEAT_WIDTH_RANGE.0, BEAT_WIDTH_RANGE.1);
                state.scroll.x = beat * state.beat_width - (pointer.x - grid.left());
            }
        } else {
            state.scroll -= wheel;
        }
    }
    let (middle_pressed, middle_down, delta) = ui.input(|i| {
        (
            i.pointer.button_pressed(PointerButton::Middle),
            i.pointer.button_down(PointerButton::Middle),
            i.pointer.delta(),
        )
    });
    if middle_pressed {
        state.panning = widgets::pointer_over(ui, grid).is_some();
    }
    if !middle_down {
        state.panning = false;
    }
    if state.panning {
        state.scroll -= delta;
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    }
}

/// A dashed 1 px outline: how a muted note is edged, so it reads as present but not playing.
fn dashed_outline(painter: &egui::Painter, rect: Rect, color: Color32) {
    if !(rect.width().is_finite() && rect.height().is_finite()) || rect.width() < 2.0 || rect.height() < 2.0 {
        return;
    }
    let (dash, gap) = (3.0, 2.0);
    let stroke = Stroke::new(1.0, color);
    let mut x = rect.left();
    while x < rect.right() {
        let end = (x + dash).min(rect.right());
        painter.line_segment([Pos2::new(x, rect.top() + 0.5), Pos2::new(end, rect.top() + 0.5)], stroke);
        painter.line_segment([Pos2::new(x, rect.bottom() - 0.5), Pos2::new(end, rect.bottom() - 0.5)], stroke);
        x = end + gap;
    }
    let mut y = rect.top();
    while y < rect.bottom() {
        let end = (y + dash).min(rect.bottom());
        painter.line_segment([Pos2::new(rect.left() + 0.5, y), Pos2::new(rect.left() + 0.5, end)], stroke);
        painter.line_segment([Pos2::new(rect.right() - 0.5, y), Pos2::new(rect.right() - 0.5, end)], stroke);
        y = end + gap;
    }
}

/// Draws the grid, the notes, the velocity lane, the key column and the ruler's bar numbers;
/// `played` is the part of the content the clip plays (› Ruler): the grid and the notes outside
/// it are shaded.
fn draw(
    ui: &Ui,
    layout: &Layout,
    clip: &PianoClip,
    state: &PianoRollState,
    grid: NoteGrid,
    preview: Option<Effect>,
    played: (f64, f64),
) {
    let painter = ui.painter_at(layout.keys.union(layout.ruler).union(layout.grid).union(layout.lane));
    let grid_painter = painter.with_clip_rect(layout.grid);
    let first_key = layout.key_at(layout.grid.bottom() - 0.5).unwrap_or(0);
    let last_key = layout.key_at(layout.grid.top() + 0.5).unwrap_or(127);

    // Rows: black keys darker, a stronger line under each C.
    painter.rect_filled(layout.grid, 0.0, theme::window_background());
    for key in (first_key..=last_key).filter(|k| layout.rows.shows(*k)) {
        let top = layout.y_of(key);
        let row = Rect::from_min_max(
            Pos2::new(layout.grid.left(), top),
            Pos2::new(layout.grid.right(), top + layout.key_height),
        );
        // With a scale, its keys are lighter; without one, the white keys are (› Scale and key).
        let fill = scale_view::row_fill(clip.scale, key, is_black(key));
        grid_painter.rect_filled(row, 0.0, fill);
        let line = if key % 12 == 0 { theme::border_strong() } else { theme::border() };
        grid_painter.hline(row.x_range(), row.bottom() - 0.5, theme::hairline(line));
    }

    // Vertical lines: the grid, bars stronger; the ruler's bar numbers.
    let first_beat = layout.beat_at(layout.grid.left()).max(0.0);
    let last_beat = layout.beat_at(layout.grid.right());
    let bar = clip.bar_beats.max(EPSILON);
    painter.rect_filled(layout.ruler, 0.0, theme::panel_background());
    let lane_painter = painter.with_clip_rect(layout.lane);
    let mut beat = (first_beat / grid.step).floor() * grid.step;
    let mut drawn = 0;
    while beat <= last_beat && drawn < MAX_LINES {
        let x = layout.x_of(beat).round() + 0.5;
        let on_bar = ((beat / bar) - (beat / bar).round()).abs() < 1e-6;
        let color = if on_bar { theme::border_strong() } else { theme::border() };
        grid_painter.vline(x, layout.grid.y_range(), theme::hairline(color));
        lane_painter.vline(x, layout.lane.y_range(), theme::hairline(color));
        beat += grid.step;
        drawn += 1;
    }
    let ruler_painter = painter.with_clip_rect(layout.ruler);
    let bar_points = (clip.bar_beats as f32 * layout.beat_width).max(1.0);
    let label_every = (36.0 / bar_points).ceil().max(1.0) as u64;
    let first_bar = (first_beat / bar).floor() as u64;
    let last_bar = (last_beat / bar).ceil().max(0.0) as u64;
    for n in (first_bar..=last_bar).filter(|n| n % label_every == 0).take(MAX_LINES) {
        let x = layout.x_of(n as f64 * bar).round() + 0.5;
        ruler_painter.vline(x, layout.ruler.y_range(), theme::hairline(theme::border_strong()));
        // The bar numbers sit under the ruler's loop row.
        let top = layout.ruler.top() + ruler_view::LOOP_ROW;
        let label = Rect::from_min_max(Pos2::new(x + 3.0, top), Pos2::new(x + 40.0, layout.ruler.bottom()));
        widgets::text_in(
            &ruler_painter,
            label,
            Align2::LEFT_CENTER,
            &(n + 1).to_string(),
            theme::mono_font(theme::TEXT_TINY),
            theme::text_value(),
        );
    }

    // Outside the part the clip plays (before its start marker or its loop, past its loop's end
    // or its end marker): shaded, and the notes there with it.
    let shade = Color32::from_black_alpha(90);
    let (played_start, played_end) = played;
    let end = layout.x_of(played_end);
    if end < layout.grid.right() {
        let past = Rect::from_min_max(Pos2::new(end.max(layout.grid.left()), layout.grid.top()), layout.grid.max);
        grid_painter.rect_filled(past, 0.0, shade);
    }
    let start = layout.x_of(played_start);
    if start > layout.grid.left() {
        let before =
            Rect::from_min_max(layout.grid.min, Pos2::new(start.min(layout.grid.right()), layout.grid.bottom()));
        grid_painter.rect_filled(before, 0.0, shade);
    }
    let outside =
        |note: &Note| note.time + note.duration <= played_start + EPSILON || note.time >= played_end - EPSILON;
    // Ghost notes of other clips, under the clip's own (› Ghost notes).
    ghost_view::paint(&grid_painter, &clip.ghosts, layout.grid, |g| layout.note_rect(&g.note()));

    // Notes, where a drag in progress puts them.
    let shown = |i: usize, note: &Note| -> Note {
        if let Some(Effect::VelocityLine { set, .. }) = &preview {
            return match set.iter().find(|(index, _)| *index == i) {
                Some((_, midi)) => Note { velocity: (midi.clamp(1.0, 127.0) / 127.0) as f32, ..*note },
                None => *note,
            };
        }
        if !state.selected.contains(&i) {
            return *note;
        }
        match preview {
            Some(Effect::Move(beats, keys)) => Note {
                time: note.time + beats,
                key: u8::try_from(i32::from(note.key) + keys).unwrap_or(note.key).min(127),
                ..*note
            },
            Some(Effect::Resize(by)) => Note { duration: (note.duration + by).max(MIN_LENGTH), ..*note },
            Some(Effect::ResizeStart(by)) => {
                Note { time: note.time + by, duration: (note.duration - by).max(MIN_LENGTH), ..*note }
            }
            Some(Effect::Velocity(by)) => {
                let midi = (f64::from(suhina_session::midi_velocity(note.velocity)) + by).clamp(1.0, 127.0);
                Note { velocity: (midi / 127.0) as f32, ..*note }
            }
            _ => *note,
        }
    };
    let selecting = match preview {
        Some(Effect::Select(rect)) => Some(rect),
        _ => None,
    };
    // The velocity line being drawn, over the lane.
    if let Some(Effect::VelocityLine { from, to, .. }) = &preview {
        lane_painter.line_segment([*from, *to], Stroke::new(1.0, theme::accent()));
    }
    // A copying drag (Ctrl): the selected notes stay where they are, their copies move.
    if matches!(state.gesture, Some(Gesture::Move { copy: true, .. })) && matches!(preview, Some(Effect::Move(..))) {
        for (_, note) in clip.notes.iter().enumerate().filter(|(i, _)| state.selected.contains(i)) {
            let rect = layout.note_rect(note);
            if rect.intersects(layout.grid) {
                let velocity = f32::from(suhina_session::midi_velocity(note.velocity));
                grid_painter.rect_filled(rect, 0.0, clip.color.gamma_multiply(0.4 + 0.6 * velocity / 127.0));
            }
        }
    }
    for (i, note) in clip.notes.iter().enumerate() {
        let note = shown(i, note);
        let rect = layout.note_rect(&note);
        if !rect.intersects(layout.grid) {
            continue;
        }
        let velocity = f32::from(suhina_session::midi_velocity(note.velocity));
        // A note an erase stroke has passed over is nearly gone; a muted note, and one a mute
        // stroke holds, is faded with a dashed edge, so it is clearly there and clearly not
        // playing (› Muting).
        let muted = note.muted || (state.muting.contains(&i) && state.mute_target);
        let unmuting = state.muting.contains(&i) && !state.mute_target;
        let alpha = if state.erasing.contains(&i) {
            0.12
        } else if muted {
            0.25
        } else {
            0.4 + 0.6 * velocity / 127.0
        };
        let alpha = if outside(&note) { alpha * 0.45 } else { alpha };
        grid_painter.rect_filled(rect, 0.0, clip.color.gamma_multiply(alpha));
        if muted {
            dashed_outline(&grid_painter, rect, theme::text_hint());
        }
        let selected = state.selected.contains(&i) || selecting.is_some_and(|r| r.intersects(rect));
        if selected {
            grid_painter.rect_stroke(rect, 0.0, Stroke::new(1.0, theme::text_primary()), StrokeKind::Inside);
        }
        if unmuting {
            // The stroke is bringing this note back: a solid edge says so before the button is up.
            grid_painter.rect_stroke(rect, 0.0, Stroke::new(1.0, theme::text_hint()), StrokeKind::Inside);
        }
        if rect.width() >= 28.0 && rect.height() >= 10.0 {
            widgets::text_in(
                &grid_painter,
                rect.shrink2(Vec2::new(3.0, 0.0)),
                Align2::LEFT_CENTER,
                &key_name(note.key),
                theme::mono_font(theme::TEXT_TINY),
                theme::window_background(),
            );
        }
        // Its velocity stem.
        let x = rect.left().round() + 0.5;
        let top = layout.lane.bottom() - 2.0 - velocity_height(note.velocity, layout.lane);
        let stem = if muted {
            theme::text_hint()
        } else if selected {
            theme::text_primary()
        } else {
            clip.color
        };
        lane_painter.vline(x, egui::Rangef::new(top, layout.lane.bottom()), Stroke::new(1.0, stem));
        lane_painter.rect_filled(Rect::from_min_size(Pos2::new(x - 2.5, top), Vec2::new(5.0, 2.0)), 0.0, stem);
    }
    if let Some(rect) = selecting {
        grid_painter.rect_stroke(rect, 0.0, theme::hairline(theme::text_secondary()), StrokeKind::Inside);
    }
    // Notes a draw or paint drag is making, outlined until they are added.
    let drawn = match &preview {
        Some(Effect::Draw(Some(note))) => vec![*note],
        _ => state.stroke.clone(),
    };
    for note in &drawn {
        let rect = layout.note_rect(note);
        grid_painter.rect_filled(rect, 0.0, clip.color.gamma_multiply(0.7));
        grid_painter.rect_stroke(rect, 0.0, Stroke::new(1.0, theme::text_primary()), StrokeKind::Inside);
    }
    // The cut line of a slice drag, and a mark on each note it would split.
    if let Some(Effect::Slice(at, cut)) = &preview {
        let x = layout.x_of(*at).round() + 0.5;
        grid_painter.vline(x, layout.grid.y_range(), Stroke::new(1.0, theme::accent()));
        for index in cut {
            if let Some(note) = clip.notes.get(*index) {
                let row = layout.note_rect(note);
                grid_painter.hline(row.x_range(), row.top() - 0.5, Stroke::new(1.0, theme::accent()));
            }
        }
    }
    painter.hline(layout.lane.x_range(), layout.lane.top() + 0.5, theme::hairline(theme::border_strong()));

    // The key column: keys, every C named.
    let key_painter = painter.with_clip_rect(layout.keys);
    key_painter.rect_filled(layout.keys, 0.0, theme::window_background());
    for key in (first_key..=last_key).filter(|k| layout.rows.shows(*k)) {
        let top = layout.y_of(key);
        let row = Rect::from_min_max(
            Pos2::new(layout.keys.left(), top),
            Pos2::new(layout.keys.right(), top + layout.key_height),
        );
        // The key under the pointer and the key that is playing are lit.
        let lit = state.hover_key() == Some(key) || state.playing_key() == Some(key);
        let fill = if lit {
            theme::control_hover()
        } else if is_black(key) {
            theme::window_background()
        } else {
            theme::control_background()
        };
        key_painter.rect_filled(row.shrink2(Vec2::new(0.0, 0.5)), 0.0, fill);
        scale_view::key_mark(&key_painter, clip.scale, key, row);
        // Folded, every row is named: the keys beside each other are no longer neighbours.
        if (key % 12 == 0 || layout.rows.folded()) && layout.key_height >= 8.0 {
            widgets::text_in(
                &key_painter,
                row.shrink2(Vec2::new(3.0, 0.0)),
                Align2::LEFT_CENTER,
                &key_name(key),
                theme::mono_font(theme::TEXT_TINY),
                theme::text_secondary(),
            );
        }
    }
    let corner = Rect::from_min_max(
        Pos2::new(layout.keys.left(), layout.ruler.top()),
        Pos2::new(layout.keys.right(), layout.ruler.bottom()),
    );
    painter.rect_filled(corner, 0.0, theme::panel_background());
    let lane_keys = Rect::from_min_max(
        Pos2::new(layout.keys.left(), layout.lane.top()),
        Pos2::new(layout.keys.right(), layout.lane.bottom()),
    );
    painter.rect_filled(lane_keys, 0.0, theme::panel_background());
    if lane_keys.height() >= 24.0 {
        let font = theme::mono_font(theme::TEXT_TINY);
        widgets::text_in(
            &painter,
            Rect::from_min_size(lane_keys.min, Vec2::new(KEYS - 3.0, 10.0)),
            Align2::RIGHT_CENTER,
            "127",
            font.clone(),
            theme::text_hint(),
        );
        widgets::text_in(
            &painter,
            Rect::from_min_max(
                Pos2::new(lane_keys.left(), lane_keys.bottom() - 10.0),
                Pos2::new(lane_keys.right() - 3.0, lane_keys.bottom()),
            ),
            Align2::RIGHT_CENTER,
            "1",
            font,
            theme::text_hint(),
        );
    }
    painter.vline(layout.keys.right() - 0.5, layout.keys.y_range(), theme::hairline(theme::border_strong()));
}

/// Names the piano roll's parts for screen readers and tests: the grid, each note in view and
/// its velocity stem.
fn for_readers(ui: &Ui, layout: &Layout, clip: &PianoClip, state: &PianoRollState) {
    let grid = ui.interact(layout.grid, Id::new("piano-roll-readers"), Sense::hover());
    let count = clip.notes.len();
    grid.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, format!("Piano roll, {count} notes")));
    ghost_view::for_readers(ui, &clip.ghosts, layout.grid, |g| layout.note_rect(&g.note()));
    for (i, note) in clip.notes.iter().enumerate() {
        let rect = layout.note_rect(note).intersect(layout.grid);
        if !rect.is_positive() {
            continue;
        }
        let selected = state.selected.contains(&i);
        // A muted note says so, so a reader and a test tell it from the others.
        let label = if note.muted {
            format!("Note {} at beat {} (muted)", key_name(note.key), round_beat(note.time))
        } else {
            format!("Note {} at beat {}", key_name(note.key), round_beat(note.time))
        };
        let response = ui.interact(rect, Id::new(("piano-roll-note", i)), Sense::hover());
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Other, true, selected, &label));
        let x = layout.x_of(note.time);
        let stem = Rect::from_min_max(Pos2::new(x - 2.0, layout.lane.top()), Pos2::new(x + 2.0, layout.lane.bottom()))
            .intersect(layout.lane);
        if stem.is_positive() {
            let velocity = suhina_session::midi_velocity(note.velocity);
            let label =
                format!("Velocity of note {} at beat {}: {velocity}", key_name(note.key), round_beat(note.time));
            let response = ui.interact(stem, Id::new(("piano-roll-stem", i)), Sense::hover());
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &label));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(time: f64, duration: f64, key: u8) -> Note {
        Note { time, duration, key, velocity: 100.0 / 127.0, release_velocity: 0.5, channel: 0, muted: false }
    }

    #[test]
    fn the_editor_spans_the_loop_or_the_clips_part_of_its_content() {
        assert_eq!(content_end(Some(4.0), 0.0, 48.0, 4.0), 4.0);
        assert_eq!(content_end(None, 2.0, 8.0, 16.0), 10.0);
        assert_eq!(content_end(None, 0.0, f64::NAN, 16.0), 16.0);
        assert_eq!(content_end(None, 0.0, 0.25, 16.0), 1.0);
        assert_eq!(content_end(None, 0.0, 1e300, 16.0), 120_000.0);
    }

    #[test]
    fn keys_are_named_with_60_as_c3() {
        assert_eq!(key_name(60), "C3");
        assert_eq!(key_name(0), "C-2");
        assert_eq!(key_name(127), "G8");
        assert_eq!(key_name(61), "C#3");
        assert!(is_black(61) && !is_black(60));
    }

    #[test]
    fn the_grid_follows_the_chosen_value_and_thins_out_when_lines_crowd() {
        assert_eq!(grid_step(Quantization::Beats(0.25), 4.0), 0.25);
        assert_eq!(grid_step(Quantization::Bars(1), 3.0), 3.0);
        assert_eq!(grid_step(Quantization::None, 4.0), 0.0);
        assert_eq!(NoteGrid::new(0.25, 40.0), NoteGrid { step: 0.25, snaps: true });
        // A sixteenth at 10 points a beat is 2.5 points: doubled twice to 10.
        assert_eq!(NoteGrid::new(0.25, 10.0).step, 1.0);
        assert_eq!(NoteGrid::new(0.0, 40.0), NoteGrid { step: 1.0, snaps: false });
        let grid = NoteGrid::new(0.25, 40.0);
        assert_eq!(grid.place(1.3, false, false), 1.25);
        assert_eq!(grid.place(1.3, false, true), 1.25);
        assert_eq!(grid.place(1.45, false, true), 1.25);
        assert_eq!(grid.place(1.3, true, false), 1.3);
    }

    #[test]
    fn a_move_goes_by_grid_steps_and_stops_at_the_limits() {
        let grid = NoteGrid::new(0.25, 40.0);
        let notes = [note(1.1, 1.0, 60), note(2.0, 1.0, 120)];
        // By whole steps, so an off-grid note stays as far off the grid.
        assert_eq!(move_delta(0.3, 0, &notes, grid, false), (0.25, 0));
        assert_eq!(move_delta(0.3, 0, &notes, grid, true), (0.3, 0));
        // Never before the start, never above key 127: the whole selection stops.
        assert_eq!(move_delta(-5.0, 0, &notes, grid, false), (-1.1, 0));
        assert_eq!(move_delta(0.0, 12, &notes, grid, false), (0.0, 7));
        assert_eq!(move_delta(0.0, -100, &notes, grid, false), (0.0, -60));
    }

    #[test]
    fn a_resize_puts_the_end_on_the_grid_and_keeps_notes_long_enough() {
        let grid = NoteGrid::new(0.25, 40.0);
        let grabbed = note(1.0, 1.0, 60);
        let other = note(3.0, 0.5, 62);
        assert_eq!(resize_by(0.3, &grabbed, &[grabbed, other], grid, false), 0.25);
        assert!((resize_by(0.3, &grabbed, &[grabbed], grid, true) - 0.3).abs() < 1e-12);
        // Dragged onto its start: one step long.
        assert_eq!(resize_by(-1.0, &grabbed, &[grabbed], grid, false), -0.75);
        // The other selected note is shorter: it keeps the shortest length.
        assert!((resize_by(-0.75, &grabbed, &[grabbed, other], grid, false) - (MIN_LENGTH - 0.5)).abs() < 1e-12);
    }

    #[test]
    fn the_playhead_is_found_in_the_clips_content_and_its_loop() {
        let plain = Placement { start: 8.0, length: 4.0, offset: 1.0, loop_range: None };
        assert_eq!(plain.content_beat(9.5), Some(2.5));
        assert_eq!(plain.content_beat(7.9), None);
        assert_eq!(plain.content_beat(12.0), None);
        // Looping beats 0 to 2 from offset 1: beat 2 of the content wraps to 0.
        let looped = Placement { start: 0.0, length: 8.0, offset: 1.0, loop_range: Some((0.0, 2.0)) };
        assert_eq!(looped.content_beat(0.5), Some(1.5));
        assert_eq!(looped.content_beat(1.5), Some(0.5));
        assert_eq!(looped.content_beat(f64::NAN), None);
    }

    #[test]
    fn an_arrangement_beat_is_the_inverse_of_the_content_beat() {
        let plain = Placement { start: 8.0, length: 4.0, offset: 1.0, loop_range: None };
        assert_eq!(plain.arrangement_beat(2.5), Some(9.5));
        // Before the clip's offset, and past its end, the clip plays nothing there.
        assert_eq!(plain.arrangement_beat(0.5), None);
        assert_eq!(plain.arrangement_beat(5.0), None);
        let looped = Placement { start: 0.0, length: 8.0, offset: 1.0, loop_range: Some((0.0, 2.0)) };
        assert_eq!(looped.arrangement_beat(1.5), Some(0.5));
        assert_eq!(looped.arrangement_beat(1.0), Some(0.0));
        // Beat 2 of the content wraps into the loop's start, which the clip never plays (it
        // starts at the offset).
        assert_eq!(looped.arrangement_beat(2.0), None);
        assert_eq!(looped.arrangement_beat(f64::NAN), None);
        // Round trip through both mappings where the clip plays.
        for at in [8.0, 9.5, 11.5] {
            let content = plain.content_beat(at);
            assert_eq!(content.and_then(|beat| plain.arrangement_beat(beat)), Some(at), "at {at}");
        }
        // The loop repeats, so a later pass comes back to the loop's start rather than to `at`.
        for at in [0.0, 0.5] {
            let content = looped.content_beat(at);
            assert_eq!(content.and_then(|beat| looped.arrangement_beat(beat)), Some(at), "at {at}");
        }
    }

    #[test]
    fn the_sixth_tool_is_mute_and_the_tools_stop_at_it() {
        assert_eq!(Tool::ALL.len(), 6);
        assert_eq!(Tool::ALL[5], (Tool::Mute, "Mute"));
        assert_eq!(tool_of("pianoRoll.tool6"), Some(Tool::Mute));
        assert_eq!(tool_of("pianoRoll.tool7"), None);
        // The wheel steps through the tools and stops at the last one, which is Mute.
        assert_eq!(step_tool(Tool::Slice, 1), Tool::Mute);
        assert_eq!(step_tool(Tool::Mute, 1), Tool::Mute);
        assert_eq!(step_tool(Tool::Select, -1), Tool::Select);
    }

    #[test]
    fn the_eraser_cursor_is_hidden_so_its_shape_can_be_drawn() {
        // The windowing layer has no eraser, so the pointer is hidden over the grid and the
        // Erase button's shape is drawn in its place.
        assert_eq!(cursor_for(Tool::Erase), Some(CursorIcon::None));
        assert_eq!(cursor_for(Tool::Mute), None);
        assert_eq!(cursor_for(Tool::Select), None);
        assert_eq!(cursor_for(Tool::Draw), Some(CursorIcon::Crosshair));
    }

    fn clip(notes: Vec<Note>) -> PianoClip {
        PianoClip {
            address: json!({ "track": 1, "index": 0 }),
            notes,
            length: 8.0,
            color: Color32::WHITE,
            bar_beats: 4.0,
            grid: 0.25,
            placement: Placement { start: 16.0, length: 8.0, offset: 0.0, loop_range: None },
            grid_menu: PianoGrid::default(),
            grid_follows: false,
            shape: ClipShape { offset: 0.0, length: 8.0, loop_range: None, room: None },
            ghosts: Ghosts::default(),
            scale: None,
        }
    }

    /// A layout 200 points tall at 10 points a row, scrolled to the top, with `rows`.
    fn folded_layout(rows: KeyRows) -> Layout {
        let grid = Rect::from_min_size(Pos2::new(36.0, 24.0), Vec2::new(400.0, 200.0));
        Layout {
            keys: grid,
            ruler: grid,
            grid,
            lane: grid,
            scroll: Vec2::ZERO,
            beat_width: 40.0,
            key_height: 10.0,
            rows,
        }
    }

    #[test]
    fn a_fold_maps_heights_to_the_shown_keys_and_back() {
        use suhina_engine::model::ScaleMode;
        let mut c = clip(vec![note(0.0, 1.0, 40), note(1.0, 1.0, 64), note(2.0, 1.0, 60)]);
        // Fold: three rows, highest first; every height in a row is its key and back.
        let layout = folded_layout(scale_view::rows(&c, Fold::Notes));
        assert_eq!(layout.rows.len(), 3);
        for (row, key) in [(0, 64), (1, 60), (2, 40)] {
            let y = 24.0 + row as f32 * 10.0;
            assert_eq!(layout.y_of(key), y);
            assert_eq!(layout.key_at(y + 5.0), Some(key));
        }
        assert_eq!(layout.key_at(24.0 + 35.0), None, "below the last row is no key");
        assert!(layout.y_of(62) < -1000.0, "a hidden key is far above the grid");
        // A note is hit and a double-click adds on the row's key.
        let state = PianoRollState::default();
        let grid = note_grid(&c, 40.0);
        assert_eq!(note_at(&layout, &c.notes, Pos2::new(36.0 + 100.0, 24.0 + 15.0)).map(|(i, _)| i), Some(2));
        let added = state.new_note(&layout, &c, grid, Pos2::new(36.0 + 140.0, 24.0 + 25.0), false).unwrap();
        assert_eq!(added.key, 40);
        // A drag up one row from 40 moves by the 20 semitones to 60.
        let moved = effect(
            &Gesture::Move { from: Pos2::new(60.0, 50.0), grabbed: 0, copy: false },
            Pos2::new(60.0, 40.0),
            &layout,
            &c,
            &c.notes[..1],
            grid,
            false,
        );
        assert!(matches!(moved, Effect::Move(_, 20)), "{moved:?}");
        // Fold to scale: C major's keys and every key with a note; without a scale, every key.
        assert_eq!(scale_view::rows(&c, Fold::Scale).len(), 128);
        c.scale = Scale::new(0, ScaleMode::Major);
        let rows = scale_view::rows(&c, Fold::Scale);
        assert!(rows.shows(60) && rows.shows(62) && !rows.shows(61));
        assert!(rows.shows(40), "E is in C major");
        c.notes.push(note(3.0, 1.0, 61));
        assert!(scale_view::rows(&c, Fold::Scale).shows(61), "a note outside the scale keeps its row");
        assert_eq!(scale_view::rows(&c, Fold::Off).len(), 128);
        // Unfolded, the rows are the keys as they always were.
        let layout = folded_layout(KeyRows::default());
        assert_eq!(layout.y_of(127), 24.0);
        assert_eq!(layout.key_at(24.0 + 67.0 * 10.0 + 1.0), Some(60));
    }

    #[test]
    fn z_fits_the_selection_and_shift_z_the_whole_clip() {
        let clip = clip(vec![note(2.0, 1.0, 60), note(3.0, 1.0, 62), note(6.0, 1.0, 64)]);
        let mut state = focused_on(&clip);
        state.view_width = 400.0;
        state.selected = BTreeSet::from([0, 1]);
        assert!(state.key_action("pianoRoll.zoomSelection", &clip, 0.0, None).is_none());
        // Beats 2 to 4 fill 400 points.
        assert_eq!((state.beat_width, state.scroll.x), (200.0, 400.0));
        assert!(state.key_action("pianoRoll.zoomClip", &clip, 0.0, None).is_none());
        assert_eq!((state.beat_width, state.scroll.x), (50.0, 0.0));
        // Nothing selected: Z fits the clip too.
        state.selected.clear();
        state.beat_width = 300.0;
        state.key_action("pianoRoll.zoomSelection", &clip, 0.0, None);
        assert_eq!(state.beat_width, 50.0);
        // A tiny selection stops at the zoom range.
        state.selected = BTreeSet::from([0]);
        state.view_width = 100_000.0;
        state.key_action("pianoRoll.zoomSelection", &clip, 0.0, None);
        assert_eq!(state.beat_width, BEAT_WIDTH_RANGE.1);
    }

    fn focused_on(clip: &PianoClip) -> PianoRollState {
        PianoRollState { shown: Some(clip.address.clone()), focused: true, ..PianoRollState::default() }
    }

    #[test]
    fn the_grid_keys_leave_a_new_grid_for_the_window_and_snap_off_snaps_nothing() {
        let mut piano = clip(Vec::new());
        let mut state = focused_on(&piano);
        assert_eq!(state.key_action("pianoRoll.gridWiden", &piano, 0.0, None), None);
        assert_eq!(state.grid_picked.take().map(|g| g.label()).as_deref(), Some("1/8"));
        state.key_action("pianoRoll.gridSnap", &piano, 0.0, None);
        let free = state.grid_picked.take().unwrap_or_default();
        assert!(!free.snap);
        piano.grid_menu = free;
        let grid = note_grid(&piano, 40.0);
        assert_eq!((grid.step, grid.snaps), (0.25, false), "the lines stay, nothing snaps");
        assert_eq!(grid.nearest_line(1.1), 1.1);
        // Following the control bar, a key whose grid it cannot hold does nothing.
        piano.grid_menu = PianoGrid::from_value("1/32");
        piano.grid_follows = true;
        state.key_action("pianoRoll.gridTriplet", &piano, 0.0, None);
        assert_eq!(state.grid_picked, None);
        state.key_action("pianoRoll.gridNarrow", &piano, 0.0, None);
        assert_eq!(state.grid_picked.map(|g| g.label()).as_deref(), Some("1/32"));
    }

    #[test]
    fn paste_goes_after_the_selection_else_at_the_playhead_inside_the_clip() {
        let clip = clip(vec![note(1.0, 0.5, 60), note(2.0, 1.0, 64)]);
        let mut state = focused_on(&clip);
        state.selected = BTreeSet::from([0]);
        let paste = state.key_action("selection.paste", &clip, 0.0, Some(2)).map(|a| a.params);
        assert_eq!(paste, Some(json!({ "track": 1, "index": 0, "time": 1.5 })));
        assert_eq!(state.selected, BTreeSet::from([2, 3]));
        // Nothing selected: the playhead at beat 19 of the arrangement is beat 3 of the clip.
        state.selected.clear();
        let paste = state.key_action("selection.paste", &clip, 19.0, Some(1)).map(|a| a.params);
        assert_eq!(paste, Some(json!({ "track": 1, "index": 0, "time": 3.0 })));
        // Outside the clip: no time, so the command puts it right after the copied notes.
        state.selected.clear();
        let paste = state.key_action("selection.paste", &clip, 40.0, Some(1)).map(|a| a.params);
        assert_eq!(paste, Some(json!({ "track": 1, "index": 0 })));
        // Nothing copied, or no keyboard: the key is left for the clip.
        assert!(state.key_action("selection.paste", &clip, 0.0, None).is_none());
        state.focused = false;
        assert!(state.key_action("selection.paste", &clip, 0.0, Some(1)).is_none());
    }

    #[test]
    fn keys_stop_at_the_limits_and_never_make_notes_shorter_than_a_step() {
        let clip = clip(vec![note(0.1, 0.25, 127), note(2.0, 1.0, 60)]);
        let mut state = focused_on(&clip);
        state.selected = BTreeSet::from([0]);
        let params = |a: Option<Action>| a.map(|a| a.params);
        // Earlier stops at the content's start; up past key 127 and an octave up do nothing.
        assert_eq!(
            params(state.key_action("pianoRoll.earlier", &clip, 0.0, None)).map(|p| p["beats"].clone()),
            Some(json!(-0.1))
        );
        assert!(state.key_action("pianoRoll.up", &clip, 0.0, None).is_none());
        assert!(state.key_action("pianoRoll.octaveUp", &clip, 0.0, None).is_none());
        assert!(state.key_action("pianoRoll.shorter", &clip, 0.0, None).is_none());
        state.selected = BTreeSet::from([1]);
        assert_eq!(
            params(state.key_action("pianoRoll.shorter", &clip, 0.0, None)).map(|p| p["by"].clone()),
            Some(json!(-0.25))
        );
        // Without a selection copy is left for the clip; Ctrl+A and Escape change only the view.
        state.selected.clear();
        assert!(state.key_action("selection.copy", &clip, 0.0, None).is_none());
        assert!(state.key_action("pianoRoll.selectAll", &clip, 0.0, None).is_none());
        assert_eq!(state.selected, BTreeSet::from([0, 1]));
        assert!(state.key_action("pianoRoll.deselect", &clip, 0.0, None).is_none());
        assert!(state.selected.is_empty());
    }
}
