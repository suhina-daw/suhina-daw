// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The detail area under or beside the views: the selected track's devices, the clip's piano roll, and the editors of plug-ins and ARA.

use super::*;

/// How soon a second Escape must follow the first to count as Escape twice, in seconds.
const DOUBLE_ESCAPE: f64 = 0.45;

impl App {
    /// A double-click on a track (an Arrangement header, a Launcher column's name or channel
    /// strip) selects it and shows its devices, opening the detail area; a clip selected on
    /// another track is no longer selected (`docs/spec/ui-clip-editor.md` › Where it opens).
    pub(super) fn devices_asked(&mut self) {
        if let Some(track) = self.arrangement.open_devices.take() {
            if self.arrangement.selected_clip().is_some_and(|(t, _)| t != track) {
                self.arrangement.clear_selected_clip();
            }
            if self.arrangement.selected_tracks.first() != Some(&track) {
                self.arrangement.selected_tracks = vec![track];
            }
            self.show_devices();
        }
        if let Some(track) = self.view.open_devices.take() {
            self.view.selected = Some(track);
            if self.view.selected_slot.is_some_and(|(t, _)| t != track) {
                self.view.selected_slot = None;
            }
            self.show_devices();
        }
        // A click anywhere on a track that is not a clip shows its devices too, the selection
        // as the click left it (#950). A hidden detail area stays hidden: opening it under the
        // pointer would move what a double-click's second click lands on.
        if std::mem::take(&mut self.arrangement.devices_clicked) | std::mem::take(&mut self.view.devices_clicked)
            && self.detail_open
        {
            self.detail_showing = detail_view::Showing::Devices;
        }
    }

    /// Selects `track` (Main too) and shows its devices, opening the detail area: after a device
    /// was dropped on it (#950).
    pub(super) fn show_devices_of(&mut self, track: TrackId) {
        self.select_tracks(vec![track]);
        self.show_devices();
    }

    /// Shows the shown track's devices in the detail area, opening it when it is hidden.
    pub(super) fn show_devices(&mut self) {
        self.detail_showing = detail_view::Showing::Devices;
        self.show_editor();
    }

    /// The Clip / Devices switch's key and menu entry, for a clip with an editor: the other part,
    /// or the devices when the detail area is hidden, which opens it.
    pub(super) fn toggle_devices(&mut self) {
        if self.piano_clip(self.shown_clip()).is_none() && self.ara_clip().is_none() {
            return;
        }
        self.detail_showing = match self.detail_showing {
            detail_view::Showing::Clip => detail_view::Showing::Devices,
            _ if !self.detail_open => detail_view::Showing::Devices,
            detail_view::Showing::Devices => detail_view::Showing::Clip,
        };
        self.show_editor();
    }

    /// The shown clip when its editor is a plug-in that edits clips (ARA, ADR 0033): an
    /// arrangement audio clip whose track runs such a plug-in. Its track, id and the plug-in's
    /// index in the track's chain.
    pub(super) fn ara_clip(&mut self) -> Option<(TrackId, ClipId, usize)> {
        let ClipRef::Arrangement(track, clip) = self.shown_clip()? else { return None };
        if self.piano_clip(Some(ClipRef::Arrangement(track, clip))).is_some() || !self.clip_uses_ara(track, clip) {
            return None;
        }
        let device = self.session()?.ara_device(track)?;
        Some((track, clip, device))
    }

    /// Whether `clip` on `track` plays through its channel's ARA plug-in: it names it, or the
    /// track's ARA plug-in is one from before clips named their own, which edits all of them
    /// (#990).
    pub(super) fn clip_uses_ara(&self, track: TrackId, clip: ClipId) -> bool {
        self.session().is_some_and(|s| {
            let p = s.project();
            let hosted = p.channel_of_track(track).and_then(|c| c.devices.first()).is_some_and(|d| d.clip_host);
            !hosted
                || p.arrangement
                    .lanes
                    .iter()
                    .filter(|l| l.track == Some(track))
                    .flat_map(|l| &l.clips)
                    .any(|c| c.id == clip && c.ara)
        })
    }

    /// Opens the selected clip in its track's plug-in that edits clips (ARA): the detail area
    /// shows the clip with the plug-in's editor in it, and the plug-in shows that clip.
    pub(super) fn open_ara_editor(&mut self) {
        let Some((track, clip, device)) = self.ara_clip() else {
            self.add_ara_plugin();
            return;
        };
        self.detail_showing = detail_view::Showing::Clip;
        self.show_editor();
        if let Ok(session) = self.projects.session_mut(None) {
            session.select_ara_clip(track, Some(clip));
        }
        self.ara_told = Some((track, Some(clip)));
        self.ara_opened = None;
        let open = self.session_mut_report(track, device).is_some_and(|r| r.editor_open);
        if !open {
            self.run(&Action::new("device.openEditor", json!({ "track": track.0, "device": device })));
        }
    }

    /// For [`App::open_ara_editor`] when the selected clip's track has no plug-in that edits
    /// clips running: adds the installed one ([`Session::ara_plugin`], Melodyne first) to the
    /// track, first in its chain, as a host that opens clips in it does. Its editor opens in
    /// the clip's place once it runs with the track's clips.
    pub(super) fn add_ara_plugin(&mut self) {
        let Some(ClipRef::Arrangement(track, clip)) = self.shown_clip() else {
            self.status = Status::Hint("Select an audio clip in the Arrangement first".into());
            return;
        };
        if self.piano_clip(Some(ClipRef::Arrangement(track, clip))).is_some() {
            self.status = Status::Hint("A MIDI clip opens in the piano roll; select an audio clip".into());
            return;
        }
        let Some((name, spec)) = self.session().and_then(Session::ara_plugin) else {
            self.status = Status::Hint(
                "No plug-in that edits clips (such as Melodyne) is installed; Settings › Plug-ins scans for it".into(),
            );
            return;
        };
        self.detail_showing = detail_view::Showing::Clip;
        self.show_editor();
        // It opens on its own once it runs bound to the clips (`detail_area`).
        self.ara_opened = None;
        // The clip plays through it from now on: the track's clip host when it has one, else the
        // installed one becomes it (#990). Its editor opens with the clip once it runs.
        self.run(&Action::new("clip.setAra", json!({ "clips": [{ "clip": clip.0 }], "on": true, "plugin": spec })));
        if !matches!(self.status, Status::Error(_)) {
            self.ara_armed = Some((track, clip));
            self.status = Status::Hint(format!("{name} edits the clip; it opens with the clip when it is ready"));
        }
    }

