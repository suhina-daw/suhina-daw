// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Drags and drops: files from the system, items from the browser, devices, and clips carried between the views.

use super::*;

impl App {
    /// Handles files dropped from the system: folders, themes and projects here; the samples and
    /// MIDI files are returned, to land where the views say they are dropped.
    pub(super) fn dropped_files(&mut self, ui: &Ui) -> Vec<BrowserItem> {
        // The missing files panel takes a dropped folder as the folder to search.
        if self.missing.open {
            return Vec::new();
        }
        let dropped: Vec<PathBuf> = ui.input(|i| {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect()
        });
        let is = |path: &PathBuf, ext: &str| path.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext));
        let mut opened = false;
        let mut clips = Vec::new();
        for path in dropped {
            if let Some(item) = drop::system_file(&path).filter(|_| !path.is_dir()) {
                clips.push(item);
            } else if path.is_dir() {
                match self.projects.execute("library.addFolder", &json!({ "folder": path.display().to_string() })) {
                    Ok(_) => {
                        self.settings.browser_open = true;
                        self.browser.relist();
                        self.status = Status::Done(format!("Added {} to the browser", path.display()));
                    }
                    Err(e) => self.status = Status::Error(e.message().to_owned()),
                }
            } else if is(&path, "json") || is(&path, "zip") {
                self.themes.import(path);
            } else if (is(&path, "als") || suhina_session::is_project_file(&path)) && !opened {
                opened = true;
                self.request_open(path);
            } else {
                let what = "is not a sample, a MIDI file, a project (.suhina), a Live Set (.als), a theme (.json), a zip of themes or a folder for the browser";
                self.status = Status::Error(format!("{} {what}", path.display()));
            }
        }
        clips
    }

    /// Samples and MIDI files dragged over the window from the system, while it reports them.
    pub(super) fn hovered_files(&self, ui: &Ui) -> Vec<BrowserItem> {
        if self.missing.open {
            return Vec::new();
        }
        ui.input(|i| {
            i.raw
                .hovered_files
                .iter()
                .filter_map(|f| f.path.as_deref())
                .filter(|p| !p.is_dir())
                .filter_map(drop::system_file)
                .collect()
        })
    }

    /// While items are dragged from the browser: the label beside the pointer, Escape to stop,
    /// and the drop when the button is let go.
    pub(super) fn browser_drag(&mut self, ui: &Ui) {
        let Some(items) = self.browser.drag.clone() else { return };
        let (mut released, mut escape, mut ctrl, mut pointer) = ui.input(|i| {
            (!i.pointer.primary_down(), i.key_pressed(Key::Escape), i.modifiers.command, i.pointer.latest_pos())
        });
        // A drag from the map's own window: that window has the pointer until the button is let go.
        if let Some(remote) = self.map_window.drag {
            (released, escape, ctrl) = (remote.released, remote.escape, remote.ctrl);
            pointer = self.drag_pointer(ui.ctx());
        }
        let target = self.drop_target(&items, pointer);
        if escape {
            self.browser.drag = None;
            self.map_window.drag = None;
            return;
        }
        let separate = ctrl != self.settings.drop_separate;
        if let Some(pointer) = pointer {
            self.drag_label(ui, pointer, &items, target, separate);
        }
        if released {
            self.browser.drag = None;
            self.map_window.drag = None;
            self.view.drop = None;
            self.arrangement.drop = None;
            if let Some(target) = target {
                // Dropped into the project: the preview has done its job.
                browser_view::stop_preview(&mut self.browser, &mut self.projects);
                self.drop_items(&items, target, separate);
            }
        }
    }

    /// While a device is carried from a chain or a strip's list: the label beside the pointer,
    /// Escape to stop, and on another track's column, header or lane when the button is let go
    /// the device moves there, or with Ctrl held is copied (#688).
    pub(super) fn device_drag(&mut self, ui: &Ui) {
        let Some((from, index)) = self.device_carry else { return };
        let (released, escape, copy, pointer) = ui.input(|i| {
            (!i.pointer.primary_down(), i.key_pressed(Key::Escape), i.modifiers.command, i.pointer.latest_pos())
        });
        if escape {
            self.device_carry = None;
            return;
        }
        let target = match self.shown {
            View::Session => self.view.drop.as_ref().and_then(|p| p.target),
            View::Arrangement => self.arrangement.drop.as_ref().and_then(|p| p.target),
        };
        let to = match target {
            Some(DropTarget::Track(t) | DropTarget::Lane { track: t, .. } | DropTarget::Slot { track: t, .. }) => {
                Some(t).filter(|t| *t != from)
            }
            _ => None,
        };
        let session = self.session();
        let name = |track: TrackId| {
            session.and_then(|s| s.project().track(track)).map_or_else(String::new, |t| t.name.clone())
        };
        let device = session
            .and_then(|s| {
                let p = s.project();
                let channel = p.track(from)?.channel?;
                p.channel(channel)?.devices.get(index).map(|d| d.name.clone())
            })
            .unwrap_or_default();
        if !released {
            if let Some(pointer) = pointer {
                let text = match to {
                    Some(to) if copy => format!("Copy {device} to {}", name(to)),
                    Some(to) => format!("Move {device} to {}", name(to)),
                    None => device.clone(),
                };
                drag_label(ui.ctx(), pointer, &text, to.is_some());
            }
            return;
        }
        self.device_carry = None;
        self.view.drop = None;
        self.arrangement.drop = None;
        if let Some(to) = to {
            if !copy {
                self.selected_device = None;
                self.device_picks.clear();
            }
            self.run(&Action::new(
                "device.moveToTrack",
                json!({ "track": from.0, "device": index, "toTrack": to.0, "copy": copy }),
            ));
        }
    }

    /// Samples and MIDI files from the system over the window: the label beside the pointer while
    /// they hover, and the drop where the views say. Without a pointer position (an OS drag on a
    /// platform that does not report one) they go where Enter puts them.
    pub(super) fn system_drag(&mut self, ui: &Ui, items: Vec<BrowserItem>, dropped: bool) {
        let pointer = self.drag_pointer(ui.ctx());
        let target = self.drop_target(&items, pointer);
        let separate = ui.input(|i| i.modifiers.command) != self.settings.drop_separate;
        if !dropped {
            if let Some(pointer) = pointer {
                self.drag_label(ui, pointer, &items, target, separate);
            }
            return;
        }
        self.view.drop = None;
        self.arrangement.drop = None;
        match (target, pointer) {
            _ if self.session().is_none() => self.status = Status::Hint("Open a project to add the files to".into()),
            (Some(target), _) => self.drop_items(&items, target, separate),
            (None, None) => self.add_to_selected(items),
            (None, Some(_)) => {
                self.status = Status::Hint("Drop samples and MIDI files on a slot, a lane or the empty area".into());
            }
        }
    }

    /// The target the views found under the pointer, or the detail area's track for devices.
    pub(super) fn drop_target(&self, items: &[BrowserItem], pointer: Option<Pos2>) -> Option<DropTarget> {
        if let Some(pointer) = pointer
            && items.len() == 1
            && items[0].kind == crate::interaction::drop::ItemKind::Sample
            && let Some((_, track, device)) = self.sample_drops.iter().find(|(area, _, _)| area.contains(pointer))
        {
            return Some(DropTarget::Sampler { track: *track, device: *device });
        }
        let target = match self.shown {
            View::Session => self.view.drop.as_ref().and_then(|p| p.target),
            View::Arrangement => self.arrangement.drop.as_ref().and_then(|p| p.target),
        };
        match (target, pointer, self.detail_drop) {
            (None, Some(pointer), Some((detail, track)))
                if detail.contains(pointer)
                    && items.iter().any(|i| i.kind.is_device() || i.kind == drop::ItemKind::Sample) =>
            {
                Some(DropTarget::Track(track))
            }
            _ => target,
        }
    }

    /// The label beside the pointer while dragging: what a drop there does, or what is dragged.
    pub(super) fn drag_label(
        &self,
        ui: &Ui,
        pointer: Pos2,
        items: &[BrowserItem],
        target: Option<DropTarget>,
        separate: bool,
    ) {
        let label = target.and_then(|t| self.plan_drop(items, t, separate)).map(|p| p.label);
        let layer = egui::LayerId::new(egui::Order::Tooltip, Id::new("browser-drag"));
        let painter = ui.ctx().layer_painter(layer);
        let text = label.clone().unwrap_or_else(|| match items {
            [one] => one.name.clone(),
            many => format!("{} items", many.len()),
        });
        let font = theme::ui_font(theme::TEXT_SMALL);
        let galley = widgets::fitted(&painter, &text, font, theme::text_primary(), 280.0);
        let rect = Rect::from_min_size(pointer + Vec2::new(14.0, 10.0), galley.size() + Vec2::new(PAD * 2.0, 6.0));
        painter.rect_filled(rect, 0.0, theme::control_active());
        painter.rect_stroke(
            rect,
            0.0,
            theme::hairline(if label.is_some() { theme::accent() } else { theme::border_strong() }),
            egui::StrokeKind::Inside,
        );
        painter.galley(rect.min + Vec2::new(PAD, 3.0), galley, theme::text_primary());
        ui.ctx().request_repaint();
    }

    /// Switches to `hovered`, the view whose name a dragged or carried clip is over, once it has
    /// stayed there for [`SWITCH_HOVER`]; a clip dragged in the Arrangement view is carried from
    /// then on.
    pub(super) fn hover_switch(&mut self, ctx: &egui::Context, hovered: Option<View>) {
        let Some(view) = hovered else {
            self.switch_hover = None;
            return;
        };
        let now = ctx.input(|i| i.time);
        let since = match self.switch_hover {
            Some((v, since)) if v == view => since,
            _ => {
                self.switch_hover = Some((view, now));
                now
            }
        };
        if now - since < SWITCH_HOVER {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(SWITCH_HOVER / 4.0));
            return;
        }
        self.switch_hover = None;
        if self.carry.is_none()
            && let Some((track, id)) = self.arrangement.dragged_clip()
        {
            let name = self
                .timeline
                .as_ref()
                .and_then(|(_, _, t)| t.lanes.iter().find(|l| l.track == track))
                .and_then(|l| l.clips.iter().find(|c| c.id == id))
                .map(|c| c.name.clone())
                .unwrap_or_default();
            self.arrangement.cancel_clip_drag();
            ctx.stop_dragging();
            self.carry = Some(Carry { from: CarryFrom::Arrangement { track, clip: id }, name });
        }
        self.shown = view;
        self.close_large_map();
    }

    /// While a clip is carried between the views: the label beside the pointer, Escape to stop,
    /// and on release a copy of it in the slot or on the lane under the pointer
    /// (`docs/spec/ui-session-view.md` › Between the views). A clip slot's drag starts one.
    pub(super) fn carry_clip(&mut self, ui: &Ui) {
        if let Some((track, scene)) = self.view.slot_drag.take() {
            let name = self
                .session()
                .and_then(|s| {
                    let p = s.project();
                    let column = p.launcher.columns.iter().find(|c| c.track == track)?;
                    let clip = column.slots.get(scene)?.clip.as_ref()?;
                    p.content(clip.content).map(|c| c.name.clone())
                })
                .unwrap_or_default();
            ui.ctx().stop_dragging();
            self.carry = Some(Carry { from: CarryFrom::Slot { track, scene }, name });
        }
        let carry = self.carry.clone();
        // Ctrl (Cmd on macOS) held at the drop: a copy, as the keymap's rule has it.
        let copy = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
        let Some(carry) = carry else { return };
        let (released, escape, pointer) =
            ui.input(|i| (!i.pointer.primary_down(), i.key_pressed(Key::Escape), i.pointer.latest_pos()));
        let target = match self.shown {
            View::Session => self.view.drop.as_ref().and_then(|p| p.target),
            View::Arrangement => self.arrangement.drop.as_ref().and_then(|p| p.target),
        };
        if escape || released {
            self.carry = None;
            self.switch_hover = None;
            self.view.drop = None;
            self.arrangement.drop = None;
            if released && !escape {
                match target {
                    Some(target) => self.drop_carried(&carry, target, copy),
                    None => self.status = Status::Hint("Drop a clip on a clip slot or a track's lane".into()),
                }
            }
            return;
        }
        let label = target.and_then(|t| self.carry_label(&carry, t, copy));
        if let Some(pointer) = pointer {
            let name = if carry.name.is_empty() { "clip" } else { carry.name.as_str() };
            drag_label(ui.ctx(), pointer, label.as_deref().unwrap_or(name), label.is_some());
        }
    }

    /// What the label beside a carried clip says over `target`, or `None` where it cannot land.
    /// `copy`: Ctrl is held — a slot-to-slot drag says Copy instead of its move or swap.
    pub(super) fn carry_label(&self, carry: &Carry, target: DropTarget, copy: bool) -> Option<String> {
        let project = self.session()?.project();
        let name = |t: TrackId| project.track(t).map_or_else(String::new, |x| x.name.clone());
        let clip = if carry.name.is_empty() { "clip" } else { carry.name.as_str() };
        match target {
            DropTarget::Slot { track, scene } => {
                if carry.from == (CarryFrom::Slot { track, scene }) {
                    return None;
                }
                // A slot-to-slot drag moves the clip, swapping with a target's clip — Ctrl
                // copies instead, as the cross-view drags do. The occupancy decides Move or
                // Swap: an occupied slot's clip may have no name, and the swap is still a swap.
                let (target_occupied, target_clip) = project
                    .launcher
                    .columns
                    .iter()
                    .find(|c| c.track == track)
                    .and_then(|c| c.slots.get(scene))
                    .map(|s| {
                        (
                            s.clip.is_some(),
                            s.clip
                                .as_ref()
                                .and_then(|placed| project.content(placed.content))
                                .map(|c| c.name.clone())
                                .unwrap_or_default(),
                        )
                    })
                    .unwrap_or((false, String::new()));
                match (copy, target_occupied) {
                    (false, false) => Some(format!("Move {clip} to {}, scene {}", name(track), scene + 1)),
                    (false, true) => {
                        let theirs = if target_clip.is_empty() { "clip" } else { target_clip.as_str() };
                        Some(format!("Swap {clip} and {theirs} in {}, scene {}", name(track), scene + 1))
                    }
                    (true, _) => Some(format!("Copy {clip} to {}, scene {}", name(track), scene + 1)),
                }
            }
            DropTarget::Lane { track, beat } => {
                let bar = (beat / self.bar_beats()).floor() as u64 + 1;
                Some(format!("Copy {clip} to {}, bar {bar}", name(track)))
            }
            _ => None,
        }
    }

    /// The carried clip's display name: "clip" when it has none.
    pub(super) fn carry_name(&self, carry: &Carry) -> String {
        if carry.name.is_empty() { "clip".into() } else { carry.name.clone() }
    }

    /// A track's name, or its id when the project has none by that id.
    pub(super) fn track_name(&self, track: TrackId) -> String {
        self.session()
            .and_then(|s| s.project().track(track).map(|t| t.name.clone()))
            .unwrap_or_else(|| track.0.to_string())
    }

    /// Beats in a bar of the shown project, 4 when it has no usable time signature.
    pub(super) fn bar_beats(&self) -> f64 {
        self.session().map_or(4.0, |s| {
            let ts = s.project().transport.time_signature;
            if ts.denominator == 0 { 4.0 } else { f64::from(ts.numerator.max(1)) * 4.0 / f64::from(ts.denominator) }
        })
    }

    /// Puts a copy of the carried clip on `target`: `clip.copy` from where it came, then
    /// `clip.paste` there, the same commands as Ctrl+C and Ctrl+V; the paste is one undo step.
    pub(super) fn drop_carried(&mut self, carry: &Carry, target: DropTarget, copy: bool) {
        // A slot-to-slot drop moves the clip: one undo step, an occupied target's clip swaps
        // into the source's slot (nothing lost). Ctrl: a copy, as the cross-view drags are.
        if let (CarryFrom::Slot { track, scene }, DropTarget::Slot { track: to_track, scene: to_scene }) =
            (&carry.from, &target)
        {
            if track == to_track && scene == to_scene {
                return;
            }
            if !copy {
                let Ok(session) = self.projects.session_mut(None) else { return };
                match session.execute(
                    "clip.moveSlot",
                    &json!({
                        "track": track.0,
                        "scene": scene + 1,
                        "toTrack": to_track.0,
                        "toScene": to_scene + 1,
                    }),
                ) {
                    Ok(out) => {
                        // The command ran under this call, not under `run`: the revision bump
                        // is explicit, so the views rebuild with the moved clip.
                        self.revision += 1;
                        let verb = if out["swapped"].as_bool().unwrap_or(false) { "Swapped" } else { "Moved" };
                        let label = format!(
                            "{verb} {} to {}, scene {}",
                            self.carry_name(carry),
                            self.track_name(*to_track),
                            to_scene + 1
                        );
                        self.status = Status::Done(label);
                    }
                    Err(e) => self.status = Status::Error(e.message().to_owned()),
                }
                return;
            }
        }
        let copy = match carry.from {
            CarryFrom::Arrangement { clip, .. } => json!({ "clips": [{ "clip": clip.0 }] }),
            CarryFrom::Slot { track, scene } => json!({ "track": track.0, "scene": scene + 1 }),
        };
        let mut paste = match target {
            DropTarget::Slot { track, scene } => {
                if carry.from == (CarryFrom::Slot { track, scene }) {
                    return;
                }
                json!({ "track": track.0, "scene": scene + 1 })
            }
            DropTarget::Lane { track, beat } => json!({ "track": track.0, "start": beat }),
            _ => return,
        };
        let Ok(session) = self.projects.session_mut(None) else { return };
        if let Err(e) = session.execute("clip.copy", &copy) {
            self.status = Status::Error(e.message().to_owned());
            return;
        }
        // On a lane the copy follows the stacking setting for its kind, as a moved clip does.
        let audio = session.copied_clip_is_audio();
        if paste.get("start").is_some() && audio.is_some_and(|audio| self.stacks(audio)) {
            paste["stack"] = json!(true);
        }
        self.status = Status::Hint(String::new());
        self.run(&Action::new("clip.paste", paste));
        if !matches!(self.status, Status::Error(_)) {
            let label = self.carry_label(carry, target, true).unwrap_or_default();
            self.status = Status::Done(label.replacen("Copy", "Copied", 1));
        }
    }

    pub(super) fn plan_drop(
        &self,
        items: &[BrowserItem],
        target: DropTarget,
        separate: bool,
    ) -> Option<drop::DropPlan> {
        let project = self.session()?.project();
        let name = |t: TrackId| project.track(t).map_or_else(|| t.0.to_string(), |x| x.name.clone());
        if let [sample] = items
            && sample.kind == drop::ItemKind::Sample
            && sample.path().is_some_and(|path| !suhina_media::stems::is_stems_path(&path))
            && let Some(track_id) = match target {
                DropTarget::Track(t) | DropTarget::Lane { track: t, .. } | DropTarget::Slot { track: t, .. } => Some(t),
                _ => None,
            }
            && let Some(track) = project.track(track_id)
            && track.content.notes
            && !track.content.audio
            && let Some(channel) = project.channels.iter().find(|c| Some(c.id) == track.channel)
            && channel.devices.is_empty()
        {
            let mut plan = drop::plan(
                items,
                DropTarget::Sampler { track: track_id, device: 0 },
                separate,
                self.settings.warp_mode.param(),
                false,
                &name,
            )?;
            plan.commands.insert(0, ("device.add", json!({"track":track_id.0,"id":"sampler"})));
            plan.label = format!("Add Sampler to {}", name(track_id));
            return Some(plan);
        }
        let stack = self.settings.sample_stacking || self.settings.midi_stacking;
        drop::plan(items, target, separate, self.settings.warp_mode.param(), stack, &name)
    }

    /// Runs a drop: decodes its samples on another thread first, so the window never waits,
    /// then runs its commands (ADR 0011).
    pub(super) fn drop_items(&mut self, items: &[BrowserItem], target: DropTarget, separate: bool) {
        let Some(plan) = self.plan_drop(items, target, separate) else {
            self.status = Status::Hint("Nothing to add there".into());
            return;
        };
        self.decode_then_run(plan.samples, plan.commands);
    }

    /// Relinks missing files (`project.relinkFiles`), their audio decoded on another thread first.
    pub(super) fn relink(&mut self, files: Vec<(String, PathBuf)>) {
        let names: Vec<String> = files.iter().map(|(f, _)| f.clone()).collect();
        let to: Vec<String> = files.iter().map(|(_, p)| p.display().to_string()).collect();
        let samples = files.into_iter().map(|(_, p)| p).collect();
        self.decode_then_run(samples, vec![("project.relinkFiles", json!({ "files": names, "to": to }))]);
    }

    /// Decodes `samples` on another thread, so the window never waits, then runs `commands`
    /// with them at hand (ADR 0011).
    pub(super) fn decode_then_run(&mut self, samples: Vec<PathBuf>, commands: Vec<(&'static str, serde_json::Value)>) {
        if samples.is_empty() {
            self.run_drop(commands);
            return;
        }
        let (send, decoded) = std::sync::mpsc::channel();
        let count = samples.len();
        let started = std::thread::Builder::new().name("suhina-drop".into()).spawn(move || {
            // A Stems file gives every audio track, each under its stream number.
            let done: Vec<_> = samples
                .into_iter()
                .flat_map(|path| {
                    let decoded: Vec<(Option<u16>, suhina_media::AudioData)> =
                        if suhina_media::stems::is_stems_path(&path) {
                            suhina_media::stems::decode_streams(&path)
                                .map(|all| all.into_iter().zip(0u16..).map(|(d, n)| (Some(n), d)).collect())
                                .unwrap_or_default()
                        } else {
                            suhina_media::decode_file(&path).map(|d| vec![(None, d)]).unwrap_or_default()
                        };
                    decoded.into_iter().map(move |(stream, data)| {
                        let data = std::sync::Arc::new(data);
                        let peaks = crate::kit::waveform::peaks(&data);
                        (path.clone(), stream, data, peaks)
                    })
                })
                .collect();
            let _ = send.send(done);
        });
        match started {
            Ok(_) => {
                self.status = Status::Hint(format!("Reading {count} sample{}", if count == 1 { "" } else { "s" }));
                self.pending_drop = Some(PendingDrop { project: self.projects.active(), commands, decoded });
            }
            Err(_) => self.status = Status::Error("could not start the sample decoder".into()),
        }
    }

    /// Picks up the samples of a drop once they are decoded and runs its commands.
    pub(super) fn poll_drop(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.pending_drop else { return };
        if pending.project != self.projects.active() {
            self.pending_drop = None;
            self.status = Status::Hint("Sample drop cancelled after switching projects".into());
            return;
        }
        match pending.decoded.try_recv() {
            Ok(decoded) => {
                let Some(pending) = self.pending_drop.take() else { return };
                if pending.commands.iter().any(|(id, params)| {
                    *id == "device.loadSample"
                        && !decoded.iter().any(|(path, _, data, _)| {
                            params["path"].as_str().is_some_and(|p| Path::new(p) == path)
                                && data.frames() >= 2
                                && (1..=32).contains(&data.channels)
                                && (1000..=384000).contains(&data.sample_rate)
                        })
                }) {
                    self.status = Status::Error("sample could not be decoded; the track was left unchanged".into());
                    return;
                }
                let mut waveforms = (*self.waveforms).clone();
                if let Ok(session) = self.projects.session_mut(None) {
                    for (path, stream, data, peaks) in decoded {
                        let key = std::path::absolute(&path).unwrap_or(path.clone()).display().to_string();
                        let key = match stream {
                            Some(n) => {
                                session.provide_stream(&path, n, data);
                                suhina_engine::model::stream_key(&key, n)
                            }
                            None => {
                                session.provide_audio(&path, data);
                                key
                            }
                        };
                        waveforms.insert(key, std::sync::Arc::new(peaks));
                    }
                }
                self.waveforms = std::sync::Arc::new(waveforms);
                self.run_drop(pending.commands);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(30))
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.pending_drop = None;
                self.status = Status::Error("reading the samples stopped".into());
            }
        }
    }

    /// Runs a drop's commands through the open projects, so what was added becomes recent.
    pub(super) fn run_drop(&mut self, commands: Vec<(&'static str, serde_json::Value)>) {
        for (id, params) in commands {
            match self.projects.execute(id, &params) {
                Ok(result) => {
                    self.revision += 1;
                    self.timeline = None;
                    // A Stems file arrives as one group: it opens folded, so the drop reads as a
                    // single lane until the group's triangle is clicked (`clip.addFiles`).
                    self.fold_new_groups(&result);
                    let skipped = result["skipped"].as_array().map_or(0, Vec::len);
                    let placed = result["clips"].as_array().map_or(0, Vec::len);
                    self.status = match (id, skipped) {
                        ("clip.addFiles", 0) => {
                            Status::Done(format!("Added {placed} clip{}", if placed == 1 { "" } else { "s" }))
                        }
                        ("clip.addFiles", n) => Status::Hint(format!(
                            "Added {placed} clips; {n} skipped: {}",
                            result["skipped"][0]["reason"].as_str().unwrap_or_default()
                        )),
                        _ => Status::Done("Added".into()),
                    };
                }
                Err(e) => {
                    self.status = Status::Error(e.message().to_owned());
                    break;
                }
            }
        }
    }

    /// How far each column's playing clip is through its loop — or through itself, without one —
    /// from the engine's monitor: the beats since the clip's launch, folded through its offset
    /// and loop, as the piano roll's playhead maps them.
    pub(super) fn progress(&self, grid: &TrackGrid) -> Vec<Option<f32>> {
        let Some(monitor) = self.session().and_then(|s| s.player()).map(|p| p.monitor()) else {
            return Vec::new();
        };
        let project = self.session().map(|s| s.project());
        grid.columns.iter().map(|column| Self::clip_progress(project, monitor, column)).collect()
    }

    /// How far the clip in `column`'s playing slot is through its loop — or through itself,
    /// without one — from the engine's beats since its launch, folded through its offset and loop.
    pub(super) fn clip_progress(
        project: Option<&suhina_engine::model::Project>,
        monitor: &suhina_engine::LauncherMonitor,
        column: &crate::interaction::grid::Column,
    ) -> Option<f32> {
        // The launcher column's index in the monitor, and the slot playing there.
        let index = column.launcher?;
        let scene = monitor.column(index)?.playing?;
        let clip = project?.launcher.columns.get(index)?.slots.get(scene)?.clip.as_ref()?;
        // A slot's clip plays from its launch: the elapsed beats through its offset and loop.
        let placement = piano_roll::Placement {
            start: 0.0,
            length: f64::INFINITY,
            offset: clip.offset,
            loop_range: clip.loop_range.map(|r| (r.start, r.end)),
        };
        let beat = placement.content_beat(monitor.elapsed_beats(index)?)?;
        // How far through the loop — or, without one, the clip — that is.
        let (from, span) = match clip.loop_range {
            Some(range) if range.end > range.start => (range.start, range.end - range.start),
            _ => (clip.offset, clip.length),
        };
        (span > 0.0).then(|| ((beat - from) / span).clamp(0.0, 1.0) as f32)
    }
}

/// Darkens the view while a project file is dragged over the window, so it is clear the window
/// takes the drop.
pub(super) fn drop_overlay(ui: &Ui, area: Rect) {
    let is = |f: &egui::HoveredFile, ext: &str| {
        f.path.as_ref().is_none_or(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)))
    };
    let (project, themes) = ui.input(|i| {
        let files = &i.raw.hovered_files;
        (
            files.iter().any(|f| is(f, "als") || f.path.as_deref().is_some_and(suhina_session::is_project_file)),
            files.iter().any(|f| is(f, "json") || is(f, "zip")),
        )
    });
    let shown = motion::toward(ui.ctx(), Id::new("drop-overlay"), project || themes, motion::HOVER);
    if shown <= 0.0 {
        return;
    }
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::window_background().gamma_multiply(DROP_DIM * shown));
    let text = theme::text_primary().gamma_multiply(shown);
    // A file whose name is not known yet counts as both; a project is the likelier.
    let what = if themes && !project { "Drop to add the themes" } else { "Drop to open" };
    painter.text(area.center(), Align2::CENTER_CENTER, what, theme::ui_font(theme::TEXT + 2.0), text);
}
