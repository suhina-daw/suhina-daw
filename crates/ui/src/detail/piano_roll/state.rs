// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll's state: its tool, zoom and scroll, the selection, the gesture under way, and what it was given.

use super::*;

/// The furthest the piano roll reaches, in beats: the engine's furthest start.
pub(super) const BEAT_LIMIT: f32 = 120_000.0;
/// How far Alt and an arrow key moves or resizes notes, in beats: a 1/128 note, finer than any grid.
const FINE_STEP: f64 = 0.03125;

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
pub(super) enum Gesture {
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
    /// The selection follows the keys from `anchor` to the pointer in the key column; `silent` (Ctrl
    /// held when it began) plays nothing.
    KeySelect { anchor: u8, silent: bool },
}

/// What the piano roll keeps between frames.
#[derive(Debug, Clone, PartialEq)]
pub struct PianoRollState {
    /// The clip shown last, as its address: another clip resets the view.
    pub(super) shown: Option<Value>,
    /// Points scrolled from beat 0 and from the top of the highest key.
    pub scroll: Vec2,
    pub beat_width: f32,
    /// The furthest beat a zoom at the pointer has shown at the right edge, so the time under
    /// the pointer stays in place past the clip's end (#910).
    pub(crate) zoom_reach: f64,
    pub key_height: f32,
    /// Indices of the selected notes.
    pub selected: BTreeSet<usize>,
    /// The selected notes as the last frame left them, each index with its note's id, so the
    /// selection follows its notes when another writer (an agent, a script, undo) adds, deletes or
    /// moves notes between frames (#1182); `None` when the last frame's own edit set the selection
    /// for the notes as they are after it, by index.
    pub(super) selected_ids: Option<Vec<(usize, NoteId)>>,
    /// What is selected in time, the notes that start in it, and what the copies in a row left
    /// behind, for an undo to come back to (› Selecting).
    pub(super) span: Option<TimeSpan>,
    pub(super) span_notes: BTreeSet<usize>,
    pub(super) span_undo: Vec<SpanStep>,
    /// An undo ran since the selection was last settled.
    pub(super) undone: bool,
    /// Where a click on an empty spot left the insert mark: where a paste goes, in beats.
    pub(super) insert_at: Option<f64>,
    /// The last press was in the piano roll, so Delete deletes notes there.
    pub focused: bool,
    /// A key this frame selected notes its command is still to add (paste, duplicate): they
    /// are kept selected until the command has run.
    pub(super) selecting_new: bool,
    pub(super) gesture: Option<Gesture>,
    pub(super) panning: bool,
    /// The right button and the wheel are choosing tools: the wheel scrolls nothing until the
    /// button is up and the wheel's smoothing has run out.
    pub(super) wheel_tools: bool,
    /// The settings quantise uses, and whether the quantise block in the MIDI tools column is open.
    pub quantize: QuantizeSettings,
    pub quantize_open: bool,
    /// The MIDI tools column's own settings, kept until Suhina closes, and whether the column is
    /// shown at all (the toolbar's three-slider button toggles it; window state, not a command).
    pub tools: ToolSettings,
    pub tools_open: bool,
    /// The toolbar's tool.
    pub tool: Tool,
    /// Notes placed over others of their key replace them: Allow stacked notes off (Settings; the
    /// window sets it each frame).
    pub notes_replace: bool,
    /// How the keys look and which of them are named (Settings; the window sets them each frame).
    pub key_style: crate::settings::PianoKeyStyle,
    pub key_names: crate::settings::PianoKeyNames,
    /// A choice made in the corner's menu this frame, for the window to keep.
    pub view_picked: Option<ViewPick>,
    /// The range of keys a drag in the key column is choosing, and what the selection was when the
    /// drag began.
    pub(super) key_live: Option<(u8, u8)>,
    pub(super) key_base: BTreeSet<usize>,
    /// A drag in the scale strip.
    pub(super) strip_drag: Option<StripDrag>,
    /// The pointer is where a drag zooms (the strip, the ruler) this frame.
    pub(crate) magnifier: bool,
    /// The length (0: one grid step) and velocity a new note takes: those of the note last
    /// drawn or clicked.
    pub(super) last_length: f64,
    pub(super) last_velocity: u8,
    /// The notes a paint stroke has painted so far, and the notes an erase stroke has passed over.
    pub(super) stroke: Vec<Note>,
    pub(super) erasing: BTreeSet<usize>,
    /// The notes a mute stroke has passed over; they are drawn faded until the stroke ends, and
    /// `mute_target` is the state the stroke gives them.
    pub(super) muting: BTreeSet<usize>,
    pub(super) mute_target: bool,
    /// The right button is playing the clip from the point under the pointer (the Mouse-first
    /// switch): letting it go stops the transport.
    pub(super) right_playing: bool,
    /// The wheel stepped the tools while the right button was held: letting it go over a note
    /// then deletes nothing (› Zoom and scroll).
    pub(super) right_stepped: bool,
    /// A grid value picked in the toolbar's grid menu or with its keys this frame, for the window
    /// to keep.
    pub grid_picked: Option<PianoGrid>,
    /// The clip of a ghost note double-clicked with the right button this frame, for the window to
    /// open (› Ghost notes).
    pub open_ghost: Option<ClipRef>,
    /// The strip's Focus switch: the focus stays on the clip picked first (› Several clips picked).
    pub focus_first: bool,
    /// The toolbar's Hide editor button was clicked this frame: the window hides the editor.
    pub hide_requested: bool,
    /// The key rows were scrolled by hand (the wheel, a pan, zooming the key height) since the
    /// clip was shown: until then a change in the editor's height centres the notes again.
    pub(super) scrolled_by_hand: bool,
    /// The grid's height the notes were last centred for.
    pub(super) centred_for: f32,
    /// The key the key column is playing, if any, and where the pointer last was on it: the
    /// window turns a change into `preview.noteOn` and `preview.noteOff`.
    pub(super) playing_key: Option<u8>,
    pub(super) hover_key: Option<u8>,
    /// When and where the last click in the grid was, while a second click could still make it a
    /// double-click; the piano roll counts its own clicks (› Double-click).
    pub(super) last_click: Option<(f64, Pos2)>,
    /// The index the note the last click added will have, so the second click of a double-click
    /// selects that note rather than deleting it.
    pub(super) click_added: Option<usize>,
    /// A workflow mode or setting picked in the toolbar's menu this frame, for the window to keep
    /// in the keymap.
    pub workflow_picked: Option<crate::settings::workflow::Pick>,
    /// A drag on the ruler in progress.
    pub(crate) ruler: RulerState,
    /// How wide the note grid was last drawn, for the zoom keys (Z, Shift+Z).
    pub(super) view_width: f32,
    /// The velocity lane's height dragged by its top edge, kept until Suhina closes; `None`: the
    /// default height (› Layout).
    pub(super) lane_wanted: Option<f32>,
    /// The toolbar's Audition button: drawn and moved notes sound once.
    pub audition: bool,
    /// Keys to sound at the end of this frame, and the track and time the sounding ones go off.
    pub(super) audition_pending: Vec<u8>,
    pub(super) audition_off: Option<(u64, f64)>,
    pub(super) audition_sounding: Vec<u8>,
    /// The notes the selecting rectangle is on, so only those it has just reached sound.
    pub(super) swept: BTreeSet<usize>,
    /// A press in the grid holds what it sounds until it is let go.
    pub(super) audition_press: bool,
    /// The modifiers held when the button went down in the grid: a drag that starts from there
    /// is chosen by them, not by keys pressed before the pointer moved far enough (#1048).
    pub(super) pressed_with: Option<egui::Modifiers>,
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
            zoom_reach: 0.0,
            key_height: DEFAULT_KEY_HEIGHT,
            selected: BTreeSet::new(),
            selected_ids: None,
            span: None,
            span_notes: BTreeSet::new(),
            span_undo: Vec::new(),
            undone: false,
            insert_at: None,
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
            key_style: crate::settings::PianoKeyStyle::default(),
            key_names: crate::settings::PianoKeyNames::default(),
            view_picked: None,
            key_live: None,
            key_base: BTreeSet::new(),
            strip_drag: None,
            magnifier: false,
            last_length: 0.0,
            last_velocity: NEW_VELOCITY,
            stroke: Vec::new(),
            erasing: BTreeSet::new(),
            muting: BTreeSet::new(),
            mute_target: true,
            right_playing: false,
            right_stepped: false,
            grid_picked: None,
            open_ghost: None,
            focus_first: false,
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
            swept: BTreeSet::new(),
            audition_press: false,
            pressed_with: None,
            ghost_source: GhostSource::default(),
            fold: Fold::default(),
        }
    }
}