    /// Draws the detail area where the window's grid puts it, beside or under the view, with its
    /// splitter in the gap between them while its size follows the splitter
    /// (`Window::detail_follows`), and returns what is left for the view. `geometry` is the view's
    /// rectangle, the area's and whether it follows. Its own rules (a track's devices at one height,
    /// the piano roll pulled to the top) set the size the grid gives it next (`detail_shown`).
    pub(super) fn detail_area(&mut self, ui: &mut Ui, actions: &mut Vec<Action>, geometry: (Rect, Rect, bool)) -> Rect {
        let (main, laid, follows) = geometry;
        // The room the view and the detail area share.
        let area = main.union(laid);
        let panel_style = crate::kit::element::WORKSPACE_PANEL.id("panel-detail").attribute("kind", "detail").enter();
        let styled_ui = panel_style.ui(ui);
        // The splitter lies in the gap beside the area: drawn and taking the pointer there too
        // (#1240).
        let splitter =
            if follows { crate::kit::native_css::splitter_between(laid, main, SPLITTER) } else { Rect::NOTHING };
        // The devices column beside a clip's editor is cut from the view's side, so the area
        // draws in the room it shares with the view; its panel clips what it holds (`overflow`).
        let mut styled_ui = styled_ui.clipped_to(area.union(splitter));
        let ui = &mut *styled_ui;
        let stacked = self.settings.device_layout == DeviceLayout::Stack;
        // The piano roll's Focus switch keeps the focus on the clip picked first.
        if self.piano_roll.focus_first
            && let Some(first) = self.arrangement.selected_clips().first().copied()
        {
            self.arrangement.focus_clip(first);
        }
        // What it shows: the selected clip, or the selected track's devices.
        let (track, clip) = match self.shown {
            View::Session => (self.view.selected, self.view.selected_slot.map(|(t, s)| ClipRef::Slot(t, s))),
            View::Arrangement => (
                self.arrangement.selected_tracks.first().copied(),
                self.arrangement.selected_clip().map(|(t, i)| ClipRef::Arrangement(t, i)),
            ),
        };
        // Where each plug-in on the shown track stands, read first: reading it needs the session
        // itself, building the detail only the project; and an editor embedded in the panel
        // decides how tall the panel is.
        let mut reports = std::collections::HashMap::new();
        if let Some(shown_track) = clip.map(ClipRef::track).or(track)
            && let Ok(session) = self.projects.session_mut(None)
        {
            // Every device's path, inside device groups too.
            let paths: Vec<Vec<usize>> = session.project().channel_of_track(shown_track).map_or_else(Vec::new, |c| {
                suhina_engine::model::walk_devices(&c.devices).into_iter().map(|(path, _)| path).collect()
            });
            for path in paths {
                // Its generated panel follows the parameters it lists (#679).
                session.list_plugin_params_at(shown_track, &path);
                if let Some(report) = session.plugin_report_at(shown_track, &path) {
                    reports.insert((shown_track, path), report);
                }
            }
        }
        // An editor embedded in a panel: the area opens tall enough for it, once, as it opens;
        // after that the user sets the height and the editor is fitted to it (#689). An ARA
        // editor in the clip's place is fitted to the room there instead.
        let embedded: Vec<_> = reports
            .iter()
            .filter(|(_, r)| !r.ara && r.editor_embedded.is_some())
            .map(|(k, r)| (k.clone(), r.editor_embedded))
            .collect();
        self.editor_in_chain = !embedded.is_empty();
        // What an editor in the chain needs at its full size: it may take the view's room above.
        let scale = ui.ctx().pixels_per_point();
        let editor_needs = embedded
            .iter()
            .filter_map(|(_, embedded)| *embedded)
            .map(|(_, height)| detail_view::row_height_with_editor(height, scale) + self.detail_insets.devices)
            .fold(0.0, f32::max);
        if !stacked {
            let needed = embedded
                .iter()
                .filter(|(key, _)| !self.fitted_editors.contains(key))
                .filter_map(|(_, embedded)| *embedded)
                .map(|(_, height)| detail_view::row_height_with_editor(height, scale) + self.detail_insets.devices)
                .fold(0.0, f32::max);
            self.detail_height = self.detail_height.max(needed);
        }
        self.fitted_editors = embedded.into_iter().map(|(key, _)| key).collect();
        // A MIDI clip shows its notes in the piano roll beside its properties; under the
        // Arrangement view it can be pulled to the top.
        let mut piano = self.piano_clip(clip);
        // The notes of other clips the piano roll draws behind this one's (› Ghost notes).
        if let (Some(piano), Some(clip), Some(session)) = (piano.as_mut(), clip, self.session()) {
            piano.ghosts =
                crate::detail::piano_roll_ghosts::ghosts(session.project(), clip, self.piano_roll.ghost_source);
            // Several MIDI clips picked: the others' notes faint where they play, the edited one
            // (the focus) the one picked last or chosen in the clip column (#1054).
            if self.shown == View::Arrangement && self.arrangement.selected_clips().len() > 1 {
                let picked: Vec<ClipRef> =
                    self.arrangement.selected_clips().iter().map(|(t, c)| ClipRef::Arrangement(*t, *c)).collect();
                crate::detail::piano_roll_ghosts::add_picked(session.project(), clip, &picked, &mut piano.ghosts);
            }
        }
        // An audio clip on a track whose plug-in edits clips (ARA) has that plug-in's editor in
        // the piano roll's place: the plug-in is told the clip is selected, and its editor opens
        // on its own the first time the clip is shown.
        let ara = match clip {
            Some(ClipRef::Arrangement(t, c)) if piano.is_none() && self.clip_uses_ara(t, c) => {
                // An ARA plug-in sits first in the track's own chain, never inside a device group.
                let mut found: Vec<_> = reports
                    .iter()
                    .filter(|((_, path), r)| r.ara && path.len() == 1)
                    .map(|((_, path), r)| (path[0], r.editor_open))
                    .collect();
                found.sort_unstable();
                found.first().map(|&(device, open)| (t, c, device, open))
            }
            _ => None,
        };
        let told = ara.map(|(t, c, _, _)| (t, Some(c)));
        if told != self.ara_told {
            if let Ok(session) = self.projects.session_mut(None) {
                // The track shown before shows nothing selected any more.
                if let Some((before, _)) = self.ara_told.filter(|(t, _)| told.is_none_or(|(now, _)| now != *t)) {
                    session.select_ara_clip(before, None);
                }
                if let Some((t, c)) = told {
                    session.select_ara_clip(t, c);
                }
            }
            self.ara_told = told;
        }
        // The editor opens in the clip's place on its own, and again until it is open: a plug-in
        // that is busy or loading again misses the first ask. Closed by the user while its clip
        // is shown, it stays closed for that clip (#719).
        // Only a double-clicked clip opens the editor on its own (#916).
        if let Some((t, c, _, _)) = ara
            && std::mem::take(&mut self.clip_opened)
        {
            self.ara_armed = Some((t, c));
            self.ara_opened = None;
        }
        match ara.filter(|(t, c, _, _)| {
            self.detail_showing == detail_view::Showing::Clip && self.ara_armed == Some((*t, *c))
        }) {
            Some((t, c, device, open)) => {
                let key = (t, c);
                if open {
                    self.ara_was_open = Some(key);
                } else {
                    if self.ara_was_open == Some(key) {
                        self.ara_opened = Some(key);
                    }
                    self.ara_was_open = None;
                    if self.ara_opened.is_some_and(|closed| closed != key) {
                        self.ara_opened = None;
                    }
                    let now = ui.input(|i| i.time);
                    let due = self.ara_open_asked.is_none_or(|(asked, at)| asked != key || now - at >= 1.0);
                    if self.ara_opened != Some(key) && due {
                        self.ara_open_asked = Some((key, now));
                        actions.push(Action::new("device.openEditor", json!({ "track": t.0, "device": device })));
                        ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
                    }
                }
            }
            // No bound plug-in shown (a reload, another view): nothing was closed by hand.
            None => self.ara_was_open = None,
        }
        // Never taller than the room there is, however thick a theme's padding.
        let least = self.least_detail_height().min((area.height() - SPLITTER).max(0.0));
        // With the devices stacked, a clip's editor (the piano roll, an ARA plug-in's editor)
        // still lies along the bottom as in a row: only the devices stack, in a column of their
        // own beside the view, shown with Devices while the editor stays open (› Stacked in
        // `docs/spec/ui-clip-editor.md`).
        // Settings › Devices › Devices beside piano roll does the same in a row.
        let split = (stacked || self.settings.view.stack_in_piano_roll) && (piano.is_some() || ara.is_some());
        let side_devices = split && self.detail_showing == detail_view::Showing::Devices;
        let stacked = stacked && !split;
        // The piano roll, or an ARA plug-in's editor in the clip's place, can be pulled to the
        // top, the edited track's lane above it (Melodyne as a MIDI clip's notes are). Pulled up,
        // it stays so while the track's devices show instead: the switch on Devices, or a click on
        // the track's header (› Pulled to the top in `docs/spec/ui-clip-editor.md`).
        let track_devices = clip.is_none() && track.is_some();
        let base = follows && !stacked && self.shown == View::Arrangement;
        // Escape twice in a row pulls the editor (or the track's devices) to the top and lets it
        // go again.
        if std::mem::take(&mut self.escape_twice) && base && (piano.is_some() || ara.is_some() || track_devices) {
            self.detail_max = !self.detail_max;
        }
        let pullable = base && (piano.is_some() || ara.is_some() || (track_devices && self.detail_max));
        let pulled = pullable && self.detail_max;
        // With the switch on Devices the track's devices take the editor's place, pulled up as it
        // was (› Where it opens in `docs/spec/ui-clip-editor.md`).
        let switch = (piano.is_some() || ara.is_some()).then_some(self.detail_showing);
        let ara = ara.filter(|_| split || self.detail_showing == detail_view::Showing::Clip);
        let piano = piano.filter(|_| split || self.detail_showing == detail_view::Showing::Clip);
        let edits = piano.is_some() || ara.is_some();
        // What the boxes add around what they hold (#1336). The area keeps the height of what it
        // shows when a theme pads it: the height the user set grows by what the padding adds.
        let boxes = detail_view::box_insets();
        let [top, _, bottom, _] = panel_style.insets();
        let shown_device = detail_view::shown_device_insets(ui.ctx(), boxes.device);
        let editing = if piano.is_some() {
            Edits::PianoRoll
        } else if ara.is_some() {
            Edits::Plugin
        } else {
            Edits::None
        };
        let insets = DetailInsets::around(top + bottom, boxes, shown_device, editing);
        self.detail_height = (self.detail_height + insets.area - self.detail_insets.area).max(0.0);
        self.detail_insets = insets;
        let focus = match clip {
            _ if !pulled || self.settings.piano_roll_top != PianoRollTop::Track => None,
            Some(detail_view::ClipRef::Arrangement(track, _)) => Some(track),
            Some(detail_view::ClipRef::Slot(..)) => None,
            None => track,
        };
        // Leaving the one track pulled to the top (Escape twice, another clip or nothing picked), the
        // view keeps that track in sight, not the top of the whole list.
        if focus.is_none() && self.arrangement.focus.is_some() {
            self.arrangement.reveal = self.arrangement.focus;
        }
        self.arrangement.focus = focus;
        suhina_plugins::trace::state(
            "detail",
            format!(
                "ara {:?} piano {} pullable {pullable} pulled {pulled} detail_max {} focus {focus:?} showing {:?}",
                ara.map(|(t, c, d, open)| (t.0, c.0, d, open)),
                piano.is_some(),
                self.detail_max,
                self.detail_showing,
            ),
        );
        // The size the area's rules give it in the room there is, which the grid lays it out at
        // from the next frame on; a frame laid out at another size is drawn again at once.
        if follows {
            let shown = if stacked {
                self.detail_width = detail_width_in(detail_view::STACK_WIDTH, area.width());
                self.detail_width
            } else {
                // Only the chain shown: no taller than its panels, but never held under the area's
                // default height. A chain of short panels (a plug-in whose editor is not embedded,
                // or no devices at all) would otherwise keep the area too short to drag up.
                self.detail_height = detail_height_in(self.detail_height, area.height(), editor_needs, least);
                // A track's devices, reached by clicking the track, are open at one height or
                // closed; the height the user drags is the clip's, its editor's and the devices
                // reached from it.
                let track_devices = !stacked && clip.is_none() && !edits && !self.editor_in_chain;
                if pulled {
                    // Above it only the edited track, or nothing.
                    let above = focus.map_or(0.0, |t| self.arrangement.focus_height(t));
                    // The gap the grid leaves over the area, where its splitter is.
                    let gap = (laid.top() - main.bottom()).max(0.0);
                    (area.height() - above.min(area.height() - least) - gap).max(least)
                } else if track_devices {
                    self.device_row_height().min((area.height() - MIN_VIEW).max(least))
                } else {
                    self.detail_height
                }
            };
            // The grid adds the panel's own padding and border to the content size it is given.
            let [top, right, bottom, left] = panel_style.insets();
            if stacked {
                self.detail_shown.x = (shown - left - right).max(0.0);
            } else {
                self.detail_shown.y = (shown - top - bottom).max(0.0);
            }
            let laid_size = if stacked { laid.width() } else { laid.height() };
            // Once for each size: a sheet that holds the area to another (a `max-height`) does not
            // keep the window repainting.
            if (shown - laid_size).abs() > 0.5 && (shown - self.detail_asked).abs() > 0.5 {
                ui.ctx().request_repaint();
            }
            self.detail_asked = shown;
        }
        let (view, detail) = (main, laid);
        // The stacked devices beside a clip's editor: a column at the view's right, a stacked
        // panel wide, with its own splitter. As a track's devices in a row, it is open whole or
        // closed: a drag past half its width or a double-click closes it.
        let (view, side) = if side_devices {
            let width = detail_width_in(detail_view::STACK_WIDTH, view.width());
            let x = view.right() - width;
            let side_splitter = Rect::from_min_max(Pos2::new(x - SPLITTER, view.top()), Pos2::new(x, view.bottom()));
            let response = ui
                .interact(side_splitter, Id::unique("detail-side-splitter"), Sense::click_and_drag())
                .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, "Devices column splitter"));
            if response.double_clicked() {
                self.detail_showing = detail_view::Showing::Clip;
            }
            if response.dragged_by(egui::PointerButton::Primary) {
                let dragged = self.side_drag.unwrap_or(width) - response.drag_delta().x;
                self.side_drag = Some(dragged);
                if dragged < width / 2.0 {
                    self.detail_showing = detail_view::Showing::Clip;
                }
            } else {
                self.side_drag = None;
            }
            let style = crate::kit::element::DIVIDER.id("detail-splitter").response(&response).enter();
            paint_splitter(ui, side_splitter, &style, true);
            (
                Rect::from_min_max(view.min, Pos2::new(side_splitter.left(), view.bottom())),
                Some(Rect::from_min_max(Pos2::new(x, view.top()), view.max)),
            )
        } else if split {
            // Closed beside the editor, the column keeps its splitter along the view's right edge:
            // dragged left past half its width, or double-clicked, it opens, as Devices does.
            let side_splitter =
                Rect::from_min_max(Pos2::new((view.right() - SPLITTER).max(view.left()), view.top()), view.max);
            let response = ui
                .interact(side_splitter, Id::unique("detail-side-splitter"), Sense::click_and_drag())
                .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, "Closed devices column splitter"));
            let width = detail_view::STACK_WIDTH;
            if response.double_clicked() {
                self.detail_showing = detail_view::Showing::Devices;
            }
            if response.dragged_by(egui::PointerButton::Primary) {
                let dragged = self.side_drag.unwrap_or(0.0) - response.drag_delta().x;
                self.side_drag = Some(dragged);
                if dragged >= width / 2.0 {
                    self.detail_showing = detail_view::Showing::Devices;
                    // The same drag goes on from the open column's width.
                    self.side_drag = Some(width);
                }
            } else {
                self.side_drag = None;
            }
            let style = crate::kit::element::DIVIDER.id("detail-splitter").response(&response).enter();
            paint_splitter(ui, side_splitter, &style, true);
            (Rect::from_min_max(view.min, Pos2::new(side_splitter.left(), view.bottom())), None)
        } else {
            (view, None)
        };

        // The splitter: dragged to make the detail area bigger or smaller; under the
        // Arrangement view, past the view's least height the piano roll goes to the top, and a
        // double-click takes it there and back.
        if splitter.is_positive() {
            let open_or_closed = !stacked && clip.is_none() && !edits && !self.editor_in_chain;
            let divider = crate::kit::divider::show(
                ui,
                crate::kit::divider::Divider {
                    name: "detail-splitter",
                    key: Id::unique("detail-splitter"),
                    strip: splitter,
                    across: stacked,
                    label: "Detail area splitter",
                    size: if stacked { detail.width() } else { detail.height() },
                    // Towards the view grows it, wherever the sheet puts the area.
                    grows_forward: if stacked {
                        detail.center().x < view.center().x
                    } else {
                        detail.center().y < view.center().y
                    },
                    bounds: (0.0, if stacked { area.width() } else { area.height() }),
                },
            );
            let response = &divider.response;
            // A double-click closes the detail area, as a double-click on a split view's divider does
            // on macOS; a double-click on the closed splitter opens it at its default size.
            if response.double_clicked() {
                self.detail_max = false;
                self.hide_editor();
            }
            if let Some(size) = divider.size {
                if stacked {
                    // Stacked, the area is open a panel wide or closed, as a track's devices in
                    // a row are: past half that width a drag closes it.
                    if size < detail_view::STACK_WIDTH / 2.0 {
                        self.hide_editor();
                    }
                } else {
                    self.detail_max = pullable && size > area.height() - MIN_VIEW;
                    let closes_under = if open_or_closed { self.device_row_height() } else { least };
                    if size < closes_under / 2.0 {
                        // Dragged down past half its least height, the detail area closes; the same
                        // drag going back up opens it again (`collapsed_splitter`).
                        self.detail_max = false;
                        self.hide_editor();
                    } else if !self.detail_max && !open_or_closed {
                        self.detail_height = size.max(least);
                    }
                }
            }
            // A theme's layout that follows the splitter holds the dragged size to the bounds the
            // native layout keeps, in the room the view and the detail area share: a drag neither
            // takes the area, and with it its splitter, away nor closes the view. Held here, after
            // the drag, as a size the layout gives no room hides the area until it is opened again.
            if follows {
                let room = area;
                if stacked {
                    self.detail_width = detail_width_in(self.detail_width, room.width());
                } else {
                    self.detail_height = detail_height_in(self.detail_height, room.height(), editor_needs, least);
                }
            }
            paint_splitter(ui, splitter, &divider.style, stacked);
        }
        // The panel's own box, its background and border, once its splitter beside it is drawn;
        // it clips what the detail area paints (`overflow`), and the detail area goes in its
        // content box (#1336).
        let detail = panel_style.paint(ui.painter(), detail).content;

        if let Some(shown_track) = clip.map(ClipRef::track).or(track) {
            self.detail_drop = Some((side.unwrap_or(detail), shown_track));
        }
        let nam_lists = self.nam_lists();
        let mut shown = self.session().map_or(detail_view::Detail::Nothing, |s| {
            detail_view::build_with(s.project(), track, clip, &mut |t, p| reports.remove(&(t, p.to_vec())), nam_lists)
        });
        // Automated parameters show what plays at the playhead, running or stopped (#706, #811);
        // without a player, at the start.
        if let Some(session) = self.session() {
            let beat = session.player().map_or(0.0, |p| p.monitor().beat());
            // A lane dragged in the Arrangement view: the knob follows the line as it is dragged.
            let dragged = self.arrangement.automation.dragged.as_ref().filter(|_| self.shown == View::Arrangement);
            shown.follow_automation_dragged(session.project(), beat, dragged);
            // An audio clip's panel: where the playhead is in the clip, for its Set buttons, and
            // what each source says its tempo is while its Find menu is open
            // (`docs/spec/clip-panel.md`).
            if let detail_view::Detail::Clip(info, _) = &mut shown
                && let Some(ClipRef::Arrangement(track, id)) = info.at
                && let Some(panel) = info.panel.as_mut()
            {
                let placed = session
                    .project()
                    .arrangement
                    .lanes
                    .iter()
                    .filter(|l| l.track == Some(track))
                    .flat_map(|l| &l.clips)
                    .find(|c| c.id == id);
                // Only while the playhead is in the clip: before or after it, Set would put a marker
                // at the clip's edge.
                panel.playhead = placed
                    .filter(|c| beat >= c.start && beat < c.start + c.length)
                    .map(|c| detail_view::content_beat_at(c, beat));
            }
            if let detail_view::Detail::Clip(info, _) = &mut shown
                && let Some(panel) = info.panel.as_mut()
                && ui.ctx().data(|d| d.get_temp::<ClipId>(detail_view::find_tempo_key())) == Some(panel.clip)
            {
                let key = detail_view::tempo_candidates_key(panel.clip);
                panel.candidates = ui.ctx().data(|d| d.get_temp::<[Option<f64>; 3]>(key)).or_else(|| {
                    let found = suhina_session::clip_tempo_candidates(session, panel.clip)?;
                    ui.ctx().data_mut(|d| d.insert_temp(key, found));
                    Some(found)
                });
            }
        }
        // What the host hands devices' canvases: whether the panel is open, and live values (a
        // band's level and gain change) while a player runs.
        if let detail_view::Detail::Devices(chain) | detail_view::Detail::Clip(_, chain) = &mut shown {
            let monitor = self.session().and_then(|s| s.player()).map(suhina_session::Player::meter_monitor);
            let open = detail_view::expanded(ui.ctx()).filter(|(track, _)| *track == chain.track).map(|(_, at)| at);
            let track = chain.track;
            let mut seen = Vec::new();
            let live = detail_view::give_host_values(&mut chain.devices, open.as_ref(), &mut |at| {
                let values = monitor.and_then(|monitor| monitor.readouts(track, &at.0));
                if let Some(values) = &values {
                    seen.extend_from_slice(values);
                }
                values
            });
            if live {
                // Frames while they move; while they rest, often enough to catch a note that starts.
                let moved = ui.ctx().data_mut(|data| {
                    let last = data.get_temp_mut_or_default::<Vec<f32>>(egui::Id::unique("device-readouts"));
                    let moved = *last != seen;
                    *last = seen;
                    moved
                });
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(if moved { 33 } else { 100 }));
            }
        }
        self.expanded_panel = detail_view::expanded_device(ui.ctx(), &shown);
        if let Some(session) = self.session() {
            for channel in session.project().channels.iter().filter(|channel| {
                session
                    .project()
                    .tracks
                    .iter()
                    .any(|t| Some(t.id) == clip.map(ClipRef::track).or(track) && t.channel == Some(channel.id))
            }) {
                let Some(track) =
                    session.project().tracks.iter().find(|t| Some(t.id) == clip.map(ClipRef::track).or(track))
                else {
                    continue;
                };
                for (index, device) in channel.devices.iter().enumerate() {
                    if device.sample_file().is_none()
                        && device.kind != suhina_engine::model::DeviceKind::Builtin("sampler".into())
                    {
                        continue;
                    }
                    let id = crate::detail::device_canvas::sample_source_id(track.id, index);
                    ui.ctx().data_mut(|d| {
                        let version = (self.projects.active(), self.revision);
                        if d.get_temp::<(Option<ProjectId>, u64)>(id.with("manual-revision")) == Some(version) {
                            return;
                        }
                        d.insert_temp(id.with("manual-revision"), version);
                        if let Some(edits) =
                            device.sample_edit_data().and_then(suhina_device_api::sample_edits::SampleEdits::decode)
                        {
                            d.insert_temp(id.with("slice-edits"), std::sync::Arc::new(edits));
                        } else {
                            d.remove::<std::sync::Arc<suhina_device_api::sample_edits::SampleEdits>>(
                                id.with("slice-edits"),
                            );
                        }
                        if let Some(points) = device.sample_slices() {
                            d.insert_temp(id.with("manual-points"), std::sync::Arc::<[f64]>::from(points));
                        } else {
                            d.remove::<std::sync::Arc<[f64]>>(id.with("manual-points"));
                        }
                    });
                    let position =
                        session.player().map_or([None; 32], |p| p.meter_monitor().sample_positions(track.id, index));
                    ui.ctx().data_mut(|d| d.insert_temp(id.with("position"), position));
                    let channels =
                        session.player().and_then(suhina_session::Player::output_info).map_or(0, |i| i.input_channels);
                    ui.ctx().data_mut(|d| {
                        d.insert_temp(id.with("input"), (track.audio_input, channels, session.is_armed(track.id)))
                    });
                    ui.ctx().data_mut(|d| {
                        d.insert_temp(
                            id.with("name"),
                            device
                                .sample_file()
                                .and_then(|file| std::path::Path::new(file).file_name())
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                        )
                    });
                    ui.ctx()
                        .data_mut(|d| d.insert_temp(id.with("recording"), session.recording_sampler(track.id, index)));
                    if let Some(data) = device.sample_file().and_then(|file| session.audio().files.get(file)) {
                        ui.ctx().data_mut(|d| d.insert_temp(id, data.clone()));
                    } else {
                        ui.ctx().data_mut(|d| d.remove::<std::sync::Arc<suhina_engine::AudioData>>(id));
                    }
                }
            }
        }
        // What the piano roll has selected, described for the clip column (› Clip column).
        if let (detail_view::Detail::Clip(info, _), Some(piano), Some(session)) = (&mut shown, &piano, self.session()) {
            info.selection = detail_view::SelectionInfo::of(
                &piano.notes,
                self.piano_roll.acting(),
                detail_view::bar_beats(session.project()),
            );
        }
        if let detail_view::Detail::Clip(info, _) = &mut shown {
            info.switch = switch;
            info.switch_both = side_devices;
            // Several arrangement clips picked: the column says how many, and its switches turn
            // them all.
            if self.shown == View::Arrangement && self.arrangement.selected_clips().len() > 1 {
                let this = match info.at {
                    Some(detail_view::ClipRef::Arrangement(_, clip)) => Some(clip),
                    _ => None,
                };
                info.others = self
                    .arrangement
                    .selected_clips()
                    .iter()
                    .filter(|(_, id)| Some(*id) != this)
                    .map(|(_, id)| id.0)
                    .collect();
                // The picked MIDI clips, each a row that gives it the piano roll's focus (#1054).
                if !info.audio
                    && let Some(session) = self.session()
                {
                    let p = session.project();
                    info.picked = self
                        .arrangement
                        .selected_clips()
                        .iter()
                        .filter_map(|&(track, id)| {
                            let lane = p.arrangement.lanes.iter().find(|l| l.track == Some(track))?;
                            let clip = lane.clips.iter().find(|c| c.id == id)?;
                            let content = p.content(clip.content)?;
                            let notes = content
                                .lanes
                                .iter()
                                .any(|l| matches!(l.data, suhina_engine::model::LaneData::Notes(_)));
                            notes.then(|| {
                                let name = format!("{} - {}", p.shown_track_name(track), content.name);
                                (track, id, name, Some(id) == this)
                            })
                        })
                        .collect();
                }
            }
        }
        // A selected slot clip's follow action comes first, the rest of the detail beside or
        // under it.
        let follow = match (self.shown, self.view.selected_slot, self.session()) {
            (View::Session, Some((t, scene)), Some(s)) => follow_view::FollowInfo::build(s.project(), t, scene),
            _ => None,
        };
        let detail = match &follow {
            Some(info) => {
                let used = ui.scope_builder(egui::UiBuilder::new().max_rect(detail), |ui| {
                    follow_view::show(ui, detail, info, stacked, actions)
                });
                if stacked {
                    let top = (used.inner.bottom() + 1.0).min(detail.bottom());
                    Rect::from_min_max(Pos2::new(detail.left(), top), detail.max)
                } else {
                    let left = (used.inner.right() + 1.0).min(detail.right());
                    Rect::from_min_max(Pos2::new(left, detail.top()), detail.max)
                }
            }
            None => detail,
        };
        if piano.is_none() {
            // A note the audition sounds goes off with the piano roll, and the notes edited while
            // selected make room now.
            actions.extend(self.piano_roll.stop_audition());
            actions.extend(self.piano_roll.settle_on_hide());
            self.piano_roll.hide();
        }
        let mut places = Vec::new();
        let options =
            piano_roll::Options { toolbar_size: self.settings.toolbar_size, workflow: self.keymap.workflow() };
        // The toolbar's tooltips show the keys the keymap has now, so a rebind (or a preset) is
        // visible on the button.
        let keymap = self.keymap.clone();
        let copied = self.session().and_then(Session::copied_notes);
        // While the transport runs, the piano roll shows where playback is: for a slot's clip,
        // the beats since its launch (its placement starts at the launch and never ends); for
        // an arrangement clip, the transport's beat through the clip's placement.
        let playhead = match self.shown {
            View::Session => self.view.selected_slot.and_then(|(track, scene)| self.slot_playhead(track, scene)),
            View::Arrangement => {
                self.session().and_then(|s| s.player()).map(|p| p.monitor()).filter(|m| m.running()).map(|m| m.beat())
            }
        };
        // Where playback starts next, for an arrangement clip's piano roll (#951).
        let start_marker = (self.shown == View::Arrangement)
            .then(|| self.session().and_then(|s| s.player()).map(|p| p.monitor().start_beat()))
            .flatten();
        detail_view::set_list_vertical(ui.ctx(), self.settings.view.device_list_vertical);
        detail_view::set_list_project(ui.ctx(), self.projects.active().map_or(0, |project| project.0));
        let picks = self.device_picks();
        let piano_roll = &mut self.piano_roll;
        let picked = ui.scope_builder(egui::UiBuilder::new().max_rect(detail), |ui| {
            if let (Some(piano), detail_view::Detail::Clip(info, _)) = (&piano, &shown) {
                // The clip editor holds the clip's column, its note tools and the piano roll.
                let editor = crate::kit::element::CLIP_EDITOR.enter();
                let mut styled_ui = editor.ui(ui);
                let ui = &mut *styled_ui;
                // The clip's panel: the Clip part with what the tools act on under it, and the note
                // tools beside them while they are shown (the toolbar's three-slider button;
                // › Note tools).
                let selected = piano_roll.acting().clone();
                // A time selection with no note in it gives the tools nothing to act on.
                let nothing = selected.is_empty() && piano_roll.span_end().is_some();
                let tools = crate::detail::midi_tools::Tools {
                    address: &piano.address,
                    notes: &piano.notes,
                    selected: &selected,
                    length: piano.length,
                    nothing,
                };
                let show_tools = piano_roll.tools_open;
                let midi = (!info.audio).then_some(detail_view::MidiTools {
                    tools: &tools,
                    state: &mut *piano_roll,
                    show_tools,
                });
                let (rest, picked) = detail_view::show_clip_column(ui, &editor, info, stacked, midi, actions);
                let picked = detail_view::Picked { showing: picked.showing, ..detail_view::Picked::default() };
                let seeks = matches!(piano.address.get("clip"), Some(serde_json::Value::Number(_)));
                let cx = piano_roll::Context { options, keymap: &keymap, copied, playhead, start_marker, seeks };
                piano_roll::show(ui, rest, piano, piano_roll, cx, actions);
                // A press anywhere in a MIDI clip's detail area, its panel and note tools too,
                // keeps the keys with the piano roll: Ctrl+X, Ctrl+D and the others go on acting
                // on the notes, not on the Arrangement's clips (a typed field still takes them).
                if !info.audio
                    && ui.input(|i| i.pointer.any_pressed())
                    && crate::kit::widgets::pointer_over(ui, detail).is_some()
                {
                    piano_roll.focused = true;
                }
                picked
            } else if let (Some((_, _, device, _)), detail_view::Detail::Clip(info, chain)) = (ara, &shown) {
                let editor = crate::kit::element::CLIP_EDITOR.enter();
                let mut styled_ui = editor.ui(ui);
                let ui = &mut *styled_ui;
                let (rest, picked) = detail_view::show_clip_column(ui, &editor, info, stacked, None, actions);
                detail_view::show_ara_editor(ui, rest, chain, device, actions, &mut places);
                picked
            } else if stacked {
                detail_view::show_stack(ui, &shown, &picks, actions, &mut places)
            } else {
                detail_view::show_row(ui, &shown, &picks, actions, &mut places)
            }
        });
        // The stacked devices' column, beside the editor: above the panel, in the view's place, so
        // the panel's box is closed first and its `overflow` does not cut the column (#1336).
        panel_style.close_box();
        let side_clicked = match (side, &shown) {
            (Some(side), detail_view::Detail::Clip(_, chain)) => {
                let devices = detail_view::Detail::Devices(chain.clone());
                ui.scope_builder(egui::UiBuilder::new().max_rect(side), |ui| {
                    detail_view::show_stack(ui, &devices, &picks, actions, &mut places)
                })
                .inner
                .clicked
            }
            _ => None,
        };
        if let Some(showing) = picked.inner.showing {
            self.detail_showing = showing;
        }
        if let Some((track, at)) = picked.inner.clicked.or(side_clicked) {
            let (add, run) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
            self.pick_device(track, at, add, run);
            // A fresh pick says where the user is now: a removal's undo or redo no longer
            // moves the selection for them.
            self.pending_device_remove = None;
        }
        self.place_editors(&places);
        self.sample_drops = ui
            .ctx()
            .data(|d| {
                d.get_temp::<Vec<crate::detail::device_canvas::SampleTarget>>(
                    crate::detail::device_canvas::sample_targets_id(),
                )
            })
            .unwrap_or_default();
        // The toolbar's × hides the editor.
        if std::mem::take(&mut self.piano_roll.hide_requested) {
            self.hide_editor();
        }
        // A workflow mode or setting picked in the piano roll's toolbar: kept in the keymap.
        if let Some(picked) = self.piano_roll.workflow_picked.take() {
            picked.apply(&mut self.keymap);
            self.keymap_changed = true;
        }
        // A choice in the corner's menu: kept in the settings.
        if let Some(picked) = self.piano_roll.view_picked.take() {
            match picked {
                piano_roll::ViewPick::KeyStyle(style) => self.settings.piano_key_style = style,
                piano_roll::ViewPick::KeyNames(names) => self.settings.piano_key_names = names,
                piano_roll::ViewPick::AllowStacked(on) => self.settings.view.notes_replace = !on,
                piano_roll::ViewPick::CoveredWhole(on) => self.settings.view.covered_whole = on,
                piano_roll::ViewPick::HighlightScale(on) => self.settings.highlight_scale = on,
            }
            self.settings_changed = true;
        }
        // A ghost note double-clicked with the right button: its clip takes over the editor.
        if let Some(clip) = self.piano_roll.open_ghost.take() {
            match clip {
                ClipRef::Arrangement(track, id) => {
                    self.shown = View::Arrangement;
                    self.arrangement.select_clip(track, id);
                }
                ClipRef::Slot(track, scene) => {
                    self.shown = View::Session;
                    self.view.select_slot(track, scene);
                }
            }
        }
        // A grid value picked in the piano roll's grid menu or with its keys: the piano roll's
        // own, apart from the control bar's launch quantisation.
        if let Some(picked) = self.piano_roll.grid_picked.take() {
            self.piano_grid = picked;
        }
        view
    }

    /// Whether the detail area lies along the bottom: in a row, or with the devices stacked
    /// while the shown clip has an editor (the piano roll, an ARA plug-in's), which stays there.
    pub(super) fn editor_along_bottom(&self) -> bool {
        self.settings.device_layout != DeviceLayout::Stack
            || self.piano_clip(self.shown_clip()).is_some()
            || matches!(self.shown_clip(), Some(ClipRef::Arrangement(t, c)) if self.clip_uses_ara(t, c))
    }

    /// The splitter of the closed detail area, in the gap the grid keeps between the `main` view
    /// and the area's empty place `detail`. Along the bottom, dragged up past half the least
    /// height, the area opens at the height the drag reaches; stacked beside the view, dragged left
    /// past half a stacked panel's width, it opens whole, as the stacked area is open a panel wide
    /// or closed. A double-click opens it at its default size.
    pub(super) fn collapsed_splitter(&mut self, ui: &mut Ui, main: Rect, detail: Rect) {
        let splitter = crate::kit::native_css::splitter_between(detail, main, SPLITTER);
        if !splitter.is_positive() {
            return;
        }
        let side = !self.editor_along_bottom();
        let room = main.union(detail);
        let divider = crate::kit::divider::show(
            ui,
            crate::kit::divider::Divider {
                name: "detail-splitter",
                key: Id::unique("detail-splitter"),
                strip: splitter,
                across: side,
                label: "Detail area splitter",
                size: 0.0,
                grows_forward: if side {
                    detail.center().x < main.center().x
                } else {
                    detail.center().y < main.center().y
                },
                bounds: (0.0, if side { room.width() } else { room.height() }),
            },
        );
        if divider.response.double_clicked() {
            self.show_editor();
            // The default content height, with the area's border and padding round it (#1336).
            self.detail_height = DETAIL_HEIGHT + self.detail_insets.area;
            self.detail_shown = Vec2::new(detail_view::STACK_WIDTH, self.detail_height);
            self.detail_width = detail_view::STACK_WIDTH;
        }
        if let Some(size) = divider.size {
            if side {
                if size >= detail_view::STACK_WIDTH / 2.0 {
                    self.show_editor();
                    // The same drag goes on from the open area's width, so it does not close it
                    // again.
                    crate::kit::divider::restart(ui, Id::unique("detail-splitter"), detail_view::STACK_WIDTH, true);
                }
            } else {
                let least = self.least_detail_height();
                // A track's devices open at their one height, at the point a drag down closes them.
                let track_devices = self.shown_clip().is_none();
                let opens_at = if track_devices { self.device_row_height() } else { least };
                if size >= opens_at / 2.0 {
                    self.show_editor();
                    // The height dragged is a clip's view's.
                    if !track_devices {
                        self.detail_height = size.max(least);
                    }
                    self.detail_shown.y = if track_devices { opens_at } else { self.detail_height };
                }
            }
        }
        paint_splitter(ui, splitter, &divider.style, side);
    }

    /// The least height of the detail area in a row: the device panels' compact height (the
    /// area's default), so a clip's panel, its piano roll or its editor never gets too short to
    /// use, or the piano roll's own when that is more (several clips picked add their strip).
    /// Dragged under half of it, the area closes (› Least height and closing).
    pub(super) fn least_detail_height(&self) -> f32 {
        // Stacked devices leave the piano roll along the bottom, whichever the switch shows.
        let piano = self.piano_roll_on_show() && self.piano_clip(self.shown_clip()).is_some();
        // Several clips picked: their strip takes room from the rows.
        let picked = self.arrangement.selected_clips().len();
        let rows = if picked > 1 { picked.min(crate::detail::piano_roll_ghosts::MAX_STRIP_ROWS) } else { 0 };
        let strip = piano_roll::CLIP_STRIP * rows as f32;
        let compact = self.device_row_height();
        if piano { (piano_roll::LEAST_HEIGHT + strip + self.detail_insets.piano).max(compact) } else { compact }
    }

    /// The height a track's devices open at: a row of devices, and the boxes around it.
    fn device_row_height(&self) -> f32 {
        detail_view::device_row_height() + self.detail_insets.devices
    }

    /// The clip the detail area shows: the Arrangement view's selected clip, or the Launcher view's
    /// selected slot's.
    /// Whether Escape was pressed twice in a row this frame (the second press within
    /// [`DOUBLE_ESCAPE`] seconds of the first), with no text field taking the keys.
    pub(super) fn double_escape(&mut self, ui: &egui::Ui) -> bool {
        let (pressed, now) = ui.input(|i| (i.key_pressed(egui::Key::Escape) && i.modifiers.is_none(), i.time));
        if !pressed || ui.ctx().egui_wants_keyboard_input() {
            return false;
        }
        match self.last_escape.take() {
            Some(at) if now - at <= DOUBLE_ESCAPE => true,
            _ => {
                self.last_escape = Some(now);
                false
            }
        }
    }

    pub(super) fn shown_clip(&self) -> Option<ClipRef> {
        match self.shown {
            View::Session => self.view.selected_slot.map(|(t, s)| ClipRef::Slot(t, s)),
            View::Arrangement => self.arrangement.selected_clip().map(|(t, i)| ClipRef::Arrangement(t, i)),
        }
    }

    /// Hides the detail area as an editor: selecting a clip opens it again.
    pub(super) fn hide_editor(&mut self) {
        self.detail_open = false;
        self.editor_closed = true;
    }

    pub(super) fn show_editor(&mut self) {
        self.detail_open = true;
        self.editor_closed = false;
    }

    /// The piano roll for the selected arrangement clip, when it is a MIDI clip; its grid is its
    /// own grid value, apart from the control bar's launch quantisation.
    /// The piano roll for the shown clip: an arrangement clip's, or the Launcher view's selected
    /// slot's clip.
    pub(super) fn piano_clip(&self, clip: Option<detail_view::ClipRef>) -> Option<piano_roll::PianoClip> {
        let mut piano = match clip? {
            detail_view::ClipRef::Arrangement(track, clip) => {
                piano_roll::arrangement_clip(self.session()?.project(), track, clip)?
            }
            detail_view::ClipRef::Slot(track, scene) => {
                piano_roll::launcher_clip(self.session()?.project(), track, scene)?
            }
        };
        piano.grid_menu = self.piano_grid;
        piano.grid = self.piano_grid.fixed_step(piano.bar_beats);
        Some(piano)
    }

    /// Whether a MIDI clip's piano roll is on show: with the switch on Clip, or with the devices
    /// stacked, where Devices opens them beside it and it stays open.
    pub(super) fn piano_roll_on_show(&self) -> bool {
        self.detail_showing == detail_view::Showing::Clip
            || self.settings.device_layout == DeviceLayout::Stack
            || self.settings.view.stack_in_piano_roll
    }

    /// The params that name the clip the piano roll shows in a `note.*` command, if it shows one.
    pub fn piano_roll_address(&self) -> Option<serde_json::Value> {
        if !self.piano_roll_on_show() {
            return None;
        }
        let clip = match self.shown {
            View::Session => self.view.selected_slot.map(|(t, s)| detail_view::ClipRef::Slot(t, s)),
            View::Arrangement => self.arrangement.selected_clip().map(|(t, i)| detail_view::ClipRef::Arrangement(t, i)),
        };
        self.piano_clip(clip).map(|c| c.address)
    }

    /// Whether the detail area shows a track's device chain — not a clip's properties and
    /// notes: the selected device's Delete key applies only while its panel is on show.
    pub(super) fn detail_shows_devices(&self) -> bool {
        let clip = match self.shown {
            View::Session => self.view.selected_slot.map(|(t, s)| detail_view::ClipRef::Slot(t, s)),
            View::Arrangement => self.arrangement.selected_clip().map(|(t, i)| detail_view::ClipRef::Arrangement(t, i)),
        };
        clip.is_none() || self.detail_showing == detail_view::Showing::Devices
    }

    /// Whether the keyboard is the piano roll's: it was pressed last and shows one clip's notes.
    /// Then an edit key acts on those notes or on nothing, and never on the clip or the view
    /// behind; only a press in another view takes the keyboard from it.
    pub(super) fn piano_has_keyboard(&self) -> bool {
        self.piano_roll.focused && self.detail_open && self.piano_roll_on_show() && self.piano_shows_the_selection()
    }

    /// Whether the piano roll shows the one clip selected: the Launcher's slot, or the only clip
    /// picked in the Arrangement.
    fn piano_shows_the_selection(&self) -> bool {
        match self.shown {
            View::Session => self
                .view
                .selected_slot
                .is_some_and(|(t, s)| self.piano_clip(Some(detail_view::ClipRef::Slot(t, s))).is_some()),
            View::Arrangement => {
                self.arrangement.selected_clips().len() == 1
                    && self
                        .arrangement
                        .selected_clip()
                        .is_some_and(|(t, i)| self.piano_clip(Some(detail_view::ClipRef::Arrangement(t, i))).is_some())
            }
        }
    }

    /// Whether Ctrl+V pastes copied notes into the piano roll although it does not have the keys:
    /// notes were copied last and the piano roll shows the one clip selected.
    pub(super) fn piano_takes_paste(&self) -> bool {
        self.copied_last == CopiedLast::Notes
            && self.session().is_some_and(|s| s.copied_notes().is_some())
            && self.detail_open
            && self.piano_roll_on_show()
            && self.piano_shows_the_selection()
    }

    /// Whether the edit keys act on the piano roll's selected notes although several clips are picked:
    /// it was pressed last and notes of the focused clip are selected. With none, the keys are the
    /// clips'.
    pub(super) fn piano_edits_focused_clip(&self) -> bool {
        self.piano_roll.focused
            && !self.piano_roll.selected.is_empty()
            && self.detail_open
            && self.piano_roll_on_show()
            && self.shown == View::Arrangement
            && self.arrangement.selected_clips().len() > 1
            && self.piano_clip(self.shown_clip()).is_some()
    }

    /// What a key does in the piano roll, when it shows a clip and has the keyboard.
    pub(super) fn piano_key(&mut self, action: &str) -> Option<Action> {
        if !self.piano_roll_on_show() {
            return None;
        }
        let clip = match self.shown {
            View::Session => self
                .view
                .selected_slot
                .map(|(t, s)| detail_view::ClipRef::Slot(t, s))
                .and_then(|clip| self.piano_clip(Some(clip)))?,
            View::Arrangement => self
                .arrangement
                .selected_clip()
                .map(|(t, i)| detail_view::ClipRef::Arrangement(t, i))
                .and_then(|clip| self.piano_clip(Some(clip)))?,
        };
        // Where playback is, in the clip's placement's input (the arrangement's beat, or the
        // beats since a slot clip's launch); 0.0 while the transport is stopped.
        let playhead = match self.shown {
            View::Arrangement => self.timeline.as_ref().map_or(0.0, |(_, _, t)| t.beat),
            View::Session => {
                let (track, scene) = self.view.selected_slot?;
                self.slot_playhead(track, scene).unwrap_or(0.0)
            }
        };
        let copied = self.session().and_then(Session::copied_notes);
        self.piano_roll.key_action(action, &clip, playhead, copied)
    }

    /// The beats the selected slot's clip has played, while the engine plays that very slot: the
    /// piano roll's playhead for a launcher clip, whose placement starts at the launch.
    pub(super) fn slot_playhead(&self, track: TrackId, scene: usize) -> Option<f64> {
        let monitor = self.session()?.player()?.monitor();
        let column = self.session()?.project().launcher.columns.iter().position(|c| c.track == track)?;
        if !monitor.running() || monitor.column(column)?.playing != Some(scene) {
            return None;
        }
        monitor.elapsed_beats(column)
    }

    /// The piano roll's state: its zoom, scroll and selected notes.
    pub fn piano_roll(&self) -> &PianoRollState {
        &self.piano_roll
    }

    /// The piano roll's state, to change: the ignored render test opens its quantise block with
    /// this, where a click would toggle it back.
    #[doc(hidden)]
    pub fn piano_roll_mut(&mut self) -> &mut PianoRollState {
        &mut self.piano_roll
    }
}

