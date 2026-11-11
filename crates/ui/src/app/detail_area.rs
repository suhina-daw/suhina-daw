// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The detail area under or beside the views: the selected track's devices, the clip's piano roll, and the editors of plug-ins and ARA.

use super::*;

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

    /// Lays out the detail area beside or under the view in `area`, draws it with its splitter,
    /// and returns what is left for the view. With `geometry` the theme's layout places it: the
    /// view's rectangle, its own, and whether its size follows the splitter
    /// (`Workspace::detail_follows`).
    pub(super) fn detail_area(
        &mut self,
        ui: &mut Ui,
        area: Rect,
        actions: &mut Vec<Action>,
        geometry: Option<(Rect, Rect, bool)>,
    ) -> Rect {
        let panel_style = crate::kit::element::WORKSPACE_PANEL.id("panel-detail").enter();
        let styled_ui = panel_style.ui(ui);
        // The splitter a theme's layout keeps lies in the gap beside the area: drawn and taking the
        // pointer there too (#1240).
        let css_splitter = geometry.map(|(view, detail, follows)| {
            if follows { crate::kit::native_css::splitter_between(detail, view, SPLITTER) } else { Rect::NOTHING }
        });
        let mut styled_ui = match (geometry, css_splitter) {
            (Some((_, detail, _)), Some(splitter)) => styled_ui.clipped_to(detail.union(splitter)),
            _ => styled_ui,
        };
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
            .map(|(_, height)| detail_view::row_height_with_editor(height, scale))
            .fold(0.0, f32::max);
        if !stacked {
            let needed = embedded
                .iter()
                .filter(|(key, _)| !self.fitted_editors.contains(key))
                .filter_map(|(_, embedded)| *embedded)
                .map(|(_, height)| detail_view::row_height_with_editor(height, scale))
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
        let least = self.least_detail_height();
        // The piano roll, or an ARA plug-in's editor in the clip's place, can be pulled to the
        // top, the edited track's lane above it (Melodyne as a MIDI clip's notes are).
        let pullable =
            geometry.is_none() && !stacked && (piano.is_some() || ara.is_some()) && self.shown == View::Arrangement;
        let pulled = pullable && self.detail_max;
        // With the switch on Devices the track's devices take the editor's place, and nothing is
        // pulled to the top (› Where it opens in `docs/spec/ui-clip-editor.md`).
        let switch = (piano.is_some() || ara.is_some()).then_some(self.detail_showing);
        let ara = ara.filter(|_| self.detail_showing == detail_view::Showing::Clip);
        let piano = piano.filter(|_| self.detail_showing == detail_view::Showing::Clip);
        let edits = piano.is_some() || ara.is_some();
        let (pullable, pulled) = (pullable && edits, pulled && edits);
        let focus = match clip {
            Some(detail_view::ClipRef::Arrangement(track, _))
                if pulled && self.settings.piano_roll_top == PianoRollTop::Track =>
            {
                Some(track)
            }
            _ => None,
        };
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
        let (view, splitter, detail) = if let Some((view, detail, _)) = geometry {
            (view, css_splitter.unwrap_or(Rect::NOTHING), detail)
        } else if stacked {
            self.detail_width = detail_width_in(self.detail_width, area.width());
            let x = area.right() - self.detail_width;
            (
                Rect::from_min_max(area.min, Pos2::new(x - SPLITTER, area.bottom())),
                Rect::from_min_max(Pos2::new(x - SPLITTER, area.top()), Pos2::new(x, area.bottom())),
                Rect::from_min_max(Pos2::new(x, area.top()), area.max),
            )
        } else {
            // Only the chain shown: no taller than its panels, but never held under the area's
            // default height. A chain of short panels (a plug-in whose editor is not embedded,
            // or no devices at all) would otherwise keep the area too short to drag up.
            self.detail_height = detail_height_in(self.detail_height, area.height(), editor_needs, least);
            // A track's devices, reached by clicking the track, are open at one height or closed; the
            // height the user drags is the clip's, its editor's and the devices reached from it.
            let track_devices = !stacked && clip.is_none() && !edits && !self.editor_in_chain;
            let shown_height = if track_devices {
                detail_view::device_row_height().min((area.height() - MIN_VIEW).max(least))
            } else {
                self.detail_height
            };
            let y = if pulled {
                // Above it only the edited track, or nothing.
                let above = focus.map_or(0.0, |t| self.arrangement.focus_height(t));
                (area.top() + above.min(area.height() - least) + SPLITTER).min(area.bottom() - least)
            } else {
                area.bottom() - shown_height
            };
            (
                Rect::from_min_max(area.min, Pos2::new(area.right(), (y - SPLITTER).max(area.top()))),
                Rect::from_min_max(Pos2::new(area.left(), y - SPLITTER), Pos2::new(area.right(), y)),
                Rect::from_min_max(Pos2::new(area.left(), y), area.max),
            )
        };

        // The splitter: dragged to make the detail area bigger or smaller; under the
        // Arrangement view, past the view's least height the piano roll goes to the top, and a
        // double-click takes it there and back.
        if splitter.is_positive() {
            let open_or_closed = !stacked && clip.is_none() && !edits && !self.editor_in_chain;
            let cursor = if stacked { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical };
            let sense = Sense::click_and_drag();
            let response = ui.interact(splitter, Id::unique("detail-splitter"), sense).on_hover_cursor(cursor);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, "Detail area splitter"));
            // A double-click closes the detail area, as a double-click on a split view's divider does
            // on macOS; a double-click on the closed splitter opens it at its default size.
            if response.double_clicked() {
                self.detail_max = false;
                self.hide_editor();
            }
            if response.dragged_by(egui::PointerButton::Primary) {
                let delta = response.drag_delta();
                if stacked {
                    self.detail_width -= delta.x;
                } else {
                    let height = self.detail_drag.unwrap_or(detail.height()) - delta.y;
                    self.detail_drag = Some(height);
                    self.detail_max = pullable && height > area.height() - MIN_VIEW;
                    let closes_under = if open_or_closed { detail_view::device_row_height() } else { least };
                    if height < closes_under / 2.0 {
                        // Dragged down past half its least height, the detail area closes; the same
                        // drag going back up opens it again (`collapsed_splitter`).
                        self.detail_max = false;
                        self.hide_editor();
                    } else if !self.detail_max && !open_or_closed {
                        self.detail_height = height.max(least);
                    }
                }
            } else {
                self.detail_drag = None;
            }
            // A theme's layout that follows the splitter holds the dragged size to the bounds the
            // native layout keeps, in the room the view and the detail area share: a drag neither
            // takes the area, and with it its splitter, away nor closes the view. Held here, after
            // the drag, as a size the layout gives no room hides the area until it is opened again.
            if let Some((view, detail, true)) = geometry {
                let room = view.union(detail);
                if stacked {
                    self.detail_width = detail_width_in(self.detail_width, room.width());
                } else {
                    self.detail_height = detail_height_in(self.detail_height, room.height(), editor_needs, least);
                }
            }
            paint_splitter(ui, splitter, &response, stacked);
        }

        if let Some(shown_track) = clip.map(ClipRef::track).or(track) {
            self.detail_drop = Some((detail, shown_track));
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
            // A note the audition sounds goes off with the piano roll.
            actions.extend(self.piano_roll.stop_audition());
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
                let (mut rest, picked) = detail_view::show_clip_column(ui, info, stacked, actions);
                let picked = detail_view::Picked { showing: picked.showing, ..detail_view::Picked::default() };
                // The MIDI tools column, between the clip's properties and the note grid. With the
                // detail area stacked there is no room beside the notes, so it is not drawn there
                // (the quantise settings and Ctrl+U still work: › Other note tools).
                if piano_roll.tools_open && !stacked {
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
                    let before = actions.len();
                    rest = crate::detail::midi_tools::show(ui, rest, &tools, piano_roll, actions);
                    if nothing {
                        actions.truncate(before);
                    }
                }
                let seeks = matches!(piano.address.get("clip"), Some(serde_json::Value::Number(_)));
                let cx = piano_roll::Context { options, keymap: &keymap, copied, playhead, start_marker, seeks };
                piano_roll::show(ui, rest, piano, piano_roll, cx, actions);
                picked
            } else if let (Some((_, _, device, _)), detail_view::Detail::Clip(info, chain)) = (ara, &shown) {
                let (rest, picked) = detail_view::show_clip_column(ui, info, stacked, actions);
                detail_view::show_ara_editor(ui, rest, chain, device, actions, &mut places);
                picked
            } else if stacked {
                detail_view::show_stack(ui, &shown, &picks, actions, &mut places)
            } else {
                detail_view::show_row(ui, &shown, &picks, actions, &mut places)
            }
        });
        if let Some(showing) = picked.inner.showing {
            self.detail_showing = showing;
        }
        if let Some((track, at)) = picked.inner.clicked {
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

    /// The splitter along the bottom while the detail area is hidden: dragged up past half the
    /// least height, the detail area opens at the height the drag reaches. Returns the room left
    /// above it for the view.
    pub(super) fn collapsed_splitter(&mut self, ui: &mut Ui, area: Rect) -> Rect {
        let splitter = Rect::from_min_max(Pos2::new(area.left(), (area.bottom() - SPLITTER).max(area.top())), area.max);
        let response = ui
            .interact(splitter, Id::unique("detail-splitter"), Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::ResizeVertical);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, "Detail area splitter"));
        if response.double_clicked() {
            self.show_editor();
            self.detail_height = DETAIL_HEIGHT;
            self.detail_width = detail_view::STACK_WIDTH;
        }
        if response.dragged_by(egui::PointerButton::Primary) {
            let height = self.detail_drag.unwrap_or(0.0) - response.drag_delta().y;
            self.detail_drag = Some(height);
            let least = self.least_detail_height();
            // A track's devices open at their one height, at the point a drag down closes them.
            let track_devices = self.shown_clip().is_none();
            let opens_at = if track_devices { detail_view::device_row_height() } else { least };
            if height >= opens_at / 2.0 {
                self.show_editor();
                // The height dragged is a clip's view's.
                if !track_devices {
                    self.detail_height = height.max(least);
                }
            }
        } else {
            self.detail_drag = None;
        }
        paint_splitter(ui, splitter, &response, false);
        Rect::from_min_max(area.min, Pos2::new(area.right(), splitter.top()))
    }

    /// The least height of the detail area in a row: the piano roll's while it shows one, else
    /// 80 px.
    pub(super) fn least_detail_height(&self) -> f32 {
        let piano = self.settings.device_layout != DeviceLayout::Stack
            && self.detail_showing == detail_view::Showing::Clip
            && self.piano_clip(self.shown_clip()).is_some();
        // Several clips picked: their strip takes room from the rows.
        let picked = self.arrangement.selected_clips().len();
        let rows = if picked > 1 { picked.min(crate::detail::piano_roll_ghosts::MAX_STRIP_ROWS) } else { 0 };
        let strip = piano_roll::CLIP_STRIP * rows as f32;
        if piano { piano_roll::LEAST_HEIGHT + strip } else { 80.0 }
    }

    /// The clip the detail area shows: the Arrangement view's selected clip, or the Launcher view's
    /// selected slot's.
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

    /// The params that name the clip the piano roll shows in a `note.*` command, if it shows one.
    pub fn piano_roll_address(&self) -> Option<serde_json::Value> {
        if self.detail_showing == detail_view::Showing::Devices {
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
        self.piano_roll.focused
            && self.detail_open
            && self.detail_showing == detail_view::Showing::Clip
            && match self.shown {
                View::Session => self
                    .view
                    .selected_slot
                    .is_some_and(|(t, s)| self.piano_clip(Some(detail_view::ClipRef::Slot(t, s))).is_some()),
                View::Arrangement => {
                    self.arrangement.selected_clips().len() == 1
                        && self.arrangement.selected_clip().is_some_and(|(t, i)| {
                            self.piano_clip(Some(detail_view::ClipRef::Arrangement(t, i))).is_some()
                        })
                }
            }
    }

    /// Whether the edit keys act on the piano roll's selected notes although several clips are picked:
    /// it was pressed last and notes of the focused clip are selected. With none, the keys are the
    /// clips'.
    pub(super) fn piano_edits_focused_clip(&self) -> bool {
        self.piano_roll.focused
            && !self.piano_roll.selected.is_empty()
            && self.detail_open
            && self.detail_showing == detail_view::Showing::Clip
            && self.shown == View::Arrangement
            && self.arrangement.selected_clips().len() > 1
            && self.piano_clip(self.shown_clip()).is_some()
    }

    /// What a key does in the piano roll, when it shows a clip and has the keyboard.
    pub(super) fn piano_key(&mut self, action: &str) -> Option<Action> {
        if self.detail_showing == detail_view::Showing::Devices {
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
fn paint_splitter(ui: &Ui, splitter: Rect, response: &egui::Response, stacked: bool) {
    let style = crate::kit::element::DIVIDER.id("detail-splitter").response(response).enter();
    let painter = ui.painter_at(splitter);
    painter.rect_filled(splitter, 0.0, style.background_or(theme::control_background()));
    let grip = if stacked { Vec2::new(2.0, 32.0) } else { Vec2::new(32.0, 2.0) };
    painter.rect_filled(Rect::from_center_size(splitter.center(), grip), 0.0, style.color_or(theme::text_hint()));
}
