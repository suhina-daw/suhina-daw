// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Ghost notes: the notes of other clips, drawn faintly behind the edited clip's own notes in the
//! piano roll so a part can be written against another (`docs/spec/ui-clip-editor.md` › Ghost
//! notes). Which clips are shown is window state, chosen in the toolbar's ghost menu: not a
//! command, not saved with the project. Ghost notes are never hit-tested.
//!
//! The time mapping: the edited clip's **first pass** (from its start until its content first
//! reaches its loop's end, or its whole length when it does not loop) is a window of time; every
//! other clip is laid out in time through its own start, offset and loop, and the part of each of
//! its notes that sounds inside the window is mapped into the edited clip's content beats.

use egui::{Color32, Id, Painter, Rect, Sense, Ui};
use suhina_engine::model::{Clip, LaneData, Note, Project, TrackId};

use crate::detail::detail_view::ClipRef;
use crate::detail::piano_roll::{Placement, key_name};
use crate::interaction::grid::default_track_colour;
use crate::kit::theme;

/// The most ghost notes one clip shows, so a big project never draws without end.
pub const MAX_GHOSTS: usize = 20_000;
/// The most notes looked at for one clip's passes round its loop, so a tiny loop of many muted
/// notes cannot take long either.
const MAX_WORK: usize = 1_000_000;
/// The most clips the menu's Clip list offers.
pub const MAX_CHOICES: usize = 500;
/// The most ghost notes given a label for readers and tests in one frame.
const MAX_LABELS: usize = 512;
/// How opaque a ghost note is drawn.
pub const GHOST_ALPHA: f32 = 0.25;
/// How wide the toolbar's ghost menu button is, in points.
pub const MENU_WIDTH: f32 = 64.0;

/// Which other clips the piano roll shows as ghost notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GhostSource {
    /// No ghost notes.
    None,
    /// The clips that play at the same time as the edited clip: on the arrangement, every other
    /// clip that overlaps it; in a slot, the other slots of the same scene.
    #[default]
    Together,
    /// The other clips of the edited clip's track (its lane, or its column of slots), each as
    /// if it started with the edited clip.
    Track,
    /// One clip the user picked, on any track, as if it started with the edited clip.
    Clip(ClipRef),
}

impl GhostSource {
    /// The name the menu shows and its accessible label carries.
    pub fn name(self, ghosts: &Ghosts) -> String {
        match self {
            Self::None => "None".into(),
            Self::Together => "Playing together".into(),
            Self::Track => "This track".into(),
            Self::Clip(clip) => {
                ghosts.choices.iter().find(|c| c.clip == clip).map_or_else(|| "Clip".to_string(), |c| c.name.clone())
            }
        }
    }
}

/// One ghost note, in the edited clip's content beats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GhostNote {
    pub start: f64,
    pub length: f64,
    pub key: u8,
    /// Its track's colour, drawn at [`GHOST_ALPHA`].
    pub color: Color32,
    /// Its track's name, as an index into [`Ghosts::sources`].
    pub source: usize,
}

impl GhostNote {
    /// The ghost as a note, to be placed on screen like one.
    pub fn note(&self) -> Note {
        Note {
            time: self.start,
            duration: self.length,
            key: self.key,
            velocity: 0.0,
            release_velocity: 0.0,
            channel: 0,
            muted: false,
        }
    }
}

/// A clip the menu's Clip list offers: where it is, and its name with its track and place.
#[derive(Debug, Clone, PartialEq)]
pub struct GhostChoice {
    pub clip: ClipRef,
    pub name: String,
}

/// What the piano roll shows behind the edited clip's notes, and the clips its menu offers.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Ghosts {
    pub notes: Vec<GhostNote>,
    /// The names of the tracks the ghost notes come from.
    pub sources: Vec<String>,
    pub choices: Vec<GhostChoice>,
}