impl PianoRollState {
    /// Puts the selection back on the notes it was on when another writer's edit since the last
    /// frame moved them to other indices (#1182): a note that is gone leaves it. A selection the
    /// window changed since (a key, a menu) is taken as it is.
    pub(super) fn follow_selection(&mut self, notes: &Notes) {
        let Some(last) = self.selected_ids.take() else { return };
        let unchanged = last.len() == self.selected.len() && self.selected.iter().zip(&last).all(|(i, (j, _))| i == j);
        if !unchanged || last.iter().all(|&(i, id)| notes.id(i) == Some(id)) {
            return;
        }
        let at: std::collections::HashMap<NoteId, usize> =
            notes.entries().enumerate().map(|(i, (id, _))| (id, i)).collect();
        self.selected = last.iter().filter_map(|(_, id)| at.get(id).copied()).collect();
    }

    /// Records which notes the selection is on, each index with its note's id, for
    /// [`Self::follow_selection`] in the next frame; `by_index` when this frame's own edit chose the
    /// indices the notes will have after it.
    pub(super) fn remember_selection(&mut self, notes: &Notes, by_index: bool) {
        self.selected_ids = if by_index {
            None
        } else {
            self.selected.iter().map(|&i| Some((i, notes.id(i)?))).collect::<Option<Vec<_>>>()
        };
    }

