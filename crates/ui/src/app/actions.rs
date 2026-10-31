// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the window does with the actions its views hand it: commands run on the active project, and the window's own actions.

use super::*;

impl App {
    /// Runs a window action (`settings::keymap::actions`) on the next frame, as its key would:
    /// for menus outside the window, such as the macOS menu bar.
    pub fn queue_action(&mut self, id: &str) {
        self.queued.push(id.to_owned());
    }

    /// Does one keymap action: the window's own, one on the selection, or a command that needs
    /// no parameters.
    pub fn do_action(&mut self, action: &str, actions: &mut Vec<Action>) {
        let has_project = !self.projects.is_empty();
        match action {
            "settings.open" => self.settings_open = !self.settings_open,
            "help.about" => self.about_open = !self.about_open,
            "file.new" if self.start_panel_open() => {}
            "file.new" => self.open_project_dialog(false),
            // With clips selected in the Arrangement, the loop is exactly their span; with none,
            // Ctrl+L turns looping on or off as before.
            // A time selection loops exactly its span; a click on a clip ends it, so then the
            // clips' span is looped (below).
            "transport.setLoopEnabled"
                if self.shown == View::Arrangement && self.arrangement.time_selection().is_some() =>
            {
                if let Some(s) = self.arrangement.time_selection() {
                    actions.push(self.loop_or_unloop(s.start, s.end));
                }
            }
            "transport.setLoopEnabled"
                if self.shown == View::Arrangement && !self.arrangement.selected_clips().is_empty() =>
            {
                let timeline = self.timeline.as_ref().map(|(_, _, t)| t);
                let span = self
                    .arrangement
                    .selected_clips()
                    .iter()
                    .filter_map(|(track, id)| {
                        timeline?
                            .lanes
                            .iter()
                            .find(|l| l.track == *track)?
                            .clips
                            .iter()
                            .find(|c| c.id == *id)
                            .map(|c| (c.start, c.start + c.length))
                    })
                    .fold(None, |span: Option<(f64, f64)>, (start, end)| {
                        Some(span.map_or((start, end), |(s, e)| (s.min(start), e.max(end))))
                    });
                if let Some((start, end)) = span.filter(|(s, e)| e > s) {
                    actions.push(self.loop_or_unloop(start, end));
                }
            }
            "file.open" => self.open_project_dialog(true),
            "view.browser" if self.keymap.keys("view.browser").is_empty() => {
                // A keymap preset (Draw-first) may leave the browser with no key: the window's own
                // action then does nothing, so the file and what happens on a key press agree.
            }
            "view.browser" => self.toggle_browser(),
            "view.sampleMap" => {
                self.settings_open = false;
                self.toggle_large_map();
            }
            "view.sampleMapWindow" => self.toggle_map_window(),
            "view.zoomIn" => self.step_zoom(1),
            "view.waveformZoomIn" | "view.waveformZoomOut" => {
                let steps = if action == "view.waveformZoomIn" { 1 } else { -1 };
                self.arrangement.waveform_zoom =
                    crate::arrangement::arrangement_view::waveform_zoom_step(self.arrangement.waveform_zoom, steps);
            }
            "view.zoomOut" => self.step_zoom(-1),
            "view.zoomReset" => {
                if self.settings.zoom != crate::settings::ZOOM {
                    self.settings.zoom = crate::settings::ZOOM;
                    self.settings_changed = true;
                }
            }
            "browser.search" => {
                if !self.settings.browser_open {
                    self.toggle_browser();
                }
                self.settings_open = false;
                // Ctrl+F searches everything: All is chosen, and what the field says is selected
                // so typing replaces it. To search one kind, choose it and click the field.
                self.browser.place = None;
                self.browser.category = suhina_session::Category::All;
                self.browser.scroll = 0.0;
                self.browser.focus_search = true;
                self.browser.select_search = true;
            }
            _ if !has_project => {}
            "file.save" => self.save_active(false),
            "file.saveAs" => self.save_active(true),
            "file.saveTemplate" => self.save_template(),
            "view.toggle" => {
                self.shown = match self.shown {
                    View::Session => View::Arrangement,
                    View::Arrangement => View::Session,
                };
            }
            "view.session" => {
                self.shown = View::Session;
                self.close_large_map();
            }
            "view.detail" => {
                self.detail_open = !self.detail_open;
                self.editor_closed = false;
            }
            "view.editor" if self.shown_clip().is_some() => self.toggle_editor(),
            "view.editor" => {}
            "view.devices" => self.toggle_devices(),
            "view.araEditor" => self.open_ara_editor(),
            "view.plugin" => self.toggle_plugin_editor(),
            "view.mappingMode" => self.toggle_mapping_mode(),
            "track.addAudio" | "track.addMidi" => {
                let kind = if action == "track.addAudio" { "audio" } else { "midi" };
                let mut params = json!({ "kind": kind });
                // Below the selected track, in its group, as the track menu's Insert does.
                if let Some(after) = self.add_track_after() {
                    params["after"] = json!(after.0);
                }
                actions.push(Action::new("track.add", params));
            }
            "view.automation" => {
                if let Some((_, _, timeline)) = self.timeline.as_ref() {
                    let tracks: Vec<TrackId> = timeline.lanes.iter().map(|l| l.track).collect();
                    self.arrangement.automation.toggle_all(&tracks, &timeline.automation);
                }
            }
            // An empty MIDI clip over the time selection on each of its MIDI tracks (Ctrl+Shift+M).
            "clip.insertMidi" => {
                let Some(span) = self.arrangement.time_selection().cloned() else {
                    self.status = Status::Hint("Select a time range in the Arrangement first".into());
                    return;
                };
                let notes: Vec<TrackId> = self.timeline.as_ref().map_or_else(Vec::new, |(_, _, t)| {
                    t.lanes.iter().filter(|l| l.notes && span.tracks.contains(&l.track)).map(|l| l.track).collect()
                });
                if notes.is_empty() {
                    self.status = Status::Hint("The time selection has no MIDI track".into());
                }
                for track in notes {
                    actions.push(Action::new(
                        "clip.create",
                        json!({ "track": track.0, "start": span.start, "length": span.end - span.start }),
                    ));
                }
            }
            "view.arrangement" => {
                self.shown = View::Arrangement;
                self.close_large_map();
            }
            "transport.toggle" => {
                let running = self.session().and_then(|s| s.player()).is_some_and(|p| p.monitor().running());
                actions.push(if running { self.stop_action() } else { self.play_action(false) });
            }
            "transport.playFromSelection" => actions.push(self.play_action(true)),
            // Quantise in the piano roll, while it shows a clip.
            "pianoRoll.quantize" => {
                let clip = match self.shown {
                    View::Session => self.view.selected_slot.map(|(t, s)| detail_view::ClipRef::Slot(t, s)),
                    View::Arrangement => {
                        self.arrangement.selected_clip().map(|(t, i)| detail_view::ClipRef::Arrangement(t, i))
                    }
                };
                let showing = self.detail_open
                    && self.detail_showing == detail_view::Showing::Clip
                    && self.piano_clip(clip).is_some();
                if showing && let Some(quantize) = self.piano_roll.quantize_selected() {
                    actions.push(quantize);
                }
            }
            // Delete in the piano roll deletes its selected notes, not the clip.
            "selection.delete" if let Some(delete) = self.piano_roll.delete_selected() => actions.push(delete),
            // Copy, cut, paste and duplicate in the piano roll act on its notes, when it has some
            // to act on; its own keys act only there.
            "selection.copy" | "selection.cut" | "selection.paste" | "selection.duplicate"
                if let Some(edit) = self.piano_key(action) =>
            {
                actions.push(edit)
            }
            _ if crate::settings::keymap::piano_roll_only(action) => actions.extend(self.piano_key(action)),
            // A range painted on an automation lane: copy, cut, paste (from its start), duplicate
            // (right after it) and delete act on it (#916).
            "selection.copy" | "selection.cut" | "selection.paste" | "selection.duplicate" | "selection.delete"
                if self.shown == View::Arrangement && self.arrangement.automation.range.is_some() =>
            {
                let edit = action.trim_start_matches("selection.");
                if edit == "paste" && !self.session().is_some_and(Session::copied_automation) {
                    self.status = Status::Hint("Copy a range of automation first".into());
                    return;
                }
                actions.extend(self.arrangement.automation.range_actions(edit));
            }
            // Points selected on an automation lane, with no range: copy and cut take them, and
            // paste puts copied automation on the lane clicked last from the play position (#916).
            "selection.copy" | "selection.cut" | "selection.paste"
                if self.shown == View::Arrangement
                    && self.arrangement.automation.range.is_none()
                    && self.arrangement.automation.takes_point_keys(action.trim_start_matches("selection."))
                    && (action != "selection.paste" || self.session().is_some_and(Session::copied_automation)) =>
            {
                let at = self.timeline.as_ref().map_or(0.0, |(_, _, t)| t.beat);
                actions.extend(self.arrangement.automation.point_actions(action.trim_start_matches("selection."), at));
            }
            // In the Launcher view copy and paste act on the selected clip slot.
            // Picked devices (a click on a strip's device or a panel of the chain): copy, cut
            // and paste act on them, before any clip.
            "selection.copy" | "selection.cut" if self.device_focus && self.selected_device.is_some() => {
                let Some((track, _)) = self.device_picks().first().cloned() else { return };
                let devices = self.picked_devices();
                actions.push(Action::new("device.copy", json!({ "track": track.0, "devices": devices })));
                if action == "selection.cut" {
                    self.selected_device = None;
                    self.device_picks.clear();
                    actions.push(Action::new("device.removeMany", json!({ "track": track.0, "devices": devices })));
                }
            }
            "selection.paste"
                if self.device_focus
                    && self.selected_device.is_some()
                    && self.session().is_some_and(|s| s.copied_devices() > 0) =>
            {
                let picks = self.device_picks();
                let Some((track, _)) = picks.first().cloned() else { return };
                // After the last picked device on the track itself (one inside a group: after
                // its group).
                let after = picks.iter().filter_map(|(_, at)| at.0.first().copied()).max().unwrap_or(0) + 1;
                actions.push(Action::new("device.paste", json!({ "track": track.0, "position": after })));
            }
            // Ctrl+D copies the picked devices right after the last of them (#685).
            "selection.duplicate" if self.device_focus && self.selected_device.is_some() => {
                let Some((track, _)) = self.device_picks().first().cloned() else { return };
                let devices = self.picked_devices();
                actions.push(Action::new("device.duplicateMany", json!({ "track": track.0, "devices": devices })));
            }
            // Track headers clicked last: the edit keys act on the selected tracks (#919); paste
            // only when tracks were copied, else a copied clip pastes on the track as before.
            "selection.copy" | "selection.cut" | "selection.paste" | "selection.duplicate"
                if !self.focused_tracks().is_empty()
                    && (action != "selection.paste" || self.session().is_some_and(|s| s.copied_tracks() > 0)) =>
            {
                let tracks = self.focused_tracks();
                let ids: Vec<u32> = tracks.iter().map(|t| t.0).collect();
                match action {
                    "selection.copy" => actions.push(Action::new("track.copy", json!({ "tracks": ids }))),
                    "selection.cut" => actions.push(Action::new("track.cut", json!({ "tracks": ids }))),
                    "selection.duplicate" => actions.push(Action::new("track.duplicate", json!({ "tracks": ids }))),
                    _ => {
                        let after = tracks.last().map(|t| t.0);
                        actions.push(Action::new("track.paste", json!({ "after": after })));
                    }
                }
            }
            // A header clicked last but its tracks gone (cut): Ctrl+V still pastes the copied
            // tracks, after the last regular track (#919).
            "selection.paste" if self.track_keys_on() && self.session().is_some_and(|s| s.copied_tracks() > 0) => {
                let suffix = self.settings.view.copy_suffix();
                actions.push(Action::new("track.paste", json!({ "after": null, "suffix": suffix })));
            }
            "selection.copy" | "selection.paste" if self.shown == View::Session => {
                let Some((track, scene)) = self.view.selected_slot else {
                    self.status = Status::Hint("Click a clip slot first".into());
                    return;
                };
                let params = json!({ "track": track.0, "scene": scene + 1 });
                actions.push(Action::new(if action == "selection.copy" { "clip.copy" } else { "clip.paste" }, params));
            }
            // Cut and duplicate of a slot's clip wait for launcher clip commands; Delete works,
            // in the arm below.
            "selection.cut" | "selection.duplicate" if self.shown == View::Session => {
                self.status = Status::Hint("In the Launcher view, a slot's clip can be copied and pasted".into());
            }
            // Take pieces picked with Ctrl and a click: Delete deletes them, the last take first so
            // the numbers of the others hold when an emptied take goes (#989).
            "selection.delete" if self.shown == View::Arrangement && !self.arrangement.take_selected.is_empty() => {
                let mut picked = std::mem::take(&mut self.arrangement.take_selected);
                picked.sort_unstable_by_key(|(t, n, p)| (t.0, std::cmp::Reverse(*n), *p));
                let mut takes: Vec<(TrackId, usize)> = picked.iter().map(|(t, n, _)| (*t, *n)).collect();
                takes.dedup();
                for (track, take) in takes {
                    let pieces: Vec<usize> =
                        picked.iter().filter(|(t, n, _)| *t == track && *n == take).map(|(_, _, p)| p + 1).collect();
                    actions.push(Action::new(
                        "take.deletePieces",
                        json!({ "track": track.0, "take": take, "pieces": pieces }),
                    ));
                }
            }
            // A time selection in the Arrangement: the keys act on what plays inside it, as in
            // clips picked by a click act whole (the arm below).
            "selection.copy"
            | "selection.cut"
            | "selection.duplicate"
            | "selection.delete"
            | "selection.split"
            | "selection.consolidate"
                if self.shown == View::Arrangement
                    && !self.device_focus
                    && self.arrangement.automation.selected_points().is_none()
                    && self.arrangement.time_selection().is_some() =>
            {
                let Some(span) = self.arrangement.time_selection().cloned() else { return };
                let tracks: Vec<u32> = span.tracks.iter().map(|t| t.0).collect();
                let params = json!({ "tracks": tracks, "start": span.start, "end": span.end });
                match action {
                    "selection.delete" => actions.push(Action::new("range.delete", params)),
                    "selection.cut" => actions.push(Action::new("range.cut", params)),
                    "selection.copy" => actions.push(Action::new("range.copy", params)),
                    "selection.split" => actions.push(Action::new("range.split", params)),
                    "selection.duplicate" => {
                        actions.push(Action::new("range.duplicate", params));
                        // The selection moves onto the copy, so Ctrl+D again goes on.
                        let length = span.end - span.start;
                        self.arrangement.set_time_selection(Some(
                            crate::arrangement::arrangement_view::TimeSelection {
                                start: span.end,
                                end: span.end + length,
                                ..span
                            },
                        ));
                    }
                    _ => {
                        // Each track of the span with something in it: the new clip runs over the
                        // whole span, its empty part silent.
                        let timeline = self.timeline.as_ref().map(|(_, _, t)| t);
                        let playing = |track: u32| {
                            timeline.and_then(|t| t.lanes.iter().find(|l| l.track.0 == track)).is_some_and(|l| {
                                l.clips.iter().any(|c| c.start < span.end && c.start + c.length > span.start)
                            })
                        };
                        // In one row of a stack: that row's clips only, the other rows left be.
                        let in_row = |track: u32| -> Option<Vec<serde_json::Value>> {
                            let (row, rows) = span.row?;
                            let lane = timeline?.lanes.iter().find(|l| l.track.0 == track)?;
                            Some(
                                lane.clips
                                    .iter()
                                    .filter(|c| c.row == row && c.rows == rows)
                                    .filter(|c| c.start < span.end && c.start + c.length > span.start)
                                    .map(|c| json!(c.id.0))
                                    .collect(),
                            )
                        };
                        for track in tracks.into_iter().filter(|t| playing(*t)) {
                            let mut params = json!({ "track": track, "start": span.start, "end": span.end });
                            if let Some(clips) = in_row(track) {
                                params["clips"] = json!(clips);
                            }
                            actions.push(Action::new("clip.consolidate", params));
                        }
                    }
                }
            }
            "selection.copy" | "selection.cut" | "selection.duplicate" | "selection.delete" => {
                // Points selected on an automation lane are deleted before any clip.
                if action == "selection.delete"
                    && let Some(delete) = self.arrangement.automation.delete_selected()
                {
                    actions.push(delete);
                    return;
                }
                // A device selected in the detail area (its panel's click) is deleted before any
                // clip, while the detail shows the chain.
                if action == "selection.delete"
                    && (self.detail_shows_devices() || self.device_focus)
                    && let Some((track, at)) = self.selected_device.clone()
                    && self.session().is_some_and(|s| {
                        let p = s.project();
                        p.tracks
                            .iter()
                            .find(|t| t.id == track)
                            .and_then(|t| t.channel)
                            .and_then(|c| p.channels.iter().find(|ch| ch.id == c))
                            .is_some_and(|ch| suhina_engine::model::device_at(&ch.devices, &at.0).is_some())
                    })
                {
                    let picks = self.device_picks();
                    let devices = self.picked_devices();
                    self.selected_device = None;
                    self.device_picks.clear();
                    if picks.len() > 1 {
                        actions.push(Action::new("device.removeMany", json!({ "track": track.0, "devices": devices })));
                    } else {
                        actions.push(Action::new("device.remove", json!({ "track": track.0, "device": at.value() })));
                    }
                    return;
                }
                // A track header clicked last: Delete removes the selected tracks (#690).
                if action == "selection.delete" {
                    let tracks: Vec<TrackId> = match self.shown {
                        View::Arrangement if self.arrangement.track_focus => self.arrangement.selected_tracks.clone(),
                        View::Session if self.view.track_focus && self.view.selected_slot.is_none() => {
                            self.view.header_tracks()
                        }
                        _ => Vec::new(),
                    };
                    if !tracks.is_empty() {
                        self.arrangement.selected_tracks.clear();
                        self.arrangement.track_focus = false;
                        self.view.track_focus = false;
                        self.view.selected = None;
                        self.view.selected_tracks.clear();
                        for track in tracks {
                            actions.push(Action::new("track.delete", json!({ "track": track.0 })));
                        }
                        return;
                    }
                }
                // The Launcher view: Delete takes the selected slot's clip off its slot, and the
                // detail area falls back to the track's devices.
                if self.shown == View::Session {
                    if action == "selection.delete"
                        && let Some((track, scene)) = self.view.selected_slot
                        && self.session().is_some_and(|s| {
                            let p = s.project();
                            p.launcher
                                .columns
                                .iter()
                                .any(|c| c.track == track && c.slots.get(scene).is_some_and(|slot| slot.clip.is_some()))
                        })
                    {
                        self.view.selected_slot = None;
                        actions.push(Action::new("clip.delete", json!({ "track": track.0, "scene": scene + 1 })));
                    }
                    return;
                }
                let clips = self.selected_clip_list();
                if clips.is_empty() {
                    return;
                }
                let id = match action {
                    "selection.copy" => "clip.copy",
                    "selection.cut" => "clip.cut",
                    "selection.duplicate" => "clip.duplicate",
                    _ => "clip.delete",
                };
                if matches!(id, "clip.cut" | "clip.delete") {
                    self.arrangement.clear_selected_clip();
                }
                let mut params = json!({ "clips": clips });
                // A copy follows the stacking setting for its kind, as a moved clip does: with
                // clips of both kinds selected, only a setting that covers them all stacks.
                if id == "clip.duplicate" && self.stacks_every_selected_clip() {
                    params["stack"] = json!(true);
                }
                actions.push(Action::new(id, params));
            }
            "selection.rename" => {
                // A track header clicked last: the track's name, in its header (#696).
                let focused_track = match self.shown {
                    View::Arrangement if self.arrangement.track_focus => {
                        self.arrangement.selected_tracks.first().copied()
                    }
                    View::Session if self.view.track_focus => self.view.selected,
                    _ => None,
                };
                if let Some(track) = focused_track {
                    let name = self
                        .session()
                        .and_then(|s| s.project().track(track))
                        .map(|t| t.name.clone())
                        .unwrap_or_default();
                    self.rename = Some((json!({ "rename_track": track.0 }), name, false));
                    return;
                }
                // Several clips selected, with the setting on: every one of them (#693).
                let selected = self.arrangement.selected_clips().to_vec();
                if self.shown == View::Arrangement && selected.len() > 1 && self.settings.view.rename_selected {
                    let (_, id) = self.arrangement.selected_clip().unwrap_or(selected[0]);
                    let name = self
                        .timeline
                        .as_ref()
                        .and_then(|(_, _, t)| t.lanes.iter().flat_map(|l| &l.clips).find(|c| c.id == id))
                        .map(|c| c.name.clone())
                        .unwrap_or_default();
                    let clips: Vec<serde_json::Value> = selected.iter().map(|(_, c)| json!({ "clip": c.0 })).collect();
                    self.rename = Some((json!({ "clip": id.0, "clips": clips }), name, false));
                    return;
                }
                // The selected clip: the Arrangement's, or the Launcher view's slot.
                let target = match self.shown {
                    View::Arrangement => self.arrangement.selected_clip().map(|(track, id)| {
                        let name = self
                            .timeline
                            .as_ref()
                            .and_then(|(_, _, t)| t.lanes.iter().find(|l| l.track == track))
                            .and_then(|l| l.clips.iter().find(|c| c.id == id))
                            .map(|c| c.name.clone())
                            .unwrap_or_default();
                        (json!({ "clip": id.0 }), name)
                    }),
                    View::Session => self.view.selected_slot.and_then(|(track, scene)| {
                        let grid = self.session().map(|s| TrackGrid::build(s.project(), None))?;
                        let slot = grid.columns.iter().find(|c| c.track == track)?.slots.get(scene)?;
                        (slot.state != crate::interaction::grid::SlotState::Empty)
                            .then(|| (json!({ "track": track.0, "scene": scene + 1 }), slot.name.clone()))
                    }),
                };
                match target {
                    Some((target, name)) => self.rename = Some((target, name, false)),
                    None => self.status = Status::Hint("Select a clip first".into()),
                }
            }
            "selection.split" | "selection.consolidate" => {
                let Some((track, id)) = self.arrangement.selected_clip() else {
                    self.status = Status::Hint("Select a clip in the Arrangement first".into());
                    return;
                };
                let timeline = self.timeline.as_ref().map(|(_, _, t)| t);
                let beat = timeline.map_or(0.0, |t| t.beat);
                let clip = timeline
                    .and_then(|t| t.lanes.iter().find(|l| l.track == track))
                    .and_then(|l| l.clips.iter().find(|c| c.id == id))
                    .map(|c| (c.start, c.length));
                let Some((start, length)) = clip else { return };
                if action == "selection.consolidate" {
                    // The selected clips on its track, from the first start to the last end,
                    // become one new clip.
                    let lane = timeline.and_then(|t| t.lanes.iter().find(|l| l.track == track));
                    let (from, to) = self
                        .arrangement
                        .selected_clips()
                        .iter()
                        .filter(|(t, _)| *t == track)
                        .filter_map(|(_, id)| lane.and_then(|l| l.clips.iter().find(|c| c.id == *id)))
                        .fold((start, start + length), |(s, e), c| (s.min(c.start), e.max(c.start + c.length)));
                    self.arrangement.clear_selected_clip();
                    let params = json!({ "track": track.0, "start": from, "end": to });
                    actions.push(Action::new("clip.consolidate", params));
                } else if beat > start && beat < start + length {
                    actions.push(Action::new("clip.split", json!({ "clip": id.0, "at": beat })));
                } else {
                    self.status = Status::Hint("Move the playhead into the selected clip to split it there".into());
                }
            }
            "selection.mute" => {
                // A device panel clicked last: the key turns that device off or on again, as its
                // header's square does.
                if (self.detail_shows_devices() || self.device_focus)
                    && let Some((track, at)) = self.selected_device.clone()
                    && let Some(enabled) = self.session().and_then(|s| {
                        let p = s.project();
                        let channel = p.track(track).and_then(|t| t.channel)?;
                        suhina_engine::model::device_at(&p.channel(channel)?.devices, &at.0).map(|d| d.enabled)
                    })
                {
                    // Every picked device follows the one clicked last (#948).
                    let picks = self.device_picks();
                    if picks.len() > 1 && picks.iter().all(|(t, _)| *t == track) {
                        let devices = self.picked_devices();
                        actions.push(Action::new(
                            "device.setEnabledMany",
                            json!({ "track": track.0, "devices": devices, "enabled": !enabled }),
                        ));
                    } else {
                        actions.push(Action::new(
                            "device.setEnabled",
                            json!({ "track": track.0, "device": at.value(), "enabled": !enabled }),
                        ));
                    }
                    return;
                }
                // Track headers clicked last: the key mutes the selected tracks, or unmutes them
                // when the first is muted (#948).
                let tracks = self.focused_tracks();
                if !tracks.is_empty() {
                    let muted = self.session().is_some_and(|s| {
                        let p = s.project();
                        tracks.first().and_then(|t| p.channel_of_track(*t)).is_some_and(|c| c.mute)
                    });
                    let ids: Vec<u32> = tracks.iter().map(|t| t.0).collect();
                    actions.push(Action::new("track.setMute", json!({ "tracks": ids, "mute": !muted })));
                    return;
                }
                // A range painted in the Arrangement: the clips under it on its tracks (in its
                // row of a stack, when it is in one), turned off or on as one.
                if self.shown == View::Arrangement
                    && let Some(span) = self.arrangement.time_selection().cloned()
                    && let Some((_, _, timeline)) = self.timeline.as_ref()
                {
                    let under: Vec<(serde_json::Value, bool)> = timeline
                        .lanes
                        .iter()
                        .filter(|l| span.tracks.contains(&l.track))
                        .flat_map(|l| {
                            l.clips.iter().filter(move |c| {
                                c.start < span.end
                                    && c.start + c.length > span.start
                                    && span.row.is_none_or(|(row, rows)| c.row == row && c.rows == rows)
                            })
                        })
                        .map(|c| (json!({ "clip": c.id.0 }), c.enabled))
                        .collect();
                    if !under.is_empty() {
                        let enabled = under.iter().any(|(_, on)| *on);
                        let clips: Vec<serde_json::Value> = under.into_iter().map(|(c, _)| c).collect();
                        actions.push(Action::new("clip.setEnabled", json!({ "clips": clips, "enabled": !enabled })));
                    }
                    return;
                }
                let clips = self.selected_clip_list();
                if clips.is_empty() {
                    return;
                }
                // What the clips are now decides which way the key turns them: any of them
                // playing turns the whole selection off, else all of them back on.
                let enabled = self.any_selected_clip_plays();
                actions.push(Action::new("clip.setEnabled", json!({ "clips": clips, "enabled": !enabled })));
            }
            // A device clicked last: Ctrl+A picks every device of its chain, or of the group's
            // branch it is in.
            "selection.selectAll" if self.device_focus && self.selected_device.is_some() => {
                let Some((track, at)) = self.selected_device.clone() else { return };
                let parent = at.split().0.to_vec();
                let siblings: Vec<usize> = self
                    .session()
                    .and_then(|s| {
                        let p = s.project();
                        let devices = &p.channel(p.track(track)?.channel?)?.devices;
                        let list = match parent.split_last() {
                            None => devices.as_slice(),
                            Some((branch, group)) => {
                                &suhina_engine::model::device_at(devices, group)?
                                    .as_group()?
                                    .branches
                                    .get(*branch)?
                                    .devices
                            }
                        };
                        Some(list.iter().enumerate().filter(|(_, d)| !d.clip_host).map(|(i, _)| i).collect())
                    })
                    .unwrap_or_default();
                if siblings.is_empty() {
                    return;
                }
                self.device_picks = siblings
                    .into_iter()
                    .map(|i| {
                        let mut path = parent.clone();
                        path.push(i);
                        (track, detail_view::DeviceAt(path))
                    })
                    .collect();
                if !self.device_picks.contains(&(track, at)) {
                    self.selected_device = self.device_picks.last().cloned();
                }
            }
            "selection.selectAll" => {
                let clips: Vec<(TrackId, ClipId)> = self
                    .session()
                    .map(|session| {
                        session
                            .project()
                            .arrangement
                            .lanes
                            .iter()
                            .filter_map(|lane| lane.track.map(|track| (track, &lane.clips)))
                            .flat_map(|(track, clips)| clips.iter().map(move |clip| (track, clip.id)))
                            .collect()
                    })
                    .unwrap_or_default();
                self.arrangement.select_clips(clips);
            }
            "selection.paste"
                if self.shown == View::Arrangement && self.session().is_some_and(Session::copied_from_tracks) =>
            {
                // Copied from several tracks: each part back on its own track, from the play
                // start marker.
                let beat = self.timeline.as_ref().map_or(0.0, |(_, _, t)| t.beat);
                actions.push(Action::new("clip.paste", json!({ "start": beat })));
            }
            "selection.paste" => {
                // After the selected clip, or at the playhead on the selected track.
                let timeline = self.timeline.as_ref().map(|(_, _, t)| t);
                let target = match self.arrangement.selected_clip() {
                    Some((track, id)) => timeline
                        .and_then(|t| t.lanes.iter().find(|l| l.track == track))
                        .and_then(|l| l.clips.iter().find(|c| c.id == id))
                        .map(|c| (track, if self.settings.view.replace_clip { c.start } else { c.start + c.length })),
                    None => self.arrangement.insert.or_else(|| {
                        self.arrangement.selected_tracks.first().map(|t| (*t, timeline.map_or(0.0, |t| t.beat)))
                    }),
                };
                match target {
                    Some((track, start)) => {
                        let mut params = json!({ "track": track.0, "start": start });
                        if self.settings.view.replace_clip
                            && let Some((_, clip)) = self.arrangement.selected_clip()
                        {
                            params["replace"] = json!(clip.0);
                        }
                        // The pasted clip follows the stacking setting for its kind, as a moved clip does.
                        let audio = self.session().and_then(Session::copied_clip_is_audio);
                        if audio.is_some_and(|audio| self.stacks(audio)) {
                            params["stack"] = json!(true);
                        }
                        actions.push(Action::new("clip.paste", params))
                    }
                    None => self.status = Status::Hint("Select a clip or a track to paste on".into()),
                }
            }
            // Picked devices (a strip's list or the chain) go into a device group: those next to
            // each other on one track, in the place of the first.
            "selection.group" if self.device_focus && self.selected_device.is_some() => {
                let picks = self.device_picks();
                let Some((track, primary)) = picks.first().cloned() else { return };
                let parent = primary.split().0.to_vec();
                if picks.iter().any(|(_, at)| at.split().0 != parent.as_slice()) {
                    self.status = Status::Hint("Group devices of one chain or branch".into());
                    return;
                }
                let mut indices: Vec<usize> = picks.iter().map(|(_, at)| at.split().1).collect();
                indices.sort_unstable();
                indices.dedup();
                let (Some(&first), Some(&last)) = (indices.first(), indices.last()) else { return };
                if last - first + 1 != indices.len() {
                    self.status = Status::Hint("Group devices that sit next to each other".into());
                    return;
                }
                let mut path = parent;
                path.push(first);
                let at = detail_view::DeviceAt(path);
                actions.push(Action::new(
                    "device.group",
                    json!({ "track": track.0, "device": at.value(), "count": indices.len() }),
                ));
                self.selected_device = Some((track, at.clone()));
                self.device_picks = vec![(track, at)];
            }
            "selection.ungroup" => {
                if self.device_focus
                    && let Some((track, at)) = self.selected_device.clone()
                {
                    actions.push(Action::new("device.ungroup", json!({ "track": track.0, "device": at.value() })));
                } else {
                    self.status = Status::Hint("Click a device group first".into());
                }
            }
            "selection.group" => {
                if self.arrangement.selected_tracks.is_empty() {
                    return;
                }
                let tracks: Vec<u32> = self.arrangement.selected_tracks.iter().map(|t| t.0).collect();
                actions.push(Action::new("track.group", json!({ "tracks": tracks })));
                self.arrangement.selected_tracks.clear();
                self.view.selected_tracks.clear();
            }
            "view.computerKeyboard" => self.toggle_typing_keys(),
            // The Arrangement's tool keys, while its toolbar shows (#991).
            id if let Some(grid) = crate::arrangement::arrangement_view::grid_key(id, self.arrangement.grid) => {
                self.arrangement.grid = grid;
                self.status = Status::Hint(format!("Arrangement grid: {}", grid.label()));
            }
            id if id.starts_with("arrangement.tool") => {
                let index =
                    id.trim_start_matches("arrangement.tool").parse::<usize>().ok().and_then(|n| n.checked_sub(1));
                if self.arrangement.toolbar
                    && self.shown == View::Arrangement
                    && let Some((tool, _)) = index.and_then(|i| arrangement_view::ArrangementTool::ALL.get(i))
                {
                    self.arrangement.tool = *tool;
                }
            }
            // The selected parts of a comp take the same span of the take above or below (#989).
            "selection.takeUp" | "selection.takeDown" => {
                let direction = if action == "selection.takeUp" { "up" } else { "down" };
                let Some(session) = self.session() else { return };
                let p = session.project();
                for (track, id) in self.arrangement.selected_clips() {
                    let comped = p
                        .arrangement
                        .lanes
                        .iter()
                        .filter(|l| l.track == Some(*track))
                        .flat_map(|l| &l.clips)
                        .find(|c| c.id == *id)
                        .is_some_and(|c| p.comp_source(*track, c).is_some());
                    if comped {
                        actions.push(Action::new("take.swap", json!({ "clip": id.0, "direction": direction })));
                    }
                }
            }
            "marker.addAtPlayhead" => {
                // Where the playhead is while playing; stopped, where playback would start.
                let monitor = self.session().and_then(|s| s.player()).map(|p| p.monitor());
                let beat = monitor.map_or(0.0, |m| if m.running() { m.beat() } else { m.start_beat() });
                let beat = if beat.is_finite() { (beat.max(0.0) * 1000.0).round() / 1000.0 } else { 0.0 };
                actions.push(Action::new("marker.add", json!({ "beat": beat })));
            }
            "selection.launch" => {
                if let (View::Session, Some((track, scene))) = (self.shown, self.view.selected_slot) {
                    actions.push(Action::new("clip.launch", json!({ "track": track.0, "scene": scene + 1 })));
                }
            }
            // Points selected on an automation lane: the arrows move them, a grid step in time or a
            // small step of value (#916).
            "selection.up" | "selection.down" | "selection.left" | "selection.right"
                if self.shown == View::Arrangement && self.arrangement.automation.selected_points().is_some() =>
            {
                let Some((_, _, timeline)) = self.timeline.as_ref() else { return };
                let step = self.arrangement.grid_step(timeline.bar_beats);
                let step = if step > 0.0 { step } else { 1.0 };
                let direction = action.trim_start_matches("selection.");
                if let Some(nudge) = self.arrangement.automation.nudge(&timeline.automation, direction, step) {
                    actions.push(nudge);
                }
            }
            "selection.up" | "selection.down" | "selection.left" | "selection.right" => {
                let (View::Session, Some(from)) = (self.shown, self.view.selected_slot) else { return };
                let step = match action {
                    "selection.up" => Step::Up,
                    "selection.down" => Step::Down,
                    "selection.left" => Step::Left,
                    _ => Step::Right,
                };
                let grid = self.session().map(|s| TrackGrid::build(s.project(), None));
                if let Some((track, scene)) = grid.and_then(|g| session_view::step_slot(&g, from, step)) {
                    self.view.select_slot(track, scene);
                }
            }
            "project.next" | "project.previous" if self.projects.len() < 2 => {}
            // Ctrl+Tab opens the switcher, or moves on in it while Ctrl stays down; letting go
            // of Ctrl switches (`tab_switcher_ui`).
            "project.next" | "project.previous" => {
                let ids = self.projects.ids();
                let count = ids.len();
                let from = self
                    .tab_switcher
                    .or_else(|| self.projects.active().and_then(|a| ids.iter().position(|i| *i == a)))
                    .unwrap_or(0);
                let step = if action == "project.next" { 1 } else { count - 1 };
                self.tab_switcher = Some((from + step) % count);
            }
            command => actions.push(Action::new_owned(command, json!({}))),
        }
    }

