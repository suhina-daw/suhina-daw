// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Session view: tracks as columns, scenes as rows, scene launch on the right, the mixer
//! along the bottom (`docs/spec/ui-session-view.md`, sizes in `docs/design/components.md`).
//! It draws a [`TrackGrid`] and turns clicks into [`Action`]s; it never changes the project itself.

use std::collections::BTreeSet;

use egui::{Align2, Color32, Id, Pos2, Rect, Response, Sense, StrokeKind, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::{MappingTarget, TrackId};

use crate::interaction::colour_picker;
use crate::interaction::drop::{DropProbe, DropTarget};
use crate::interaction::empty_space::empty_space;
use crate::interaction::grid::{Column, ColumnKind, SlotState, TrackGrid};
use crate::interaction::menus::{self, TrackMenu};
use crate::interaction::nesting::{self, Place};
use crate::kit::meter::{self, Meters};
use crate::kit::motion;
use crate::kit::theme::{self, PAD, ROW};
use crate::kit::widgets::{self, Action};

/// One row: a slot and the line under it.
const PITCH: f32 = ROW + 1.0;
/// Width of a group header's fold arrow.
const FOLD_ARROW: f32 = 12.0;
/// Mark sizes inside a slot.
const CLIP_MARK: f32 = 10.0;
/// The part of a clip's slot that launches it: the mark and the padding around it.
const LAUNCH_WIDTH: f32 = PAD + CLIP_MARK + PAD;
const EMPTY_MARK: f32 = 3.0;
/// The play and stop glyph size in a scene row, where the glyph sits on the row's fill.
const SLOT_GLYPH: f32 = 4.5;
/// The band that takes the pointer to resize the mixer, either side of its top line.
const MIXER_EDGE: f32 = 3.0;

/// A click in a strip's device list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceEvent {
    /// Picked: alone, or with Ctrl (Cmd) added to the picks or taken out, or with Shift the run
    /// from the last one picked.
    Pick { track: TrackId, index: usize, add: bool, run: bool },
    /// Double-clicked: a plug-in's editor opens.
    Open { track: TrackId, index: usize, plugin: bool },
}

/// What the view keeps between frames: scrolling, the selection, and a value being dragged.
#[derive(Debug, Default)]
pub struct SessionViewState {
    pub scroll: Vec2,
    /// The selected track (its header was clicked, or a clip on it selected); the detail area
    /// shows its devices.
    pub selected: Option<TrackId>,
    /// The selected clip slot, its track and scene from 0: a click on the clip's name, a launch,
    /// or a click on an empty slot selected it. The detail area shows its clip; Ctrl+C and Ctrl+V
    /// act on it.
    pub selected_slot: Option<(TrackId, usize)>,
    /// A track's header (or a return's column) was the last thing clicked: Delete removes the
    /// track then (#690). A click on a slot takes it away.
    pub track_focus: bool,
    /// Something has been copied, so a slot's menu can paste it; the window says so each frame.
    pub can_paste: bool,
    /// A clip slot whose drag started this frame (track, scene from 0): the window carries the
    /// clip from there (`docs/spec/ui-session-view.md` › Between the views).
    pub slot_drag: Option<(TrackId, usize)>,
    /// Launching a clip leaves the selection as it is (Settings › Detail area › Select on
    /// launch is off); by default a launch selects the clip.
    pub keep_selection_on_launch: bool,
    /// A column header being dragged to move its track, and where it was let go.
    track_drag: Option<TrackId>,
    track_drop: Option<(TrackId, f32)>,
    pub(crate) drag: Option<(TrackId, Control, f64)>,
    /// Group tracks folded shut: their tracks' columns are hidden. The arrangement view's,
    /// copied in while this view shows.
    pub folded: BTreeSet<TrackId>,
    /// Where the mixer strip's pan and volume controls were drawn last frame: the wheel over one
    /// changes its value instead of scrolling the view.
    pub(crate) value_areas: Vec<Rect>,
    /// Lists that scroll with the wheel themselves (a strip's devices): the view leaves the wheel
    /// over them alone, with or without Alt.
    pub(crate) scroll_areas: Vec<Rect>,
    /// The volume as a knob beside the pan knob instead of a plain value
    /// (Settings › Session and mixer › Mixer controls).
    pub knobs: bool,
    /// A drag from the browser over the window: the view says which target is under the
    /// pointer (`docs/spec/browser.md` › Dragging into the project).
    pub drop: Option<DropProbe>,
    /// The engine's levels this frame, for the mixer's meters.
    pub meters: Meters,
    /// Keys that launch slots, scenes and stop buttons, and the mode that maps them.
    pub launch_keys: crate::session::launch_keys::LaunchKeys,
    /// The mixer's height in points (Settings keeps it; 0 is the default height). A drag of its
    /// top edge sets it when it ends; [`SessionViewState::mixer_dragged`] is the height meanwhile.
    pub mixer_height: f32,
    mixer_dragged: Option<f32>,
    /// The fader's height the divider under the strips' device lists sets (Settings keeps it; 0
    /// is automatic), and the height while it is dragged.
    pub strip_fader: f32,
    pub(crate) strip_fader_dragged: Option<f32>,
    /// Some strip holds an audio input's block, so every strip keeps its room.
    pub(crate) reserve_input: bool,
    /// How far each strip's device list is scrolled.
    pub(crate) device_scroll: std::collections::BTreeMap<TrackId, f32>,
    /// The device row clicked last in a strip (track, device index): it stays lit.
    pub shown_device: Option<(TrackId, usize)>,
    /// The devices picked in the window (the strips' lists and the detail area's chain light the
    /// same ones); the window sets it before each frame.
    pub device_picks: Vec<(TrackId, usize)>,
    /// What a strip's device list was asked this frame, for the window to do.
    pub device_event: Option<DeviceEvent>,
    /// A strip's device row was clicked: the window opens the detail area on the track.
    pub open_detail: bool,
    /// A column's name or channel strip was double-clicked: the window shows that track's devices
    /// (`docs/spec/ui-clip-editor.md` › Where it opens).
    pub open_devices: Option<TrackId>,
    /// A double-click on an empty slot does nothing (Settings › Arrangement › Double-click makes a
    /// clip is off); by default it makes an empty clip of the track's kind.
    pub double_click_does_nothing: bool,
}