    /// What the toolbar's Cut, Copy, Paste and Duplicate buttons do: the same as Ctrl+X, Ctrl+C,
    /// Ctrl+V and the duplicate key, on the selected notes (› Copy, cut, paste and duplicate).
    /// `None` when there is nothing to act on: no note selected, or nothing copied for Paste.
    pub(super) fn clipboard_action(
        &mut self,
        button: IconButton,
        clip: &PianoClip,
        copied: Option<usize>,
    ) -> Option<Action> {
        let action = match button {
            IconButton::Cut if !self.acting().is_empty() => "selection.cut",
            IconButton::Copy if !self.acting().is_empty() => "selection.copy",
            IconButton::Paste if copied.is_some_and(|n| n > 0) => "selection.paste",
            IconButton::Duplicate if !self.acting().is_empty() => "selection.duplicate",
            _ => return None,
        };
        // What the keys run: the same command with the same params.
        self.key_action(action, clip, 0.0, copied)
    }

    /// `note.delete` for the selected notes, when the piano roll has the keyboard and notes are
    /// selected; the selection is then cleared.
    pub fn delete_selected(&mut self) -> Option<Action> {
        if !self.focused || self.acting().is_empty() {
            return None;
        }
        let mut params = self.shown.clone()?;
        params["notes"] = json!(self.acting().iter().collect::<Vec<_>>());
        self.selected.clear();
        self.span_notes.clear();
        Some(Action::new("note.delete", params))
    }

    /// `note.quantize` for the selected notes, or every note when none is selected, with the
    /// last settings; `None` when no clip is shown.
    pub fn quantize_selected(&self) -> Option<Action> {
        let mut params = self.shown.clone()?;
        if self.acting().is_empty() && self.span.is_some() {
            return None;
        }
        if !self.acting().is_empty() {
            params["notes"] = json!(self.acting().iter().collect::<Vec<_>>());
        }
        params["grid"] = json!(self.quantize.grid);
        params["amount"] = json!(self.quantize.amount);
        params["ends"] = json!(self.quantize.ends);
        self.replacing(&mut params);
        Some(Action::new("note.quantize", params))
    }