    /// The X on one of several picked devices removes every picked device, as Delete does
    /// (#686): its `device.remove` becomes `device.removeMany` of the picks, and the picks go.
    pub(super) fn remove_every_pick(&mut self, action: Action) -> Action {
        if action.id != "device.remove" || self.device_picks.len() < 2 {
            return action;
        }
        let track = action.params["track"].as_u64().and_then(|t| u32::try_from(t).ok()).map(TrackId);
        let at = detail_view::DeviceAt::parse(&action.params["device"]);
        let (Some(track), Some(at)) = (track, at) else { return action };
        if !self.device_picks.contains(&(track, at)) || self.device_picks.iter().any(|(t, _)| *t != track) {
            return action;
        }
        let devices: Vec<Value> = self.device_picks.iter().map(|(_, at)| at.value()).collect();
        self.selected_device = None;
        self.device_picks.clear();
        Action::new("device.removeMany", json!({ "track": track.0, "devices": devices }))
    }

    /// Whether a clip of this kind (audio, or notes) leaves the clips it lands on as they are:
    /// Options › Settings › sample stacking for audio, MIDI stacking for notes.
    pub(super) fn stacks(&self, audio: bool) -> bool {
        if audio { self.settings.sample_stacking } else { self.settings.midi_stacking }
    }

