// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Native host controls for sample preparation. They dispatch commands and never alter audio
//! parameters directly. The app replaces the snapshot every frame, including project changes.

use egui::{Id, Rect, Ui};
use serde_json::json;
use suhina_engine::model::TrackId;
use suhina_session::{
    AnalysisKind, AnalysisPhase, AnalysisView, ModalFitOutcome, ProjectId, SampleAnalysisOutcome, Session,
};

use super::device_canvas::remember_keyboard_owner;
use crate::interaction::device_at::DeviceAt;
use crate::kit::widgets::Action;

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
            let analyze = ui.add_enabled(!running, egui::Button::new("Analyze sample"));
            remember_keyboard_owner(&analyze);
            if analyze.clicked() {
                actions.push(Action::new("instrument.analyzeSample",json!({"track":track_value,"device":device_value})));
            }
            let fit = ui.add_enabled(!running, egui::Button::new("Fit resonances"));
            remember_keyboard_owner(&fit);
            if fit.clicked() {
                actions.push(Action::new("instrument.fitSample",json!({"track":track_value,"device":device_value})));
            }
            if let Some(view) = view.filter(|v| running || v.phase == AnalysisPhase::Ready) {
                let cancel = ui.button(if running { "Cancel analysis" } else { "Discard proposal" });
                remember_keyboard_owner(&cancel);
                if cancel.clicked() {
                    actions.push(Action::new("instrument.cancelAnalysis",json!({"track":track_value,"device":device_value,"job":view.job})));
                }
            }
        });
        let Some(view) = view else {
            ui.label("Analyze a root note, or fit resonances from an isolated decaying strike.");
            ui.label("Fitting is experimental. Inspect the result before accepting; the source stays playable.");
            return;
        };
        match view.phase {
            AnalysisPhase::Running => {
                let fraction = if view.total > 0 { (view.completed as f32 / view.total as f32).clamp(0.0,1.0) } else { 0.0 };
                ui.add(egui::ProgressBar::new(fraction).text(if view.kind == AnalysisKind::Modal { "Fitting resonances locally…" } else { "Analyzing locally…" }));
                ui.ctx().request_repaint();
            }
            AnalysisPhase::Ready => {
                if let Some(report) = &view.modal {
                    if let Some(profile) = report.profile {
                        let error = report.modes.iter().map(|m|m.validation_rms_db).fold(0.0_f64, f64::max);
                        ui.horizontal(|ui| {
                            ui.label(format!("Experimental: {} modes · root {} · envelope error ≤ {:.2} dB",profile.modes().len(),profile.reference_key(),error))
                                .on_hover_text(format!("Largest mode RMS error on held-out time windows in this recording. This is not a realism score or independent note/velocity validation. Channel {}; frames {}..{}; measured {:.2} seconds. Initial phases, attack and room response are not reconstructed.",report.channel+1,report.range.start,report.range.end,report.analyzed_seconds));
                            let accept = ui.button("Accept model");
                            remember_keyboard_owner(&accept);
                            if accept.clicked() {
                                actions.push(Action::new("instrument.acceptFit",json!({"track":track_value,"device":device_value,"job":view.job})));
                            }
                        });
                        ui.label("Accept selects Modal, resets model controls and enables all fitted modes. One undo step.");
                        ui.label("One recording cannot establish physical identity or behaviour at other notes and dynamics.");
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
                if let Some(proposal) = report.proposal {
                    let mut correction = ui.ctx().data(|data| data.get_temp::<Correction>(correction_id()))
                        .filter(|c| c.job == view.job)
                        .unwrap_or(Correction { job:view.job.clone(),root:proposal.root });
                    ui.horizontal(|ui| {
                        ui.label("Root key");
                        let response = ui.add(egui::DragValue::new(&mut correction.root).range(0..=127).speed(0.1));
                        remember_keyboard_owner(&response);
                        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::SpinButton,true,"Proposed root key"));
                        ui.label(format!("Measured {:.2} Hz, {:+.1} cents",proposal.hz,proposal.cents))
                            .on_hover_text(format!("Periodicity {:.3}; spread {:.1} cents; voiced {} / {} active windows. Analyzed frames {}..{}. These measurements are not a correctness probability.", proposal.periodicity, proposal.spread_cents, report.voiced_windows, report.active_windows, report.range.start, report.range.end));
                        let accept = ui.button("Accept root");
                        remember_keyboard_owner(&accept);
                        if accept.clicked() {
                            actions.push(Action::new("instrument.acceptAnalysis",json!({"track":track_value,"device":device_value,"job":view.job,"root":correction.root})));
                        }
                    });
                    ui.label("Accept changes the root key in one undo step. The cents offset remains in the source.");
                    ui.label("For isolated notes: chords can produce an ambiguous root. Listen before accepting.");
                    ui.ctx().data_mut(|data| data.insert_temp(correction_id(),correction));
                } else {
                    ui.label(match report.outcome {
                        SampleAnalysisOutcome::Silent => "The analyzed region is silent or constant. Choose a sounding region.",
                        SampleAnalysisOutcome::TooShort => "The analyzed region is too short for a stable root. Use a longer isolated note.",
                        SampleAnalysisOutcome::NoStablePitch | SampleAnalysisOutcome::Stable => "No stable root was found. Keep the sample playable and set Root by ear.",
                    });
                }
            }
            AnalysisPhase::Accepted => {
                if view.kind == AnalysisKind::Modal {
                    ui.label("Experimental modal profile accepted. Play it and edit the model controls.");
                    ui.label("Undo restores the previous instrument. The source sample is retained.");
                }
                if let Some(root) = view.accepted_root {
                    ui.label(format!("Root key {root} accepted. Undo restores the previous mapping."));
                }
            }
            AnalysisPhase::Cancelled | AnalysisPhase::Stale | AnalysisPhase::Failed => {
                ui.label(view.message.as_deref().unwrap_or("Analysis is unavailable. Try analyzing the current sample again."));
            }
        }
    });
}
