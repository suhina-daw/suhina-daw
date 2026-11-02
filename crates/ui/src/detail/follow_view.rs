// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The follow action of the selected slot clip, in the detail area beside its other parts
//! (`docs/spec/ui-session-view.md` › Detail area: follow action, sizes in
//! `docs/design/components.md` › Follow action). Every control changes it through
//! `clip.setFollowAction`, one undo step per change; it never changes the project itself.

use crate::kit::surface::Backgrounds as _;
use egui::{Align2, Id, Pos2, Rect, Response, Ui, Vec2};
use serde_json::{Value, json};
use suhina_engine::model::{FollowAction, FollowKind, Project, TrackId};

use crate::kit::motion;
use crate::kit::theme::{self, PAD, ROW};
use crate::kit::widgets::{self, Action, Drag};

/// Width of the section in a row, and of a control's label.
pub const WIDTH: f32 = 196.0;
const LABEL_WIDTH: f32 = 64.0;
/// One control's row, and the field in it (components › Dropdown).
const FIELD_ROW: f32 = 18.0;
const FIELD: f32 = 15.0;
const SWITCH_WIDTH: f32 = 32.0;
/// Rows under the title: time, linked, action A, chance A, action B, chance B.
const ROWS: usize = 6;
/// Height of the section stacked at the top of the detail column.
pub const STACK_HEIGHT: f32 = ROW + PAD + ROWS as f32 * FIELD_ROW + PAD;
/// Steps of a dragged follow time, in beats: a sixteenth note, and the change per point of drag.
const TIME_STEP: f64 = 0.25;
const TIME_PER_POINT: f64 = 0.125;
/// Change of a chance per point of drag.
const CHANCE_PER_POINT: f64 = 0.5;

/// A slot clip's follow action, as the section shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct FollowInfo {
    pub track: TrackId,
    /// The clip's scene, from 0.
    pub scene: usize,
    pub follow: FollowAction,
    /// How many scenes the project has: the scenes a jump can go to.
    pub scenes: usize,
    /// Beats in one bar of the project's time signature.
    pub bar_beats: f64,
}

impl FollowInfo {
    /// The follow action of the clip in `scene` (from 0) on `track`; `None` when that slot has
    /// no clip.
    pub fn build(project: &Project, track: TrackId, scene: usize) -> Option<Self> {
        let clip = project
            .launcher
            .columns
            .iter()
            .find(|c| c.track == track)
            .and_then(|c| c.slots.get(scene))
            .and_then(|s| s.clip.as_ref())?;
        let ts = project.transport.time_signature;
        let bar_beats =
            if ts.denominator == 0 { 4.0 } else { f64::from(ts.numerator.max(1)) * 4.0 / f64::from(ts.denominator) };
        Some(Self { track, scene, follow: clip.follow_action, scenes: project.launcher.scenes.len(), bar_beats })
    }

    /// `clip.setFollowAction` for this clip, changing what `change` holds.
    fn action(&self, change: Value) -> Action {
        let mut params = json!({ "track": self.track.0, "scene": self.scene + 1 });
        if let (Some(params), Value::Object(change)) = (params.as_object_mut(), change) {
            params.extend(change);
        }
        Action::new("clip.setFollowAction", params)
    }
}

/// A follow time in bars, beats and sixteenths, as a clip's length reads: `1.0` is one bar,
/// `0.2.2` two beats and two sixteenths.
pub fn format_time(beats: f64, bar_beats: f64) -> String {
    if !beats.is_finite() || !bar_beats.is_finite() || bar_beats <= 0.0 {
        return "-".into();
    }
    let sixteenths = (beats.max(0.0) * 4.0).round();
    let per_bar = (bar_beats * 4.0).round().max(1.0);
    let bar = (sixteenths / per_bar).floor();
    let rest = sixteenths - bar * per_bar;
    let (beat, sixteenth) = ((rest / 4.0).floor(), rest % 4.0);
    if sixteenth == 0.0 { format!("{bar}.{beat}") } else { format!("{bar}.{beat}.{sixteenth}") }
}