    /// The clips the Arrangement view has selected, as `clip.*` commands name them.
    pub(super) fn selected_clip_list(&self) -> Vec<serde_json::Value> {
        self.arrangement.selected_clips().iter().map(|(_, clip)| json!({ "clip": clip.0 })).collect()
    }

    /// Whether any selected clip plays now. The window reads the clips it drew; a clip it cannot
    /// find counts as playing, so the first key press turns the selection off.
    pub(super) fn any_selected_clip_plays(&self) -> bool {
        let Some((_, _, timeline)) = self.timeline.as_ref() else { return true };
        self.arrangement.selected_clips().iter().any(|(track, id)| {
            timeline
                .lanes
                .iter()
                .find(|l| l.track == *track)
                .and_then(|l| l.clips.iter().find(|c| c.id == *id))
                .is_none_or(|c| c.enabled)
        })
    }

    /// Whether the stacking setting covers every selected clip: with clips of both kinds picked,
    /// only when both settings are on does a copy leave what it lands on alone.
    pub(super) fn stacks_every_selected_clip(&self) -> bool {
        let Some((_, _, timeline)) = self.timeline.as_ref() else { return false };
        let selected = self.arrangement.selected_clips();
        if selected.is_empty() {
            return false;
        }
        selected.iter().all(|(track, id)| {
            timeline
                .lanes
                .iter()
                .find(|l| l.track == *track)
                .and_then(|l| l.clips.iter().find(|c| c.id == *id))
                .is_none_or(|c| self.stacks(c.audio))
        })
    }

