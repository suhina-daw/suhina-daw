// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A track's header beside its lane: name, routing, inputs, meter and the color band.

use super::*;

/// The output dropdown: "→ Main" or a group; picking one runs `track.setOutput`.
pub(super) fn routing(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    lane: &Lane,
    groups: &[(TrackId, String)],
    background: Color32,
    actions: &mut Vec<Action>,
) {
    let current = lane.output_name.as_deref().unwrap_or("Main");
    let label = Rect::from_min_size(rect.min, Vec2::new(18.0, rect.height()));
    widgets::text_in(
        painter,
        label,
        Align2::LEFT_CENTER,
        "→",
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_on(background),
    );
    let field = Rect::from_min_max(Pos2::new(label.right(), rect.top()), rect.max);
    let mut picked = None;
    let response = widgets::button_area(
        ui,
        field,
        Id::unique(("output", lane.track.0)),
        format!("{} output: {current}", lane.name),
    );
    widgets::subtle_dropdown(ui, painter, field, &response, current, background);
    egui::Popup::menu(&response).show(|ui| {
        if crate::kit::menu::item(ui, "Main", lane.output_name.is_none()).clicked() && lane.output_name.is_some() {
            picked = Some(serde_json::Value::from("main"));
        }
        for (group, name) in groups.iter().filter(|(g, _)| *g != lane.track) {
            if crate::kit::menu::item(ui, name, lane.output == Some(*group)).clicked() && lane.output != Some(*group) {
                picked = Some(serde_json::Value::from(group.0));
            }
        }
    });
    if let Some(to) = picked {
        actions.push(Action::new("track.setOutput", json!({ "track": lane.track.0, "to": to })));
    }
}

/// The MIDI input dropdown (ADR 0017): "MIDI" and what the track hears while armed; its menu
/// runs `track.setMidiInput`.
pub(super) fn midi_input(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    lane: &Lane,
    ports: &[String],
    background: Color32,
    actions: &mut Vec<Action>,
) {
    let current = menus::midi_input_text(&lane.midi_input);
    let label = Rect::from_min_size(rect.min, Vec2::new(28.0, rect.height()));
    widgets::text_in(
        painter,
        label,
        Align2::LEFT_CENTER,
        "MIDI",
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_on(background),
    );
    let field = Rect::from_min_max(Pos2::new(label.right(), rect.top()), rect.max);
    let response = widgets::button_area(
        ui,
        field,
        Id::unique(("midi-input", lane.track.0)),
        format!("{} MIDI input: {current}", lane.name),
    );
    widgets::subtle_dropdown(ui, painter, field, &response, &current, background);
    egui::Popup::menu(&response).show(|ui| menus::midi_input_choices(ui, lane.track, &lane.midi_input, ports, actions));
}

/// The audio input dropdown (ADR 0017): "In" and the track's input channels and monitoring;
/// its menu runs `track.setAudioInput`.
pub(super) fn audio_input(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    lane: &Lane,
    channels: u16,
    background: Color32,
    actions: &mut Vec<Action>,
) {
    let current = menus::audio_input_text(&lane.audio_input);
    let label = Rect::from_min_size(rect.min, Vec2::new(28.0, rect.height()));
    widgets::text_in(
        painter,
        label,
        Align2::LEFT_CENTER,
        "Audio",
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_on(background),
    );
    let field = Rect::from_min_max(Pos2::new(label.right(), rect.top()), rect.max);
    let response = widgets::button_area(
        ui,
        field,
        Id::unique(("audio-input", lane.track.0)),
        format!("{} audio input: {current}", lane.name),
    );
    widgets::subtle_dropdown(ui, painter, field, &response, &current, background);
    egui::Popup::menu(&response)
        .show(|ui| menus::audio_input_choices(ui, lane.track, &lane.audio_input, channels, actions));
}

/// Width of the meter lying in a return or main row, left of its volume.
pub(super) const LYING_METER: f32 = 48.0;

