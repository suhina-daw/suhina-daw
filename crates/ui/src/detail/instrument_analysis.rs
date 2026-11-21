// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Native host controls for sample preparation. They dispatch commands and never alter audio
//! parameters directly. The app replaces the snapshot every frame, including project changes.

use egui::{Align, Color32, FontId, Id, Layout, Pos2, Rect, RichText, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::TrackId;
use suhina_session::{
    AnalysisKind, AnalysisPhase, AnalysisView, FolderFileOutcome, ModalFitOutcome, ProjectId, SampleAnalysisOutcome,
    Session,
};

use super::device_canvas::remember_keyboard_owner;
use crate::interaction::device_at::DeviceAt;
use crate::interaction::key_name::{key_name, parse_key_name, pitch_class_name};
use crate::kit::theme;
use crate::kit::widgets::{Action, caps_button, number_input, spaced_galley};

/// A fitted model holds at most this many recordings.
const MAX_RECORDINGS: usize = 32;

#[derive(Clone, Default)]
struct Snapshot {
    project: Option<ProjectId>,
    target: Option<(TrackId, DeviceAt)>,
    view: Option<AnalysisView>,
}

#[derive(Clone)]
struct Correction {
    job: String,
    root: u8,
}

fn snapshot_id() -> Id {
    Id::unique("instrument-analysis-snapshot")
}
fn correction_id() -> Id {
    Id::unique("instrument-analysis-root-correction")
}

pub(crate) fn publish(ctx: &egui::Context, project: Option<ProjectId>, session: Option<&mut Session>) {
    let mut snapshot = Snapshot { project, ..Snapshot::default() };
    if let Some(session) = session {
        snapshot.view = session.instrument_analysis_view();
        if let Some(view) = &snapshot.view {
            snapshot.target =
                session.project().tracks.iter().map(|track| track.id).chain(std::iter::once(TrackId::MAIN)).find_map(
                    |track| {
                        let channel = session.project().channel_of_track(track)?;
                        suhina_engine::model::walk_devices(&channel.devices)
                            .into_iter()
                            .find(|(_, device)| device.id == view.device)
                            .map(|(path, _)| (track, DeviceAt(path)))
                    },
                );
        }
    }
    ctx.data_mut(|data| {
        let previous = data.get_temp::<Snapshot>(snapshot_id());
        if previous.as_ref().is_some_and(|old| old.project != snapshot.project) {
            data.remove::<Correction>(correction_id());
        }
        data.insert_temp(snapshot_id(), snapshot);
    });
}

/// How the analysis is drawn over a canvas's own artwork: the canvas's ink and accent, and how
/// much the canvas is scaled.
#[derive(Clone, Copy)]
pub(crate) struct Look {
    pub ink: Color32,
    pub accent: Color32,
    pub scale: f32,
    /// The canvas's own background under the bounds (its texture and texture coordinates), which
    /// a result that covers the controls under it is drawn on.
    pub ground: Option<(egui::TextureId, Rect)>,
}

impl Look {
    fn muted(&self) -> Color32 {
        self.ink.gamma_multiply(0.78)
    }
    fn font(&self, size: f32) -> FontId {
        theme::ui_font(size * self.scale)
    }
    fn light(&self, ctx: &egui::Context, size: f32) -> FontId {
        theme::ui_font_weight(ctx, size * self.scale, theme::UI_LIGHT)
    }
    fn bold(&self, ctx: &egui::Context, size: f32) -> FontId {
        theme::ui_font_weight(ctx, size * self.scale, theme::UI_BOLD)
    }
}

/// The card a fit's result stands on while it covers the controls under it.
const CARD: Color32 = Color32::from_rgba_premultiplied(236, 225, 207, 246);
/// Where the analysis's own block starts in its bounds, and how wide it is, in canvas units.
const BLOCK_LEFT: f32 = 596.0;
const BLOCK_WIDTH: f32 = 164.0;

/// A text button in spaced capitals in the canvas's ink; `primary` underlines it in the accent.
fn button(ui: &mut Ui, look: &Look, label: &str, text: &str, primary: bool, enabled: bool) -> egui::Response {
    let font = look.bold(ui.ctx(), 8.5);
    let response =
        caps_button(ui, label, text, font, 2.0 * look.scale, look.ink, primary.then_some(look.accent), enabled);
    remember_keyboard_owner(&response);
    response
}

/// A caption in spaced capitals at the left of `rect`, centered on it.
fn caption(ui: &Ui, look: &Look, rect: Rect, text: &str) {
    let galley = spaced_galley(ui, &text.to_uppercase(), look.bold(ui.ctx(), 8.5), 2.0 * look.scale, look.muted());
    let pos = Pos2::new(rect.left(), rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(pos, galley, look.muted());
}

/// One line of text in `rect`, cut with an ellipsis where it is too long, `hover` on it.
fn line_in(ui: &mut Ui, look: &Look, rect: Rect, text: &str, size: f32, hover: Option<&str>) {
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)), |ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
        let response = ui.label(RichText::new(text).font(look.font(size)).color(look.muted()));
        if let Some(hover) = hover {
            response.on_hover_text(hover);
        }
    });
}

