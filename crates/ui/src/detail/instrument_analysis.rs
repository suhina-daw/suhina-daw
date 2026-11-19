// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Native host controls for sample preparation. They dispatch commands and never alter audio
//! parameters directly. The app replaces the snapshot every frame, including project changes.

use egui::{Id, Rect, Ui};
use serde_json::json;
use suhina_engine::model::TrackId;
use suhina_session::{AnalysisPhase, AnalysisView, ProjectId, SampleAnalysisOutcome, Session};

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
    ui.scope_builder(egui::UiBuilder::new().max_rect(bounds), |ui| {
        ui.set_clip_rect(ui.clip_rect().intersect(bounds));
        ui.horizontal(|ui| {
            ui.strong("Sample analysis");
            let running = view.is_some_and(|v| v.phase == AnalysisPhase::Running);
            if ui.add_enabled(!running, egui::Button::new("Analyze sample")).clicked() {
                actions.push(Action::new("instrument.analyzeSample",json!({"track":track_value,"device":device_value})));
            }
            if let Some(view) = view.filter(|_| running) {
                if ui.button("Cancel analysis").clicked() {
                    actions.push(Action::new("instrument.cancelAnalysis",json!({"track":track_value,"device":device_value,"job":view.job})));
                }
            }
        });
        let Some(view) = view else {
            ui.label("Find a root note in an isolated pitched sample. Listen and check the proposal before accepting.");
            return;
        };
        match view.phase {
            AnalysisPhase::Running => {
                let fraction = if view.total > 0 { (view.completed as f32 / view.total as f32).clamp(0.0,1.0) } else { 0.0 };
                ui.add(egui::ProgressBar::new(fraction).text("Analyzing locally…"));
                ui.ctx().request_repaint();
            }
            AnalysisPhase::Ready => {
                let Some(report) = &view.report else { return; };
                if let Some(proposal) = report.proposal {
                    let mut correction = ui.ctx().data(|data| data.get_temp::<Correction>(correction_id()))
                        .filter(|c| c.job == view.job)
                        .unwrap_or(Correction { job:view.job.clone(),root:proposal.root });
                    ui.horizontal(|ui| {
                        ui.label("Root key");
                        let response = ui.add(egui::DragValue::new(&mut correction.root).range(0..=127).speed(0.1));
                        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue,true,"Proposed root key"));
                        ui.label(format!("Measured {:.2} Hz, {:+.1} cents",proposal.hz,proposal.cents));
                        if ui.button("Accept root").clicked() {
                            actions.push(Action::new("instrument.acceptAnalysis",json!({"track":track_value,"device":device_value,"job":view.job,"root":correction.root})));
                        }
                    });
                    ui.label("Accept changes the root key in one undo step. The cents offset remains in the source.");
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