    /// Makes the whole window `steps` zoom steps larger or smaller and keeps that in the settings.
    pub(super) fn step_zoom(&mut self, steps: i32) {
        if self.settings.step_zoom(steps) {
            self.settings_changed = true;
        }
    }

    /// Draws the window at the zoom the settings keep. The keymap owns the zoom keys, so the
    /// toolkit's own Ctrl and +, which the settings would not remember, is turned off.
    pub(super) fn apply_zoom(&self, ctx: &egui::Context) {
        if ctx.options(|o| o.zoom_with_keyboard) {
            ctx.options_mut(|o| o.zoom_with_keyboard = false);
        }
        let wanted = self.settings.zoom_factor();
        if (ctx.zoom_factor() - wanted).abs() > f32::EPSILON {
            ctx.set_zoom_factor(wanted);
        }
    }

    /// Ctrl+L over a span: loops it, or, when the loop is on over that very span already, turns
    /// looping off, so the same key takes the loop away again.
    pub(super) fn loop_or_unloop(&self, start: f64, end: f64) -> Action {
        let looping = self.session().is_some_and(|s| {
            let transport = &s.project().transport;
            transport.loop_enabled
                && transport.loop_range.is_some_and(|r| (r.start - start).abs() < 1e-6 && (r.end - end).abs() < 1e-6)
        });
        if looping {
            Action::new("transport.setLoopEnabled", json!({ "enabled": false }))
        } else {
            Action::new("transport.setLoop", json!({ "start": start, "end": end, "enabled": true }))
        }
    }

