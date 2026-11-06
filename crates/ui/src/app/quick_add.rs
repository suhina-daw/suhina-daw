// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The quick-add palette (`docs/spec/browser.md` › Quick add): a shortcut opens a search field
//! in the middle of the window, and Enter adds what was found to the selected track, or as a
//! clip on a new track at the palette's place. Each choice is one of the commands the browser's
//! drops run, so agents and the CLI do the same.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;

use suhina_library::Item;
use suhina_plugins::scan::PluginFormat;
use suhina_session::{Category, Found, PreparedSearch};

use super::*;
use crate::kit::floating_list;

/// At most this many results are asked for, per kind, and listed.
const LIMIT: usize = 200;

/// The palette's width, in points.
const WIDTH: f32 = 460.0;

/// The rows that show at once, whatever the search finds; the rest scroll.
const VISIBLE_ROWS: usize = 12;

/// The kinds of item the buttons under the list narrow the search to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Instruments,
    Effects,
    /// Samples and MIDI files: what becomes a clip.
    Samples,
    Presets,
}

impl Kind {
    pub(super) const ALL: [Kind; 4] = [Self::Instruments, Self::Effects, Self::Samples, Self::Presets];

    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Instruments => "Instruments",
            Self::Effects => "Effects",
            Self::Samples => "Samples",
            Self::Presets => "Presets",
        }
    }

    /// The browser's categories it searches.
    fn categories(self) -> &'static [Category] {
        match self {
            Self::Instruments => &[Category::Instruments],
            Self::Effects => &[Category::Effects],
            Self::Samples => &[Category::Samples, Category::Midi],
            Self::Presets => &[Category::Presets],
        }
    }

    /// Whether it lists plug-ins, so the format buttons mean something.
    fn has_plugins(self) -> bool {
        matches!(self, Self::Instruments | Self::Effects)
    }
}

/// What the palette's buttons narrow the list to. Kept while the window is open, so the next
/// palette starts where the last one was left. Nothing chosen means everything.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Filter {
    pub(super) kinds: Vec<Kind>,
    pub(super) formats: Vec<PluginFormat>,
}

impl Filter {
    /// The categories to search, in the order their results are listed.
    fn categories(&self) -> Vec<Category> {
        if self.kinds.is_empty() {
            return vec![Category::All];
        }
        Kind::ALL.iter().filter(|k| self.kinds.contains(k)).flat_map(|k| k.categories().iter().copied()).collect()
    }

    /// Whether the format buttons show: with instruments or effects chosen.
    pub(super) fn shows_formats(&self) -> bool {
        self.kinds.iter().any(|k| k.has_plugins())
    }

    /// Whether `found` passes the format buttons: a plug-in of a chosen format, or anything that
    /// is not a plug-in. While the format buttons are hidden, every format passes.
    pub(super) fn keeps(&self, found: &Found) -> bool {
        match found.format {
            Some(format) if self.shows_formats() && !self.formats.is_empty() => self.formats.contains(&format),
            _ => true,
        }
    }
}

/// Flips `item` in `list`: in when it was out, out when it was in.
fn toggle<T: PartialEq>(list: &mut Vec<T>, item: T) {
    match list.iter().position(|x| *x == item) {
        Some(at) => {
            list.remove(at);
        }
        None => list.push(item),
    }
}

/// The palette while it is open.
pub(super) struct QuickAdd {
    /// What the field says.
    text: String,
    /// The lit row.
    picked: usize,
    /// The track a device goes on, and after which a new track goes; `None` for new tracks at
    /// the end.
    track: Option<TrackId>,
    /// Where a sample or MIDI file starts, in beats.
    beat: f64,
    /// What the search found, only what can be added here; it stays while the next search
    /// looks, so the list does not jump.
    found: Vec<Found>,
    /// The text and kinds the running or shown search is for.
    searched: Option<(String, Vec<Kind>)>,
    /// A search of a large library still looking, on a thread of its own.
    job: Option<(Arc<AtomicBool>, Receiver<Vec<Found>>)>,
    /// The field still has to take the keyboard.
    focus: bool,
}