/// An action's name on screen: "Next", "Jump to scene 3".
pub fn action_label(kind: FollowKind) -> String {
    match kind {
        FollowKind::Jump(slot) => format!("Jump to scene {}", u64::from(slot) + 1),
        named => {
            let name = named.name();
            let mut chars = name.chars();
            chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
        }
    }
}

/// The choices of an action's dropdown: the named actions, then a jump to each scene.
pub fn action_choices(scenes: usize) -> Vec<FollowKind> {
    let jumps = (0..scenes).filter_map(|s| u32::try_from(s).ok()).map(FollowKind::Jump);
    FollowKind::NAMED.into_iter().chain(jumps).collect()
}

/// Draws the section: in a row a column of [`WIDTH`] at the left of `area`, stacked
/// [`STACK_HEIGHT`] at its top. Returns the space it took.
pub fn show(ui: &mut Ui, area: Rect, info: &FollowInfo, stacked: bool, actions: &mut Vec<Action>) -> Rect {
    let column = if stacked {
        Rect::from_min_size(area.min, Vec2::new(area.width(), STACK_HEIGHT))
    } else {
        Rect::from_min_max(area.min, Pos2::new(area.left() + WIDTH, area.bottom()))
    }
    .intersect(area);
    let painter = ui.painter_at(column);
    painter.fill_background(column, 0.0, theme::panel_background());
    let f = &info.follow;
    let id = Id::new(("follow", info.track.0, info.scene));

    // The title, and the switch that turns the follow action on or off.
    let title = Rect::from_min_size(column.min, Vec2::new(column.width(), ROW)).shrink2(Vec2::new(PAD, 0.0));
    widgets::text_in(
        &painter,
        title,
        Align2::LEFT_CENTER,
        "Follow action",
        theme::ui_font(theme::TEXT),
        theme::text_primary(),
    );
    let switch_rect = Rect::from_center_size(
        Pos2::new(title.right() - SWITCH_WIDTH / 2.0, title.center().y),
        Vec2::new(SWITCH_WIDTH, FIELD),
    );
    let label = if f.enabled { "Follow action on" } else { "Follow action off" };
    if switch(ui, &painter, switch_rect, id.with("on"), label, f.enabled, true) {
        actions.push(info.action(json!({ "enabled": !f.enabled })));
    }

    let mut y = column.top() + ROW + PAD;
    let mut row = || {
        let r = Rect::from_min_max(Pos2::new(column.left() + PAD, y), Pos2::new(column.right() - PAD, y + FIELD));
        y += FIELD_ROW;
        r
    };
    // Off, the settings are still there to change; they read as inactive.
    let label_color = if f.enabled { theme::text_secondary() } else { theme::text_hint() };
    let label_of = |painter: &egui::Painter, r: Rect, text: &str| {
        let rect = Rect::from_min_size(r.min, Vec2::new(LABEL_WIDTH, r.height()));
        widgets::text_in(painter, rect, Align2::LEFT_CENTER, text, theme::ui_font(theme::TEXT_SMALL), label_color);
    };
    let field_of = |r: Rect| Rect::from_min_max(Pos2::new(r.left() + LABEL_WIDTH, r.top()), r.max);

    let r = row();
    label_of(&painter, r, "Time");
    if let Some(time) = time_field(ui, &painter, field_of(r), id.with("time"), info) {
        actions.push(info.action(json!({ "time": time })));
    }

    let r = row();
    label_of(&painter, r, "Linked");
    let linked = Rect::from_min_size(field_of(r).min, Vec2::new(SWITCH_WIDTH, FIELD));
    let label = if f.linked { "Linked to the clip's length" } else { "Not linked to the clip's length" };
    if switch(ui, &painter, linked, id.with("linked"), label, f.linked, false) {
        actions.push(info.action(json!({ "linked": !f.linked })));
    }

    for (name, kind, chance, key, chance_key, default) in [
        ("A", f.action_a, f.chance_a, "actionA", "chanceA", FollowAction::default().chance_a),
        ("B", f.action_b, f.chance_b, "actionB", "chanceB", FollowAction::default().chance_b),
    ] {
        let r = row();
        label_of(&painter, r, &format!("Action {name}"));
        if let Some(picked) = action_field(ui, &painter, field_of(r), id.with(key), name, kind, info.scenes) {
            actions.push(info.action(json!({ key: picked.name() })));
        }
        let r = row();
        label_of(&painter, r, &format!("Chance {name}"));
        if let Some(value) = chance_field(ui, &painter, field_of(r), id.with(chance_key), name, chance, default) {
            actions.push(info.action(json!({ chance_key: value })));
        }
    }
    column
}