/// The ghost notes `source` gives the edited clip `edited`, and the clips the menu offers.
pub fn ghosts(project: &Project, edited: ClipRef, source: GhostSource) -> Ghosts {
    let mut ghosts = Ghosts { choices: choices(project, edited), ..Ghosts::default() };
    let Some(edited_clip) = clip_at(project, edited) else {
        return ghosts;
    };
    let mine = placement(edited, edited_clip);
    let Some(window) = first_pass(&mine) else {
        return ghosts;
    };
    // The clips shown, each with the time it is laid out in.
    let mut shown: Vec<(TrackId, &Clip, Placement)> = Vec::new();
    let mut add = |clip: ClipRef, rebase: bool| {
        if clip == edited {
            return;
        }
        if let Some(other) = clip_at(project, clip) {
            let mut at = placement(clip, other);
            if rebase {
                at.start = mine.start;
            }
            shown.push((clip.track(), other, at));
        }
    };
    match (source, edited) {
        (GhostSource::None, _) => {}
        (GhostSource::Together, ClipRef::Arrangement(..)) => {
            for lane in &project.arrangement.lanes {
                let Some(track) = lane.track else { continue };
                for clip in &lane.clips {
                    if clip.enabled && overlaps(clip, window) {
                        add(ClipRef::Arrangement(track, clip.id), false);
                    }
                }
            }
        }
        (GhostSource::Together, ClipRef::Slot(_, scene)) => {
            for column in &project.launcher.columns {
                if column.slots.get(scene).and_then(|s| s.clip.as_ref()).is_some_and(|c| c.enabled) {
                    add(ClipRef::Slot(column.track, scene), false);
                }
            }
        }
        (GhostSource::Track, ClipRef::Arrangement(track, _)) => {
            let lane = project.arrangement.lanes.iter().find(|l| l.track == Some(track));
            for clip in lane.map_or(&[][..], |l| &l.clips) {
                add(ClipRef::Arrangement(track, clip.id), true);
            }
        }
        (GhostSource::Track, ClipRef::Slot(track, _)) => {
            let column = project.launcher.columns.iter().find(|c| c.track == track);
            for scene in 0..column.map_or(0, |c| c.slots.len()) {
                add(ClipRef::Slot(track, scene), true);
            }
        }
        (GhostSource::Clip(clip), _) => add(clip, true),
    }
    for (track, clip, at) in shown {
        if ghosts.notes.len() >= MAX_GHOSTS {
            break;
        }
        let Some(notes) = notes_of(project, track, clip) else { continue };
        let Some(position) = project.tracks.iter().position(|t| t.id == track) else { continue };
        let name = project.tracks.get(position).map_or_else(String::new, |t| t.name.clone());
        let rgb = project.tracks.get(position).and_then(|t| t.color).unwrap_or_else(|| default_track_colour(position));
        let color = Color32::from_rgb(rgb.0, rgb.1, rgb.2);
        let source = ghosts.sources.iter().position(|s| *s == name).unwrap_or(ghosts.sources.len());
        let before = ghosts.notes.len();
        map_notes(&at, notes, window, &mut ghosts.notes, |start, length, key| GhostNote {
            start,
            length,
            key,
            color,
            source,
        });
        // A track is named only when it gave a ghost note.
        if source == ghosts.sources.len() && ghosts.notes.len() > before {
            ghosts.sources.push(name);
        }
    }
    ghosts
}

/// The part of the edited clip's time its ghost notes are mapped into: from `start` to `end` in
/// time, playing content beat `content` at `start`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    pub start: f64,
    pub end: f64,
    pub content: f64,
}

/// The edited clip's first pass: from its start until its content first reaches its loop's end,
/// or its whole length when it does not loop. `None` for a clip that plays nothing.
pub fn first_pass(clip: &Placement) -> Option<Window> {
    let (content, length) = match valid_loop(clip) {
        Some((start, end)) => {
            let first = first_beat(clip.offset, start, end);
            (first, clip.length.min(end - first))
        }
        None => (clip.offset, clip.length),
    };
    let end = clip.start + length;
    (length > 0.0 && end.is_finite() && content.is_finite() && clip.start.is_finite()).then_some(Window {
        start: clip.start,
        end,
        content,
    })
}

/// Maps the notes of a clip laid out at `clip` into `window`: every part of a note that sounds
/// inside the window becomes one ghost note in the window's content beats, made by `make` from
/// its start, length and key. Stops at [`MAX_GHOSTS`] notes in `out`.
pub fn map_notes<T>(
    clip: &Placement,
    notes: &[Note],
    window: Window,
    out: &mut Vec<T>,
    make: impl Fn(f64, f64, u8) -> T,
) {
    let clip_end = clip.start + clip.length;
    let from = window.start.max(clip.start);
    let to = window.end.min(clip_end);
    if to.partial_cmp(&from) != Some(std::cmp::Ordering::Greater) || notes.is_empty() {
        return;
    }
    // Each pass: when it starts in time, and the content beats it plays.
    let emit = |at_start: f64, content_start: f64, content_end: f64, out: &mut Vec<T>| {
        let pass_end = (at_start + (content_end - content_start)).min(clip_end);
        for note in notes {
            if out.len() >= MAX_GHOSTS {
                return;
            }
            if note.muted || note.time < content_start || note.time >= content_end {
                continue;
            }
            let at = at_start + (note.time - content_start);
            let end = (at + note.duration.max(0.0)).min(pass_end);
            let (start, end) = (at.max(from), end.min(to));
            if end > start && start.is_finite() && end.is_finite() {
                out.push(make(window.content + (start - window.start), end - start, note.key));
            }
        }
    };
    match valid_loop(clip) {
        None => emit(clip.start, clip.offset, clip.offset + clip.length, out),
        Some((start, end)) => {
            let first = first_beat(clip.offset, start, end);
            let first_end = clip.start + (end - first);
            if first_end > from {
                emit(clip.start, first, end, out);
            }
            // The passes round the loop after the first: only those inside the window.
            let period = end - start;
            let skip = if from > first_end { ((from - first_end) / period).floor() } else { 0.0 };
            let mut pass = skip;
            let mut count = 0;
            loop {
                let at = first_end + pass * period;
                if at.partial_cmp(&to) != Some(std::cmp::Ordering::Less) || count >= MAX_WORK || out.len() >= MAX_GHOSTS
                {
                    break;
                }
                emit(at, start, end, out);
                pass += 1.0;
                count += notes.len();
            }
        }
    }
}

