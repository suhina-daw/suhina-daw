// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Native host controls for sample preparation. They dispatch commands and never alter audio
//! parameters directly. The app replaces the snapshot every frame, including project changes.

use egui::{Id, Rect, Ui};
use serde_json::json;
use suhina_engine::model::TrackId;
use suhina_session::{
    AnalysisKind, AnalysisPhase, AnalysisView, FolderFileOutcome, FolderReport, ModalFitEngine, ModalFitOutcome,
    ProjectId, SampleAnalysisOutcome, Session,
};

use super::device_canvas::remember_keyboard_owner;
use crate::interaction::device_at::DeviceAt;
use crate::interaction::key_name::{key_name, parse_key_name, pitch_class_name};
use crate::kit::widgets::{Action, flow_button, number_input, progress_bar};

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

/// Draw the reserved SDK sample-analysis region. Parameters remain in their usual SDK controls.
pub(crate) fn show(ui: &mut Ui, bounds: Rect, track: TrackId, at: &DeviceAt, actions: &mut Vec<Action>) {
    let snapshot = ui.ctx().data(|data| data.get_temp::<Snapshot>(snapshot_id())).unwrap_or_default();
    let view =
        snapshot.view.as_ref().filter(|_| snapshot.target.as_ref().is_some_and(|(t, path)| *t == track && path == at));
    let track_value = if track == TrackId::MAIN { json!("main") } else { json!(track.0) };
    let device_value = view.map_or_else(|| at.value(), |v| json!({"id":v.device.0}));
    ui.scope_builder(egui::UiBuilder::new().max_rect(bounds)
        .id_salt(("instrument-analysis", track.0, &at.0))
        .layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
        ui.set_clip_rect(ui.clip_rect().intersect(bounds));
        ui.horizontal(|ui| {
            ui.strong("Sample analysis");
            let running = view.is_some_and(|v| v.phase == AnalysisPhase::Running);
            let analyze = flow_button(ui, "Analyze sample", !running);
            remember_keyboard_owner(&analyze);
            if analyze.clicked() {
                actions.push(Action::new("instrument.analyzeSample",json!({"track":track_value,"device":device_value})));
            }
            let fit = flow_button(ui, "Fit resonances", !running);
            remember_keyboard_owner(&fit);
            if fit.clicked() {
                actions.push(Action::new("instrument.fitSample",json!({"track":track_value,"device":device_value})));
            }
            // The app asks for the folder, then runs instrument.fitFolder on this Instrument.
            let folder = flow_button(ui, "Fit folder…", !running);
            remember_keyboard_owner(&folder);
            if folder.clicked() {
                actions.push(Action::new("ui.instrumentFitFolder",json!({"track":track_value,"device":device_value})));
            }
            if let Some(view) = view.filter(|v| running || v.phase == AnalysisPhase::Ready) {
                let cancel = flow_button(ui, if running { "Cancel analysis" } else { "Discard proposal" }, true);
                remember_keyboard_owner(&cancel);
                if cancel.clicked() {
                    actions.push(Action::new("instrument.cancelAnalysis",json!({"track":track_value,"device":device_value,"job":view.job})));
                }
            }
        });
        let Some(view) = view else {
            ui.label("Analyze a root, fit one decaying strike, or fit a folder of notes.");
            ui.label("Fitting is experimental. Inspect the result before accepting; the source stays playable.");
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
                progress_bar(ui, fraction, &text);
                ui.ctx().request_repaint();
            }
            AnalysisPhase::Ready => {
                if let Some(folder) = &view.folder {
                    folder_ready(ui, view, folder, &track_value, &device_value, actions);
                    return;
                }
                if let Some(report) = &view.modal {
                    if let Some(profile) = report.profile {
                        let error = report.modes.iter().map(|m|m.validation_rms_db).fold(0.0_f64, f64::max);
                        // The revision-4 recording when there is one; its modes are what will play.
                        let modes = if report.recording.is_some() { &report.recording_modes } else { &report.modes };
                        let mut summary = format!("Experimental: {} modes", modes.len());
                        for (count, what) in [
                            (modes.iter().filter(|m| m.sustained).count(), "sustained"),
                            (modes.iter().filter(|m| m.fast_share > 0.0).count(), "two-part"),
                            (modes.iter().filter(|m| m.beat_share > 0.0).count(), "beating"),
                            (modes.iter().filter(|m| m.loose).count(), "loose"),
                        ] {
                            if count > 0 {
                                summary.push_str(&format!(" · {count} {what}"));
                            }
                        }
                        if let Some(recording) = report.recording.as_ref().filter(|r| !r.attack.is_empty()) {
                            summary.push_str(&format!(" · attack {:.0} ms", recording.attack.len() as f64 * 1000.0 / f64::from(recording.attack_rate.max(1))));
                        }
                        // The held layer of a bowed, blown or sung recording.
                        if let Some(held) = report.recording.as_ref().and_then(|r| r.sustain) {
                            if held.vibrato_hz > 0.0 {
                                summary.push_str(&format!(" · vibrato {:.1} Hz ±{:.0} cents", held.vibrato_hz, held.vibrato_cents));
                            }
                            if held.tremolo_hz > 0.0 {
                                summary.push_str(&format!(" · tremolo {:.1} Hz", held.tremolo_hz));
                            }
                            if held.noise.iter().any(|v| *v > 0.0) {
                                summary.push_str(" · held noise");
                            }
                            if held.release_seconds > 0.0 {
                                summary.push_str(&format!(" · release {:.2} s", held.release_seconds));
                            }
                        }
                        summary.push_str(&format!(" · root {} · envelope error ≤ {:.2} dB", key_name(profile.reference_key()), error));
                        ui.label(summary)
                                .on_hover_text(format!("Largest mode RMS error on held-out time windows in this recording. This is not a realism score or independent note/velocity validation. Channel {}; frames {}..{}; measured {:.2} seconds. Phases and the attack are measured from this recording; room response is not separated.",report.channel+1,report.range.start,report.range.end,report.analyzed_seconds));
                        // One row of actions that wraps: Accept and Add first, then the A/B.
                        ui.horizontal_wrapped(|ui| {
                            let accept = flow_button(ui, "Accept model", true);
                            remember_keyboard_owner(&accept);
                            if accept.clicked() {
                                actions.push(Action::new("instrument.acceptFit",json!({"track":track_value,"device":device_value,"job":view.job})));
                            }
                            // A chord: each note becomes its own recording on its own key.
                            if report.notes.len() >= 2 {
                                let names: Vec<String> = report.notes.iter().map(|n| key_name(n.key)).collect();
                                let notes = flow_button(ui, &format!("Accept as notes ({})", names.join(" ")), true);
                                remember_keyboard_owner(&notes);
                                if notes.clicked() {
                                    actions.push(Action::new("instrument.acceptFit",json!({"track":track_value,"device":device_value,"job":view.job,"notes":true})));
                                }
                            }
                            // A recording joins the model already accepted on this Instrument.
                            if let Some(count) = view.model_recordings.filter(|n| *n < MAX_RECORDINGS && report.recording.is_some()) {
                                let add = flow_button(ui, &format!("Add to model ({count})"), true);
                                remember_keyboard_owner(&add);
                                if add.clicked() {
                                    actions.push(Action::new("instrument.addFit",json!({"track":track_value,"device":device_value,"job":view.job})));
                                }
                                // Another instrument: Source morph weighs it against the others.
                                let other = flow_button(ui, "Add as new source", true);
                                remember_keyboard_owner(&other);
                                if other.clicked() {
                                    actions.push(Action::new("instrument.addFit",json!({"track":track_value,"device":device_value,"job":view.job,"newSource":true})));
                                }
                            }
                            // Level-matched A/B against the source before accepting.
                            for (label, which) in [("Play original", "original"), ("Play fitted", "fitted")] {
                                let play = flow_button(ui, label, true);
                                remember_keyboard_owner(&play);
                                if play.clicked() {
                                    actions.push(Action::new("instrument.auditionFit",json!({"track":track_value,"device":device_value,"job":view.job,"which":which})));
                                }
                            }
                        });
                        if let Some(comparison) = &view.comparison {
                                let bands: Vec<String> = comparison.bands.iter().map(|b| format!("{:.0} Hz {:+.1} dB", b.center_hz, b.difference_db)).collect();
                                ui.label(format!("Spectral distance {:.1} dB · level {:+.1} dB", comparison.mean_distance_db, comparison.level_difference_db))
                                    .on_hover_text(format!("Octave bands after level matching, fitted minus original: {}. Smaller is closer; this measures the same recording, not realism.", bands.join(", ")));
                        }
                        ui.label(match report.engine {
                            ModalFitEngine::Recording => "Engine: Recording (a held tone). Accept plays the sample itself, its held part sustained in grains; Source Modal plays the fitted resonances instead.",
                            ModalFitEngine::Resonance => "Engine: Resonance (a struck or plucked sound). Accept plays the fitted resonances; Source Sample plays the recording instead.",
                        });
                        ui.label("Accept replaces the model; Add joins it by note and velocity. One undo step each. Set Root first.");
                    } else {
                        ui.label(match report.outcome {
                            ModalFitOutcome::Silent => "The selected region is silent or constant. Choose a sounding strike.",
                            ModalFitOutcome::TooShort => "Too little decaying audio for estimation and validation. Choose a longer ring.",
                            ModalFitOutcome::NoDecayingModes | ModalFitOutcome::Fitted => "No reliable separated decays were found. Keep the sample or try a clearer isolated strike.",
                        });
                    }
                    return;
                }
                let Some(report) = &view.report else { return; };
                // A phrase's held notes: each can become a recording of one fitted model.
                if report.notes.len() >= 2 {
                    let names: Vec<String> = report.notes.iter().take(12).map(|n| key_name(n.root)).collect();
                    let more = if report.notes.len() > 12 { " …" } else { "" };
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("Phrase of {} notes: {}{more}", report.notes.len(), names.join(" ")));
                        let phrase = flow_button(ui, "Fit notes as model", true);
                        remember_keyboard_owner(&phrase);
                        if phrase.clicked() {
                            actions.push(Action::new("instrument.fitPhrase",json!({"track":track_value,"device":device_value})));
                        }
                    });
                }
                // One stable note, or else the tonal center of a loop or phrase.
                let proposed = report.proposal.map(|proposal| (
                    proposal.root,
                    format!("Measured {:.2} Hz ({} {:+.1} cents)",proposal.hz,key_name(proposal.root),proposal.cents),
                    format!("Periodicity {:.3}; spread {:.1} cents; voiced {} / {} active windows. Analyzed frames {}..{}. These measurements are not a correctness probability.", proposal.periodicity, proposal.spread_cents, report.voiced_windows, report.active_windows, report.range.start, report.range.end),
                    ["Accept changes the root key in one undo step. The cents offset remains in the source.",
                     "For isolated notes: chords can produce an ambiguous root. Listen before accepting."],
                )).or_else(|| report.key.map(|key| {
                    let name = |k: &suhina_session::SampleKey| format!("{} {}", pitch_class_name(k.tonic), if k.mode == suhina_session::SampleKeyMode::Major { "major" } else { "minor" });
                    // Relative keys share their notes; a close runner-up is shown, not hidden.
                    let alternative = if key.runner_up.correlation >= key.key.correlation - 0.1 { format!(" · or {}", name(&key.runner_up)) } else { String::new() };
                    (
                        key.root,
                        format!("Key {}{} → root {}, tuning {:+.1} cents",name(&key.key),alternative,key_name(key.root),key.tuning_cents),
                        format!("Pitch-class profile of {} windows compared with major and minor key profiles: {} {:.2}, {} {:.2}. A correlation is a fit, not a probability. Analyzed frames {}..{}.", key.windows, name(&key.key), key.key.correlation, name(&key.runner_up), key.runner_up.correlation, report.range.start, report.range.end),
                        ["Accept sets the root to the key's tonic in one undo step; playing other keys transposes the loop.",
                         "A short loop's key can be ambiguous. Listen, and correct the root if its home note differs."],
                    )
                }));
                if let Some((root, measured, hover, notes)) = proposed {
                    let mut correction = ui.ctx().data(|data| data.get_temp::<Correction>(correction_id()))
                        .filter(|c| c.job == view.job)
                        .unwrap_or(Correction { job:view.job.clone(),root });
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Root key");
                        let response = number_input(ui, None, egui::DragValue::new(&mut correction.root).range(0..=127).speed(0.1)
                            .custom_formatter(|key, _| key_name(key.round().clamp(0.0, 127.0) as u8))
                            .custom_parser(|text| parse_key_name(text).map(f64::from)));
                        remember_keyboard_owner(&response);
                        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::SpinButton,true,"Proposed root key"));
                        ui.label(measured).on_hover_text(hover);
                        let accept = flow_button(ui, "Accept root", true);
                        remember_keyboard_owner(&accept);
                        if accept.clicked() {
                            actions.push(Action::new("instrument.acceptAnalysis",json!({"track":track_value,"device":device_value,"job":view.job,"root":correction.root})));
                        }
                    });
                    for note in notes {
                        ui.label(note);
                    }
                    if let Some(glide) = report.glide {
                        ui.label(format!("Glides in from {} over {:.2} s.", key_name(glide.from_midi.round().clamp(0.0, 127.0) as u8), glide.seconds));
                    }
                    ui.ctx().data_mut(|data| data.insert_temp(correction_id(),correction));
                } else {
                    ui.label(match report.outcome {
                        SampleAnalysisOutcome::Silent => "The analyzed region is silent or constant. Choose a sounding region.",
                        SampleAnalysisOutcome::TooShort => "The analyzed region is too short for a stable root. Use a longer isolated note.",
                        SampleAnalysisOutcome::NoStablePitch | SampleAnalysisOutcome::Stable | SampleAnalysisOutcome::TonalCenter => "No stable note or clear key was found. Keep the sample playable and set Root by ear.",
                    });
                }
            }
            AnalysisPhase::Accepted => {
                if matches!(view.kind, AnalysisKind::Modal | AnalysisKind::Folder) {
                    ui.label(match view.model_recordings {
                        Some(count) => format!("Fitted model holds {count} recording(s). Play it and edit the model controls."),
                        None => "Experimental modal profile accepted. Play it and edit the model controls.".to_owned(),
                    });
                    ui.label("Undo restores the previous instrument. The source sample is retained.");
                }
                if let Some(root) = view.accepted_root {
                    ui.label(format!("Root key {} accepted. Undo restores the previous mapping.", key_name(root)));
                }
            }
            AnalysisPhase::Cancelled | AnalysisPhase::Stale | AnalysisPhase::Failed => {
                ui.label(view.message.as_deref().unwrap_or("Analysis is unavailable. Try analyzing the current sample again."));
            }
        }
    });
}