impl SessionViewState {
    /// The fader's height the strips use: the divider's while it is dragged, else the saved one.
    pub(crate) fn strip_fader_shown(&self) -> f32 {
        self.strip_fader_dragged.unwrap_or(self.strip_fader)
    }

    /// The mixer's height being dragged to, and while no drag runs.
    pub fn mixer_dragged(&self) -> Option<f32> {
        self.mixer_dragged
    }

    /// Selects the clip slot at `scene` (from 0) on `track`, and the track with it.
    pub fn select_slot(&mut self, track: TrackId, scene: usize) {
        self.track_focus = false;
        self.selected_slot = Some((track, scene));
        self.selected = Some(track);
    }
}

/// Where a key moves the selected slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Up,
    Down,
    Left,
    Right,
}

/// The slot one `step` from `from` (a track and a scene from 0): up and down within the track,
/// left and right to the next track with clip slots in the same scene. `None` at the edge of the
/// grid, or when `from` is not in it.
pub fn step_slot(grid: &TrackGrid, from: (TrackId, usize), step: Step) -> Option<(TrackId, usize)> {
    let (track, scene) = from;
    let slotted: Vec<&Column> = grid.columns.iter().filter(|c| c.kind == ColumnKind::Track).collect();
    let at = slotted.iter().position(|c| c.track == track)?;
    let scenes = slotted.get(at)?.slots.len();
    match step {
        Step::Up => scene.checked_sub(1).filter(|s| *s < scenes).map(|s| (track, s)),
        Step::Down => Some(scene + 1).filter(|s| *s < scenes).map(|s| (track, s)),
        Step::Left => at.checked_sub(1).and_then(|i| slotted.get(i)).map(|c| (c.track, scene)),
        Step::Right => slotted.get(at + 1).map(|c| (c.track, scene)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Control {
    Volume,
    Pan,
    /// The send to this return track.
    Send(TrackId),
}

fn column_width(column: &Column) -> f32 {
    if column.kind == ColumnKind::Return { theme::RETURN_COLUMN } else { theme::TRACK_COLUMN }
}

fn rgb(color: suhina_engine::model::Rgb) -> Color32 {
    Color32::from_rgb(color.0, color.1, color.2)
}

/// How much of the track's colour an empty slot shows: the whole column reads as the track's,
/// the whole column in its track's colour.
const EMPTY_TINT: f32 = 0.12;

/// Draws the Session view in the space left in `ui` and adds what the user asked for to
/// `actions`. `progress` is how far each column's playing clip is (0 … 1), by column index.
pub fn show(
    ui: &mut Ui,
    grid: &TrackGrid,
    progress: &[Option<f32>],
    state: &mut SessionViewState,
    actions: &mut Vec<Action>,
) {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::window_background());

    // Fixed bands: header on top, mixer at the bottom, scene column on the right.
    let header = Rect::from_min_size(area.min, Vec2::new(area.width(), ROW));
    let mixer_height = mixer_height(state, area.height(), grid.scenes.len());
    let mixer =
        Rect::from_min_max(Pos2::new(area.left(), (area.bottom() - mixer_height).max(header.bottom())), area.max);
    let body =
        Rect::from_min_max(Pos2::new(area.left(), header.bottom() + 1.0), Pos2::new(area.right(), mixer.top() - 1.0));
    let scenes_x = (area.right() - theme::SCENE_COLUMN).max(area.left());
    // Return tracks sit against the scene column; regular tracks fill the space left of them.
    let returns_width: f32 =
        grid.columns.iter().filter(|c| c.kind == ColumnKind::Return).map(|c| column_width(c) + 1.0).sum();
    let returns_x = (scenes_x - returns_width).max(area.left());
    let tracks_clip = Rect::from_min_max(area.min, Pos2::new(returns_x - 1.0, area.bottom()));
    let returns_clip = Rect::from_min_max(Pos2::new(returns_x, area.top()), Pos2::new(scenes_x - 1.0, area.bottom()));

    // Scrolling: the body scrolls both ways; header and mixer follow the tracks sideways. The
    // wheel over a mixer value is that value's, so the view leaves it alone.
    // The tracks in their groups, and those a folded group hides.
    let places: Vec<Place> = grid.columns.iter().filter(|c| c.kind != ColumnKind::Return).map(Column::place).collect();
    let hidden = nesting::hidden(&places, &state.folded);
    let content_width: f32 = grid
        .columns
        .iter()
        .filter(|c| c.kind != ColumnKind::Return && !hidden.contains(&c.track))
        .map(|c| column_width(c) + 1.0)
        .sum();
    let content_height = (grid.scenes.len() as f32 + 1.0) * PITCH;
    let fields = std::mem::take(&mut state.value_areas);
    let lists = std::mem::take(&mut state.scroll_areas);
    if ui.rect_contains_pointer(area) {
        // Alt and the wheel over a value field are the field's; the wheel alone scrolls.
        let pointer = ui.ctx().pointer_hover_pos();
        let over_field = (widgets::wheel_moves_value(ui)
            && pointer.is_some_and(|pointer| fields.iter().any(|rect| rect.contains(pointer))))
            || pointer.is_some_and(|pointer| lists.iter().any(|rect| rect.contains(pointer)));
        if !over_field {
            let delta = ui.input(|i| i.smooth_scroll_delta());
            state.scroll -= delta;
        }
    }
    // Past the last track, room for the buttons that add one, so they keep their width (and
    // their text) however many tracks there are.
    let room = crate::interaction::empty_space::empty_space_width(ui);
    state.scroll.x = state.scroll.x.clamp(0.0, (content_width + room - tracks_clip.width()).max(0.0));
    state.scroll.y = state.scroll.y.clamp(0.0, (content_height - body.height()).max(0.0));

    // Lines between the bands.
    painter.hline(area.x_range(), header.bottom(), theme::hairline(theme::border_strong()));
    painter.hline(area.x_range(), mixer.top() - 1.0, theme::hairline(theme::border_strong()));

    let groups =
        grid.columns.iter().filter(|c| c.kind == ColumnKind::Group).map(|c| (c.track, c.name.clone())).collect();
    let bands = Bands {
        midi_ports: &grid.midi_ports,
        input_channels: grid.input_channels,
        header,
        body,
        mixer,
        top: body.top() - state.scroll.y,
        scenes: grid.scenes.len(),
        places,
        groups,
        mapped: &grid.mapped,
    };
    // Where each track column is this frame, for a dragged header to find its place.
    let mut spans: Vec<(&Column, egui::Rangef)> = Vec::new();
    // The returns' columns too, which take dropped devices and plug-ins as a track's do (#687).
    let mut return_spans: Vec<(&Column, egui::Rangef)> = Vec::new();
    let mut track_x = area.left() - state.scroll.x;
    let mut return_x = returns_x;
    let mut track_number = 0;
    state.reserve_input = grid.columns.iter().any(|c| c.plays_audio);
    for (index, column) in grid.columns.iter().enumerate() {
        let width = column_width(column);
        let (x, clip) = if column.kind == ColumnKind::Return {
            (&mut return_x, returns_clip)
        } else {
            track_number += 1;
            if hidden.contains(&column.track) {
                continue;
            }
            (&mut track_x, tracks_clip)
        };
        let span = egui::Rangef::new(*x, *x + width);
        *x += width + 1.0;
        if column.kind == ColumnKind::Return {
            return_spans.push((column, span));
        } else {
            spans.push((column, span));
        }
        if span.max < clip.left() || span.min > clip.right() {
            continue;
        }
        let progress = progress.get(index).copied().flatten();
        draw_column(ui, &painter, &bands, clip, span, column, track_number, progress, state, actions);
    }
    // The empty space right of the last track: buttons that add a track, and new tracks for
    // what the browser drops there.
    let empty = Rect::from_min_max(
        Pos2::new(track_x.max(area.left()), area.top()),
        Pos2::new(returns_x - 1.0, mixer.top() - 1.0),
    );
    if empty.width() > 24.0 {
        // Under the buttons, so they keep their clicks; a right-click there inserts tracks.
        let space = ui.interact(empty, Id::new("session-empty-space"), Sense::click());
        crate::interaction::empty_space::empty_space_menu(&space, actions);
        empty_space(ui, &painter, empty, actions);
    }
    if let Some(probe) = state.drop.as_mut() {
        let row_of = |y: f32| ((y - bands.top) / PITCH).floor().max(0.0) as usize;
        if probe.devices && !probe.files && !probe.clip {
            for (column, span) in &return_spans {
                let whole = Rect::from_x_y_ranges(*span, egui::Rangef::new(header.top(), mixer.bottom()));
                probe.offer(whole.intersect(returns_clip), DropTarget::Track(column.track));
            }
        }
        for (column, span) in &spans {
            // Devices alone land on the track wherever its column is under the pointer: the
            // header, the slots or the mixer strip, a group's and a return's too.
            if probe.devices && !probe.files && !probe.clip {
                let whole = Rect::from_x_y_ranges(*span, egui::Rangef::new(header.top(), mixer.bottom()));
                probe.offer(whole, DropTarget::Track(column.track));
                continue;
            }
            if column.kind != ColumnKind::Track {
                continue;
            }
            let header_rect = Rect::from_x_y_ranges(*span, header.y_range());
            if !probe.clip {
                probe.offer(header_rect, DropTarget::Track(column.track));
            }
            let column_body = Rect::from_x_y_ranges(*span, body.y_range()).intersect(tracks_clip);
            if column_body.contains(probe.pointer) {
                let scene = row_of(probe.pointer.y).min(grid.scenes.len());
                // A carried clip lands in a slot that is there; files may add a scene below.
                if probe.clip && scene >= grid.scenes.len() {
                    continue;
                }
                let top = bands.top + scene as f32 * PITCH;
                let cell = Rect::from_x_y_ranges(*span, egui::Rangef::new(top, top + ROW)).intersect(column_body);
                // The slot under the pointer is the target, for a dropped file and for a dragged
                // clip alike: a slot-to-slot drag moves the clip there (Ctrl: a copy).
                probe.offer(
                    if probe.files || probe.clip { cell } else { column_body },
                    DropTarget::Slot { track: column.track, scene },
                );
            }
        }
        if !probe.clip && empty.width() > 0.0 && empty.contains(probe.pointer) {
            let scene = if probe.pointer.y > body.top() { row_of(probe.pointer.y).min(grid.scenes.len()) } else { 0 };
            probe.offer(empty, DropTarget::NewTracks { beat: None, scene: Some(scene) });
        }
        if let Some(target_area) = probe.area {
            painter.with_clip_rect(area).rect_stroke(
                target_area.shrink(0.5),
                0.0,
                egui::Stroke::new(1.5, theme::accent()),
                StrokeKind::Inside,
            );
        }
    }
    // A header let go over a column moves its track there, into the column's group: before the
    // column under the pointer, or after it from its right half; after an open group's header
    // is into that group. A group moves with its tracks.
    let target_of = |x: f32| spans.iter().find(|(_, span)| span.contains(x)).map(|(c, span)| (*c, x > span.center()));
    if let Some(dragged) = state.track_drag
        && let Some(pointer) = ui.ctx().pointer_interact_pos()
        && let Some((column, after)) = target_of(pointer.x)
        && column.track != dragged
        && let Some((_, span)) = spans.iter().find(|(c, _)| c.track == column.track)
    {
        let x = if after { span.max + 0.5 } else { span.min - 0.5 };
        painter.with_clip_rect(tracks_clip).vline(x, area.y_range(), egui::Stroke::new(2.0, theme::accent()));
    }
    if let Some((dragged, x)) = state.track_drop.take()
        && let Some((target, after)) = target_of(x)
        && let Some(action) = nesting::drop_action(&bands.places, dragged, target.track, after, &state.folded)
    {
        actions.push(action);
    }

    if returns_width > 0.0 {
        painter.vline(returns_x - 0.5, area.y_range(), theme::hairline(theme::border_strong()));
    }

    // Scene column, fixed on the right.
    painter.vline(scenes_x - 0.5, area.y_range(), theme::hairline(theme::border_strong()));
    let scene_span = egui::Rangef::new(scenes_x, area.right());
    let header_rect = Rect::from_x_y_ranges(scene_span, header.y_range());
    painter.rect_filled(header_rect, 0.0, theme::control_background());
    widgets::text_in(
        &painter,
        header_rect.shrink2(Vec2::new(PAD, 0.0)),
        Align2::LEFT_CENTER,
        "Main",
        theme::ui_font(theme::TEXT),
        theme::text_primary(),
    );
    scene_column(
        ui,
        &painter.with_clip_rect(Rect::from_x_y_ranges(scene_span, body.y_range())),
        scene_span,
        body.top() - state.scroll.y,
        grid,
        &mut state.launch_keys,
        actions,
    );
    let main_mixer = Rect::from_x_y_ranges(scene_span, mixer.y_range());
    painter.rect_filled(main_mixer, 0.0, theme::panel_background());
    let level = state.meters.main;
    // The main meter stands where a track's does: beside the fader, or at the top of the strip
    // while the mixer is too low for a fader.
    let shared =
        crate::session::mixer_strip::Shared { reserve_input: state.reserve_input, fader: state.strip_fader_shown() };
    let parts = crate::session::mixer_strip::layout_shared(main_mixer, false, false, shared);
    if let Some(list) = parts.devices {
        crate::session::mixer_strip::main_devices(&painter.with_clip_rect(main_mixer), list, &grid.main_devices);
    }
    if let (Some(readout), Some(fader)) = (parts.readout, parts.fader) {
        let main_painter = painter.with_clip_rect(main_mixer);
        crate::session::mixer_strip::main_fader(ui, &main_painter, readout, fader, grid.main_volume_db, state, actions);
    }
    let at = parts.meter.unwrap_or_else(|| {
        Rect::from_min_size(
            main_mixer.min + Vec2::splat(PAD),
            Vec2::new(meter::WIDTH, parts.toggles.top() - main_mixer.top() - PAD * 2.0),
        )
    });
    meter::show(ui, &painter.with_clip_rect(main_mixer), at, "Main", meter::Source::Main, &level, actions);
    // The metronome in the main strip's toggle row, as the main channel's click control.
    let click =
        Rect::from_min_size(parts.toggles.min, Vec2::new(28.0_f32.min(parts.toggles.width()), parts.toggles.height()));
    main_metronome(ui, &painter.with_clip_rect(main_mixer), click, grid.metronome, actions);
    mixer_edge(ui, &painter, area, mixer, mixer_height, state);
}

/// The main strip's metronome button: a click turns the click on or off; held, a slider shows
/// over it and dragging up or down sets the click's level until the button is let go.
fn main_metronome(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    (on, db, _): (bool, f64, suhina_engine::ClickSound),
    actions: &mut Vec<Action>,
) {
    let response = ui
        .interact(rect.intersect(painter.clip_rect()), Id::new("main-metronome"), Sense::click_and_drag())
        .on_hover_text("Metronome: click to turn it on or off; hold and drag up or down for its level");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Main metronome"));
    if response.clicked() {
        actions.push(Action::new("transport.setMetronome", json!({ "on": !on })));
    }
    let fill = if on { theme::control_active() } else { theme::control_background() };
    painter.rect_filled(rect, 0.0, fill);
    let glyph = if on { theme::accent() } else { theme::text_primary() };
    crate::session::record_controls::metronome_glyph(painter, rect, glyph);
    // Held: the level's slider beside the button, where there is room.
    crate::session::record_controls::metronome_level(ui, &response, rect, (on, db), actions);
}

/// The mixer's height this frame: the one being dragged to, else the saved one, kept between the
/// least height and the most this view allows (`docs/design/components.md` › Mixer strip).
fn mixer_height(state: &SessionViewState, view_height: f32, scenes: usize) -> f32 {
    let (least, default, _) = crate::settings::view_settings::MIXER_HEIGHTS;
    let (least, default) = (least as f32, default as f32);
    let saved = if state.mixer_height > 0.0 { state.mixer_height } else { default };
    let wanted = state.mixer_dragged.unwrap_or(saved);
    // Up to just under the stop row under the last scene, and always up to the default height;
    // never over the header.
    let under_slots = view_height - ROW - 1.0 - (scenes as f32 + 1.0) * PITCH - 1.0;
    let most = under_slots.max(default).min(view_height - ROW - 1.0).max(least);
    wanted.clamp(least, most)
}

/// The mixer's top line: dragged up or down to resize it, double-clicked for its default height.
fn mixer_edge(ui: &Ui, painter: &egui::Painter, area: Rect, mixer: Rect, height: f32, state: &mut SessionViewState) {
    let line = mixer.top() - 0.5;
    let rect = Rect::from_x_y_ranges(area.x_range(), egui::Rangef::new(line - MIXER_EDGE, line + MIXER_EDGE));
    let response = ui
        .interact(rect.intersect(area), Id::new("session-mixer-edge"), Sense::click_and_drag())
        .on_hover_cursor(egui::CursorIcon::ResizeVertical);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Mixer height"));
    if response.double_clicked() {
        state.mixer_dragged = None;
        state.mixer_height = crate::settings::view_settings::MIXER_HEIGHTS.1 as f32;
    } else if response.dragged_by(egui::PointerButton::Primary) {
        // From the height shown, so a drag past either end does not have to come back first.
        state.mixer_dragged = Some(height - response.drag_delta().y);
    } else if response.drag_stopped_by(egui::PointerButton::Primary) && state.mixer_dragged.take().is_some() {
        state.mixer_height = height.round();
    }
    if response.hovered() || response.dragged() {
        painter.hline(area.x_range(), line, egui::Stroke::new(2.0, theme::accent()));
    }
}

/// The view's horizontal bands, where the first scene row is after scrolling, and the keys
/// mapped to slots and stop buttons.
struct Bands<'a> {
    /// The MIDI input ports there are, and the audio input device's channels, for the tracks'
    /// menus.
    midi_ports: &'a [String],
    input_channels: u16,
    header: Rect,
    body: Rect,
    mixer: Rect,
    top: f32,
    scenes: usize,
    /// Every track but the returns, in track order, and the group tracks with their names.
    places: Vec<Place>,
    groups: Vec<(TrackId, String)>,
    mapped: &'a [(MappingTarget, String)],
}

/// One column: header, slots and stop button, mixer strip, and its separator, inside `clip`.
#[allow(clippy::too_many_arguments)]
fn draw_column(
    ui: &Ui,
    painter: &egui::Painter,
    bands: &Bands,
    clip: Rect,
    span: egui::Rangef,
    column: &Column,
    track_number: usize,
    progress: Option<f32>,
    state: &mut SessionViewState,
    actions: &mut Vec<Action>,
) {
    let id = Id::new(("session-column", column.track.0));
    let header = Rect::from_x_y_ranges(span, bands.header.y_range()).intersect(clip);
    column_header(ui, painter, header, column, bands, state, actions);
    let body_clip = bands.body.intersect(clip);
    column_body(ui, &painter.with_clip_rect(body_clip), body_clip, span, bands, column, progress, id, state, actions);
    let mixer_rect = Rect::from_x_y_ranges(span, bands.mixer.y_range());
    let mixer_painter = painter.with_clip_rect(bands.mixer.intersect(clip));
    // A return column has no slots: its body selects it and has its menu, as its header does.
    if column.kind == ColumnKind::Return {
        let body = Rect::from_x_y_ranges(span, bands.body.y_range()).intersect(body_clip);
        if body.width() > 0.0 && body.height() > 0.0 {
            let response = ui.interact(body, id.with("return-body"), Sense::click());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Other, true, format!("{} column", column.name))
            });
            if response.clicked() {
                state.selected = Some(column.track);
                state.selected_slot = None;
                state.track_focus = true;
            }
            track_menu_on(&response, column, bands, actions);
        }
    }
    let strip = crate::session::mixer_strip::show(
        ui,
        &mixer_painter,
        mixer_rect,
        column,
        &bands.groups,
        track_number,
        bands.input_channels,
        state,
        id,
        actions,
    );
    // The strip's background has the track's menu, as its header does.
    track_menu_on(&strip, column, bands, actions);
    // Column separator through header, grid and mixer.
    let full = egui::Rangef::new(bands.header.top(), bands.mixer.bottom());
    painter.with_clip_rect(clip).vline(span.max + 0.5, full, theme::hairline(theme::border_strong()));
}