/// The clip's loop when it has one that can be played: finite, its end after its start.
fn valid_loop(clip: &Placement) -> Option<(f64, f64)> {
    clip.loop_range.filter(|(start, end)| start.is_finite() && end.is_finite() && end > start)
}

/// The content beat a looping clip starts on: its offset, or, for an offset at or past the loop's
/// end, where the offset falls inside the loop (as [`Placement::content_beat`] folds it).
fn first_beat(offset: f64, start: f64, end: f64) -> f64 {
    if offset < end { offset } else { start + (offset - start).rem_euclid(end - start) }
}

/// Where a clip plays in time: an arrangement clip at its place; a slot's clip from its launch
/// (beat 0), for ever when it loops, else for its length.
fn placement(at: ClipRef, clip: &Clip) -> Placement {
    let loop_range = clip.loop_range.map(|l| (l.start, l.end));
    match at {
        ClipRef::Arrangement(..) => {
            Placement { start: clip.start, length: clip.length, offset: clip.offset, loop_range }
        }
        ClipRef::Slot(..) => Placement {
            start: 0.0,
            length: if loop_range.is_some() { f64::INFINITY } else { clip.length },
            offset: clip.offset,
            loop_range,
        },
    }
}

/// Whether an arrangement clip plays during any of `window`.
fn overlaps(clip: &Clip, window: Window) -> bool {
    clip.start < window.end && clip.start + clip.length > window.start
}

fn clip_at(project: &Project, at: ClipRef) -> Option<&Clip> {
    match at {
        ClipRef::Arrangement(track, id) => {
            project.arrangement.lanes.iter().find(|l| l.track == Some(track))?.clips.iter().find(|c| c.id == id)
        }
        ClipRef::Slot(track, scene) => {
            project.launcher.columns.iter().find(|c| c.track == track)?.slots.get(scene)?.clip.as_ref()
        }
    }
}

/// The notes `clip` plays on `track`; `None` for an audio clip.
fn notes_of<'a>(project: &'a Project, track: TrackId, clip: &Clip) -> Option<&'a [Note]> {
    let content = project.content(clip.content)?;
    if content.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_))) {
        return None;
    }
    content.lanes.iter().filter(|l| l.target.is_none_or(|t| t == track)).find_map(|l| match &l.data {
        LaneData::Notes(notes) => Some(notes.as_slice()),
        LaneData::Audio(_) => None,
    })
}

/// Every MIDI clip but the edited one, by name, track and place: the arrangement's, then the
/// slots'.
fn choices(project: &Project, edited: ClipRef) -> Vec<GhostChoice> {
    let ts = project.transport.time_signature;
    let bar = if ts.denominator == 0 { 4.0 } else { f64::from(ts.numerator.max(1)) * 4.0 / f64::from(ts.denominator) };
    let track_name = |track: TrackId| project.track(track).map_or("", |t| t.name.as_str());
    let content_name = |track: TrackId, clip: &Clip| -> Option<String> {
        notes_of(project, track, clip)?;
        let name = project.content(clip.content).map_or("", |c| c.name.as_str());
        Some(if name.is_empty() { "clip".to_string() } else { name.to_string() })
    };
    let mut out = Vec::new();
    for lane in &project.arrangement.lanes {
        let Some(track) = lane.track else { continue };
        for clip in &lane.clips {
            let at = ClipRef::Arrangement(track, clip.id);
            if at == edited || out.len() >= MAX_CHOICES {
                continue;
            }
            if let Some(content) = content_name(track, clip) {
                let number = (clip.start / bar).floor().max(0.0) as u64 + 1;
                let name = format!("{content} on {} at bar {number}", track_name(track));
                out.push(GhostChoice { clip: at, name });
            }
        }
    }
    for column in &project.launcher.columns {
        for (scene, slot) in column.slots.iter().enumerate() {
            let at = ClipRef::Slot(column.track, scene);
            let Some(clip) = slot.clip.as_ref() else { continue };
            if at == edited || out.len() >= MAX_CHOICES {
                continue;
            }
            if let Some(content) = content_name(column.track, clip) {
                let name = format!("{content} on {} in scene {}", track_name(column.track), scene + 1);
                out.push(GhostChoice { clip: at, name });
            }
        }
    }
    out
}