/// Text in `rect` that wraps onto as many lines as it needs, `hover` on it.
fn wrapped_in(ui: &mut Ui, look: &Look, rect: Rect, text: &str, hover: Option<&str>) {
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min)), |ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        ui.spacing_mut().item_spacing.y = 2.0 * look.scale;
        let response = ui.label(RichText::new(text).font(look.font(10.5)).color(look.muted()));
        if let Some(hover) = hover {
            response.on_hover_text(hover);
        }
    });
}

/// A row of controls in `rect`, from its left or, with `right`, from its right.
fn row<R>(ui: &mut Ui, look: &Look, rect: Rect, right: bool, add: impl FnOnce(&mut Ui) -> R) -> R {
    let layout = if right { Layout::right_to_left(Align::Center) } else { Layout::left_to_right(Align::Center) };
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect).layout(layout), |ui| {
        ui.spacing_mut().item_spacing.x = 12.0 * look.scale;
        add(ui)
    })
    .inner
}

/// The loaded sample's file name in spaced capitals and its facts under it (sample rate,
/// channels, length), or that none is loaded, from the sample the app publishes under `source`.
pub(crate) fn sample_file(ui: &mut Ui, bounds: Rect, look: Look, source: Id) {
    let name = ui.ctx().data(|data| data.get_temp::<String>(source.with("name"))).unwrap_or_default();
    let data = ui.ctx().data(|data| data.get_temp::<std::sync::Arc<suhina_engine::AudioData>>(source));
    let (title, facts) = if name.is_empty() {
        ("No sample".to_owned(), "Drop an audio file on the waveform".to_owned())
    } else {
        let facts = data
            .filter(|audio| audio.sample_rate > 0)
            .map(|audio| {
                let channels = usize::from(audio.channels.max(1));
                let seconds = (audio.samples.len() / channels) as f64 / f64::from(audio.sample_rate);
                let rate = if audio.sample_rate % 1000 == 0 {
                    format!("{} kHz", audio.sample_rate / 1000)
                } else {
                    format!("{:.1} kHz", f64::from(audio.sample_rate) / 1000.0)
                };
                let layout = match channels {
                    1 => "mono".to_owned(),
                    2 => "stereo".to_owned(),
                    n => format!("{n} channels"),
                };
                format!("{rate}  ·  {layout}  ·  {seconds:.2} s")
            })
            .unwrap_or_default();
        (name, facts)
    };
    let painter = ui.painter_at(bounds);
    let s = look.scale;
    let galley = spaced_galley(ui, &title.to_uppercase(), look.bold(ui.ctx(), 9.5), 2.4 * s, look.ink);
    painter.galley(bounds.min, galley, look.ink);
    let facts = painter.layout_no_wrap(facts, look.font(11.0), look.muted());
    painter.galley(bounds.min + Vec2::new(0.0, 19.0 * s), facts, look.muted());
}