/// An on/off switch: "On" or "Off", lit when on (in the accent for the section's own switch).
/// Returns whether it was clicked.
fn switch(ui: &Ui, painter: &egui::Painter, rect: Rect, id: Id, label: &str, on: bool, accent: bool) -> bool {
    let response = widgets::button_area(ui, rect, id, label);
    let (fill, text) = if accent {
        let lit = motion::toward(ui.ctx(), response.id.with("on"), on, motion::VALUE);
        let off = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        (motion::mix(off, theme::accent(), lit), motion::mix(theme::text_secondary(), theme::text_inverse(), lit))
    } else {
        widgets::toggle_colors(ui, &response, on)
    };
    painter.fill_background(rect, 0.0, fill);
    let word = if on { "On" } else { "Off" };
    widgets::text_in(painter, rect, Align2::CENTER_CENTER, word, theme::ui_font(theme::TEXT_SMALL), text);
    response.clicked()
}

/// A value field's background, the fill of `share` 0 … 1 from the left, and its value.
fn value_look(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    response: &Response,
    share: Option<f64>,
    text: &str,
    dim: bool,
) {
    let fill = if response.dragged() {
        theme::control_active()
    } else {
        motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, response))
    };
    painter.fill_background(rect, 0.0, fill);
    if let Some(share) = share {
        let width = rect.width() * share.clamp(0.0, 1.0) as f32;
        painter.fill_background(
            Rect::from_min_size(rect.min, Vec2::new(width, rect.height())),
            0.0,
            theme::control_playing(),
        );
    }
    let color = if dim { theme::text_hint() } else { theme::text_value() };
    widgets::text_in(
        painter,
        rect.shrink2(Vec2::new(4.0, 0.0)),
        Align2::RIGHT_CENTER,
        text,
        theme::mono_font(theme::TEXT_SMALL),
        color,
    );
}

/// The follow time: dragged in sixteenths, a double-click resets it to one bar. Linked, it shows
/// the time it keeps for when it is not, dimmed. Returns the time to set, in beats.
fn time_field(ui: &Ui, painter: &egui::Painter, rect: Rect, id: Id, info: &FollowInfo) -> Option<f64> {
    let f = &info.follow;
    let mut preview = ui.data(|d| d.get_temp::<Option<f64>>(id)).flatten();
    let shown = format_time(preview.unwrap_or(f.time), info.bar_beats);
    let response = widgets::value_area(ui, rect, id, "Follow time", &shown);
    let range = (TIME_STEP, FollowAction::MAX_TIME);
    let done =
        match widgets::drag_value(ui, &response, &mut preview, f.time, TIME_PER_POINT, 1.0, range, info.bar_beats) {
            Drag::Done(value) | Drag::Set(value) => {
                Some(((value / TIME_STEP).round() * TIME_STEP).clamp(range.0, range.1))
            }
            Drag::None | Drag::Moving(_) => None,
        };
    ui.data_mut(|d| d.insert_temp(id, preview));
    let shown = format_time(preview.map_or(f.time, |p| (p / TIME_STEP).round() * TIME_STEP), info.bar_beats);
    value_look(ui, painter, rect, &response, None, &shown, f.linked || !f.enabled);
    done.filter(|t| (t - f.time).abs() > f64::EPSILON)
}

/// A chance, 0 … 100, with its share shown as a fill; a double-click resets it. Returns the
/// chance to set.
fn chance_field(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    id: Id,
    name: &str,
    chance: u32,
    default: u32,
) -> Option<u32> {
    let max = f64::from(FollowAction::MAX_CHANCE);
    let mut preview = ui.data(|d| d.get_temp::<Option<f64>>(id)).flatten();
    let response = widgets::value_area(ui, rect, id, format!("Chance {name}"), chance);
    let value = f64::from(chance);
    let done = match widgets::drag_value(
        ui,
        &response,
        &mut preview,
        value,
        CHANCE_PER_POINT,
        1.0,
        (0.0, max),
        f64::from(default),
    ) {
        Drag::Done(v) | Drag::Set(v) => Some(v.round().clamp(0.0, max) as u32),
        Drag::None | Drag::Moving(_) => None,
    };
    ui.data_mut(|d| d.insert_temp(id, preview));
    let shown = preview.map_or(value, f64::round);
    value_look(ui, painter, rect, &response, Some(shown / max), &format!("{shown:.0}"), chance == 0);
    done.filter(|c| *c != chance)
}