    /// Whether the edit keys act on tracks: a track header was the last thing clicked (in the
    /// Launcher view, with no slot selected).
    pub(super) fn track_keys_on(&self) -> bool {
        match self.shown {
            View::Arrangement => self.arrangement.track_focus,
            View::Session => self.view.track_focus && self.view.selected_slot.is_none(),
        }
    }

    /// The tracks the edit keys act on: the selected ones while [`Self::track_keys_on`], in
    /// project order.
    pub(super) fn focused_tracks(&self) -> Vec<TrackId> {
        let selected: Vec<TrackId> = match self.shown {
            View::Arrangement if self.arrangement.track_focus => self.arrangement.selected_tracks.clone(),
            View::Session if self.view.track_focus && self.view.selected_slot.is_none() => self.view.header_tracks(),
            _ => Vec::new(),
        };
        let Some(session) = self.session() else { return Vec::new() };
        session.project().tracks.iter().map(|t| t.id).filter(|id| selected.contains(id)).collect()
    }

    /// Where a track added from the keyboard goes: after the lowest selected track that is not a
    /// return track; `None` (after the last regular track) without one.
    pub(super) fn add_track_after(&self) -> Option<TrackId> {
        let project = self.session()?.project();
        let selected: Vec<TrackId> = match self.shown {
            View::Session => self.view.header_tracks(),
            View::Arrangement => self.arrangement.selected_tracks.clone(),
        };
        project
            .tracks
            .iter()
            .rev()
            .filter(|t| selected.contains(&t.id))
            .find(|t| {
                let role = project.channels.iter().find(|c| Some(c.id) == t.channel).map(|c| c.role);
                role != Some(suhina_engine::model::ChannelRole::Effect)
            })
            .map(|t| t.id)
    }