/// Draw the reserved SDK sample-analysis region: at its right the root the analysis suggests,
/// with Accept and Again, and fitting a model from the sample; a fitted model's result, while it
/// waits for acceptance, over the whole region. Parameters remain in their usual SDK controls.
pub(crate) fn show(
    ui: &mut Ui,
    bounds: Rect,
    look: Look,
    (track, device): (TrackId, usize),
    at: &DeviceAt,
    actions: &mut Vec<Action>,
) {
    let snapshot = ui.ctx().data(|data| data.get_temp::<Snapshot>(snapshot_id())).unwrap_or_default();
    let view =
        snapshot.view.as_ref().filter(|_| snapshot.target.as_ref().is_some_and(|(t, path)| *t == track && path == at));
    let track_value = if track == TrackId::MAIN { json!("main") } else { json!(track.0) };
    let device_value = view.map_or_else(|| at.value(), |v| json!({"id":v.device.0}));
    let has_sample = ui
        .ctx()
        .data(|data| data.get_temp::<String>(super::device_canvas::sample_source_id(track, device).with("name")))
        .is_none_or(|name| !name.is_empty());
    let s = look.scale;
    let place =
        |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(bounds.min + Vec2::new(x, y) * s, Vec2::new(w, h) * s);
    ui.scope_builder(egui::UiBuilder::new().max_rect(bounds).id_salt(("instrument-analysis", track.0, &at.0)), |ui| {
        // A little room past the bounds' edge, so a word's last letter is never cut.
        ui.set_clip_rect(ui.clip_rect().intersect(bounds.expand2(Vec2::new(6.0 * s, 0.0))));
        ui.visuals_mut().override_text_color = Some(look.ink);
        let running = view.is_some_and(|v| v.phase == AnalysisPhase::Running);
        let wide = view.filter(|v| v.phase == AnalysisPhase::Ready && (v.folder.is_some() || v.modal.is_some()));
        if let Some(view) = wide {
            fit_result(ui, &look, bounds, place, view, (&track_value, &device_value), actions);
            return;
        }
        let block = |y: f32, h: f32| place(BLOCK_LEFT, y, BLOCK_WIDTH, h);
        let header = block(4.0, 16.0);
        let caption_text = match view.map(|v| (v.kind, v.phase)) {
            Some((AnalysisKind::Root, AnalysisPhase::Running)) => "Analyzing",
            Some((_, AnalysisPhase::Running)) => "Fitting",
            Some((AnalysisKind::Modal | AnalysisKind::Folder, AnalysisPhase::Accepted)) => "Model fit",
            _ => "Suggested",
        };
        caption(ui, &look, header, caption_text);
        row(ui, &look, header, true, |ui| {
            if let Some(view) = view.filter(|_| running) {
                if button(ui, &look, "Cancel analysis", "Cancel", false, true).clicked() {
                    actions.push(Action::new("instrument.cancelAnalysis",json!({"track":track_value,"device":device_value,"job":view.job})));
                }
            } else {
                let again = view.is_some_and(|v| v.report.is_some() || v.accepted_root.is_some());
                if button(ui, &look, "Analyze sample", if again { "Again" } else { "Analyze" }, false, has_sample).clicked() {
                    actions.push(Action::new("instrument.analyzeSample",json!({"track":track_value,"device":device_value})));
                }
            }
        });
        let fit_buttons = |ui: &mut Ui, actions: &mut Vec<Action>| {
            row(ui, &look, block(74.0, 16.0), false, |ui| {
                if button(ui, &look, "Fit resonances", "Fit model", false, has_sample && !running).clicked() {
                    actions.push(Action::new("instrument.fitSample",json!({"track":track_value,"device":device_value})));
                }
                // The app asks for the folder, then runs instrument.fitFolder on this Instrument.
                if button(ui, &look, "Fit folder…", "Fit folder…", false, !running).clicked() {
                    actions.push(Action::new("ui.instrumentFitFolder",json!({"track":track_value,"device":device_value})));
                }
            });
        };
        let Some(view) = view else {
            wrapped_in(
                ui,
                &look,
                block(24.0, 44.0),
                "Analyze a root, fit one decaying strike, or fit a folder of notes.",
                Some("Fitting is experimental. Inspect the result before accepting; the source stays playable."),
            );
            fit_buttons(ui, actions);
            return;
        };
        match view.phase {
            AnalysisPhase::Running => {
                let fraction = if view.total > 0 { (view.completed as f32 / view.total as f32).clamp(0.0,1.0) } else { 0.0 };
                let text = match view.kind {
                    AnalysisKind::Modal => "Fitting resonances locally…".to_owned(),
                    AnalysisKind::Folder => format!("Fitting {} of {} locally…", (view.completed + 1).min(view.total.max(1)), view.total.max(1)),
                    AnalysisKind::Root => "Analyzing locally…".to_owned(),
                };
                let track_rect = block(34.0, 2.0);
                let painter = ui.painter();
                painter.rect_filled(track_rect, 1.0, look.ink.gamma_multiply(0.18));
                painter.rect_filled(
                    Rect::from_min_size(track_rect.min, Vec2::new(track_rect.width() * fraction, track_rect.height())),
                    1.0,
                    look.accent,
                );
                line_in(ui, &look, block(44.0, 16.0), &text, 10.5, None);
                ui.ctx().request_repaint();
            }
            AnalysisPhase::Ready => {
                let Some(report) = &view.report else { return; };
                // One stable note, or else the tonal center of a loop or phrase.
                let proposed = report.proposal.map(|proposal| (
                    proposal.root,
                    format!("{:+.0} cents  ·  {:.0} Hz", proposal.cents, proposal.hz),
                    format!("Measured {:.2} Hz ({} {:+.1} cents). Periodicity {:.3}; spread {:.1} cents; voiced {} / {} active windows. Analyzed frames {}..{}. These measurements are not a correctness probability.", proposal.hz, key_name(proposal.root), proposal.cents, proposal.periodicity, proposal.spread_cents, report.voiced_windows, report.active_windows, report.range.start, report.range.end),
                    "Accept changes the root key in one undo step. The cents offset remains in the source. For isolated notes: chords can produce an ambiguous root. Listen before accepting.",
                )).or_else(|| report.key.map(|key| {
                    let name = |k: &suhina_session::SampleKey| format!("{} {}", pitch_class_name(k.tonic), if k.mode == suhina_session::SampleKeyMode::Major { "major" } else { "minor" });
                    // Relative keys share their notes; a close runner-up is shown, not hidden.
                    let alternative = if key.runner_up.correlation >= key.key.correlation - 0.1 { format!(" · or {}", name(&key.runner_up)) } else { String::new() };
                    (
                        key.root,
                        format!("{}{}", name(&key.key), alternative),
                        format!("Key {}{} → root {}, tuning {:+.1} cents. Pitch-class profile of {} windows compared with major and minor key profiles: {} {:.2}, {} {:.2}. A correlation is a fit, not a probability. Analyzed frames {}..{}.", name(&key.key), alternative, key_name(key.root), key.tuning_cents, key.windows, name(&key.key), key.key.correlation, name(&key.runner_up), key.runner_up.correlation, report.range.start, report.range.end),
                        "Accept sets the root to the key's tonic in one undo step; playing other keys transposes the loop. A short loop's key can be ambiguous. Listen, and correct the root if its home note differs.",
                    )
                }));
                let Some((root, short, measured, note)) = proposed else {
                    wrapped_in(ui, &look, block(24.0, 44.0), match report.outcome {
                        SampleAnalysisOutcome::Silent => "The analyzed region is silent or constant. Choose a sounding region.",
                        SampleAnalysisOutcome::TooShort => "The analyzed region is too short for a stable root. Use a longer isolated note.",
                        SampleAnalysisOutcome::NoStablePitch | SampleAnalysisOutcome::Stable | SampleAnalysisOutcome::TonalCenter => "No stable note or clear key was found. Keep the sample playable and set Root by ear.",
                    }, None);
                    fit_buttons(ui, actions);
                    return;
                };
                let mut correction = ui.ctx().data(|data| data.get_temp::<Correction>(correction_id()))
                    .filter(|c| c.job == view.job)
                    .unwrap_or(Correction { job:view.job.clone(),root });
                let glide = report.glide.map(|glide| format!(" Glides in from {} over {:.2} s.", key_name(glide.from_midi.round().clamp(0.0, 127.0) as u8), glide.seconds)).unwrap_or_default();
                let hover = format!("{measured}{glide}\n{note}");
                let proposal_row = block(22.0, 30.0);
                row(ui, &look, proposal_row, false, |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0 * s;
                    let step = |ui: &mut Ui, label: &str, text: &str| {
                        let response = caps_button(ui, label, text, look.light(ui.ctx(), 20.0), 0.0, look.accent, None, true);
                        remember_keyboard_owner(&response);
                        response
                    };
                    if step(ui, "Lower the suggested root", "‹").clicked() {
                        correction.root = correction.root.saturating_sub(1);
                    }
                    let response = ui.scope(|ui| {
                        ui.style_mut().override_font_id = Some(look.light(ui.ctx(), 22.0));
                        number_input(ui, None, egui::DragValue::new(&mut correction.root).range(0..=127).speed(0.1)
                            .custom_formatter(|key, _| key_name(key.round().clamp(0.0, 127.0) as u8))
                            .custom_parser(|text| parse_key_name(text).map(f64::from)))
                    }).inner;
                    remember_keyboard_owner(&response);
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::SpinButton,true,"Proposed root key"));
                    if step(ui, "Raise the suggested root", "›").clicked() {
                        correction.root = correction.root.saturating_add(1).min(127);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if button(ui, &look, "Accept root", "Accept", true, true).on_hover_text(note).clicked() {
                            actions.push(Action::new("instrument.acceptAnalysis",json!({"track":track_value,"device":device_value,"job":view.job,"root":correction.root})));
                        }
                    });
                });
                line_in(ui, &look, block(54.0, 16.0), &short, 10.5, Some(&hover));
                row(ui, &look, block(74.0, 16.0), false, |ui| {
                    // A phrase's held notes: each can become a recording of one fitted model.
                    if report.notes.len() >= 2 {
                        let names: Vec<String> = report.notes.iter().take(12).map(|n| key_name(n.root)).collect();
                        let more = if report.notes.len() > 12 { " …" } else { "" };
                        let text = format!("Phrase of {} notes: {}{more}", report.notes.len(), names.join(" "));
                        if button(ui, &look, "Fit notes as model", "Fit notes", false, true).on_hover_text(text).clicked() {
                            actions.push(Action::new("instrument.fitPhrase",json!({"track":track_value,"device":device_value})));
                        }
                    }
                    if button(ui, &look, "Discard proposal", "Discard", false, true).clicked() {
                        actions.push(Action::new("instrument.cancelAnalysis",json!({"track":track_value,"device":device_value,"job":view.job})));
                    }
                });
                ui.ctx().data_mut(|data| data.insert_temp(correction_id(),correction));
            }
            AnalysisPhase::Accepted => {
                let mut lines = Vec::new();
                if matches!(view.kind, AnalysisKind::Modal | AnalysisKind::Folder) {
                    lines.push(match view.model_recordings {
                        Some(count) => format!("Fitted model holds {count} recording(s). Play it and edit the model controls."),
                        None => "Experimental modal profile accepted. Play it and edit the model controls.".to_owned(),
                    });
                }
                if let Some(root) = view.accepted_root {
                    lines.push(format!("Root key {} accepted.", key_name(root)));
                }
                wrapped_in(ui, &look, block(24.0, 44.0), &lines.join(" "), Some("Undo restores the previous instrument and mapping. The source sample is retained."));
                fit_buttons(ui, actions);
            }
            AnalysisPhase::Cancelled | AnalysisPhase::Stale | AnalysisPhase::Failed => {
                wrapped_in(ui, &look, block(24.0, 44.0), view.message.as_deref().unwrap_or("Analysis is unavailable. Try analyzing the current sample again."), None);
                fit_buttons(ui, actions);
            }
        }
    });
}