/// The width of a detail area stacked beside the view, in a `room` this wide shared with it: at
/// least 180 px, and leaving the view 240 px.
fn detail_width_in(width: f32, room: f32) -> f32 {
    width.clamp(180.0, (room - 240.0).max(180.0))
}

/// The height of a detail area in a row under the view, in a `room` this tall shared with it: at
/// least `least`, and no taller than leaves the view its least height, unless an embedded editor
/// `needs` more. A chain of short panels (a plug-in whose editor is not embedded, or no devices at
/// all) would otherwise keep the area too short to drag up.
fn detail_height_in(height: f32, room: f32, needs: f32, least: f32) -> f32 {
    let most = (room - MIN_VIEW).max(needs.min(room - SPLITTER)).max(least);
    height.clamp(least, most)
}

/// The detail area's splitter: a strip with a grip across its middle, `#detail-splitter` (a
/// `divider`, with `:hover` and `:active`). The strip takes the element's `background-color` and
/// the grip its `color`; without them, the control background and the hint colour.
fn paint_splitter(ui: &Ui, splitter: Rect, style: &crate::kit::element::Entered, stacked: bool) {
    let painter = ui.painter_at(splitter);
    painter.rect_filled(splitter, 0.0, style.background_or(theme::control_background()));
    let grip = if stacked { Vec2::new(2.0, 32.0) } else { Vec2::new(32.0, 2.0) };
    painter.rect_filled(Rect::from_center_size(splitter.center(), grip), 0.0, style.color_or(theme::text_hint()));
}