/// Where a return or main row shows its meter: lying down, ending at `right`, centered on `y`.
pub(super) fn lying_meter(right: f32, y: f32) -> Rect {
    Rect::from_min_max(Pos2::new(right - LYING_METER, y - meter::WIDTH / 2.0), Pos2::new(right, y + meter::WIDTH / 2.0))
}

/// A header's color band in `band`, where its layout puts it: the track's color down its left
/// edge, the full height of the header, at full strength while the track is off too, as the one
/// place its color stays beside the gray header (#1393). A click opens the palette.
pub(super) fn color_band(
    ui: &Ui,
    painter: &egui::Painter,
    band: Rect,
    clip: Rect,
    lane: &Lane,
    id: Id,
    actions: &mut Vec<Action>,
) {
    painter.rect_filled(band, 0.0, color_picker::fill(lane.color, false));
    if band.intersects(clip) {
        color_picker::area(ui, band.intersect(clip), id.with("color"), lane.track, &lane.name, lane.color, actions);
    }
}

/// Where a header stands among the drawn lanes: how many lanes are inside it (a group's fold
/// triangle shows when there are any) and the tracks in the order drawn, for a Shift-click's run.
pub(super) struct HeaderOrder<'a> {
    pub(super) children: usize,
    pub(super) tracks: &'a [TrackId],
}

/// The track list's groups, for a header's menu.
pub(super) struct Nest<'a> {
    /// Every track but the returns, in track order.
    pub(super) places: &'a [Place],
    /// The group tracks, with their names.
    pub(super) groups: &'a [(TrackId, String)],
}