/// A button on the fit card: its words say what it does, and while it is pointed at or has the
/// keyboard's focus `explain` replaces the card's bottom line, so each choice tells what it does.
#[allow(clippy::too_many_arguments)]
fn choice(
    ui: &mut Ui,
    look: &Look,
    label: &str,
    text: &str,
    primary: bool,
    explain: &str,
    hint: &mut Option<String>,
) -> bool {
    let response = button(ui, look, label, text, primary, true);
    if response.hovered() || response.has_focus() {
        *hint = Some(explain.to_owned());
    }
    response.clicked()
}

/// A fitted model's result waiting for acceptance, over the whole region on a card that takes
/// the pointer: what was fitted in a few words (the details in its tooltip), how the body layer
/// can use it, the A/B against the source, and Discard. The bottom line says what the button
/// pointed at does; until then, to listen first and then choose.
fn fit_result(
    ui: &mut Ui,
    look: &Look,
    bounds: Rect,
    place: impl Fn(f32, f32, f32, f32) -> Rect,
    view: &AnalysisView,
    (track, device): (&serde_json::Value, &serde_json::Value),
    actions: &mut Vec<Action>,
) {
    // On the canvas's own surface where it has one, so the result reads as part of the page.
    if let Some((texture, uv)) = look.ground {
        ui.painter().image(texture, bounds, uv, Color32::WHITE);
    } else {
        ui.painter().rect_filled(bounds, 0.0, CARD);
    }
    // The card takes the pointer, so the controls under it wait until the result is settled.
    let _ = ui.interact(bounds, ui.scope_id().with("fit-card"), egui::Sense::click_and_drag());
    let mut hint: Option<String> = None;
    let job = &view.job;
    let header = place(0.0, 4.0, 760.0, 16.0);
    caption(ui, look, header, "Fitted sound ready");
    row(ui, look, header, true, |ui| {
        if choice(ui, look, "Discard proposal", "Discard", false, "Drop this result; nothing changes.", &mut hint) {
            actions.push(Action::new("instrument.cancelAnalysis", json!({"track":track,"device":device,"job":job})));
        }
        if view.modal.as_ref().is_some_and(|report| report.profile.is_some()) {
            // Level-matched A/B against the source before accepting.
            for (label, which, explain) in [
                ("Play fitted", "fitted", "Hear the fitted sound, at the recording's level."),
                ("Play original", "original", "Hear the recording it was fitted from."),
            ] {
                if choice(ui, look, label, label, false, explain, &mut hint) {
                    actions.push(Action::new(
                        "instrument.auditionFit",
                        json!({"track":track,"device":device,"job":job,"which":which}),
                    ));
                }
            }
        }
    });
    let summary_rect = place(0.0, 26.0, 768.0, 16.0);
    let actions_rect = place(0.0, 48.0, 768.0, 18.0);
    let note_rect = place(0.0, 72.0, 768.0, 16.0);
    // How the model the body layer has now is named on the buttons.
    let has_model = view.model_recordings.is_some();
    let recordings = view.model_recordings.unwrap_or(0);
    let replace = if has_model { "Replace model" } else { "Use as model" };
    let replace_explain = if has_model {
        format!("The body layer plays this sound instead of its model's {recordings} recording(s). One undo step.")
    } else {
        "The body layer plays this sound as its model. One undo step.".to_owned()
    };
    let add_explain =
        format!("Joins the model's {recordings} recording(s): notes and velocities between them blend. One undo step.");
    let source_explain = "Joins the model as an instrument of its own: Source morph on the body layer blends between instruments. One undo step.";
    let listen = "Listen with Play original and Play fitted, then choose how the body layer uses it.";
    if let Some(folder) = &view.folder {
        let skipped = folder.files.iter().filter(|f| f.outcome.reason().is_some()).count();
        let left_out = folder.files.iter().filter(|f| f.outcome == FolderFileOutcome::LeftOut).count();
        let entries = if folder.phrase { "notes" } else { "files" };
        let mut summary = format!("{} of {} {entries} fitted", folder.recordings.len(), folder.files.len());
        if left_out > 0 {
            summary.push_str(&format!(" · {left_out} left out ({MAX_RECORDINGS} at most)"));
        }
        if skipped > 0 {
            summary.push_str(&format!(" · {skipped} skipped"));
        }
        // Every file and what became of it, in the hover, at most 40 lines.
        let mut details: Vec<String> = folder
            .files
            .iter()
            .take(40)
            .map(|file| match (&file.outcome, file.key, file.velocity) {
                (FolderFileOutcome::Skipped(reason), _, _) => format!("{}: {reason}", file.name),
                (outcome, Some(key), Some(velocity)) => format!(
                    "{}: {} at velocity {:.0} %{}",
                    file.name,
                    key_name(key),
                    velocity * 100.0,
                    if *outcome == FolderFileOutcome::LeftOut { " (left out)" } else { "" }
                ),
                _ => file.name.clone(),
            })
            .collect();
        if folder.files.len() > 40 {
            details.push(format!("… and {} more", folder.files.len() - 40));
        }
        line_in(ui, look, summary_rect, &summary, 11.0, Some(&details.join("\n")));
        if folder.recordings.is_empty() {
            line_in(
                ui,
                look,
                actions_rect,
                "No file gave a fitted recording: each needs one isolated note with a stable pitch.",
                10.5,
                None,
            );
            return;
        }
        row(ui, look, actions_rect, false, |ui| {
            let explain = format!("{replace_explain} Files of one note become velocity layers, quietest first.");
            if choice(ui, look, "Accept model", replace, true, &explain, &mut hint) {
                actions.push(Action::new("instrument.acceptFit", json!({"track":track,"device":device,"job":job})));
            }
            if has_model {
                let explain = format!("{add_explain} None join if a note and velocity is there already.");
                if choice(ui, look, &format!("Add to model ({recordings})"), "Add to model", false, &explain, &mut hint)
                {
                    actions.push(Action::new("instrument.addFit", json!({"track":track,"device":device,"job":job})));
                }
                if choice(ui, look, "Add as new source", "Add as another instrument", false, source_explain, &mut hint)
                {
                    actions.push(Action::new(
                        "instrument.addFit",
                        json!({"track":track,"device":device,"job":job,"newSource":true}),
                    ));
                }
            }
        });
        line_in(
            ui,
            look,
            note_rect,
            hint.as_deref().unwrap_or("Choose how the body layer uses these recordings."),
            10.5,
            None,
        );
        return;
    }
    let Some(report) = &view.modal else { return };
    let Some(profile) = report.profile else {
        line_in(
            ui,
            look,
            summary_rect,
            match report.outcome {
                ModalFitOutcome::Silent => "The selected region is silent or constant. Choose a sounding strike.",
                ModalFitOutcome::TooShort => {
                    "Too little decaying audio for estimation and validation. Choose a longer ring."
                }
                ModalFitOutcome::NoDecayingModes | ModalFitOutcome::Fitted => {
                    "No reliable separated decays were found. Keep the sample or try a clearer isolated strike."
                }
            },
            11.0,
            None,
        );
        return;
    };
    let error = report.modes.iter().map(|m| m.validation_rms_db).fold(0.0_f64, f64::max);
    // The revision-4 recording when there is one; its modes are what will play.
    let modes = if report.recording.is_some() { &report.recording_modes } else { &report.modes };
    // In a few words what was fitted; the measurements in the tooltip.
    let summary = format!(
        "{} partials from {}, within {:.1} dB of the recording · experimental",
        modes.len(),
        key_name(profile.reference_key()),
        error
    );
    let mut details = format!("{} modes", modes.len());
    for (count, what) in [
        (modes.iter().filter(|m| m.sustained).count(), "sustained"),
        (modes.iter().filter(|m| m.fast_share > 0.0).count(), "two-part"),
        (modes.iter().filter(|m| m.beat_share > 0.0).count(), "beating"),
        (modes.iter().filter(|m| m.loose).count(), "loose"),
    ] {
        if count > 0 {
            details.push_str(&format!(" · {count} {what}"));
        }
    }
    if let Some(recording) = report.recording.as_ref().filter(|r| !r.attack.is_empty()) {
        details.push_str(&format!(
            " · attack {:.0} ms",
            recording.attack.len() as f64 * 1000.0 / f64::from(recording.attack_rate.max(1))
        ));
    }
    // The held layer of a bowed, blown or sung recording.
    if let Some(held) = report.recording.as_ref().and_then(|r| r.sustain) {
        if held.vibrato_hz > 0.0 {
            details.push_str(&format!(" · vibrato {:.1} Hz ±{:.0} cents", held.vibrato_hz, held.vibrato_cents));
        }
        if held.tremolo_hz > 0.0 {
            details.push_str(&format!(" · tremolo {:.1} Hz", held.tremolo_hz));
        }
        if held.noise.iter().any(|v| *v > 0.0) {
            details.push_str(" · held noise");
        }
        if held.release_seconds > 0.0 {
            details.push_str(&format!(" · release {:.2} s", held.release_seconds));
        }
    }
    if let Some(comparison) = &view.comparison {
        let bands: Vec<String> =
            comparison.bands.iter().map(|b| format!("{:.0} Hz {:+.1} dB", b.center_hz, b.difference_db)).collect();
        details.push_str(&format!(
            "\nSpectral distance {:.1} dB, level {:+.1} dB. Octave bands after level matching, fitted minus original: {}. Smaller is closer; this measures the same recording, not realism.",
            comparison.mean_distance_db,
            comparison.level_difference_db,
            bands.join(", ")
        ));
    }
    let hover = format!(
        "{details}\nEnvelope error ≤ {error:.2} dB: the largest mode RMS error on held-out time windows in this recording. This is not a realism score or independent note/velocity validation. Channel {}; frames {}..{}; measured {:.2} seconds. Phases and the attack are measured from this recording; room response is not separated.",
        report.channel + 1,
        report.range.start,
        report.range.end,
        report.analyzed_seconds
    );
    line_in(ui, look, summary_rect, &summary, 11.0, Some(&hover));
    row(ui, look, actions_rect, false, |ui| {
        if choice(ui, look, "Accept model", replace, true, &replace_explain, &mut hint) {
            actions.push(Action::new("instrument.acceptFit", json!({"track":track,"device":device,"job":job})));
        }
        // A chord: each note becomes its own recording on its own key.
        if report.notes.len() >= 2 {
            let names: Vec<String> = report.notes.iter().map(|n| key_name(n.key)).collect();
            let label = format!("Accept as notes ({})", names.join(" "));
            let explain = format!(
                "Each note of the chord ({}) becomes a recording of its own, on its own key{}. One undo step.",
                names.join(" "),
                if has_model { ", instead of the model" } else { "" }
            );
            if choice(ui, look, &label, "Use each note", false, &explain, &mut hint) {
                actions.push(Action::new(
                    "instrument.acceptFit",
                    json!({"track":track,"device":device,"job":job,"notes":true}),
                ));
            }
        }
        // A recording joins the model already accepted on this Instrument.
        if view.model_recordings.is_some_and(|n| n < MAX_RECORDINGS) && report.recording.is_some() {
            if choice(ui, look, &format!("Add to model ({recordings})"), "Add to model", false, &add_explain, &mut hint)
            {
                actions.push(Action::new("instrument.addFit", json!({"track":track,"device":device,"job":job})));
            }
            // Another instrument: Source morph weighs it against the others.
            if choice(ui, look, "Add as new source", "Add as another instrument", false, source_explain, &mut hint) {
                actions.push(Action::new(
                    "instrument.addFit",
                    json!({"track":track,"device":device,"job":job,"newSource":true}),
                ));
            }
        }
    });
    let note = hint.unwrap_or_else(|| format!("{listen} Set Root first."));
    line_in(ui, look, note_rect, &note, 10.5, None);
}
