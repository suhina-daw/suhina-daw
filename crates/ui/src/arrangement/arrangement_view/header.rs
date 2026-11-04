// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A track's header beside its lane: name, routing, inputs, meter and the colour band.

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
    let current = lane.output.and_then(|t| groups.iter().find(|(g, _)| *g == t)).map_or("Main", |(_, name)| name);
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
    let response =
        widgets::button_area(ui, field, Id::new(("output", lane.track.0)), format!("{} output: {current}", lane.name));
    widgets::subtle_dropdown(ui, painter, field, &response, current, background);
    egui::Popup::menu(&response).show(|ui| {
        let text = |t: &str| egui::RichText::new(t).font(theme::ui_font(theme::TEXT_SMALL));
        if ui.selectable_label(lane.output.is_none(), text("Main")).clicked() && lane.output.is_some() {
            picked = Some(serde_json::Value::from("main"));
        }
        for (group, name) in groups.iter().filter(|(g, _)| *g != lane.track) {
            if ui.selectable_label(lane.output == Some(*group), text(name)).clicked() && lane.output != Some(*group) {
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
        Id::new(("midi-input", lane.track.0)),
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
        Id::new(("audio-input", lane.track.0)),
        format!("{} audio input: {current}", lane.name),
    );
    widgets::subtle_dropdown(ui, painter, field, &response, &current, background);
    egui::Popup::menu(&response)
        .show(|ui| menus::audio_input_choices(ui, lane.track, &lane.audio_input, channels, actions));
}

/// Width of the meter lying in a return or main row, left of its volume.
pub(super) const LYING_METER: f32 = 48.0;

/// Where a return or main row shows its meter: lying down, ending at `right`, centred on `y`.
pub(super) fn lying_meter(right: f32, y: f32) -> Rect {
    Rect::from_min_max(Pos2::new(right - LYING_METER, y - meter::WIDTH / 2.0), Pos2::new(right, y + meter::WIDTH / 2.0))
}

fn nesting_band_width(depth: usize) -> f32 {
    match depth {
        0 => COLOUR_BAND,
        1 => 4.0,
        2 => 3.0,
        _ => 2.0,
    }
}

/// A header's colour band: the track's colour down its left edge, the full height of the header
/// (dimmed while the track is off). A click opens the palette; gives the band's rectangle.
pub(super) fn colour_band(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    clip: Rect,
    lane: &Lane,
    id: Id,
    actions: &mut Vec<Action>,
) -> Rect {
    let band = Rect::from_min_size(rect.min, Vec2::new(nesting_band_width(lane.depth), rect.height()));
    let at = crate::kit::control_layout::place("arrangement-color", band, clip);
    if !crate::kit::control_layout::placement("arrangement-color").hidden {
        painter.rect_filled(at, 0.0, colour_picker::fill(lane.color, lane.mute));
        if at.intersects(clip) {
            colour_picker::area(ui, at.intersect(clip), id.with("colour"), lane.track, &lane.name, lane.color, actions);
        }
    }
    band
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
    // A track lower than the theme allows for its placements keeps Suhina's own layout.
    let low = crate::kit::style_class::theme_compact_below().is_some_and(|below| rect.height() < below);
    let _native = crate::kit::control_layout::native_only(low);
    let mut work_trace = crate::kit::frame_trace::WorkSpan::new("arrangement track header");
    let _style_class = crate::kit::style_class::enter_named("track-header", &format!("track-{}", lane.track.0));
    crate::kit::native_css::set_variable("--track-colour", crate::kit::native_css::hex(rgb(lane.color)));
    crate::kit::native_css::mark(
        crate::kit::native_css::HostState::Selected,
        state.selected_tracks.contains(&lane.track),
    );
    crate::kit::native_css::mark(crate::kit::native_css::HostState::Muted, lane.mute);
    crate::kit::native_css::mark(crate::kit::native_css::HostState::Armed, lane.armed);
    work_trace.mark("style");
    let _artwork = crate::kit::artwork::track_color(rgb(lane.color));
    let spacious = rect.height() >= 60.0;
    let tiny = rect.height() < 20.0;
    let header_row = if tiny {
        (rect.height() - 2.0).max(1.0)
    } else if spacious {
        20.0
    } else {
        HEADER_ROW
    };
    let field_height = if spacious { 20.0 } else { FIELD };
    let header_pad = if tiny {
        1.0
    } else if rect.height() < 48.0 {
        2.0
    } else if spacious {
        6.0
    } else {
        HEADER_PAD_Y
    };
    let header_gap = if rect.height() < 48.0 {
        4.0
    } else if spacious {
        10.0
    } else {
        PAD
    };
    let id = Id::new(("arrangement-header", lane.track.0));
    let clip = rect.intersect(lanes_area);

    // The header itself: click selects (Ctrl adds, Shift takes a run), right-click opens the
    // track's menu, a drag moves the track. Its controls, added after it, take their own clicks.
    let body = ui.interact(clip, id.with("body"), Sense::click_and_drag());
    body.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Track {}", lane.name)));
    if body.clicked() {
        state.targeted = Some(lane.track);
        let (shift, command) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
        state.pick_track(lane.track, shift, command, order.tracks);
        // The track, not a clip, is what Delete acts on now (#690); a clip on it stays selected,
        // and the detail area shows the track's devices (#950).
        state.track_focus = true;
        state.devices_clicked = true;
        state.automation.let_go();
    }
    // A double-click shows the track's devices in the detail area.
    if body.double_clicked() {
        state.open_devices = Some(lane.track);
    }
    if body.drag_started_by(egui::PointerButton::Primary) {
        state.track_drag = Some(lane.track);
    }
    if body.drag_stopped_by(egui::PointerButton::Primary)
        && let Some(dragged) = state.track_drag.take()
        && let Some(pointer) = body.interact_pointer_pos().or_else(|| ui.ctx().pointer_latest_pos())
    {
        state.track_drop = Some((dragged, pointer.y));
    }
    let selection = state.selected_tracks.clone();
    // Open until a click outside: its items close it themselves, and a colour dragged in its
    // picker does not (#712).
    egui::Popup::context_menu(&body).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        let into: Vec<(TrackId, String)> = nesting::move_targets(nest.places, lane.track)
            .into_iter()
            .filter_map(|g| nest.groups.iter().find(|(t, _)| *t == g).cloned())
            .collect();
        let menu = TrackMenu {
            track: lane.track,
            name: &lane.name,
            colour: lane.color,
            is_group: lane.kind == ColumnKind::Group,
            is_return: lane.kind == ColumnKind::Return,
            selection: &selection,
            into: &into,
            out: nesting::out_of_group(nest.places, lane.track),
            midi: lane.plays_notes.then_some((&lane.midi_input, timeline.midi_ports.as_slice())),
            audio: lane.plays_audio.then_some((&lane.audio_input, timeline.input_channels)),
            tracks_copied: state.tracks_copied,
        };
        menus::track_menu(ui, &menu, actions);
    });
    work_trace.mark("interaction");
    let selected = state.selected_tracks.contains(&lane.track) || state.focus == Some(lane.track);
    // Preserve track identity when selected: a small brightness lift and outline, without
    // replacing the track colour with the global selection accent.
    let background = if selected { theme::track_header_selected() } else { theme::track_header_background() };
    let strength = crate::kit::style_class::theme_header_tint().unwrap_or(state.header_colour_strength);
    let surface = colour_surface(background, rgb(lane.color), strength, lane.mute);
    let header_fill = if selected { motion::mix(surface, theme::text_primary(), 0.07) } else { surface };
    if !crate::kit::skin::paint_colored(
        painter,
        rect,
        "track.body",
        crate::kit::skin::State::response(&body, selected),
        header_fill,
        rgb(lane.color),
    ) {
        painter.fill_background(rect, 0.0, header_fill);
    }
    crate::kit::liquid::paint(painter, rect, header_fill, crate::kit::liquid::Surface::Track);
    // The sheet's own background for the header (`.arrangement .track-header`) over the native one.
    crate::kit::native_css::surface(painter, rect.intersect(clip), 0.0, egui::Color32::TRANSPARENT);
    // The name and the controls' ink read on whichever background is shown.
    let header_fill = crate::kit::style_class::background(header_fill);
    if selected {
        let outline = motion::mix(theme::border_strong(), saturated(rgb(lane.color)), 0.5);
        painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.0, outline), StrokeKind::Inside);
    }
    work_trace.mark("background");
    // A track inside groups has their colours down its indent, the outermost first, so a group and
    // its tracks read as one block: the group's colour runs from its header down all of them.
    let mut ancestors = Vec::new();
    let mut parent = lane.parent;
    while let Some(group) = parent.filter(|_| ancestors.len() < lane.depth) {
        let Some(group_lane) = timeline.lanes.iter().find(|l| l.track == group) else { break };
        ancestors.push(group_lane.color);
        parent = group_lane.parent;
    }
    let mut band_left = rect.left();
    // Deep groups must not consume the controls' entire width in a narrow column.
    let indent = (0..ancestors.len()).map(|level| nesting_band_width(level) + 2.0).sum::<f32>();
    let indent_room = (rect.width() - COLOUR_BAND - 3.0 * PAD - meter::WIDTH - 114.0).max(0.0);
    let indent_scale = if indent > 0.0 { (indent_room / indent).min(1.0) } else { 1.0 };
    for (level, color) in ancestors.iter().rev().enumerate() {
        let width = nesting_band_width(level) * indent_scale;
        let strip = Rect::from_min_size(Pos2::new(band_left, rect.top()), Vec2::new(width, rect.height()));
        if strip.intersects(clip) {
            painter.rect_filled(strip.intersect(clip), 0.0, rgb(*color));
        }
        band_left += width + 2.0 * indent_scale;
    }
    // The track's own band follows its ancestors and opens its colour palette.
    let band_rect = Rect::from_min_max(Pos2::new(band_left, rect.top()), rect.max);
    let band = colour_band(ui, painter, band_rect, clip, lane, id, actions);
    // The meter stands at the right edge; the rows keep clear of it.
    let meter_x = rect.right() - PAD - meter::WIDTH;
    let inner = Rect::from_min_max(
        Pos2::new(band.right() + PAD, rect.top() + header_pad),
        Pos2::new(meter_x - PAD, rect.bottom() - header_pad),
    );
    let narrow = rect.height() >= SHOWS_MIXER
        && inner.width() < FOLD + 2.0 + SOLO + PAD + ON_TOGGLE + 3.0 * (SOLO + 2.0) + PAD + LEAST_NAME;
    let solo_width = if narrow || rect.height() < SHOWS_MIXER { 18.0 } else { SOLO };
    let on_width = if narrow || rect.height() < SHOWS_MIXER { 24.0 } else { ON_TOGGLE };
    let meter_bottom = if rect.height() >= SHOWS_MIXER {
        inner.top() + header_row + header_gap + field_height
    } else {
        inner.top() + header_row
    };
    let at = Rect::from_min_max(
        Pos2::new(meter_x, inner.top()),
        Pos2::new(meter_x + meter::WIDTH, meter_bottom.min(inner.bottom())),
    );
    let level = state.meters.track(lane.track);
    let at = crate::kit::control_layout::place("arrangement-meter", at, rect);
    if !crate::kit::control_layout::placement("arrangement-meter").hidden {
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
    let fold = Some(Rect::from_min_size(inner.min, Vec2::new(FOLD, header_row)));
    if let Some(fold) = fold.filter(|_| !crate::kit::control_layout::placement("arrangement-fold").hidden) {
        let fold = crate::kit::control_layout::place("arrangement-fold", fold, clip);
        let folded = if group_fold {
            state.folds.is_folded(lane.track)
        } else {
            rect.height() <= f32::from(state.sizes.height(TrackSize::Compact))
        };
        let label = if group_fold {
            if folded { format!("Unfold {}", lane.name) } else { format!("Fold {}", lane.name) }
        } else if folded {
            format!("Expand {}", lane.name)
        } else {
            format!("Collapse {}", lane.name)
        };
        let response = widgets::button_area(ui, fold.intersect(clip), id.with("fold"), label);
        // `.fold`, `:checked` while folded, and `fold` artwork (`.on` while folded).
        let _style = crate::kit::style_class::enter_named_response("fold", "fold", &response);
        crate::kit::native_css::mark(crate::kit::native_css::HostState::Checked, folded);
        let color = motion::mix(header_fill, theme::text_on(header_fill), 0.75 + 0.25 * widgets::hover(ui, &response));
        let color = crate::kit::style_class::foreground(color);
        let fold_state = crate::kit::skin::State::response(&response, folded);
        let drawn = crate::kit::skin::paint(painter, fold, "fold", fold_state, color, 0.0);
        let center = fold.center();
        let (wide, tall) = (7.0, 4.0);
        // Pointing right while the group is folded, down while it is open.
        let points = if folded {
            vec![
                Pos2::new(center.x - tall / 2.0, center.y - wide / 2.0),
                Pos2::new(center.x - tall / 2.0, center.y + wide / 2.0),
                Pos2::new(center.x + tall / 2.0, center.y),
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
                state.toggle_height(lane.track, rect.height());
            }
            // What the lanes are changes without the project changing: draw the next frame.
            ui.ctx().request_repaint();
        }
    }
    // Every header keeps the fold triangle's room, so a group's tracks line up under it and their
    // indent shows; a group without the triangle (no tracks yet) lines up the same.
    let first_x = inner.left() + FOLD + 2.0;
    // On a roomy header the name gets the top line; buttons and level controls sit below.
    // Narrow/short headers retain their compact arrangement, including send overflow rules.
    let name_first = rect.height() >= SHOWS_MIXER
        && inner.width() - FOLD - 2.0 - on_width - 3.0 * (solo_width + 2.0) - PAD - PAN_FIELD - 4.0 >= LEAST_VOLUME;
    let automation_x = first_x;
    let first_x = if narrow {
        inner.left()
    } else if name_first {
        first_x
    } else {
        first_x + solo_width + PAD
    };
    let row_top = if name_first || narrow { inner.top() + header_row + header_gap } else { inner.top() };
    let row = Rect::from_min_max(Pos2::new(first_x, row_top), Pos2::new(inner.right(), row_top + header_row));
    let toggle = Rect::from_min_size(row.min, Vec2::new(on_width, header_row));
    let solo = Rect::from_min_size(Pos2::new(toggle.right() + 2.0, row.top()), Vec2::new(solo_width, header_row));
    let cue = Rect::from_min_size(Pos2::new(solo.right() + 2.0, row.top()), Vec2::new(solo_width, header_row));
    // A track that can be armed has its arm button after cue, as wide.
    let arm = lane
        .armable
        .then(|| Rect::from_min_size(Pos2::new(cue.right() + 2.0, row.top()), Vec2::new(solo_width, header_row)));
    let monitor_width = if rect.height() < SHOWS_MIXER { 24.0 } else { 32.0 };
    // A track with take lanes has a button beside it that hides and shows them (#953).
    let has_takes = timeline.takes.iter().any(|t| t.track == lane.track)
        || timeline.live_takes.iter().any(|t| t.track == lane.track && t.finished);
    // Only where it ends inside its row, before the take lanes' button when that ends the row
    // (#876); a narrower or lower header leaves the monitoring to the MIDI input menu.
    let monitor_end = if has_takes && !narrow { row.right() - solo_width - 4.0 } else { row.right() };
    let monitor = lane
        .plays_notes
        .then(|| {
            Rect::from_min_size(
                Pos2::new(arm.unwrap_or(cue).right() + 2.0, row.top()),
                Vec2::new(monitor_width, header_row),
            )
        })
        .filter(|at| at.right() <= monitor_end);
    let controls_end = monitor.unwrap_or(arm.unwrap_or(cue)).right();
    // A lane tall enough for two rows has its volume and pan beside the buttons, where the
    // other level controls are, and the automation button, the name and the sends on the row
    // below, the button under the track-on toggle; a short lane keeps one row of buttons, name
    // and automation button.
    let tall = rect.height() >= SHOWS_MIXER;
    // Beside the buttons only where the volume bar keeps its room; a narrow header has the name
    // on the first row and the volume, the sends and the pan across the second, the bar as wide
    // as it can be.
    let beside_room = row.right() - PAN_FIELD - 4.0 - (controls_end + PAD);
    let side = tall && beside_room >= LEAST_VOLUME;
    let stacked = tall && !side;
    let below_top = if name_first || narrow { inner.top() } else { row.bottom() + header_gap };
    // In the stacked layout the level row also uses the space below automation.
    let below_left = if stacked { automation_x } else { first_x };
    let below =
        Rect::from_min_max(Pos2::new(below_left, below_top), Pos2::new(inner.right(), below_top + field_height));
    // The automation button shows and hides the track's lanes.
    let lanes_button = if narrow {
        Rect::from_min_size(Pos2::new(inner.right() - solo_width, inner.top()), Vec2::new(solo_width, header_row))
    } else if side {
        Rect::from_min_size(
            Pos2::new(below.left() + (on_width - solo_width) / 2.0, below.top()),
            Vec2::new(solo_width, header_row),
        )
    } else {
        Rect::from_min_size(Pos2::new(automation_x, row.top()), Vec2::new(solo_width, header_row))
    };
    if lanes_button.intersects(clip) && !crate::kit::control_layout::placement("arrangement-automation").hidden {
        let lanes_button = crate::kit::control_layout::place("arrangement-automation", lanes_button, clip);
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
    let takes_button = has_takes.then(|| {
        if side {
            lanes_button.translate(Vec2::new(SOLO + 2.0, 0.0))
        } else if narrow {
            lanes_button.translate(Vec2::new(-(SOLO + 2.0), 0.0))
        } else {
            Rect::from_min_size(Pos2::new(row.right() - solo_width, row.top()), Vec2::new(solo_width, header_row))
        }
    });
    if let Some(button) = takes_button.filter(|b| b.intersects(clip)) {
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
        let (fill, text) = widgets::toggle_colors(ui, &response, !hidden);
        painter.rect_filled(button, 0.0, fill);
        // Three stacked lines: the lanes under the track.
        let glyph = button.shrink2(Vec2::new(4.0, 4.0));
        for i in 0..3 {
            let y = glyph.top() + glyph.height() * i as f32 / 2.0;
            painter.hline(glyph.x_range(), y, egui::Stroke::new(1.0, text));
        }
    }
    let lanes_right = takes_button.filter(|_| side).unwrap_or(lanes_button).right();
    // Two rows: the sends at the end of the lower row, as many as leave the name its room.
    let step = SEND_WIDTH + SEND_GAP;
    let stacked_pan =
        Rect::from_min_max(Pos2::new(below.right() - PAN_FIELD, below.top()), Pos2::new(below.right(), below.bottom()));
    let send_room = if side {
        below.right() - lanes_right - PAD - LEAST_NAME
    } else if stacked {
        stacked_pan.left() - 4.0 - below.left() - LEAST_VOLUME
    } else {
        0.0
    };
    let overflow = lane.sends.len() > (send_room / step).floor().max(0.0) as usize;
    let navigation = if overflow && send_room >= step + 36.0 { 36.0 } else { 0.0 };
    let shown_sends = lane.sends.len().min(((send_room - navigation) / step).floor().max(0.0) as usize);
    let sends_left = if stacked {
        stacked_pan.left() - 4.0 - step * shown_sends as f32 - navigation + SEND_GAP
    } else {
        below.right() - step * shown_sends as f32 - navigation + SEND_GAP
    };
    let mut name = if narrow {
        Rect::from_min_max(
            Pos2::new(automation_x, inner.top()),
            Pos2::new(lanes_button.left() - PAD, inner.top() + header_row),
        )
    } else if side {
        Rect::from_min_max(
            Pos2::new(lanes_right + PAD, below.top()),
            Pos2::new(sends_left - SEND_GAP - 4.0, below.top() + header_row),
        )
    } else {
        // The take lanes' button, when there is one, ends the row and the name stops before it.
        let end = takes_button.map_or(row.right(), |b| b.left() - 4.0);
        Rect::from_min_max(Pos2::new(controls_end + PAD, row.top()), Pos2::new(end, row.bottom()))
    };
    // Keep a usable level control even on a single-row lane. Reserve the name's
    // minimum first; sends and pan stay in the expanded layout.
    let compact_volume = if !tall && name.width() >= 28.0 + PAD + 48.0 {
        let width = (name.width() * 0.45).clamp(48.0, 80.0);
        let volume = Rect::from_min_max(Pos2::new(name.right() - width, row.top()), name.right_bottom());
        name.max.x = volume.left() - PAD;
        Some(volume)
    } else {
        None
    };
    name = crate::kit::control_layout::place("arrangement-name", name, clip);
    let name_visible = !crate::kit::control_layout::placement("arrangement-name").hidden;
    let name_style = crate::kit::style_class::enter_named("track-name", &format!("track-{}", lane.track.0));
    crate::kit::native_css::set_variable("--track-fill", crate::kit::native_css::hex(header_fill));
    if name_visible {
        crate::kit::skin::paint_colored(
            painter,
            name,
            "track.name",
            crate::kit::skin::State::response(&body, selected),
            header_fill,
            rgb(lane.color),
        );
        // The sheet's own background for the name (`.arrangement .track-name`).
        crate::kit::native_css::surface(painter, name.intersect(clip), 0.0, egui::Color32::TRANSPARENT);
    }
    // Ink that reads on the fill, unless the sheet gives the name a colour.
    let name_color = crate::kit::style_class::foreground(if lane.mute {
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
    let name_size = if tiny {
        10.0
    } else if spacious {
        theme::TEXT + 2.0
    } else {
        theme::TEXT
    };
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
        d.insert_temp(Id::new((crate::app::NAME_RECT_TRACK, u64::from(lane.track.0))), spot);
    });
    if solo.intersects(clip)
        && let Some(action) =
            widgets::solo_button(ui, painter, solo, id.with("solo"), &controlled, &lane.name, lane.solo)
    {
        actions.push(action);
    }
    if cue.intersects(clip)
        && let Some(action) = widgets::cue_button(ui, painter, cue, id.with("cue"), lane.track.0, &lane.name, lane.cue)
    {
        actions.push(action);
    }
    if let Some(arm) = arm.filter(|a| a.intersects(clip))
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
    if toggle.intersects(clip) && !crate::kit::control_layout::placement("arrangement-enable").hidden {
        let toggle = crate::kit::control_layout::place("arrangement-enable", toggle, clip);
        let label = if lane.mute { format!("Turn {} on", lane.name) } else { format!("Turn {} off", lane.name) };
        let response = widgets::button_area(ui, toggle.intersect(clip), id.with("on"), label);
        // `#control-track-on`, as in the mixer: `:checked` while the track is on, `:muted` while
        // it is off, and `track-on` artwork (`.on`).
        let _style = crate::kit::style_class::enter_named_response("button", "track-on", &response);
        crate::kit::native_css::mark(crate::kit::native_css::HostState::Muted, lane.mute);
        if response.clicked() {
            actions.push(Action::new("track.setMute", json!({ "tracks": controlled, "mute": !lane.mute })));
        }
        let (fill, text) = widgets::toggle_colors(ui, &response, !lane.mute);
        widgets::button_surface(painter, toggle, &response, !lane.mute, fill, 0.0);
        let text = crate::kit::style_class::foreground(text);
        let state = crate::kit::skin::State::response(&response, !lane.mute);
        if !crate::kit::skin::paint(painter, toggle, "track-on", state, text, 0.0) {
            widgets::track_number_text(painter, toggle, &number, text, lane.mute);
        }
    } else if !crate::kit::control_layout::placement("arrangement-number").hidden {
        // With the toggle hidden, the number can stay as a label of its own (`.track-number`,
        // placed as `arrangement-number`), which turns nothing off.
        let at = crate::kit::control_layout::place("arrangement-number", toggle, clip);
        if at.intersects(clip) {
            widgets::track_number_text(painter, at, &number, theme::text_secondary(), lane.mute);
        }
    }

    let monitor = monitor
        .filter(|_| !crate::kit::control_layout::placement("arrangement-monitor").hidden)
        .map(|at| crate::kit::control_layout::place("arrangement-monitor", at, clip));
    if let Some(at) = monitor.filter(|at| at.intersects(clip)) {
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
        // `#control-monitor`, `:checked` while monitoring is In, and `monitor` artwork.
        let _style = crate::kit::style_class::enter_named_response("button", "monitor", &response);
        let (fill, ink) = widgets::toggle_colors(ui, &response, active);
        widgets::button_surface(painter, at, &response, active, fill, 0.0);
        let ink = crate::kit::style_class::foreground(ink);
        if !crate::kit::skin::paint(
            painter,
            at,
            "monitor",
            crate::kit::skin::State::response(&response, active),
            ink,
            0.0,
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

    if let Some(volume) = compact_volume {
        value_control(ui, painter, volume, clip, lane, Control::Volume, false, id, state, actions);
    }

    if narrow {
        // Name and automation are above; the second row keeps the four track controls
        // and a draggable volume field. Pan and sends return when the column is widened.
        let volume = Rect::from_min_max(Pos2::new(controls_end + PAD, row.top()), row.right_bottom());
        value_control(ui, painter, volume, clip, lane, Control::Volume, false, id, state, actions);
    }

    // Two rows: volume and pan beside the buttons, the sends after the name below; or, narrow,
    // the volume, the sends and the pan across the second row.
    if tall && !narrow {
        let (volume, pan) = if side {
            let left = controls_end + PAD;
            let pan =
                Rect::from_min_max(Pos2::new(row.right() - PAN_FIELD, row.top()), Pos2::new(row.right(), row.bottom()));
            (Rect::from_min_max(Pos2::new(left, row.top()), Pos2::new((pan.left() - 4.0).max(left), row.bottom())), pan)
        } else {
            let right = (sends_left - SEND_GAP).max(below.left());
            (Rect::from_min_max(below.min, Pos2::new(right, below.bottom())), stacked_pan)
        };
        value_control(ui, painter, volume, clip, lane, Control::Volume, false, id, state, actions);
        value_control(ui, painter, pan, clip, lane, Control::Pan, false, id, state, actions);
        let offset_id = id.with("send-scroll-offset");
        let max_offset = lane.sends.len().saturating_sub(shown_sends);
        let mut offset = ui.ctx().data(|d| d.get_temp::<usize>(offset_id)).unwrap_or(0).min(max_offset);
        if navigation > 0.0 && shown_sends > 0 {
            let viewport = Rect::from_min_max(
                Pos2::new(sends_left, below.top()),
                Pos2::new(sends_left + step * shown_sends as f32 + navigation - SEND_GAP, below.bottom()),
            );
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
            for (index, (glyph, label, enabled)) in
                [("‹", "Previous sends", offset > 0), ("›", "Next sends", offset < max_offset)].into_iter().enumerate()
            {
                let at = Rect::from_min_size(
                    Pos2::new(sends_left + step * shown_sends as f32 + index as f32 * 18.0, below.top()),
                    Vec2::new(16.0, field_height),
                );
                let name = if index == 0 { "arrangement-sends-previous" } else { "arrangement-sends-next" };
                if crate::kit::control_layout::placement(name).hidden {
                    continue;
                }
                let at = crate::kit::control_layout::place(name, at, clip);
                let response = widgets::button_area(
                    ui,
                    at.intersect(clip),
                    id.with(("send-scroll", index)),
                    format!("{label} on {}", lane.name),
                )
                .on_hover_text("Scroll sends with Shift + mouse wheel");
                widgets::track_button_face(ui, painter, at, &response, theme::control_background());
                widgets::text_in(
                    painter,
                    at,
                    Align2::CENTER_CENTER,
                    glyph,
                    theme::ui_font(theme::TEXT + 2.0),
                    if enabled { theme::text_primary() } else { theme::text_hint() },
                );
                if enabled && response.clicked() {
                    offset = if index == 0 { offset.saturating_sub(1) } else { (offset + 1).min(max_offset) };
                }
            }
        }
        ui.ctx().data_mut(|d| d.insert_temp(offset_id, offset));
        for (index, send) in lane.sends.iter().skip(offset).take(shown_sends).enumerate() {
            let knob = Rect::from_min_size(
                Pos2::new(sends_left + step * index as f32, below.top()),
                Vec2::new(SEND_WIDTH, field_height),
            );
            send_knob(ui, painter, knob, clip, lane, send, offset + index, id, state, header_fill, actions);
        }
    }

    // Row 3, taller still: where the track's audio goes.
    if rect.height() >= SHOWS_ROUTING {
        let top = row.bottom().max(below.bottom()) + PAD + 4.0;
        let line = Rect::from_min_max(Pos2::new(inner.left(), top), Pos2::new(inner.right(), top + DROPDOWN));
        if line.bottom() <= inner.bottom()
            && line.intersects(clip)
            && !crate::kit::control_layout::placement("arrangement-output").hidden
        {
            let line = crate::kit::control_layout::place("arrangement-output", line, clip);
            routing(ui, painter, line, lane, nest.groups, header_fill, actions);
        }
        // Taller still, on a track that plays notes: which controllers it hears when armed.
        let input = line.translate(Vec2::new(0.0, DROPDOWN + PAD));
        if rect.height() >= SHOWS_MIDI_INPUT
            && input.bottom() <= inner.bottom()
            && input.intersects(clip)
            && !crate::kit::control_layout::placement("arrangement-input").hidden
        {
            let input = crate::kit::control_layout::place("arrangement-input", input, clip);
            if lane.plays_notes {
                midi_input(ui, painter, input, lane, &timeline.midi_ports, header_fill, actions);
            } else if lane.plays_audio {
                audio_input(ui, painter, input, lane, timeline.input_channels, header_fill, actions);
            }
        }
    }

    work_trace.mark("controls and routing");
    // The bottom edge resizes the lane; a return's top edge, as the fixed rows sit along the
    // bottom and grow upward.
    let upward = lane.kind == ColumnKind::Return;
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