    /// Auto-arm (Settings › MIDI, #834): when the selected track changes, arms it and disarms
    /// the track auto-arm armed before, so a MIDI controller or the audio input plays into what
    /// the user looks at. A track armed by hand is never disarmed here, and one already armed is
    /// left as it is. A track that plays neither notes nor audio, a return, the main track or a
    /// group leaves the arm where it is, and so does any change while a take is recording.
    /// Through `track.setArm`, as the arm button, so an audio track with no input gets the
    /// device's first one as the button gives it.
    pub(super) fn follow_selection_with_arm(&mut self) {
        let project = self.projects.active();
        // A track it armed that is no longer armed (disarmed by hand, or deleted) is not its own
        // any more: armed by hand later, it stays. Looked at with the setting off too, so a
        // track disarmed and armed again by hand meanwhile is the user's when it comes back on.
        if let Some(project) = project
            && let Some(&track) = self.auto_arm.armed.get(&project)
            && !self.session().is_some_and(|s| s.is_armed(track))
        {
            self.auto_arm.armed.remove(&project);
        }
        if !self.settings.auto_arm {
            // Turned on again, it arms the track selected then.
            self.auto_arm.seen = None;
            return;
        }
        let selected = self.selected_track();
        let seen = project.map(|p| (p, selected));
        if self.auto_arm.seen == seen {
            return;
        }
        let Some((project, Some(track))) = seen else {
            self.auto_arm.seen = seen;
            return;
        };
        let Some(session) = self.session() else { return };
        if session.is_recording() {
            // Not seen yet: once the take ends, the track selected then is armed.
            return;
        }
        let p = session.project();
        let takes_input = p.track(track).is_some_and(|t| {
            let role = p.channels.iter().find(|c| Some(c.id) == t.channel).map(|c| c.role);
            (t.content.notes || t.content.audio)
                && matches!(role, None | Some(suhina_engine::model::ChannelRole::Regular))
        });
        let already = session.is_armed(track);
        self.auto_arm.seen = seen;
        if !takes_input {
            return;
        }
        let before = self.auto_arm.armed.get(&project).copied();
        if let Some(before) = before.filter(|b| *b != track) {
            self.auto_arm.armed.remove(&project);
            if self.session().is_some_and(|s| s.is_armed(before)) {
                self.run(&Action::new("track.setArm", json!({ "track": before.0, "armed": false })));
            }
        }
        if !already {
            self.run(&Action::new("track.setArm", json!({ "track": track.0, "armed": true })));
            if self.session().is_some_and(|s| s.is_armed(track)) {
                self.auto_arm.armed.insert(project, track);
            }
        }
    }