/// What the detail area edits a clip in, for [`DetailInsets::around`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Edits {
    /// Nothing: it shows a row of devices.
    None,
    /// The piano roll, in the clip editor.
    PianoRoll,
    /// A plug-in's editor that edits clips (ARA), in the clip editor with no piano roll round it.
    Plugin,
}

/// What the detail area's boxes add above and below what they hold: their borders and padding.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct DetailInsets {
    /// Around what the area shows now: [`DetailInsets::devices`] or [`DetailInsets::piano`].
    pub area: f32,
    /// Around a row of devices: the workspace panel's, the detail area's and a device panel's.
    pub devices: f32,
    /// Around the piano roll: the workspace panel's, the clip editor's and the piano roll's.
    pub piano: f32,
}

impl DetailInsets {
    /// The insets inside a workspace panel whose border and padding take `panel` above and below:
    /// the detail area's and a device panel's (`device`, as shown) round a row of devices, the
    /// clip editor's and the piano roll's round the piano roll, and the clip editor's alone round
    /// a plug-in's editor, which no `piano-roll` box holds.
    pub(super) fn around(panel: f32, boxes: detail_view::BoxInsets, device: f32, edits: Edits) -> Self {
        let devices = panel + boxes.detail + device;
        let piano = panel + boxes.editor + boxes.piano;
        let area = match edits {
            Edits::None => devices,
            Edits::PianoRoll => piano,
            Edits::Plugin => panel + boxes.editor,
        };
        Self { area, devices, piano }
    }
}