/// Whether an item of `kind` (as `library.search` names it) is offered; `notes` is whether the
/// target track plays notes (`None` with no track selected: a new track).
pub(super) fn offered(kind: &str, notes: Option<bool>) -> bool {
    match kind {
        // An instrument goes on a track that plays notes, or makes a new one.
        "instrument" => notes != Some(false),
        // Effects go on the selected track only: a MIDI effect on one that plays notes.
        "midi_effect" => notes == Some(true),
        "effect" => notes.is_some(),
        // A preset is for a built-in device; adding it says so when the track cannot take it.
        "sample" | "midi" | "preset" => true,
        // Projects, device graphs and Stems files are added from the browser.
        _ => false,
    }
}

/// What a row says an item is.
pub(super) fn kind_label(found: &Found) -> &'static str {
    match (found.kind, found.plugin) {
        ("instrument", true) => "Plug-in instrument",
        (_, true) => "Plug-in effect",
        ("instrument", false) => "Instrument",
        ("effect", false) => "Effect",
        ("midi_effect", false) => "MIDI effect",
        ("sample", _) => "Sample",
        ("midi", _) => "MIDI file",
        ("preset", _) => "Preset",
        _ => "Item",
    }
}

/// A row of the list: the name over its kind, a plug-in's format at the right edge.
pub(super) fn row(found: &Found) -> floating_list::Row {
    let tag = found.format.map(|f| f.label().to_string());
    let label = match found.format {
        Some(format) => format!("Add {} ({})", found.name, format.label()),
        None => format!("Add {}", found.name),
    };
    floating_list::Row { text: format!("{}\n{}", found.name, kind_label(found)), tag, label }
}

/// The place a beat is at, as the header names it: `Bar 17`, or `Bar 17.3` between bars.
pub(super) fn place_label(beat: f64, bar_beats: f64) -> String {
    let bar_beats = if bar_beats > 0.0 { bar_beats } else { 4.0 };
    let bar = (beat / bar_beats).floor();
    let within = (beat - bar * bar_beats).floor();
    if within < 1.0 { format!("Bar {}", bar + 1.0) } else { format!("Bar {}.{}", bar + 1.0, within + 1.0) }
}

/// Why `found` cannot go on a track that plays no notes, or `None` when it can.
fn refused(found: &Found, notes: Option<bool>) -> Option<String> {
    if offered(found.kind, notes) {
        return None;
    }
    Some(match found.kind {
        "effect" | "midi_effect" if notes.is_none() => {
            format!("Select a track for {}: an effect goes on the selected track", found.name)
        }
        "instrument" => format!("{} is an instrument: it goes on a MIDI track, not on this one", found.name),
        "midi_effect" => format!("{} is a MIDI effect: it goes on a MIDI track, not on this one", found.name),
        _ => format!("{} cannot be added from here", found.name),
    })
}

/// Takes this frame's presses of `key` out of the input, so nothing else sees them.
fn take_key(ctx: &egui::Context, key: Key) -> bool {
    ctx.input_mut(|i| {
        let mut pressed = false;
        i.events.retain(|event| match event {
            egui::Event::Key { key: k, pressed: true, .. } if *k == key => {
                pressed = true;
                false
            }
            egui::Event::Key { key: k, pressed: false, .. } if *k == key => false,
            _ => true,
        });
        pressed
    })
}

/// Runs `searches` one after another and lists what they found in that order.
fn run_all(searches: Vec<PreparedSearch>, cancel: &AtomicBool) -> Vec<Found> {
    let mut found = Vec::new();
    for search in searches {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        found.extend(search.run(cancel));
    }
    found
}

/// A toggle button of the palette, named for tests and assistive tools as `Quick add: <title>`.
fn toggle_button(ui: &mut Ui, on: bool, title: &str) -> bool {
    let text = egui::RichText::new(title).size(theme::TEXT_SMALL);
    let response = ui.selectable_label(on, text);
    let label = format!("Quick add: {title}");
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, label.clone()));
    response.clicked()
}

impl App {
    /// What the palette's field says, or said when it was closed without adding: it is kept
    /// until something is added.
    pub fn quick_add_text(&self) -> &str {
        self.quick_add.as_ref().map_or(self.quick_add_kept.as_str(), |p| p.text.as_str())
    }

    /// Whether the quick-add palette is open.
    pub fn quick_add_open(&self) -> bool {
        self.quick_add.is_some()
    }

    /// The palette's target track and place in beats, while it is open.
    pub fn quick_add_target(&self) -> Option<(Option<TrackId>, f64)> {
        self.quick_add.as_ref().map(|p| (p.track, p.beat))
    }