/// The track's right-click menu on `response`: its header, its strip, a return's column.
fn track_menu_on(response: &Response, column: &Column, bands: &Bands, actions: &mut Vec<Action>) {
    // Open until a click outside: its items close it themselves, and a colour dragged in its
    // picker does not (#712).
    egui::Popup::context_menu(response).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        let into: Vec<(TrackId, String)> = nesting::move_targets(&bands.places, column.track)
            .into_iter()
            .filter_map(|g| bands.groups.iter().find(|(t, _)| *t == g).cloned())
            .collect();
        let menu = TrackMenu {
            track: column.track,
            name: &column.name,
            colour: column.color,
            is_group: column.kind == ColumnKind::Group,
            is_return: column.kind == ColumnKind::Return,
            selection: &[],
            into: &into,
            out: nesting::out_of_group(&bands.places, column.track),
            midi: column.plays_notes.then_some((&column.midi_input, bands.midi_ports)),
            audio: column.plays_audio.then_some((&column.audio_input, bands.input_channels)),
        };
        menus::track_menu(ui, &menu, actions);
    });
}

fn column_header(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    column: &Column,
    bands: &Bands,
    state: &mut SessionViewState,
    actions: &mut Vec<Action>,
) {
    if rect.width() <= 0.0 {
        return;
    }
    // Right-click opens the track's menu.
    // A click selects the track, a drag moves it, a right-click opens its menu.
    let response = ui.interact(rect, Id::new(("session-header", column.track.0)), Sense::click_and_drag());
    if response.drag_started_by(egui::PointerButton::Primary) {
        state.track_drag = Some(column.track);
    }
    if response.drag_stopped_by(egui::PointerButton::Primary)
        && let Some(dragged) = state.track_drag.take()
        && let Some(pointer) = response.interact_pointer_pos().or_else(|| ui.ctx().pointer_latest_pos())
    {
        state.track_drop = Some((dragged, pointer.x));
    }
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Track {}", column.name)));
    track_menu_on(&response, column, bands, actions);
    if response.clicked() {
        state.selected = Some(column.track);
        state.selected_slot = None;
        state.track_focus = true;
    }
    if response.double_clicked() {
        state.open_devices = Some(column.track);
    }
    // The header is the track's colour, its name in the text colour that reads on it; a selected
    // track's header has an outline in that colour.
    let fill = colour_picker::fill(column.color, column.mute);
    painter.rect_filled(rect, 0.0, fill);
    let color = theme::text_on(fill);
    if state.selected == Some(column.track) {
        painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.5, color), StrokeKind::Inside);
    }
    // Nested tracks step in a little; a group's name follows its fold arrow.
    let mut text = rect.shrink2(Vec2::new(PAD, 0.0));
    text.min.x += (column.depth as f32 * 4.0).min(text.width() / 2.0);
    if column.kind == ColumnKind::Group {
        let arrow = Rect::from_min_size(text.min, Vec2::new(FOLD_ARROW, text.height())).intersect(rect);
        if arrow.is_positive() {
            let id = Id::new(("session-fold", column.track.0));
            nesting::fold_arrow(ui, painter, arrow, id, column.track, &column.name, color, &mut state.folded);
        }
        text.min.x = (text.min.x + FOLD_ARROW).min(text.max.x);
    }
    widgets::text_in(painter, text, Align2::LEFT_CENTER, &column.name, theme::ui_font(theme::TEXT), color);
    // The name's place, for a rename in place (Ctrl+R, #696).
    let pass = ui.ctx().cumulative_pass_nr();
    ui.ctx().data_mut(|d| {
        d.insert_temp(Id::new((crate::app::NAME_RECT_TRACK, u64::from(column.track.0))), (text, pass));
    });
}