#[allow(clippy::too_many_arguments)]
pub(super) fn track_header(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    lanes_area: Rect,
    lane: &Lane,
    track_number: usize,
    nest: &Nest<'_>,
    order: &HeaderOrder<'_>,
    timeline: &Timeline,
    state: &mut ArrangementViewState,
    actions: &mut Vec<Action>,
) {
    let mut work_trace = crate::kit::frame_trace::WorkSpan::new("arrangement track header");
    // The click is taken before the header's element is entered, so the frame that selects the
    // track paints it from `track-header[selected]`, border included, not from the unselected look.
    let id = Id::unique(("arrangement-header", lane.track.0));
    let clip = rect.intersect(lanes_area);

    // The header itself: click selects (Ctrl adds, Shift takes a run), right-click opens the
    // track's menu, a drag moves the track. Its controls, added after it, take their own clicks.
    let body = ui.interact(clip, id.with("body"), Sense::click_and_drag());
    body.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Track {}", lane.name)));
    if body.clicked() {
        state.targeted = Some(lane.track);
        let (shift, command) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
        // A clip selected on another track would keep the detail area on that track, which is
        // what it shows first: one plain click makes this track the one worked on, as a
        // double-click does, without a group's double-click folding it (#1339).
        if !shift && !command && state.selected_clip().is_some_and(|(track, _)| track != lane.track) {
            state.clear_selected_clip();
        }
        state.pick_track(lane.track, shift, command, order.tracks);
        // The track, not a clip, is what Delete acts on now (#690); a clip on it stays selected,
        // and the detail area shows the track's devices (#950).
        state.track_focus = true;
        state.devices_clicked = true;
        state.automation.let_go();
    }
    // A container header double-click folds it; an ordinary track opens its devices.
    if body.double_clicked() {
        if lane.kind == ColumnKind::Group {
            state.folds.toggle(lane.track);
            ui.ctx().request_repaint();
        } else {
            state.open_devices = Some(lane.track);
        }
    }
    if body.drag_started_by(egui::PointerButton::Primary) {
        state.gesture.begin(Gesture::Track(nesting::TrackDrag::new(nest.places, lane.track, &state.selected_tracks)));
    }
    if body.drag_stopped_by(egui::PointerButton::Primary)
        && let Some(dragged) = state.gesture.end_track(|t| t.source == lane.track)
        && let Some(pointer) = body.interact_pointer_pos().or_else(|| ui.ctx().pointer_latest_pos())
    {
        state.track_drop = Some((dragged, pointer.y));
    }
    // While the color tool edits the track, its header is styled as neither selected nor muted,
    // so a sheet's selected or muted background never hides the color being chosen.
    let preview = crate::kit::selection::previews_track(ui.ctx(), lane.track);
    let own = crate::kit::element::TRACK_HEADER
        .id(&format!("track-{}", lane.track.0))
        .color(rgb(lane.color))
        .attr(crate::kit::element::Selected, state.selected_tracks.contains(&lane.track) && !preview)
        .attr(crate::kit::element::Muted, lane.mute && !preview)
        .attr(crate::kit::element::Armed, lane.armed)
        .enter();
    work_trace.mark("style");
    // What the header holds goes in its content box, inside its border and padding (#1336), and
    // its rows are as tall as that box allows.
    let inside = own.content_in(rect);
    // A track inside groups has their colors down its indent, the outermost first, so a group and
    // its tracks read as one block: the group's color runs from its header down all of them.
    let mut ancestors = Vec::new();
    let mut parent = lane.parent;
    while let Some(group) = parent.filter(|_| ancestors.len() < lane.depth) {
        let Some(group_lane) = timeline.lanes.iter().find(|l| l.track == group) else { break };
        ancestors.push(group_lane.color);
        parent = group_lane.parent;
    }
    // Its parts, as the sheet lays them out for a header of its kind and size (#1453).
    let has_takes = timeline.takes.iter().any(|t| t.track == lane.track);
    let mut parts = super::header_layout::layout(
        Some(&own),
        rect,
        super::header_layout::Header {
            container: lane.kind == ColumnKind::Group,
            headers_left: state.headers_left,
            depth: lane.depth,
            ancestors: ancestors.len(),
            armable: lane.armable,
            notes: lane.plays_notes,
            takes: has_takes,
            sends: lane.sends.len(),
        },
    );
    // A presentation folder (an imported playlist group) has no mixer channel: no meter, track
    // switch, solo, cue, volume, pan, sends or routing, only its name and its fold.
    if lane.folder {
        (parts.meter, parts.track_on, parts.solo, parts.cue) = (None, None, None, None);
        (parts.volume, parts.pan, parts.output, parts.input) = (None, None, None, None);
        (parts.previous, parts.next) = (None, None);
        parts.sends.clear();
        parts.shown_sends = 0;
        parts.navigation = false;
    }
    let selection = state.selected_tracks.clone();
    // Open until a click outside: its items close it themselves, and a color dragged in its
    // picker does not (#712).
    egui::Popup::context_menu(&body).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        let into: Vec<(TrackId, String)> = nesting::move_targets(nest.places, lane.track)
            .into_iter()
            .filter_map(|g| nest.groups.iter().find(|(t, _)| *t == g).cloned())
            .collect();
        let menu = TrackMenu {
            track: lane.track,
            name: &lane.name,
            color: lane.color,
            is_group: lane.kind == ColumnKind::Group,
            is_return: lane.kind == ColumnKind::Return,
            selection: &selection,
            into: &into,
            out: nesting::out_of_group(nest.places, lane.track),
            midi: lane.plays_notes.then_some((&lane.midi_input, timeline.midi_ports.as_slice())),
            audio: lane.plays_audio.then_some((&lane.audio_input, timeline.input_channels)),
            tracks_copied: state.tracks_copied,
            key_readers: &timeline.key_readers,
        };
        menus::track_menu(ui, &menu, actions);
    });
    work_trace.mark("interaction");
    let selected = state.selected_tracks.contains(&lane.track) || state.focus == Some(lane.track);
    // A selected track's header is filled with the one selection color every selected header
    // and clip shares (Settings › Appearance › Selection color); its own color stays in its band.
    // With Clip's own color, and for the track the piano roll edits without its being selected,
    // the header keeps its color with a small brightness lift and an outline.
    let selection_fill = (state.selected_tracks.contains(&lane.track) && !lane.mute && !preview)
        .then(|| crate::kit::selection::fill(&own))
        .flatten();
    // The theme's header colors (`--track-headerBackground`, `--track-headerSelectedBackground`),
    // which the track's color is worked into; a rule for `track-header` paints over the result.
    // While the color tool edits the track, the unselected one, so the header shows the color
    // with the shading it will have.
    let background = own
        .style()
        .and_then(|style| {
            style.color_variable(if selected && !preview {
                "--track-headerSelectedBackground"
            } else {
                "--track-headerBackground"
            })
        })
        .unwrap_or(egui::Color32::TRANSPARENT);
    let strength = crate::kit::style_class::theme_header_tint().unwrap_or(state.header_color_strength);
    // A muted track's header is the `track-header[muted]` gray, as its clips are, not a dimmer
    // shade of its color (#1393); its color stays in the band down its edge. The gray is the
    // computed background, so `--track-headerMutedBackground` reaches it.
    let header_fill = if preview {
        color_surface(background, rgb(lane.color), strength)
    } else if lane.mute {
        own.computed_background_or(muted_header_gray(selected))
    } else if let Some(fill) = selection_fill {
        fill
    } else {
        let surface = color_surface(background, rgb(lane.color), strength);
        if selected { motion::mix(surface, crate::kit::element::name_ink(), 0.07) } else { surface }
    };
    if preview
        || !crate::kit::skin::paint_in_view(
            painter,
            rect,
            "track.body",
            crate::kit::skin::State::response(&body, selected),
            // Artwork's `currentColor`: the header's own `color` where the sheet gives one.
            own.own_color_or(header_fill),
        )
    {
        painter.rect_filled(rect, 0.0, header_fill);
    }
    crate::kit::liquid::paint(painter, rect, &own, header_fill, crate::kit::liquid::Surface::Track);
    // The sheet's own background for the header (`arrangement track-header`) over the native one.
    if preview {
        own.paint_over(
            painter,
            rect.intersect(clip),
            crate::kit::element::Native { fill: header_fill, ..Default::default() },
            0.0,
        );
    } else {
        own.paint(painter, rect.intersect(clip));
    }
    // The lane's whole height: what folding saves and restores, and where its resize edge is.
    let lane_rect = rect;
    let rect = inside;
    // The name and the controls' ink read on whichever background is shown.
    let header_fill = if preview { header_fill } else { own.background_or(header_fill) };
    if selected {
        // `track-header[selected]`'s border color, else a shade of the track's own.
        let outline = own.border_or(motion::mix(own.border(), saturated(rgb(lane.color)), 0.5));
        let outline = if preview { crate::kit::selection::readable(outline, header_fill) } else { outline };
        painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.0, outline), StrokeKind::Inside);
    }
    work_trace.mark("background");
    // The groups' bands down the indent, at the scale the layout gave them.
    if let Some(indent) = parts.indent {
        let mut edge = if state.headers_left { indent.right() } else { indent.left() };
        for (level, color) in ancestors.iter().rev().enumerate() {
            let width = super::header_layout::nesting_band_width(level) * parts.indent_scale;
            let band_left = if state.headers_left { edge - width } else { edge };
            let strip = Rect::from_min_size(Pos2::new(band_left, indent.top()), Vec2::new(width, indent.height()));
            if strip.intersects(clip) {
                painter.rect_filled(strip.intersect(clip), 0.0, rgb(*color));
            }
            let step = width + 2.0 * parts.indent_scale;
            edge += if state.headers_left { -step } else { step };
        }
    }
    // The track's own band follows its ancestors and opens its color palette.
    if let Some(band) = parts.band {
        color_band(ui, painter, band, clip, lane, id, actions);
    }
    let level = state.meters.track(lane.track);
    if let Some(at) = parts.meter {
        meter::show(
            ui,
            &painter.with_clip_rect(painter.clip_rect().intersect(clip)),
            at,
            &lane.name,
            meter::Source::Track(lane.track),
            &level,
            actions,
        );
    }

    work_trace.mark("meter and bands");
    // Row 1: a group's fold triangle, track on, solo, cue, arm, name. Mixer controls act on the whole
    // selection while this track is part of one.
    let controlled: Vec<u32> = state.controlled_tracks(lane.track).iter().map(|t| t.0).collect();
    let group_fold = lane.kind == ColumnKind::Group && order.children > 0;
    if let Some(fold) = parts.fold {
        let folded = if group_fold {
            state.folds.is_folded(lane.track)
        } else {
            lane_rect.height() <= f32::from(state.sizes.height(TrackSize::Compact))
        };
        let label = if group_fold {
            if folded { format!("Unfold {}", lane.name) } else { format!("Fold {}", lane.name) }
        } else if folded {
            format!("Expand {}", lane.name)
        } else {
            format!("Collapse {}", lane.name)
        };
        let target = if group_fold { parts.fold_target(inside).unwrap_or(fold) } else { fold }.intersect(clip);
        let response = widgets::button_area(ui, target, id.with("fold"), &label).on_hover_text(if group_fold {
            format!("{label}. You can also double-click the track header.")
        } else {
            label
        });
        // `fold`, `[checked]` while folded, and `fold` artwork (`.on` while folded).
        let el =
            crate::kit::element::FOLD.id("fold").response(&response).attr(crate::kit::element::Checked, folded).enter();
        let color = motion::mix(header_fill, theme::text_on(header_fill), 0.75 + 0.25 * widgets::hover(ui, &response));
        let color = el.color_or(color);
        if group_fold {
            nesting::disclosure_face(ui, painter, target, &response, &el, color);
        }
        let fold_state = crate::kit::skin::State::response(&response, folded);
        let drawn = crate::kit::skin::paint_as(&el, painter, fold, "fold", fold_state, color);
        let center = fold.center();
        let (wide, tall) = (7.0, 4.0);
        // A closed container points into its header; an open one points down.
        let direction = if group_fold && state.headers_left { -1.0 } else { 1.0 };
        let points = if folded {
            vec![
                Pos2::new(center.x - direction * tall / 2.0, center.y - wide / 2.0),
                Pos2::new(center.x - direction * tall / 2.0, center.y + wide / 2.0),
                Pos2::new(center.x + direction * tall / 2.0, center.y),
            ]
        } else {
            vec![
                Pos2::new(center.x - wide / 2.0, center.y - tall / 2.0),
                Pos2::new(center.x + wide / 2.0, center.y - tall / 2.0),
                Pos2::new(center.x, center.y + tall / 2.0),
            ]
        };
        if !drawn {
            painter.add(egui::Shape::convex_polygon(points, color, egui::Stroke::NONE));
        }
        if response.clicked() {
            if group_fold {
                state.folds.toggle(lane.track);
            } else {
                state.toggle_height(lane.track, lane_rect.height());
            }
            // What the lanes are changes without the project changing: draw the next frame.
            ui.ctx().request_repaint();
        }
    }
    // The automation button shows and hides the track's lanes.
    if let Some(lanes_button) = parts.automation.filter(|b| b.intersects(clip)) {
        automation::header_button(
            ui,
            painter,
            lanes_button,
            lane.track,
            &lane.name,
            &mut state.automation,
            &timeline.automation,
        );
    }
    // Beside the automation button below the buttons, left of it at a narrow header's right end,
    // and at the right end of a one-row header, where the automation button starts the row.
    if let Some(button) = parts.takes.filter(|b| b.intersects(clip)) {
        let hidden = state.takes_hidden.contains(&lane.track);
        let label = if hidden {
            format!("Show take lanes of {}", lane.name)
        } else {
            format!("Hide take lanes of {}", lane.name)
        };
        let response = widgets::button_area(ui, button.intersect(clip), id.with("takes"), label);
        if response.clicked() && !state.takes_hidden.remove(&lane.track) {
            state.takes_hidden.insert(lane.track);
        }
        // A `button`, `[checked]` while the take lanes show: only a rule for that state lights it.
        let lit = widgets::lit_toggle(
            ui,
            &response,
            crate::kit::element::BUTTON.builder(),
            &[crate::kit::element::Checked],
            !hidden,
        );
        let (fill, text) = lit.colors();
        lit.el.paint_over(
            painter,
            button,
            crate::kit::element::Native { radius: 0.0, fill },
            lit.toggle.image(lit.lit),
        );
        drop(lit);
        // Three stacked lines: the lanes under the track.
        let glyph = button.shrink2(Vec2::new(4.0, 4.0));
        let stroke = egui::Stroke::new(1.0, text);
        super::clip::stacked_lines(painter, glyph.x_range(), glyph.bottom(), glyph.height() / 2.0, stroke);
    }
    let name_visible = parts.name.is_some();
    let mut name = parts.name.unwrap_or(Rect::NOTHING);
    let name_style = crate::kit::element::TRACK_NAME.id(&format!("track-{}", lane.track.0)).enter();
    if name_visible {
        crate::kit::skin::paint_in_view(
            painter,
            name,
            "track.name",
            crate::kit::skin::State::response(&body, selected),
            name_style.own_color_or(header_fill),
        );
        // The sheet's own background for the name (`arrangement track-name`).
        name_style.paint(painter, name.intersect(clip));
    }
    // Ink that reads on the fill, unless the sheet gives the name a color.
    let name_color = name_style.color_or(if lane.mute {
        motion::mix(header_fill, theme::text_on(header_fill), 0.75)
    } else {
        theme::text_on(header_fill)
    });
    if name_visible && lane.kind == ColumnKind::Track && (lane.plays_notes || lane.plays_audio) {
        let type_rect = Rect::from_min_size(name.min, Vec2::new(16.0, name.height()));
        crate::kit::mixer_controls::track_type(
            ui,
            painter,
            type_rect,
            id.with("track-type"),
            &lane.name,
            (lane.plays_notes, lane.plays_audio),
            name_color,
        );
        name.min.x = (type_rect.right() + 3.0).min(name.right());
    }
    let name_size = parts.name_size;
    if name_visible {
        widgets::text_in(painter, name, Align2::LEFT_CENTER, &lane.name, theme::ui_font(name_size), name_color);
    }
    work_trace.mark("layout and name");
    // The name's place, for a rename in place (Ctrl+R, #696).
    drop(name_style);
    let pass = ui.ctx().cumulative_pass_nr();
    ui.ctx().data_mut(|d| {
        let spot = crate::app::NameSpot {
            rect: name.intersect(clip),
            pass,
            fill: header_fill,
            text: name_color,
            size: name_size,
        };
        d.insert_temp(Id::unique((crate::app::NAME_RECT_TRACK, u64::from(lane.track.0))), spot);
    });
    if let Some(solo) = parts.solo.filter(|r| r.intersects(clip))
        && let Some(action) =
            widgets::solo_button(ui, painter, solo, id.with("solo"), &controlled, &lane.name, lane.solo)
    {
        actions.push(action);
    }
    if let Some(cue) = parts.cue.filter(|r| r.intersects(clip))
        && let Some(action) = widgets::cue_button(ui, painter, cue, id.with("cue"), lane.track.0, &lane.name, lane.cue)
    {
        actions.push(action);
    }
    if let Some(arm) = parts.arm.filter(|a| a.intersects(clip))
        && let Some(action) =
            widgets::arm_button(ui, painter, arm, id.with("arm"), lane.track.0, &lane.name, lane.armed)
    {
        actions.push(action);
    }
    let number = if lane.kind == ColumnKind::Return {
        lane.name.chars().next().map(String::from).unwrap_or_default()
    } else {
        track_number.to_string()
    };
    if let Some(toggle) = parts.track_on.filter(|r| r.intersects(clip)) {
        let label = if lane.mute { format!("Turn {} on", lane.name) } else { format!("Turn {} off", lane.name) };
        let response = widgets::button_area(ui, toggle.intersect(clip), id.with("on"), label);
        // `#control-track-on`, as in the mixer: `[checked]` while the track is on, `[muted]` while
        // it is off, and `track-on` artwork (`.on`).
        if response.clicked() {
            actions.push(Action::new("track.setMute", json!({ "tracks": controlled, "mute": !lane.mute })));
        }
        // Lit only by a rule for `[checked]`: one for every `button` does not hide which tracks are on.
        let lit = widgets::lit_toggle(
            ui,
            &response,
            crate::kit::element::BUTTON.id("track-on").attr(crate::kit::element::Muted, lane.mute),
            &[crate::kit::element::Checked],
            !lane.mute,
        );
        let (fill, text) = lit.colors();
        let el = lit.el;
        widgets::toggle_surface(painter, toggle, &el, &response, fill, lit.toggle.image(lit.lit), 0.0);
        let state = crate::kit::skin::State::response(&response, !lane.mute);
        if !crate::kit::skin::paint_as(&el, painter, toggle, "track-on", state, text) {
            widgets::track_number_text(painter, toggle, &number, text, lane.mute);
        }
    }

    if let Some(at) = parts.monitor.filter(|at| at.intersects(clip)) {
        let mode = lane.midi_input.monitor;
        let text = menus::monitor_name(mode);
        let response = widgets::button_area(
            ui,
            at.intersect(clip),
            id.with("midi-monitor"),
            format!("MIDI monitoring of {}: {text}", lane.name),
        )
        .on_hover_text("Choose when you hear the MIDI input");
        let active = mode == suhina_engine::model::Monitor::In;
        // `#control-monitor`, `[checked]` while monitoring is In, and `monitor` artwork.
        let lit = widgets::lit_toggle(
            ui,
            &response,
            crate::kit::element::BUTTON.id("monitor"),
            &[crate::kit::element::Checked],
            active,
        );
        let (fill, ink) = lit.colors();
        let el = lit.el;
        widgets::toggle_surface(painter, at, &el, &response, fill, lit.toggle.image(lit.lit), 0.0);
        if !crate::kit::skin::paint_as(
            &el,
            painter,
            at,
            "monitor",
            crate::kit::skin::State::response(&response, active),
            ink,
        ) {
            widgets::text_in(
                painter,
                at,
                Align2::CENTER_CENTER,
                text,
                theme::ui_font(if at.height() < 16.0 { 9.0 } else { 10.0 }),
                ink,
            );
        }
        egui::Popup::menu(&response).show(|ui| menus::midi_monitor_choices(ui, lane.track, mode, actions));
    }

    // The volume where the layout puts it (at the end of a one-row header, after the buttons or
    // under them), and the pan.
    if let Some(volume) = parts.volume {
        value_control(ui, painter, volume, clip, lane, Control::Volume, false, id, state, actions);
    }
    if let Some(pan) = parts.pan {
        value_control(ui, painter, pan, clip, lane, Control::Pan, false, id, state, actions);
    }
    // The sends the header has room for, scrolled with ‹ and › where it has room for only some.
    let shown_sends = parts.shown_sends;
    if shown_sends > 0 {
        let offset_id = id.with("send-scroll-offset");
        let max_offset = lane.sends.len().saturating_sub(shown_sends);
        let mut offset = ui.ctx().data(|d| d.get_temp::<usize>(offset_id)).unwrap_or(0).min(max_offset);
        if parts.navigation {
            let viewport = parts
                .sends
                .iter()
                .chain([&parts.previous, &parts.next])
                .flatten()
                .fold(Rect::NOTHING, |all, at| all.union(*at));
            state.send_scroll_areas.push(viewport.intersect(clip));
            if ui.rect_contains_pointer(viewport.intersect(clip)) {
                let scroll = ui.input_mut(|i| {
                    if i.modifiers.shift {
                        let mut delta = 0.0;
                        i.raw.events.retain(|event| {
                            if let egui::Event::MouseWheel { delta: wheel, .. } = event {
                                delta += wheel.x + wheel.y;
                                false
                            } else {
                                true
                            }
                        });
                        i.smooth_scroll_delta = Vec2::ZERO;
                        delta
                    } else {
                        0.0
                    }
                });
                if scroll < 0.0 {
                    offset = (offset + 1).min(max_offset);
                }
                if scroll > 0.0 {
                    offset = offset.saturating_sub(1);
                }
            }
            for (index, ((glyph, label, enabled), at)) in
                [("‹", "Previous sends", offset > 0), ("›", "Next sends", offset < max_offset)]
                    .into_iter()
                    .zip([parts.previous, parts.next])
                    .enumerate()
            {
                let Some(at) = at else { continue };
                let response = widgets::button_area(
                    ui,
                    at.intersect(clip),
                    id.with(("send-scroll", index)),
                    format!("{label} on {}", lane.name),
                )
                .on_hover_text("Scroll sends with Shift + mouse wheel");
                let el = crate::kit::element::BUTTON.response(&response).enter();
                widgets::track_button_face(painter, at, &el, &response, el.background());
                widgets::text_in(
                    painter,
                    at,
                    Align2::CENTER_CENTER,
                    glyph,
                    theme::ui_font(theme::TEXT + 2.0),
                    el.color_or(if enabled { el.color() } else { crate::kit::element::hint_ink() }),
                );
                if enabled && response.clicked() {
                    offset = if index == 0 { offset.saturating_sub(1) } else { (offset + 1).min(max_offset) };
                }
            }
        }
        ui.ctx().data_mut(|d| d.insert_temp(offset_id, offset));
        for (index, send) in lane.sends.iter().skip(offset).take(shown_sends).enumerate() {
            if let Some(knob) = parts.sends.get(index).copied().flatten() {
                send_knob(ui, painter, knob, clip, lane, send, id, state, header_fill, actions);
            }
        }
    }

    // Taller still: where the track's audio goes, and under it, on a track that plays notes,
    // which controllers it hears when armed. A presentation folder is no place to send audio.
    if let Some(line) = parts.output.filter(|line| line.intersects(clip)) {
        let outputs: Vec<_> = nest
            .groups
            .iter()
            .filter(|(id, _)| timeline.lanes.iter().any(|l| l.track == *id && !l.folder))
            .cloned()
            .collect();
        routing(ui, painter, line, lane, &outputs, header_fill, actions);
    }
    if let Some(input) = parts.input.filter(|input| input.intersects(clip)) {
        if lane.plays_notes {
            midi_input(ui, painter, input, lane, &timeline.midi_ports, header_fill, actions);
        } else if lane.plays_audio {
            audio_input(ui, painter, input, lane, timeline.input_channels, header_fill, actions);
        }
    }

    work_trace.mark("controls and routing");
    // The bottom edge resizes the lane; a return's top edge, as the fixed rows sit along the
    // bottom and grow upward.
    let upward = lane.kind == ColumnKind::Return;
    let rect = lane_rect;
    let edge = if upward {
        Rect::from_min_max(Pos2::new(rect.left(), rect.top() - 1.0), Pos2::new(rect.right(), rect.top() + 3.0))
    } else {
        Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - 3.0), Pos2::new(rect.right(), rect.bottom() + 2.0))
    };
    if edge.intersects(lanes_area) {
        let response = ui
            .interact(edge.intersect(lanes_area), id.with("resize"), Sense::drag())
            .on_hover_cursor(egui::CursorIcon::ResizeVertical);
        if response.dragged_by(egui::PointerButton::Primary) {
            let dy = response.drag_delta().y;
            state.resize_lanes(lane.track, state.height(lane), if upward { -dy } else { dy });
        }
    }
}

/// The background the base sheet gives `arrangement track-header[muted]` (`[muted][selected]` while
/// selected), worked out once: the fallback where no sheet styles the window.
pub(super) fn muted_header_gray(selected: bool) -> Color32 {
    static GRAYS: std::sync::OnceLock<[Color32; 2]> = std::sync::OnceLock::new();
    super::clip::muted_gray(&GRAYS, "track-header", selected)
}
