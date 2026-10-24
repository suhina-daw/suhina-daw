// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A device group's panel: its branches with their volume, pan, mute and solo, its Mix and Gain.

use super::*;

/// Width of a device group's panel in a row, and of its Mix and Gain in the header.
pub(super) const GROUP_WIDTH: f32 = 260.0;

pub(super) const MIX_BOX: f32 = 62.0;

pub(super) const GAIN_BOX: f32 = 66.0;

/// Height of one branch's row in a group's panel, and of its Mix and Gain rows.
pub(super) const BRANCH_ROW: f32 = 20.0;

/// Width of a branch's M and S buttons, and of its volume and pan values.
pub(super) const BRANCH_BUTTON: f32 = 16.0;

pub(super) const BRANCH_VALUE: f32 = 44.0;

/// The branch of the group at `at` whose devices the chain shows, kept per group while the app
/// runs (the first one until another is clicked).
pub(super) fn shown_branch(ui: &Ui, track: TrackId, at: &DeviceAt, branches: usize) -> usize {
    let id = Id::new(("group-branch", track.0, at));
    ui.data(|d| d.get_temp::<usize>(id)).unwrap_or(0).min(branches.saturating_sub(1))
}

/// The devices of `devices` as the chain shows them, in order: each device, and after a group
/// the devices of its shown branch, nested groups the same way.
pub(super) fn flattened<'a>(ui: &Ui, track: TrackId, devices: &'a [DeviceBox], out: &mut Vec<&'a DeviceBox>) {
    for device in devices {
        out.push(device);
        if let Some(group) = &device.group
            && let Some(branch) = group.branches.get(shown_branch(ui, track, &device.at, group.branches.len()))
        {
            flattened(ui, track, &branch.devices, out);
        }
    }
}

/// How tall a group's panel is: its header (with Mix and Gain), a row per branch and the row
/// that adds one.
pub(super) fn group_height(group: &GroupBox) -> f32 {
    ROW + PAD + (group.branches.len() + 1) as f32 * BRANCH_ROW + PAD
}

/// A value dragged sideways in a group's panel: the change a drag this frame makes, in
/// `per_point` units per point, and whether the drag is still on. A double-click asks for the
/// default.
pub(super) struct Dragged {
    pub(super) delta: f64,
    pub(super) dragging: bool,
    pub(super) ended: bool,
    pub(super) reset: bool,
}

pub(super) fn value_row(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    id: Id,
    label: &str,
    text: &str,
    per_point: f64,
) -> Dragged {
    let response = ui.interact(rect, id, Sense::click_and_drag()).on_hover_text("Drag sideways; double-click resets");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, format!("{label} {text}")));
    let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
    painter.rect_filled(rect, 0.0, fill);
    let inner = rect.shrink2(Vec2::new(PAD / 2.0, 0.0));
    if !label.is_empty() {
        widgets::text_in(
            painter,
            inner,
            Align2::LEFT_CENTER,
            label,
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_hint(),
        );
    }
    let align = if label.is_empty() { Align2::CENTER_CENTER } else { Align2::RIGHT_CENTER };
    widgets::text_in(painter, inner, align, text, theme::ui_font(theme::TEXT_SMALL), theme::text_value());
    Dragged {
        delta: f64::from(response.drag_delta().x) * per_point,
        dragging: response.dragged() && !response.drag_stopped(),
        ended: response.drag_stopped(),
        reset: response.double_clicked(),
    }
}

/// A branch value (volume or pan) dragged: shown from a draft while the drag lasts and set with
/// one command when it ends, so a drag is one undo step.
pub(super) fn branch_value(ui: &Ui, id: Id, stored: f64, dragged: &Dragged, min: f64, max: f64) -> (f64, Option<f64>) {
    let draft = ui.data(|d| d.get_temp::<f64>(id));
    if dragged.reset {
        ui.data_mut(|d| d.remove::<f64>(id));
        // Both reset to 0: 0 dB, or the centre.
        return (stored, Some(0.0f64.clamp(min, max)));
    }
    if dragged.dragging {
        let next = (draft.unwrap_or(stored) + dragged.delta).clamp(min, max);
        ui.data_mut(|d| d.insert_temp(id, next));
        return (next, None);
    }
    if dragged.ended {
        ui.data_mut(|d| d.remove::<f64>(id));
        let value = (draft.unwrap_or(stored) + dragged.delta).clamp(min, max);
        return (value, (value != stored).then_some(value));
    }
    (draft.unwrap_or(stored), None)
}