/// A ready folder fit: what became of its files, and Accept or Add for its recordings.
fn folder_ready(
    ui: &mut Ui,
    view: &AnalysisView,
    folder: &FolderReport,
    track: &serde_json::Value,
    device: &serde_json::Value,
    actions: &mut Vec<Action>,
) {
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
    // The summary on its own line and the buttons on a row that wraps: neither runs past the
    // panel's right edge.
    ui.label(summary).on_hover_text(details.join("\n"));
    ui.horizontal_wrapped(|ui| {
        if folder.recordings.is_empty() {
            return;
        }
        let accept = flow_button(ui, "Accept model", true);
        remember_keyboard_owner(&accept);
        if accept.clicked() {
            actions.push(Action::new("instrument.acceptFit", json!({"track":track,"device":device,"job":view.job})));
        }
        if let Some(count) = view.model_recordings {
            let add = flow_button(ui, &format!("Add to model ({count})"), true);
            remember_keyboard_owner(&add);
            if add.clicked() {
                actions.push(Action::new("instrument.addFit", json!({"track":track,"device":device,"job":view.job})));
            }
            let other = flow_button(ui, "Add as new source", true);
            remember_keyboard_owner(&other);
            if other.clicked() {
                actions.push(Action::new(
                    "instrument.addFit",
                    json!({"track":track,"device":device,"job":view.job,"newSource":true}),
                ));
            }
        }
    });
    if folder.recordings.is_empty() {
        ui.label("No file gave a fitted recording: each needs one isolated note with a stable pitch.");
    } else {
        ui.label("Files of one note are velocity layers, quietest first. Accept replaces the model with them; Add puts them all into it, or none if a note and velocity is there already. One undo step each.");
    }
}