    /// Opens the palette for the selected track and the place: the beat under the pointer over
    /// the Arrangement, the play start marker elsewhere.
    pub(super) fn open_quick_add(&mut self) {
        let Some(session) = self.session() else { return };
        let project = session.project();
        let track = self.selected_track().filter(|t| *t != TrackId::MAIN && project.track(*t).is_some());
        let pointer = if self.shown == View::Arrangement { self.arrangement.pointer_beat } else { None };
        let beat = pointer.unwrap_or_else(|| session.player().map_or(0.0, |p| p.monitor().start_beat())).max(0.0);
        self.quick_add = Some(QuickAdd {
            // What was typed before it was closed without adding (to select a track, say).
            text: self.quick_add_kept.clone(),
            picked: 0,
            track,
            beat,
            found: Vec::new(),
            searched: None,
            job: None,
            focus: true,
        });
    }

    /// Whether the palette's track plays notes; `None` without a track.
    fn quick_add_notes(&self, track: Option<TrackId>) -> Option<bool> {
        let track = track?;
        let project = self.session()?.project();
        Some(project.track(track)?.content.notes)
    }

    /// Searches again when the field or the kinds changed, and picks up the answer of a search
    /// still looking. What the last search found stays listed meanwhile.
    fn quick_add_search(&mut self, ctx: &egui::Context) {
        let Some(palette) = self.quick_add.as_mut() else { return };
        let wanted = (palette.text.clone(), self.quick_add_filter.kinds.clone());
        if palette.searched.as_ref() != Some(&wanted) {
            palette.searched = Some(wanted);
            if let Some((cancel, _)) = palette.job.take() {
                cancel.store(true, Ordering::Relaxed);
            }
            let text = palette.text.clone();
            let mut searches = Vec::new();
            for category in self.quick_add_filter.categories() {
                match suhina_session::prepare_search(&mut self.projects, &text, category, None, &[], LIMIT) {
                    Ok(search) => searches.push(search),
                    Err(e) => self.status = Status::Error(e.message().to_owned()),
                }
            }
            let files: usize = searches.iter().map(PreparedSearch::files).sum();
            if files > crate::browser::SEARCH_ON_THE_WINDOW_THREAD {
                let cancel = Arc::new(AtomicBool::new(false));
                let (send, answer) = std::sync::mpsc::channel();
                let looking = Arc::clone(&cancel);
                let started = std::thread::Builder::new().name("quick-add search".into()).spawn(move || {
                    let found = run_all(searches, &looking);
                    if !looking.load(Ordering::Relaxed) {
                        let _ = send.send(found);
                    }
                });
                if let Some(palette) = self.quick_add.as_mut() {
                    match started {
                        Ok(_) => palette.job = Some((cancel, answer)),
                        // The next frame tries again.
                        Err(_) => palette.searched = None,
                    }
                }
            } else {
                let found = run_all(searches, &AtomicBool::new(false));
                self.quick_add_results(found);
            }
        }
        let answer = self.quick_add.as_ref().and_then(|p| p.job.as_ref()).and_then(|(_, a)| a.try_recv().ok());
        match answer {
            Some(found) => {
                if let Some(palette) = self.quick_add.as_mut() {
                    palette.job = None;
                }
                self.quick_add_results(found);
            }
            None if self.quick_add.as_ref().is_some_and(|p| p.job.is_some()) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(30));
            }
            None => {}
        }
    }

    /// Lists what can be added to the palette's track, the lit row back at the top.
    fn quick_add_results(&mut self, found: Vec<Found>) {
        let notes = self.quick_add.as_ref().and_then(|p| self.quick_add_notes(p.track));
        if let Some(palette) = self.quick_add.as_mut() {
            palette.found = found.into_iter().filter(|f| offered(f.kind, notes)).collect();
            palette.picked = 0;
        }
    }

    /// The palette, while it is open: the header naming where things go, the field, the format
    /// buttons for plug-ins, what it found, and the kind buttons. Up and Down move, Enter or a
    /// click adds and closes, Escape or a click outside closes.
    pub(super) fn quick_add_ui(&mut self, ctx: &egui::Context) {
        if self.quick_add.is_none() {
            return;
        }
        if self.session().is_none() {
            self.quick_add = None;
            return;
        }
        if take_key(ctx, Key::Escape) {
            self.close_quick_add();
            return;
        }
        let enter = take_key(ctx, Key::Enter);
        let (down, up) = floating_list::take_arrow_moves(ctx);
        self.quick_add_search(ctx);
        let header = {
            let Some(palette) = self.quick_add.as_ref() else { return };
            let target = match palette.track {
                Some(track) => format!("Track: {}", self.track_name(track)),
                None => "New track".to_string(),
            };
            let place = place_label(palette.beat, self.bar_beats());
            if palette.track.is_none() {
                format!("{target} · {place} · select a track for effects")
            } else {
                format!("{target} · {place}")
            }
        };
        let mut filter = self.quick_add_filter.clone();
        let Some(palette) = self.quick_add.as_mut() else { return };
        // Only what the format buttons keep; the lit row counts among those.
        let listed: Vec<usize> = (0..palette.found.len()).filter(|&i| filter.keeps(&palette.found[i])).collect();
        palette.picked = floating_list::step(palette.picked, listed.len(), down, up);
        let rows: Vec<floating_list::Row> = listed.iter().map(|&i| row(&palette.found[i])).collect();
        let searching = palette.job.is_some();
        let field_id = egui::Id::unique("quick-add-field");
        if palette.focus {
            ctx.memory_mut(|m| m.request_focus(field_id));
            palette.focus = false;
        }
        let text = &mut palette.text;
        let formats = &mut filter.formats;
        let shows_formats = filter.kinds.iter().any(|k| k.has_plugins());
        let heading = |ui: &mut Ui| {
            // "Searching…" sits at the header's right end, so nothing below it moves.
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&header).color(theme::text_hint()).size(theme::TEXT_SMALL));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let note = if searching { "Searching…" } else { "" };
                    ui.label(egui::RichText::new(note).color(theme::text_hint()).size(theme::TEXT_SMALL));
                });
            });
            let field = egui::TextEdit::singleline(text)
                .id(field_id)
                .hint_text("Search devices, plug-ins, samples and presets")
                .desired_width(f32::INFINITY);
            ui.add(field).widget_info(|| {
                egui::WidgetInfo::text_edit(true, "", "Quick add", "Search devices, plug-ins, samples and presets")
            });
            if shows_formats {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Formats").color(theme::text_hint()).size(theme::TEXT_SMALL));
                    for format in PluginFormat::ALL {
                        if toggle_button(ui, formats.contains(&format), format.label()) {
                            toggle(formats, format);
                        }
                    }
                });
            }
        };
        let kinds = &mut filter.kinds;
        let footer = |ui: &mut Ui| {
            ui.separator();
            ui.horizontal(|ui| {
                for kind in Kind::ALL {
                    if toggle_button(ui, kinds.contains(&kind), kind.title()) {
                        toggle(kinds, kind);
                    }
                }
            });
        };
        let look =
            floating_list::Look { width: WIDTH, row_height: 34.0, visible_rows: VISIBLE_ROWS, fixed_height: true };
        let picked = (!rows.is_empty()).then_some(palette.picked);
        let shown = floating_list::show(
            ctx,
            egui::Id::unique("quick-add"),
            look,
            heading,
            footer,
            &rows,
            picked,
            down + up > 0,
        );
        // A button changed what is listed: the lit row goes back to the top.
        if filter != self.quick_add_filter {
            palette.picked = 0;
            self.quick_add_filter = filter;
        }
        // Keep the keyboard in the field while it is open.
        ctx.memory_mut(|m| m.request_focus(field_id));
        let clicked_outside =
            ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !shown.rect.contains(p)));
        let chosen = shown.clicked.or_else(|| enter.then_some(palette.picked).filter(|_| !rows.is_empty()));
        if let Some(place) = chosen.and_then(|place| listed.get(place).copied()) {
            let Some(palette) = self.quick_add.take() else { return };
            // Adding is what clears the field for next time; a choice that cannot be added here
            // keeps it, so the search is there once a track is selected.
            self.quick_add_kept.clear();
            if let Some(found) = palette.found.get(place).cloned() {
                if refused(&found, self.quick_add_notes(palette.track)).is_some() {
                    self.quick_add_kept = palette.text;
                }
                self.quick_add_choose(&found, palette.track, palette.beat);
            }
        } else if clicked_outside {
            self.close_quick_add();
        }
    }

    /// Closes the palette without adding, keeping what its field says for the next one.
    fn close_quick_add(&mut self) {
        if let Some(palette) = self.quick_add.take() {
            self.quick_add_kept = palette.text;
        }
    }

    /// Adds `found`: a device or a preset to `track` (or on a new track that becomes the
    /// selected one), a sample or MIDI file as a clip on `track` starting at `beat` (on a new
    /// track after it when it cannot hold the file, or with no track).
    fn quick_add_choose(&mut self, found: &Found, track: Option<TrackId>, beat: f64) {
        let track = track.filter(|t| self.session().is_some_and(|s| s.project().track(*t).is_some()));
        let notes = self.quick_add_notes(track);
        if let Some(why) = refused(found, notes) {
            self.status = Status::Hint(why);
            return;
        }
        // A device: what `device.*` takes on a track and `track.add` on a new one.
        let device = match (&found.item, found.kind) {
            (Item::Plugin(spec), _) => Some(("device.addPlugin", json!({ "plugin": spec }), json!({ "device": spec }))),
            (Item::Device(id), _) => Some(("device.add", json!({ "id": id }), json!({ "device": id }))),
            (Item::Voice(path, voice), _) => {
                let preset = json!({ "preset": path.display().to_string(), "voice": voice });
                Some(("device.add", preset.clone(), preset))
            }
            (Item::Stock(key), _) => Some(("device.add", json!({ "stock": key }), json!({ "stock": key }))),
            (Item::File(path), "preset") => {
                let preset = json!({ "preset": path.display().to_string() });
                Some(("device.add", preset.clone(), preset))
            }
            (Item::File(_), _) => None,
        };
        if let Some((id, mut params, new_track)) = device {
            match track {
                Some(track) => {
                    params["track"] = json!(track.0);
                    if self.quick_add_run(id, &params).is_some() {
                        self.show_devices_of(track);
                        self.status = Status::Done(format!("Added {} to {}", found.name, self.track_name(track)));
                    }
                }
                None => {
                    // A new track with the device, of the kind it needs, as one undo step.
                    let added = self.quick_add_run("track.add", &new_track);
                    let new = added
                        .as_ref()
                        .and_then(|r| r["track"].as_u64())
                        .and_then(|t| u32::try_from(t).ok())
                        .map(TrackId);
                    if let Some(new) = new {
                        self.show_devices_of(new);
                        self.status = Status::Done(format!("Added {} on a new track", found.name));
                    }
                }
            }
            return;
        }
        let Item::File(path) = &found.item else { return };
        let mut params = json!({
            "paths": [path.display().to_string()],
            "start": beat,
            "warp": self.settings.warp_mode.param(),
            "stack": self.settings.sample_stacking || self.settings.midi_stacking,
        });
        let holds_clips = track
            .and_then(|t| self.session().and_then(|s| s.project().track(t).map(|x| x.content)))
            .is_some_and(|c| c.audio || c.notes);
        // Onto the selected track, as a drop on its lane: a file it cannot hold goes on a new track
        // right after it (`clip.addFiles` decides, and an empty track takes the file's kind).
        if let Some(track) = track.filter(|_| holds_clips) {
            params["track"] = json!(track.0);
        }
        let samples = if found.kind == "sample" { vec![path.clone()] } else { Vec::new() };
        self.decode_then_run(samples, vec![("clip.addFiles", params)]);
    }

    /// Runs one of the palette's commands through the open projects, so what was added becomes
    /// recent, as a drop from the browser does.
    fn quick_add_run(&mut self, id: &str, params: &Value) -> Option<Value> {
        suhina_plugins::trace!("action {id} {params}");
        match self.projects.execute(id, params) {
            Ok(result) => {
                self.revision += 1;
                self.timeline = None;
                Some(result)
            }
            Err(e) => {
                self.status = Status::Error(e.message().to_owned());
                None
            }
        }
    }

    /// Waits for the samples of a drop (or a palette's sample) being read and runs its commands:
    /// tests wait on this signal rather than on time.
    pub fn finish_drop(&mut self) {
        let Some(pending) = self.pending_drop.as_mut() else { return };
        if let Ok(done) = pending.decoded.recv() {
            let (send, decoded) = std::sync::mpsc::channel();
            let _ = send.send(done);
            pending.decoded = decoded;
        }
        self.poll_drop(&egui::Context::default());
    }
}

#[cfg(test)]
#[path = "quick_add_tests.rs"]
mod tests;