/// An action's dropdown: the named actions and a jump to each scene. Returns the one picked,
/// when it is not the current one.
fn action_field(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    id: Id,
    name: &str,
    current: FollowKind,
    scenes: usize,
) -> Option<FollowKind> {
    let shown = action_label(current);
    let response = widgets::button_area(ui, rect, id, format!("Action {name}: {shown}"));
    widgets::dropdown(ui, painter, rect, &response, &shown);
    let mut picked = None;
    egui::Popup::menu(&response).show(|ui| {
        let text = |t: String| egui::RichText::new(t).font(theme::ui_font(theme::TEXT_SMALL));
        for kind in action_choices(scenes) {
            if ui.selectable_label(kind == current, text(action_label(kind))).clicked() && kind != current {
                picked = Some(kind);
            }
        }
    });
    picked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo::demo_project;

    #[test]
    fn a_slot_clip_shows_its_follow_action() {
        let mut project = demo_project();
        let column = project.launcher.columns.iter_mut().find(|c| c.track == TrackId(2)).expect("Bass has slots");
        let clip = column.slots[1].clip.as_mut().expect("Bass has a clip in scene 2");
        clip.follow_action =
            FollowAction { enabled: true, action_b: FollowKind::Jump(0), chance_b: 30, ..Default::default() };
        let info = FollowInfo::build(&project, TrackId(2), 1).expect("the clip");
        assert_eq!(
            (info.track, info.scene, info.scenes, info.bar_beats),
            (TrackId(2), 1, project.launcher.scenes.len(), 4.0)
        );
        assert!(info.follow.enabled);
        assert_eq!((info.follow.action_b, info.follow.chance_b), (FollowKind::Jump(0), 30));
        // An empty slot, a scene past the last, or a track without slots shows nothing.
        assert_eq!(FollowInfo::build(&project, TrackId(2), 99), None);
        assert_eq!(FollowInfo::build(&project, TrackId(99), 0), None);
    }

    #[test]
    fn a_change_asks_for_the_command_with_its_slot() {
        let info = FollowInfo::build(&demo_project(), TrackId(2), 1).expect("the clip");
        let action = info.action(json!({ "chanceA": 40 }));
        assert_eq!(action.id, "clip.setFollowAction");
        assert_eq!(action.params, json!({ "track": 2, "scene": 2, "chanceA": 40 }));
    }

    #[test]
    fn times_read_in_bars_beats_and_sixteenths() {
        assert_eq!(format_time(4.0, 4.0), "1.0");
        assert_eq!(format_time(6.0, 4.0), "1.2");
        assert_eq!(format_time(2.5, 4.0), "0.2.2");
        assert_eq!(format_time(0.25, 4.0), "0.0.1");
        assert_eq!(format_time(9.0, 3.0), "3.0");
        assert_eq!(format_time(7.0, 3.5), "2.0");
        assert_eq!(format_time(f64::NAN, 4.0), "-");
        assert_eq!(format_time(4.0, 0.0), "-");
    }

    #[test]
    fn the_dropdown_lists_every_action_and_each_scene() {
        let choices = action_choices(3);
        assert_eq!(choices.len(), FollowKind::NAMED.len() + 3);
        assert_eq!(choices.last(), Some(&FollowKind::Jump(2)));
        let labels: Vec<String> = choices.iter().map(|k| action_label(*k)).collect();
        assert_eq!(labels[..2], ["Nothing".to_string(), "Stop".to_string()]);
        assert_eq!(labels.last().map(String::as_str), Some("Jump to scene 3"));
        assert_eq!(action_choices(0).len(), FollowKind::NAMED.len());
    }
}