    /// What a key does to the piano roll showing `clip`, when it has the keyboard: the selection
    /// actions change the selection here, the others become a `note.*` command. `playhead` is the
    /// arrangement's playhead in beats, `copied` how many notes `note.paste` would add. `None`
    /// when the key does nothing here; the window keeps such a key from any other view.
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
        if !self.selecting_new {
            self.settle_selection(&clip.notes);
        }
        let acting = self.acting().clone();
        let chosen: Vec<Note> = acting.iter().filter_map(|i| clip.notes.get(*i).copied()).collect();
        let count = clip.notes.len();
        let mut params = clip.address.clone();
        if action == "selection.paste" {
            // In the stretch after a time selection, as far in as the first note was; after the
            // selection; at the insert mark; at the playhead when it is inside the clip; else where
            // the command puts it (right after the copied notes).
            let copied = copied?;
            let first = chosen.iter().map(|n| n.time).fold(f64::INFINITY, f64::min);
            let end = chosen.iter().map(|n| n.time + n.duration).fold(f64::NEG_INFINITY, f64::max);
            let time = match &self.span {
                Some(span) => Some(span.end + if first.is_finite() { first - span.start } else { 0.0 }),
                None if end.is_finite() => Some(end),
                None => self.insert_at.or_else(|| clip.placement.content_beat(playhead)),
            };
            if let Some(time) = time {
                params["time"] = json!(round_beat(time));
            }
            self.select_copies((count..count.saturating_add(copied)).collect(), &clip.notes);
            self.replacing(&mut params);
            return Some(Action::new("note.paste", params));
        }
        match action {
            "pianoRoll.selectAll" => {
                self.selected = (0..count).collect();
                return None;
            }
            "pianoRoll.deselect" => {
                self.selected.clear();
                self.clear_span();
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
        params["notes"] = json!(acting.iter().collect::<Vec<_>>());
        let grid = note_grid(clip, self.beat_width);
        // Each of these says how far it goes, on the grid or off it.
        let free =
            matches!(action, "pianoRoll.earlier" | "pianoRoll.later" | "pianoRoll.earlierFine" | "pianoRoll.laterFine");
        let moves = |beats: f64, keys: i32| {
            let (beats, moved) = move_delta(beats, keys, &chosen, grid, free);
            // A pitch move that does not fit leaves the pitch alone, so no note changes its name.
            (beats, if moved == keys { keys } else { 0 })
        };
        let (id, beats, keys) = match action {
            "selection.copy" => return Some(Action::new("note.copy", params)),
            "selection.cut" => {
                self.selected.clear();
                self.span_notes.clear();
                return Some(Action::new("note.cut", params));
            }
            "selection.duplicate" => {
                // A time selection's copies go by its length, so its spacing to the next one stays;
                // the selected notes' go right after them, the command's own place.
                if let Some(span) = &self.span {
                    params["beats"] = json!(round_beat(span.end - span.start));
                }
                self.select_copies((count..count.saturating_add(chosen.len())).collect(), &clip.notes);
                self.replacing(&mut params);
                return Some(Action::new("note.duplicate", params));
            }
            "pianoRoll.shorter" | "pianoRoll.longer" | "pianoRoll.shorterFine" | "pianoRoll.longerFine" => {
                // To the next or the previous grid line, or a fine step on, per note: one command,
                // one undo step.
                let longer = matches!(action, "pianoRoll.longer" | "pianoRoll.longerFine");
                let fine = matches!(action, "pianoRoll.shorterFine" | "pianoRoll.longerFine");
                let grid = if fine { TimelineGrid { step: FINE_STEP, snaps: false, ..grid } } else { grid };
                let mut named = Vec::new();
                let mut lengths = Vec::new();
                for &index in &acting {
                    let Some(note) = clip.notes.get(index) else { continue };
                    let end = key_step_end(note, &clip.notes, grid, longer, self.notes_replace);
                    if (end - note.time - note.duration).abs() > EPSILON {
                        named.push(index);
                        lengths.push(round_beat(end - note.time));
                    }
                }
                if named.is_empty() {
                    return None;
                }
                params["notes"] = json!(named);
                params["lengths"] = json!(lengths);
                self.replacing(&mut params);
                return Some(Action::new("note.resize", params));
            }
            "pianoRoll.earlier" => ("note.move", moves(key_step(&chosen, grid, true), 0).0, 0),
            "pianoRoll.later" => ("note.move", moves(key_step(&chosen, grid, false), 0).0, 0),
            "pianoRoll.earlierFine" => ("note.move", moves(-FINE_STEP, 0).0, 0),
            "pianoRoll.laterFine" => ("note.move", moves(FINE_STEP, 0).0, 0),
            "pianoRoll.up" => ("note.move", 0.0, moves(0.0, 1).1),
            "pianoRoll.down" => ("note.move", 0.0, moves(0.0, -1).1),
            "pianoRoll.octaveUp" => ("note.move", 0.0, moves(0.0, 12).1),
            "pianoRoll.octaveDown" => ("note.move", 0.0, moves(0.0, -12).1),
            _ => return None,
        };
        if beats == 0.0 && keys == 0 {
            return None;
        }
        // Past the furthest start the command would refuse it, and nothing may have moved.
        if chosen.iter().any(|n| n.time + beats > f64::from(BEAT_LIMIT)) {
            return None;
        }
        self.audition_moved(&chosen, keys);
        self.span_moved(beats);
        params["beats"] = json!(round_beat(beats));
        params["semitones"] = json!(keys);
        self.replacing(&mut params);
        Some(Action::new(id, params))
    }

    /// Zooms and scrolls time so beats `start` to `end` fill the note grid's width (as far as the
    /// zoom range allows), keys untouched.
    pub(super) fn zoom_to(&mut self, start: f64, end: f64) {
        let width = self.view_width.max(1.0);
        let span = (end - start).max(EPSILON) as f32;
        self.beat_width = clamp_beat_width(width / span, &PIANO_ROLL_BEAT_WIDTHS);
        self.scroll.x = (start.max(0.0) as f32 * self.beat_width).max(0.0);
    }

    /// Forgets the clip, so the next one shown is fitted to the view again.
    pub fn hide(&mut self) {
        self.shown = None;
        self.selected.clear();
        self.clear_span();
        self.gesture = None;
        self.focused = false;
        self.quantize_open = false;
        self.selecting_new = false;
        self.stroke.clear();
        self.erasing.clear();
        self.audition_pending.clear();
        self.key_live = None;
    }

    /// Keeps `note`'s length and velocity for the next note drawn, and for the key column's
    /// preview.
    pub(super) fn remember(&mut self, note: &Note) {
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
    pub(super) fn new_note(
        &self,
        layout: &Layout,
        clip: &PianoClip,
        grid: TimelineGrid,
        pos: Pos2,
        free: bool,
    ) -> Option<Note> {
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

/// Where a grid key leaves the end of `note`. Longer: the next grid line, or the start of the next
/// note of its key when that comes first and `stops` says it may not be run over; a note already
/// touching it stays. Shorter: the previous grid line, never less than one step from its start. Off
/// the grid, a whole step either way.
pub(super) fn key_step_end(note: &Note, notes: &[Note], grid: TimelineGrid, longer: bool, stops: bool) -> f64 {
    let end = note.time + note.duration;
    if longer {
        let line = if grid.snaps { grid.next_line(end) } else { end + grid.step };
        if !stops {
            return line;
        }
        let next = notes
            .iter()
            .filter(|n| n.key == note.key && n.time > note.time + EPSILON)
            .map(|n| n.time)
            .fold(f64::INFINITY, f64::min);
        if next <= end + EPSILON { end } else { line.min(next) }
    } else {
        let line = if grid.snaps { grid.previous_line(end) } else { end - grid.step };
        line.max(note.time + grid.step).min(end)
    }
}

/// The `note.add` params for `notes`.
pub(super) fn add_params(clip: &PianoClip, notes: &[Note]) -> Value {
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
pub(super) fn tool_of(action: &str) -> Option<Tool> {
    let index = action.strip_prefix("pianoRoll.tool")?;
    let index: usize = index.parse().ok()?;
    index.checked_sub(1).and_then(|i| Tool::ALL.get(i)).map(|(tool, _)| *tool)
}

/// What the window tells the piano roll about the settings that change what it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Options {
    /// How large the toolbar is drawn: Settings › Clip editor › Piano roll › Piano roll toolbar size.
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
    /// Where playback starts next, as an arrangement beat, for an arrangement clip: shown in the
    /// piano roll, which a click on its ruler moves (#951). `None` for a slot's clip.
    pub start_marker: Option<f64>,
    /// The clip is on the arrangement: a click on the ruler sets where playback starts (#951).
    pub seeks: bool,
}