#[allow(clippy::too_many_arguments)]
fn column_body(
    ui: &Ui,
    painter: &egui::Painter,
    clip: Rect,
    span: egui::Rangef,
    bands: &Bands,
    column: &Column,
    progress: Option<f32>,
    id: Id,
    state: &mut SessionViewState,
    actions: &mut Vec<Action>,
) {
    // A track's slots can be selected, copied, pasted into and dragged; a group's only launch.
    let editable = column.kind == ColumnKind::Track;
    let can_paste = state.can_paste;
    let (top, scenes) = (bands.top, bands.scenes);
    let mapping = state.launch_keys.mode.on && column.kind != ColumnKind::Return;
    let label = |target: MappingTarget| bands.mapped.iter().find(|(t, _)| *t == target).map(|(_, l)| l.as_str());
    for (scene, slot) in column.slots.iter().enumerate() {
        let rect = Rect::from_x_y_ranges(
            span,
            egui::Rangef::new(top + scene as f32 * PITCH, top + scene as f32 * PITCH + ROW),
        );
        if !rect.intersects(clip) {
            continue;
        }
        let number = scene + 1;
        let target = MappingTarget::Slot { track: column.track, scene };
        if mapping {
            // The mapping mode: a click picks the slot, empty or not, and launches nothing.
            let response = widgets::button_area(
                ui,
                rect.intersect(clip),
                id.with(scene),
                format!("Map a key to {} scene {number}", column.name),
            );
            if response.clicked() {
                state.launch_keys.pick(target);
            }
            slot_cell(painter, rect, column, slot, 0.0, 1.0, progress, false);
            crate::session::launch_keys::overlay(
                painter,
                rect,
                label(target),
                state.launch_keys.mode.selected == Some(target),
                0.0,
            );
            painter.hline(span, rect.bottom() + 0.5, theme::hairline(theme::border()));
            continue;
        }
        let has_clip = !matches!(slot.state, SlotState::Empty | SlotState::EmptyStop);
        let key = (column.track, scene);
        // A clip on a track: its mark launches it, its name selects it. A group's slot launches
        // the tracks inside it from anywhere.
        let selectable = has_clip && column.kind == ColumnKind::Track;
        let launch_rect = if selectable {
            Rect::from_min_max(rect.min, Pos2::new((rect.left() + LAUNCH_WIDTH).min(rect.right()), rect.bottom()))
        } else {
            rect
        };
        let name = if slot.name.is_empty() { "clip" } else { slot.name.as_str() };
        let sense = if editable { Sense::click_and_drag() } else { Sense::click() };
        let launch = match slot.state {
            // An empty slot without a stop button launches nothing. On a track it can be selected
            // (to paste into), and a double-click makes an empty clip in it — MIDI on a notes
            // track, audio on one that plays audio.
            SlotState::Empty if editable || column.notes => {
                let response = ui.interact(rect.intersect(clip), id.with(scene), Sense::click());
                response.widget_info(|| {
                    let label = format!("Empty slot on {} (scene {number})", column.name);
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label)
                });
                Some(response)
            }
            SlotState::Empty => None,
            SlotState::EmptyStop => Some(widgets::button_area(
                ui,
                rect.intersect(clip),
                id.with(scene),
                format!("Stop {} (scene {number})", column.name),
            )),
            _ => {
                let verb = if slot.state == SlotState::Playing { "Stop" } else { "Launch" };
                let label = format!("{verb} {name} on {} (scene {number})", column.name);
                let response = ui.interact(launch_rect.intersect(clip), id.with(scene), sense);
                response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
                Some(response)
            }
        };
        let select = selectable.then(|| {
            let name_rect = Rect::from_min_max(Pos2::new(launch_rect.right(), rect.top()), rect.max);
            let label = format!("Select {name} on {} (scene {number})", column.name);
            let response = ui.interact(name_rect.intersect(clip), id.with(("select", scene)), sense);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
            response
        });
        let hover = [&launch, &select].into_iter().flatten().map(|r| widgets::hover(ui, r)).fold(0.0, f32::max);
        let pulse = if slot.state == SlotState::Queued { motion::pulse(ui.ctx()) } else { 1.0 };
        // The launch button launches as the pointer goes down, and releases a gate or repeat
        // clip as it goes up. A playing trigger clip's press stops it instead.
        if slot.state != SlotState::Empty
            && let Some(response) = &launch
            && crate::session::slot_launch::press_and_release(
                ui,
                response,
                column.track,
                number,
                slot.launch,
                slot.state == SlotState::Playing,
                actions,
            )
            && selectable
            && !state.keep_selection_on_launch
        {
            state.select_slot(column.track, scene);
        }
        if !has_clip
            && (column.notes || column.plays_audio)
            && !state.double_click_does_nothing
            && launch.as_ref().is_some_and(Response::double_clicked)
        {
            actions.push(Action::new("clip.create", json!({ "track": column.track.0, "scene": number })));
            // The new clip is selected, so the detail area shows it.
            state.select_slot(column.track, scene);
        }
        if select.as_ref().is_some_and(Response::clicked) {
            state.select_slot(column.track, scene);
        }
        if editable {
            // The slot's rename draft dies with its menu — either of the slot's two responses may
            // host it, so the check is once for the pair: a reopen starts from the name as it is
            // now.
            let menu_open = [&launch, &select].into_iter().flatten().any(egui::Response::context_menu_opened);
            if !menu_open {
                ui.ctx().data_mut(|d| d.remove::<String>(Id::new(("slot-rename", column.track, number))));
            }
            for response in [&launch, &select].into_iter().flatten() {
                if response.clicked() || response.secondary_clicked() {
                    state.track_focus = false;
                }
                // An empty slot is selected by a click, any slot by a right-click: Ctrl+C and
                // Ctrl+V act on it. A slot selects its track with it, as a clip's does, so the
                // detail area and auto-arm (#834) follow it.
                if (!has_clip && response.clicked()) || response.secondary_clicked() {
                    state.select_slot(column.track, scene);
                }
                if has_clip && response.drag_started_by(egui::PointerButton::Primary) {
                    state.select_slot(column.track, scene);
                    state.slot_drag = Some(key);
                }
                // The slot's menu, and a clip's launch settings under it.
                // The menu stays open on a click inside: the rename's field and button live here.
                egui::Popup::context_menu(response).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(
                    |ui| {
                        menus::slot_menu(ui, column.track, number, &slot.name, has_clip, can_paste, actions);
                        if let Some(launch) = slot.launch {
                            ui.separator();
                            crate::session::slot_launch::launch_menu(ui, column.track, number, launch, actions);
                        }
                    },
                );
            }
        } else if let (Some(response), Some(launch)) = (&launch, slot.launch) {
            egui::Popup::context_menu(response)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| crate::session::slot_launch::launch_menu(ui, column.track, number, launch, actions));
        }
        let selected = state.selected_slot == Some(key);
        // The name's place, for a rename in place (Ctrl+R).
        let pass = ui.ctx().cumulative_pass_nr();
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                egui::Id::new((crate::app::NAME_RECT_SLOT, u64::from(column.track.0), number as u64)),
                (rect, pass),
            );
        });
        slot_cell(painter, rect, column, slot, hover, pulse, progress, selectable && selected);
        if column.armed && !has_clip && column.kind == ColumnKind::Track {
            let recording = column.recording == Some(scene);
            let (track, name) = (column.track.0, column.name.as_str());
            crate::session::record_controls::slot_record(
                ui,
                painter,
                rect.intersect(clip),
                LAUNCH_WIDTH,
                track,
                name,
                number,
                recording,
                actions,
            );
        }
        if selected && !selectable {
            painter.rect_stroke(rect, 0.0, theme::hairline(theme::text_primary()), StrokeKind::Inside);
        }
        painter.hline(span, rect.bottom() + 0.5, theme::hairline(theme::border()));
    }

    if column.kind == ColumnKind::Return {
        // Returns have no slots, but the scene lines run through them.
        for scene in 0..scenes {
            painter.hline(span, top + scene as f32 * PITCH + ROW + 0.5, theme::hairline(theme::border()));
        }
        return;
    }
    // Under the last scene: the track's stop button.
    let stop_top = top + scenes as f32 * PITCH;
    let stop_rect = Rect::from_x_y_ranges(span, egui::Rangef::new(stop_top, stop_top + ROW));
    if !stop_rect.intersects(clip) {
        return;
    }
    let response =
        widgets::button_area(ui, stop_rect.intersect(clip), id.with("stop"), format!("Stop {}", column.name));
    let target = MappingTarget::Stop { track: column.track };
    if response.clicked() {
        if mapping {
            state.launch_keys.pick(target);
        } else {
            actions.push(Action::new("track.stop", json!({ "track": column.track.0 })));
        }
    }
    let fill = motion::mix(theme::panel_background(), theme::panel_hover(), widgets::hover(ui, &response));
    painter.rect_filled(stop_rect, 0.0, fill);
    let square = Rect::from_center_size(stop_rect.center(), Vec2::splat(7.0));
    let color = if column.stop_queued { theme::accent() } else { theme::text_secondary() };
    painter.rect_stroke(square, 0.0, theme::hairline(color), StrokeKind::Inside);
    if mapping {
        crate::session::launch_keys::overlay(
            painter,
            stop_rect,
            label(target),
            state.launch_keys.mode.selected == Some(target),
            0.0,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn slot_cell(
    painter: &egui::Painter,
    rect: Rect,
    column: &Column,
    slot: &crate::interaction::grid::Slot,
    hover: f32,
    pulse: f32,
    progress: Option<f32>,
    selected: bool,
) {
    let track_color = rgb(column.color);
    // A clip slot is filled with the clip's colour (its track's unless it has its own).
    let clip_color = slot.color.map_or(track_color, rgb);
    let empty =
        motion::mix(theme::window_background(), track_color, if column.mute { EMPTY_TINT / 2.0 } else { EMPTY_TINT });
    let mark_rect =
        |size: f32| Rect::from_min_size(Pos2::new(rect.left() + PAD, rect.center().y - size / 2.0), Vec2::splat(size));
    let name_rect = Rect::from_min_max(
        Pos2::new(rect.left() + PAD + CLIP_MARK + PAD, rect.top()),
        Pos2::new(rect.right() - PAD, rect.bottom()),
    );
    let font = theme::ui_font(theme::TEXT);
    match slot.state {
        SlotState::Empty => {
            painter.rect_filled(rect, 0.0, empty);
        }
        SlotState::EmptyStop => {
            painter.rect_filled(rect, 0.0, motion::mix(empty, theme::panel_background(), hover));
            painter.rect_filled(mark_rect(EMPTY_MARK), 0.0, theme::slot_empty());
        }
        SlotState::Stopped | SlotState::Queued => {
            // The clip in its colour, lighter under the pointer; a deactivated clip is grey.
            let base = if slot.enabled { clip_color } else { theme::control_background() };
            let fill = motion::mix(base, Color32::WHITE, 0.14 * hover);
            let fill = if column.mute { motion::mix(theme::control_background(), fill, 0.45) } else { fill };
            painter.rect_filled(rect, 0.0, fill);
            let (mark, text) = if slot.enabled {
                (theme::text_on(fill), theme::text_on(fill))
            } else {
                (theme::text_hint(), theme::text_hint())
            };
            if slot.state == SlotState::Queued {
                painter.rect_filled(mark_rect(CLIP_MARK), 0.0, mark.gamma_multiply(0.5));
                // The outline breathes while the clip waits for its launch.
                let accent = theme::accent().gamma_multiply(0.45 + 0.55 * pulse);
                painter.rect_stroke(mark_rect(CLIP_MARK), 0.0, theme::hairline(accent), StrokeKind::Outside);
                painter.rect_stroke(rect, 0.0, theme::hairline(accent), StrokeKind::Inside);
            } else {
                // The mark is the shape its click does: a play triangle while stopped.
                let box_rect = mark_rect(CLIP_MARK);
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        box_rect.center() + Vec2::new(-3.0, -5.0),
                        box_rect.center() + Vec2::new(5.0, 0.0),
                        box_rect.center() + Vec2::new(-3.0, 5.0),
                    ],
                    mark,
                    egui::Stroke::NONE,
                ));
            }
            widgets::text_in(painter, name_rect, Align2::LEFT_CENTER, &slot.name, font, text);
        }
        SlotState::Playing => {
            // A playing clip keeps its colour, brighter, so the column still reads as its track.
            let fill = motion::mix(clip_color, Color32::WHITE, 0.10 + 0.14 * hover);
            let fill = if column.mute { motion::mix(theme::control_background(), fill, 0.45) } else { fill };
            painter.rect_filled(rect, 0.0, fill);
            let ink = theme::text_on(fill);
            // The mark is the shape its click does: a stop square while playing. A stopped
            // clip's mark is the play triangle the same way, for its click's launch.
            painter.rect_filled(
                Rect::from_center_size(mark_rect(CLIP_MARK).center(), Vec2::splat(CLIP_MARK - 2.0)),
                0.0,
                ink,
            );
            widgets::text_in(painter, name_rect, Align2::LEFT_CENTER, &slot.name, font, ink);
            let done = progress.unwrap_or(1.0).clamp(0.0, 1.0);
            let line =
                Rect::from_min_size(Pos2::new(rect.left(), rect.bottom() - 3.0), Vec2::new(rect.width() * done, 3.0));
            painter.rect_filled(line, 0.0, ink);
        }
    }
    // A selected clip is outlined as a selected arrangement clip is; a queued one keeps its
    // breathing outline.
    if selected && slot.state != SlotState::Queued {
        painter.rect_stroke(rect, 0.0, egui::Stroke::new(2.0, theme::text_primary()), StrokeKind::Inside);
    }
}