/// Where a loop's start or end may be dragged: the start from 0 to a quarter beat before the end,
/// the end from a quarter beat after the start. Never crossed (`max` at least `min`), so a loop
/// shorter than that (from a command or an import) does not make `f64::clamp` panic.
pub(super) fn loop_bounds(edit: ClipEdit, start: f64, end: f64) -> (f64, f64) {
    let (min, max) = match edit {
        ClipEdit::LoopStart => (0.0, (end - 0.25).min(120_000.0)),
        // Never past the longest loop, even for a start within a quarter beat of it.
        _ => ((start + 0.25).min(120_000.0), 120_000.0),
    };
    let min = if min.is_finite() { min } else { 0.0 };
    (min, if max.is_finite() { max.max(min) } else { min })
}

pub(super) fn pan_text(pan: f64) -> String {
    let amount = (pan.abs() * 50.0).round();
    if amount < 1.0 {
        "C".into()
    } else if pan < 0.0 {
        format!("{amount}L")
    } else {
        format!("{amount}R")
    }
}

pub(super) fn db_text(db: f64) -> String {
    if db.is_finite() { format!("{db:.1} dB") } else { "-inf dB".into() }
}

/// A device group's panel: its header (switch, name, Mix, Gain, remove), one row per branch
/// (its name, which shows its devices after the panel, its volume and pan, M and S) and a row
/// that adds a branch (`docs/spec/device-groups.md` › The device panel). Returns whether the
/// panel was clicked.
#[allow(clippy::too_many_arguments)]
pub(super) fn group_panel(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    track: TrackId,
    device: &DeviceBox,
    group: &GroupBox,
    selected: bool,
    actions: &mut Vec<Action>,
) -> bool {
    let at = device.at.value();
    let panel = ui
        .interact(rect, Id::new(("device-panel", track.0, &device.at)), Sense::click_and_drag())
        .on_hover_text("Right-click for the group's menu; drag onto another track to move it there, Ctrl to copy it");
    if panel.drag_started_by(egui::PointerButton::Primary) && device.at.top() {
        crate::interaction::drop::start_device_carry(ui.ctx(), track, device.index);
    }
    panel.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Device {}", device.name)));
    let clicked = panel.clicked();
    panel.context_menu(|ui| menus::device_menu(ui, track, &device.at, Some(group.branches.len()), actions));
    painter.rect_filled(rect, 0.0, theme::panel_background());
    let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), ROW));
    painter.rect_filled(header, 0.0, theme::control_background());
    let switch = Rect::from_min_size(
        Pos2::new(header.left() + PAD, header.center().y - ON_SWITCH / 2.0),
        Vec2::splat(ON_SWITCH),
    );
    let label = if device.enabled { format!("Bypass {}", device.name) } else { format!("Enable {}", device.name) };
    let bypass = widgets::button_area(ui, switch, Id::new(("device-bypass", track.0, &device.at)), &label);
    if bypass.clicked() {
        actions.push(Action::new(
            "device.setEnabled",
            json!({ "track": track.0, "device": at, "enabled": !device.enabled }),
        ));
    }
    if device.enabled {
        painter.rect_filled(switch, 0.0, theme::text_primary());
    } else {
        painter.rect_stroke(switch, 0.0, theme::hairline(theme::text_hint()), egui::StrokeKind::Inside);
    }
    let close = Rect::from_center_size(Pos2::new(header.right() - PAD - 6.0, header.center().y), Vec2::splat(12.0));
    let remove = widgets::button_area(
        ui,
        close,
        Id::new(("device-remove", track.0, &device.at)),
        format!("Remove {}", device.name),
    );
    if remove.clicked() {
        actions.push(Action::new("device.remove", json!({ "track": track.0, "device": at })));
    }
    let cross = if remove.hovered() { theme::text_primary() } else { theme::text_hint() };
    for (from, to) in [(Vec2::new(-3.5, -3.5), Vec2::new(3.5, 3.5)), (Vec2::new(-3.5, 3.5), Vec2::new(3.5, -3.5))] {
        painter.line_segment([close.center() + from, close.center() + to], egui::Stroke::new(1.2, cross));
    }
    // Mix and Gain sit in the header, each dragged sideways; the engine smooths what they send.
    let gain_row = Rect::from_min_max(
        Pos2::new(close.left() - 4.0 - GAIN_BOX, header.top() + 2.0),
        Pos2::new(close.left() - 4.0, header.bottom() - 2.0),
    );
    let mix_row = Rect::from_min_max(
        Pos2::new(gain_row.left() - 2.0 - MIX_BOX, gain_row.top()),
        Pos2::new(gain_row.left() - 2.0, gain_row.bottom()),
    );
    let name_color = if device.enabled { theme::text_primary() } else { theme::text_hint() };
    let name = Rect::from_min_max(
        Pos2::new(switch.right() + PAD, header.top()),
        Pos2::new((mix_row.left() - PAD).max(switch.right() + PAD), header.bottom()),
    );
    widgets::text_in(painter, name, Align2::LEFT_CENTER, &device.name, theme::ui_font(theme::TEXT), name_color);
    let mix = value_row(
        ui,
        painter,
        mix_row,
        Id::new(("group-mix", track.0, &device.at)),
        "Mix",
        &format!("{:.0}%", group.mix * 100.0),
        0.005,
    );
    let mix_value = if mix.reset {
        Some(1.0)
    } else {
        (mix.delta != 0.0 || mix.ended).then(|| (group.mix + mix.delta).clamp(0.0, 1.0))
    };
    if let Some(value) = mix_value {
        actions.push(Action::new(
            "device.setParam",
            json!({ "track": track.0, "device": at, "param": "mix", "value": value, "continuous": mix.dragging }),
        ));
    }
    let silent = group.gain_db <= suhina_engine::SILENT_DB;
    let gain = value_row(
        ui,
        painter,
        gain_row,
        Id::new(("group-gain", track.0, &device.at)),
        "Gain",
        &if silent { "-inf".into() } else { format!("{:.1}", group.gain_db) },
        0.1,
    );
    let gain_value = if gain.reset {
        Some(0.0)
    } else {
        (gain.delta != 0.0 || gain.ended).then(|| (group.gain_db + gain.delta).clamp(suhina_engine::SILENT_DB, 12.0))
    };
    if let Some(value) = gain_value {
        actions.push(Action::new(
            "device.setParam",
            json!({ "track": track.0, "device": at, "param": "gain", "value": value, "continuous": gain.dragging }),
        ));
    }
    let left = rect.left() + PAD;
    let width = rect.width() - PAD * 2.0;
    let mut y = header.bottom() + PAD;

    let shown = shown_branch(ui, track, &device.at, group.branches.len());
    for (index, branch) in group.branches.iter().enumerate() {
        let row = Rect::from_min_size(Pos2::new(left, y), Vec2::new(width, BRANCH_ROW - 2.0));
        y += BRANCH_ROW;
        let solo = Rect::from_min_size(
            Pos2::new(row.right() - BRANCH_BUTTON, row.top()),
            Vec2::new(BRANCH_BUTTON, row.height()),
        );
        let mute = solo.translate(Vec2::new(-BRANCH_BUTTON - 2.0, 0.0));
        let pan = Rect::from_min_size(Pos2::new(mute.left() - 2.0 - 32.0, row.top()), Vec2::new(32.0, row.height()));
        let volume = Rect::from_min_size(
            Pos2::new(pan.left() - 2.0 - BRANCH_VALUE, row.top()),
            Vec2::new(BRANCH_VALUE, row.height()),
        );
        let name = Rect::from_min_max(row.min, Pos2::new(volume.left() - 2.0, row.bottom()));
        let pick = widgets::button_area(
            ui,
            name,
            Id::new(("group-branch-name", track.0, &device.at, index)),
            format!("Show branch {}", branch.name),
        );
        if pick.clicked() {
            ui.data_mut(|d| d.insert_temp(Id::new(("group-branch", track.0, &device.at)), index));
        }
        let lit = index == shown;
        let fill = if lit {
            theme::control_active()
        } else {
            motion::mix(theme::window_background(), theme::control_hover(), widgets::hover(ui, &pick))
        };
        painter.rect_filled(name, 0.0, fill);
        let color = if branch.mute { theme::text_hint() } else { theme::text_secondary() };
        widgets::text_in(
            painter,
            name.shrink2(Vec2::new(4.0, 0.0)),
            Align2::LEFT_CENTER,
            &branch.name,
            theme::ui_font(theme::TEXT_SMALL),
            color,
        );

        let volume_id = Id::new(("branch-volume", track.0, &device.at, index));
        let shown_db = ui.data(|d| d.get_temp::<f64>(volume_id)).unwrap_or(branch.volume_db);
        let dragged = value_row(ui, painter, volume, volume_id.with("drag"), "", &db_text(shown_db), 0.1);
        let stored = if branch.volume_db.is_finite() { branch.volume_db } else { -70.0 };
        let (_, set) = branch_value(ui, volume_id, stored, &dragged, -70.0, 6.0);
        if let Some(db) = set {
            actions.push(Action::new(
                "branch.setVolume",
                json!({ "track": track.0, "device": at, "branch": index, "db": commands::exact_db_param(db, -70.0) }),
            ));
        }
        let pan_id = Id::new(("branch-pan", track.0, &device.at, index));
        let shown_pan = ui.data(|d| d.get_temp::<f64>(pan_id)).unwrap_or(branch.pan);
        let dragged = value_row(ui, painter, pan, pan_id.with("drag"), "", &pan_text(shown_pan), 0.01);
        let (_, set) = branch_value(ui, pan_id, branch.pan, &dragged, -1.0, 1.0);
        if let Some(value) = set {
            actions.push(Action::new(
                "branch.setPan",
                json!({ "track": track.0, "device": at, "branch": index, "pan": value }),
            ));
        }

        let response = widgets::button_area(
            ui,
            mute,
            Id::new(("branch-mute", track.0, &device.at, index)),
            if branch.mute { format!("Unmute branch {}", branch.name) } else { format!("Mute branch {}", branch.name) },
        );
        let (fill, text) = widgets::toggle_colors(ui, &response, branch.mute);
        painter.rect_filled(mute, 0.0, fill);
        widgets::text_in(painter, mute, Align2::CENTER_CENTER, "M", theme::ui_font(theme::TEXT_SMALL), text);
        if response.clicked() {
            actions.push(Action::new(
                "branch.setMute",
                json!({ "track": track.0, "device": at, "branch": index, "mute": !branch.mute }),
            ));
        }
        let response = widgets::button_area(
            ui,
            solo,
            Id::new(("branch-solo", track.0, &device.at, index)),
            if branch.solo {
                format!("End solo on branch {}", branch.name)
            } else {
                format!("Solo branch {}", branch.name)
            },
        );
        let lit = motion::toward(ui.ctx(), response.id.with("on"), branch.solo, motion::VALUE);
        let off = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        painter.rect_filled(solo, 0.0, motion::mix(off, theme::accent(), lit));
        let text = motion::mix(theme::text_secondary(), theme::window_background(), lit);
        widgets::text_in(painter, solo, Align2::CENTER_CENTER, "S", theme::ui_font(theme::TEXT_SMALL), text);
        if response.clicked() {
            actions.push(Action::new(
                "branch.setSolo",
                json!({ "track": track.0, "device": at, "branch": index, "solo": !branch.solo }),
            ));
        }
    }
    let add = Rect::from_min_size(Pos2::new(left, y), Vec2::new(width, BRANCH_ROW - 2.0));
    let response = widgets::button_area(
        ui,
        add,
        Id::new(("branch-add", track.0, &device.at)),
        format!("Add a branch to {}", device.name),
    );
    let fill = motion::mix(theme::window_background(), theme::control_hover(), widgets::hover(ui, &response));
    painter.rect_filled(add, 0.0, fill);
    widgets::text_in(
        painter,
        add,
        Align2::CENTER_CENTER,
        "+ Branch",
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_hint(),
    );
    if response.clicked() {
        actions.push(Action::new("branch.add", json!({ "track": track.0, "device": at })));
    }
    if selected {
        painter.rect_stroke(rect, 1.0, theme::hairline(theme::selection()), egui::StrokeKind::Inside);
    }
    clicked
}

/// The line over a device inside a group, so the devices after a group's panel read as its
/// shown branch: one step lower for each group it is in.
pub(super) fn inside_group_mark(painter: &egui::Painter, rect: Rect, at: &DeviceAt) {
    let depth = at.0.len() / 2;
    if depth == 0 {
        return;
    }
    for level in 0..depth {
        let y = rect.top() + level as f32 * 3.0 + 1.0;
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            egui::Stroke::new(2.0, theme::accent()),
        );
    }
}