/// Paints the ghost notes on `painter` (the grid's), each at the rectangle `rect_of` gives it:
/// a fill in its track's colour at [`GHOST_ALPHA`], no name, no edge and no velocity stem.
pub(crate) fn paint(painter: &Painter, ghosts: &Ghosts, visible: Rect, rect_of: impl Fn(&GhostNote) -> Rect) {
    for ghost in &ghosts.notes {
        let rect = rect_of(ghost);
        if rect.intersects(visible) {
            painter.rect_filled(rect, 0.0, ghost.color.gamma_multiply(GHOST_ALPHA));
        }
    }
}

/// Labels the ghost notes in view for readers and tests ("Ghost note E1 at beat 0 from Bass").
/// The labels only describe: they sense nothing, so a click there reaches the grid.
pub(crate) fn for_readers(ui: &Ui, ghosts: &Ghosts, visible: Rect, rect_of: impl Fn(&GhostNote) -> Rect) {
    let mut labelled = 0;
    for (i, ghost) in ghosts.notes.iter().enumerate() {
        if labelled >= MAX_LABELS {
            break;
        }
        let rect = rect_of(ghost).intersect(visible);
        if !rect.is_positive() {
            continue;
        }
        labelled += 1;
        let from = ghosts.sources.get(ghost.source).map_or("", String::as_str);
        let label = format!("Ghost note {} at beat {} from {from}", key_name(ghost.key), round(ghost.start));
        let response = ui.interact(rect, Id::new(("piano-roll-ghost", i)), Sense::hover());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &label));
    }
}

fn round(beat: f64) -> f64 {
    (beat * 1000.0).round() / 1000.0
}

/// The toolbar's ghost menu: its button reads "Ghosts" and its label what is shown ("Ghost notes
/// Playing together"); its menu offers
/// None, Playing together, This track and, in a submenu, every other MIDI clip. A pick changes
/// `source` at once: window state, not a command.
pub(crate) fn menu(ui: &mut Ui, rect: Rect, ghosts: &Ghosts, source: &mut GhostSource) -> Rect {
    let font = theme::ui_font(theme::TEXT_SMALL);
    let mut picked = None;
    let mut shown = rect;
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        // The button is short, so it fits beside the workflow menu; it is dimmed while nothing
        // shows, and its tooltip and accessible label name what does.
        let color = if *source == GhostSource::None { theme::text_hint() } else { theme::text_value() };
        let text = egui::RichText::new("Ghosts   ").font(font.clone()).color(color);
        let menu = ui.menu_button(text, |ui| {
            let mut entry = |ui: &mut Ui, choice: GhostSource, shown: &str, label: &str| {
                let on = *source == choice;
                let response = ui.selectable_label(on, egui::RichText::new(shown).font(font.clone()));
                response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, label));
                if response.clicked() {
                    picked = Some(choice);
                    ui.close();
                }
            };
            for choice in [GhostSource::None, GhostSource::Together, GhostSource::Track] {
                let name = choice.name(ghosts);
                entry(ui, choice, &name, &format!("Ghost notes: {name}"));
            }
            ui.separator();
            let chosen = matches!(source, GhostSource::Clip(_));
            let title = egui::RichText::new("Clip\u{2026}").font(font.clone());
            let clip_menu = ui.menu_button(title, |ui| {
                if ghosts.choices.is_empty() {
                    ui.label(egui::RichText::new("No other MIDI clip").font(font.clone()).color(theme::text_hint()));
                }
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for choice in &ghosts.choices {
                        let label = format!("Ghost notes from {}", choice.name);
                        entry(ui, GhostSource::Clip(choice.clip), &choice.name, &label);
                    }
                });
            });
            clip_menu.response.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::Button, true, chosen, "Ghost notes: Clip\u{2026}")
            });
        });
        let label = format!("Ghost notes {}", source.name(ghosts));
        crate::detail::piano_roll::menu_arrow(ui, menu.response.rect, color);
        menu.response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        shown = menu.response.rect;
        menu.response.on_hover_text(format!(
            "Ghost notes: {}. Notes of other clips drawn faintly behind this clip's notes",
            source.name(ghosts)
        ));
    });
    if let Some(picked) = picked {
        *source = picked;
    }
    shown
}

#[cfg(test)]
#[path = "piano_roll_ghosts_tests.rs"]
mod tests;