fn scene_column(
    ui: &Ui,
    painter: &egui::Painter,
    span: egui::Rangef,
    top: f32,
    grid: &TrackGrid,
    keys: &mut crate::session::launch_keys::LaunchKeys,
    actions: &mut Vec<Action>,
) {
    let clip = painter.clip_rect();
    for scene in &grid.scenes {
        let y = top + (scene.number - 1) as f32 * PITCH;
        let rect = Rect::from_x_y_ranges(span, egui::Rangef::new(y, y + ROW));
        if !rect.intersects(clip) {
            continue;
        }
        let label = if scene.name.is_empty() {
            format!("{} scene {}", if scene.playing { "Stop" } else { "Launch" }, scene.number)
        } else {
            format!("{} scene {} {}", if scene.playing { "Stop" } else { "Launch" }, scene.number, scene.name)
        };
        let response = widgets::button_area(ui, rect.intersect(clip), Id::new(("scene", scene.number)), label);
        let target = MappingTarget::Scene { scene: scene.number.saturating_sub(1) };
        if response.clicked() {
            if keys.mode.on {
                keys.pick(target);
            } else if scene.playing {
                // A playing row's click stops its clips; anything else launches.
                actions.push(Action::new("scene.stop", json!({ "scene": scene.number })));
            } else {
                actions.push(Action::new("scene.launch", json!({ "scene": scene.number })));
            }
        }
        // A click in the menu's text fields keeps it open; its buttons close it.
        egui::Popup::context_menu(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| menus::scene_menu(ui, &scene.menu(grid.scenes.len() == 1), actions));
        let hover = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        let playing = motion::toward(ui.ctx(), response.id.with("playing"), scene.playing, motion::VALUE);
        let fill = motion::mix(hover, theme::control_playing(), playing);
        painter.rect_filled(rect, 0.0, fill);
        // The row's glyph: the play arrow while its clips wait for a launch, the stop square
        // while the row plays — the click does what the glyph says.
        let glyph = Rect::from_min_size(
            Pos2::new(rect.left() + PAD, rect.center().y - SLOT_GLYPH / 2.0),
            Vec2::splat(SLOT_GLYPH),
        );
        crate::kit::widgets::play_glyph(
            painter,
            glyph.center(),
            scene.playing,
            motion::mix(theme::text_secondary(), theme::text_primary(), widgets::hover(ui, &response).max(playing)),
            SLOT_GLYPH,
        );
        let inner = rect.shrink2(Vec2::new(PAD, 0.0));
        let mut name = scene.name.clone();
        if let Some(tempo) = scene.tempo_bpm {
            name = format!("{name} {tempo:.0}").trim().to_string();
        }
        if let Some(ts) = scene.time_signature {
            name = format!("{name} {}/{}", ts.numerator, ts.denominator).trim().to_string();
        }
        let number = scene.number.to_string();
        widgets::text_in(
            painter,
            inner,
            Align2::RIGHT_CENTER,
            &number,
            theme::mono_font(theme::TEXT_SMALL),
            theme::text_hint(),
        );
        let name_rect = Rect::from_min_max(
            Pos2::new(glyph.right() + PAD, inner.top()),
            Pos2::new(inner.right() - 18.0, inner.bottom()),
        );
        widgets::text_in(
            painter,
            name_rect,
            Align2::LEFT_CENTER,
            &name,
            theme::ui_font(theme::TEXT),
            theme::text_primary(),
        );
        if keys.mode.on {
            crate::session::launch_keys::overlay(
                painter,
                rect,
                grid.mapped_to(target),
                keys.mode.selected == Some(target),
                18.0,
            );
        }
        painter.hline(span, rect.bottom() + 0.5, theme::hairline(theme::border()));
    }
    let stop_top = top + grid.scenes.len() as f32 * PITCH;
    let rect = Rect::from_x_y_ranges(span, egui::Rangef::new(stop_top, stop_top + ROW));
    if rect.intersects(clip) {
        let response = widgets::button_area(ui, rect.intersect(clip), Id::new("stop-all"), "Stop all clips");
        if response.clicked() {
            actions.push(Action::new("launcher.stopAll", json!({})));
        }
        painter.rect_filled(
            rect,
            0.0,
            motion::mix(theme::panel_background(), theme::panel_hover(), widgets::hover(ui, &response)),
        );
        let square = Rect::from_min_size(Pos2::new(rect.left() + PAD, rect.center().y - 3.5), Vec2::splat(7.0));
        painter.rect_stroke(square, 0.0, theme::hairline(theme::text_secondary()), StrokeKind::Inside);
        let text = Rect::from_min_max(Pos2::new(square.right() + PAD, rect.top()), rect.max);
        widgets::text_in(
            painter,
            text,
            Align2::LEFT_CENTER,
            "Stop all",
            theme::ui_font(theme::TEXT),
            theme::text_secondary(),
        );
    }
}