    /// `action` widened to the selection it is part of (#948): `device.setEnabled` on one of
    /// several devices picked on its track becomes `device.setEnabledMany` on all of them, and
    /// `track.setMute`, `track.setVolume` or `track.setPan` on one of several selected tracks
    /// sets them all; `None` otherwise.
    pub(super) fn to_selection(&self, action: &Action) -> Option<Action> {
        let id = |v: &serde_json::Value| v.as_u64().and_then(|n| u32::try_from(n).ok());
        match action.id {
            "device.setEnabled" => {
                let track = TrackId(id(&action.params["track"])?);
                let device = detail_view::DeviceAt::parse(&action.params["device"])?;
                let picks = self.device_picks();
                if picks.len() < 2 || !picks.contains(&(track, device)) || picks.iter().any(|(t, _)| *t != track) {
                    return None;
                }
                let devices = self.picked_devices();
                let enabled = action.params["enabled"].clone();
                Some(Action::new(
                    "device.setEnabledMany",
                    json!({ "track": track.0, "devices": devices, "enabled": enabled }),
                ))
            }
            // A value set on one of several selected tracks, by a click, a drag, a typed number or
            // the arrow keys, is set on all of them (#707, #948).
            "track.setMute" | "track.setVolume" | "track.setPan" => {
                let [one] = action.params["tracks"].as_array()?.as_slice() else { return None };
                let track = TrackId(id(one)?);
                let selected = &self.arrangement.selected_tracks;
                if selected.len() < 2 || !selected.contains(&track) {
                    return None;
                }
                let tracks: Vec<u32> = selected.iter().filter(|t| **t != TrackId::MAIN).map(|t| t.0).collect();
                let mut params = action.params.clone();
                params["tracks"] = json!(tracks);
                Some(Action::new(action.id, params))
            }
            _ => None,
        }
    }

    /// Selects `tracks` in both views as a header click would, the first one shown in the
    /// detail area and scrolled into view (#918); nothing for none.
    pub(super) fn select_tracks(&mut self, tracks: Vec<TrackId>) {
        let Some(&first) = tracks.first() else { return };
        self.arrangement.selected_tracks.clone_from(&tracks);
        self.arrangement.track_focus = true;
        self.arrangement.reveal = Some(first);
        self.view.selected = Some(first);
        self.view.selected_tracks = tracks;
        self.view.selected_slot = None;
        self.view.track_focus = true;
        self.view.reveal = Some(first);
    }

    /// The track selected in the view shown.
    pub(super) fn selected_track(&self) -> Option<TrackId> {
        match self.shown {
            View::Session => self.view.selected,
            View::Arrangement => self.arrangement.selected_tracks.first().copied(),
        }
    }
}

/// A mapping command's target in words: "track 2 scene 1", "scene 3", "the stop button of track 2".
pub(super) fn mapping_target(p: &serde_json::Value) -> String {
    match p["target"].as_str() {
        Some("slot") => format!("track {} scene {}", p["track"], p["scene"]),
        Some("scene") => format!("scene {}", p["scene"]),
        _ => format!("the stop button of track {}", p["track"]),
    }
}

/// What the status bar says after a command ran, or nothing for silent ones.
pub(super) fn describe(action: &Action) -> Option<String> {
    let p = &action.params;
    Some(match action.id {
        "clip.launch" => format!("Launched scene {} on track {}", p["scene"], p["track"]),
        "scene.launch" => format!("Launched scene {}", p["scene"]),
        "track.stop" => format!("Stopping track {}", p["track"]),
        "launcher.stopAll" => "Stopping all clips".into(),
        "launcher.addMapping" => format!(
            "Mapped {} to {}",
            p["source"].as_str().unwrap_or_default().trim_start_matches("key:"),
            mapping_target(p)
        ),
        "launcher.removeMapping" => format!("Removed the keys of {}", mapping_target(p)),
        "launcher.setQuantization" => format!("Launch quantisation {}", p["value"].as_str().unwrap_or_default()),
        "clip.setLaunchMode" => format!("Launch mode {}", p["mode"].as_str().unwrap_or_default()),
        "clip.setQuantization" => format!("Clip launch quantisation {}", p["value"].as_str().unwrap_or_default()),
        "clip.setLegato" => {
            if p["on"] == true {
                "Legato on".into()
            } else {
                "Legato off".into()
            }
        }
        "transport.setTempo" => format!("Tempo {}", p["bpm"]),
        "clip.copy" => "Copied the clip".into(),
        "clip.paste" if p.get("scene").is_some() => format!("Pasted into scene {} on track {}", p["scene"], p["track"]),
        "clip.paste" => "Pasted the clip".into(),
        "edit.undo" => "Undone".into(),
        "edit.redo" => "Redone".into(),
        _ => return None,
    })
}

/// The track to select once `removed` are deleted: the first track after the last of them that
/// stays, or the last one before them when nothing stays after; returns and Main are left out
/// (#918).
pub(super) fn track_after_removing(project: &suhina_engine::model::Project, removed: &[TrackId]) -> Option<TrackId> {
    let numbered: Vec<TrackId> =
        project.tracks.iter().map(|t| t.id).filter(|id| project.track_number(*id).is_some()).collect();
    let last = numbered.iter().rposition(|id| removed.contains(id))?;
    let first = numbered.iter().position(|id| removed.contains(id))?;
    // A deleted group takes the tracks inside it along.
    let inside = |track: TrackId| {
        let mut parent = project.track(track).and_then(|t| t.parent);
        for _ in 0..project.tracks.len() {
            let Some(group) = parent else { return false };
            if removed.contains(&group) {
                return true;
            }
            parent = project.track(group).and_then(|t| t.parent);
        }
        false
    };
    let gone = |id: &TrackId| removed.contains(id) || inside(*id);
    numbered
        .iter()
        .skip(last + 1)
        .find(|id| !gone(id))
        .or_else(|| numbered.iter().take(first).rev().find(|id| !gone(id)))
        .copied()
}
