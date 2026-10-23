// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The whole window, driven like a user would: clicks become the commands the CLI and MCP run,
//! and edits change the project through the session. One module per part of the window; this
//! file holds what several of them use (`demo_app`, `window`, `window_with_step` and the
//! projects they open).

use egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use suhina_engine::model::TrackId;
use suhina_session::Session;
use suhina_ui::arrangement::arrangement_view::{self, ArrangementViewState};
use suhina_ui::arrangement::timeline::Timeline;
use suhina_ui::demo::demo_project;
use suhina_ui::interaction::grid::{SlotState, TrackGrid};
use suhina_ui::kit::widgets::Action;
use suhina_ui::session::session_view::{self, SessionViewState};
use suhina_ui::settings::themes::FileAction;
use suhina_ui::{App, OpenAs, Status, View, kit::theme};

mod arrangement;
mod control_bar;
mod detail;
mod export;
mod mixer;
mod session;
mod settings;
mod snapshots;
mod start;
mod tabs;

pub(super) const SIZE: (f32, f32) = (1280.0, 800.0);

/// A file dropped on the window in a test.
#[derive(Debug)]
pub(super) struct TestFile(std::path::PathBuf);

impl egui::DroppedFile for TestFile {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        Ok(Vec::new())
    }
}

pub(super) struct SessionHarness {
    pub(super) grid: TrackGrid,
    pub(super) state: SessionViewState,
    pub(super) actions: Vec<Action>,
}

pub(super) fn view() -> Harness<'static, SessionHarness> {
    let view = SessionHarness {
        grid: TrackGrid::build(&demo_project(), None),
        state: SessionViewState::default(),
        actions: Vec::new(),
    };
    let mut harness = Harness::builder().with_size(SIZE).build_ui_state(
        |ui, v: &mut SessionHarness| session_view::show(ui, &v.grid, &[], &mut v.state, &mut v.actions),
        view,
    );
    theme::install(&harness.ctx);
    harness.run();
    harness
}

pub(super) fn clicked(harness: &mut Harness<'static, SessionHarness>, label: &str) -> Vec<Action> {
    harness.state_mut().actions.clear();
    harness.get_by_label(label).click();
    harness.run();
    harness.state().actions.clone()
}

pub(super) fn window(app: App) -> Harness<'static, App> {
    window_with_step(app, 0.25)
}

/// Like [`window`], with `step` seconds a frame (a double-click needs frames closer together),
/// and a second of frames for animations to settle.
pub(super) fn window_with_step(app: App, step: f32) -> Harness<'static, App> {
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(step)
        .with_max_steps((1.0 / step).ceil() as u64)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    // Frames, not a run to rest: a window that repaints every frame while it settles (the
    // sample map's grid building, an animation) would spend the run's max and fail, where
    // these frames advance the same second.
    harness.run_steps(((1.0_f64 / step as f64).ceil() as usize).max(1));
    harness
}

/// The demo project in the window, with the browser hidden so the views have the whole width
/// (the browser has tests of its own, in `browser.rs`).
pub(super) fn demo_app() -> App {
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(demo_project()), &[]);
    app
}

/// The demo with the clip menus' name field on (Settings › Arrangement › Name field in clip
/// menus), as the tests of renaming in the menu use it.
pub(super) fn demo_app_with_name_field() -> App {
    let mut app = demo_app();
    let mut settings = app.settings().clone();
    settings.view.menu_name_field = true;
    app.set_settings(&egui::Context::default(), settings);
    app
}

pub(super) fn arrangement_app() -> Harness<'static, App> {
    let mut harness = window(demo_app());
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness
}

/// MIDI ports by name that open into nothing.
pub(super) struct StandMidi(Vec<String>);

impl suhina_session::MidiSystem for StandMidi {
    fn ports(&self) -> Result<Vec<String>, String> {
        Ok(self.0.clone())
    }

    fn open(
        &self,
        _name: &str,
        _input: std::sync::Arc<suhina_session::LiveInput>,
    ) -> Result<Box<dyn std::any::Any + Send>, String> {
        Ok(Box::new(()))
    }
}

/// The demo project with a group track, "Group", that tracks can play into.
pub(super) fn project_with_a_group() -> suhina_engine::model::Project {
    use suhina_engine::model::{Channel, ChannelId, ChannelRole, ContentKinds, Track};
    let mut project = demo_project();
    project.tracks.push(Track {
        id: TrackId(20),
        name: "Group".into(),
        content: ContentKinds::default(),
        channel: Some(ChannelId(120)),
        ..project.tracks[0].clone()
    });
    project.channels.push(Channel {
        id: ChannelId(120),
        name: "Group".into(),
        role: ChannelRole::Submix,
        sends: Vec::new(),
        ..project.channels[0].clone()
    });
    project
}

pub(super) fn names(h: &Harness<'static, App>) -> Vec<String> {
    h.state().session().map(|s| s.project().tracks.iter().map(|t| t.name.clone()).collect()).unwrap_or_default()
}

/// The demo project with an audio track, "Loop", whose clip plays `loop.wav` (two seconds of a
/// tone that fades out), and the waveforms of that file.
pub(super) fn project_with_audio() -> (suhina_engine::model::Project, suhina_ui::kit::waveform::Waveforms) {
    use suhina_engine::model::*;
    let mut project = demo_project();
    project.tracks.push(Track {
        id: TrackId(30),
        name: "Loop".into(),
        content: ContentKinds { notes: false, audio: true },
        channel: Some(ChannelId(130)),
        ..project.tracks[0].clone()
    });
    project.channels.push(Channel {
        id: ChannelId(130),
        name: "Loop".into(),
        sends: Vec::new(),
        ..project.channels[0].clone()
    });
    let region = AudioRegion {
        file: "loop.wav".into(),
        absolute_file: None,
        sample_rate: Some(8000),
        channels: Some(1),
        duration_seconds: Some(2.0),
        warped: true,
        stretch: StretchMode::default(),
        warp_markers: vec![WarpMarker { time: 0.0, audio_time: 0.0 }, WarpMarker { time: 4.0, audio_time: 2.0 }],
        gain: 1.0,
        transpose: 0.0,
        stream: None,
    };
    project.contents.push(ClipContent {
        id: ContentId(300),
        name: "Loop".into(),
        length: 4.0,
        lanes: vec![ContentLane { target: None, data: LaneData::Audio(region) }],
        extensions: Extensions::default(),
        scale: None,
    });
    let clip = Clip {
        id: Default::default(),
        content: ContentId(300),
        start: 0.0,
        length: 16.0,
        offset: 0.0,
        loop_range: Some(LoopRange { start: 0.0, end: 4.0 }),
        enabled: true,
        fade_in: 0.0,
        fade_out: 0.0,
        fade_in_curve: Default::default(),
        fade_out_curve: Default::default(),
        follow_action: Default::default(),
        launch: Default::default(),
        color: None,
        extensions: Extensions::default(),
    };
    project.arrangement.lanes.push(ArrangementLane {
        id: Default::default(),
        name: "Loop".into(),
        track: Some(TrackId(30)),
        clips: vec![clip],
    });
    let samples: Vec<f32> = (0..16000).map(|i| (i as f32 * 0.3).sin() * (1.0 - i as f32 / 16000.0)).collect();
    let mut pool = suhina_engine::AudioPool::new();
    pool.files.insert(
        "loop.wav".into(),
        std::sync::Arc::new(suhina_engine::AudioData { sample_rate: 8000, channels: 1, samples }),
    );
    (project, suhina_ui::kit::waveform::from_pool(&pool))
}

pub(super) fn audio_app() -> App {
    let (project, waveforms) = project_with_audio();
    let mut app = App::new();
    app.set_session(Session::new(project), &[]);
    app.set_waveforms(waveforms);
    app
}

/// The demo project with a utility device on Drums.
pub(super) fn project_with_a_device() -> suhina_engine::model::Project {
    use suhina_engine::model::{Device, DeviceKind, DeviceParam, Extensions};
    let mut project = demo_project();
    project.channels[0].devices.push(Device {
        name: "Utility".into(),
        kind: DeviceKind::Builtin("utility".into()),
        enabled: true,
        state: None,
        params: vec![DeviceParam::new("gain", -3.5), DeviceParam::new("width", 1.2)],
        automation: Vec::new(),
        extensions: Extensions::default(),
    });
    project
}

/// An audio system with one device on one host, whose players run nowhere. `WASAPI` is the host
/// whose shared mode Windows gives the device's buffer in, so the Settings row there names the
/// engine's own block; its exclusive mode is possible, and does ask the device. `ALSA` is a host
/// whose driver declares no buffer range of its own: it keeps its own size, and Suhina's own are
/// offered.
pub(super) struct StandAudio(&'static str);

impl suhina_session::AudioSystem for StandAudio {
    fn hosts(&self) -> Vec<suhina_session::audio::HostInfo> {
        use suhina_session::audio::{DeviceInfo, HostInfo};
        let wasapi = self.0 == "WASAPI";
        let undeclared = self.0 == "ALSA";
        vec![HostInfo {
            name: self.0.into(),
            default: true,
            devices: vec![DeviceInfo {
                name: if wasapi { "Apollo" } else { "Stand Out" }.into(),
                default: true,
                channels: 2,
                sample_rates: vec![48_000],
                default_sample_rate: Some(48_000),
                // WASAPI shared mode plays the period Windows gives the device (480 frames here),
                // whatever is asked, so the sizes listed there are the engine's own block; a
                // driver that declared nothing is offered those too, and asked for none of them.
                buffer_sizes: if wasapi || undeclared {
                    vec![64, 128, 256, 512, 1024, 2048, 4096]
                } else {
                    vec![256, 512]
                },
                buffer_range: if undeclared {
                    None
                } else if wasapi {
                    Some((480, 480))
                } else {
                    Some((256, 512))
                },
                exclusive: wasapi,
            }],
            inputs: vec![suhina_session::audio::InputInfo {
                name: "Stand In".into(),
                default: true,
                channels: 2,
                sample_rates: vec![48_000],
            }],
            error: None,
        }]
    }

    fn open(
        &self,
        project: &suhina_engine::model::Project,
        audio: &suhina_engine::AudioPool,
        _assets: &suhina_engine::DeviceAssets,
        _choice: &suhina_session::OutputChoice,
        _previous: Option<suhina_session::Player>,
    ) -> Result<suhina_session::audio::Opened, String> {
        let mut renderer = suhina_engine::Renderer::stopped(project, audio, 48_000).map_err(|e| e.to_string())?;
        let player = suhina_session::Player::new(renderer.controller(), None);
        Ok(suhina_session::audio::Opened { player, notice: None, plugins_kept: true })
    }
}

/// Presses and lets go twice at `at`, a frame apart: a double-click. It waits a second first,
/// as a person would, so an earlier click does not make it a triple-click.
pub(super) fn double_click_at(harness: &mut Harness<'static, App>, at: egui::Pos2) {
    harness.hover_at(at);
    harness.run();
    let ready = harness.ctx.input(|i| i.time) + 1.0;
    while harness.ctx.input(|i| i.time) < ready {
        harness.step();
    }
    for _ in 0..2 {
        harness.hover_at(at);
        harness.drag_at(at);
        harness.drop_at(at);
        harness.step();
    }
    harness.run();
}

/// Picks `value` in the control bar's grid box, as a user does: open it, click the value.
pub(super) fn choose_grid(harness: &mut Harness<'static, App>, value: &str) {
    harness.get_by_label("Grid and launch quantisation").click();
    harness.run();
    harness.get_by_label(value).click();
    harness.run();
}

/// The grid value of the active project's session.
pub(super) fn grid_of(harness: &Harness<'static, App>) -> Option<suhina_engine::Quantization> {
    harness.state().session().map(|s| s.quantization())
}

/// Zooms the Arrangement view in eight times (64 points a beat) around a point just inside
/// Beat B's left edge, so that edge stays in view; returns where the edge is then.
pub(super) fn zoom_in_at_beat_b(harness: &mut Harness<'static, App>) -> f32 {
    let clip = harness.get_by_label("Clip Beat B on Drums at bar 17").rect();
    // 8 points a beat by default: 4 points in is half a beat into the clip, which starts on beat 64.
    let at = egui::pos2(clip.left() + 4.0, clip.center().y);
    harness.hover_at(at);
    harness.run();
    harness.input_mut().events.push(egui::Event::Zoom(8.0));
    harness.run();
    let left = harness.get_by_label("Clip Beat B on Drums at bar 17").rect().left();
    assert!((left - (at.x - 32.0)).abs() <= 1.0, "zoomed to 64 points a beat around the pointer: {left}");
    left
}

/// Drags Beat B by `by` points, taking hold of it just inside its left edge, which stays in
/// view when zoomed in.
pub(super) fn drag_beat_b(harness: &mut Harness<'static, App>, by: egui::Vec2) {
    let clip = harness.get_by_label("Clip Beat B on Drums at bar 17").rect();
    // By its name bar: its body selects time.
    let from = egui::pos2(clip.left() + 16.0, clip.top() + 6.0);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from + by * 0.5);
    harness.run();
    harness.hover_at(from + by);
    harness.run();
    harness.drop_at(from + by);
    harness.run();
}

/// Where Beat B (on beat 64 in the demo) starts now, after a move of less than two bars.
pub(super) fn beat_b_start(harness: &Harness<'static, App>) -> Option<f64> {
    harness
        .state()
        .session()?
        .project()
        .arrangement
        .lanes
        .first()?
        .clips
        .iter()
        .map(|c| c.start)
        .find(|s| (63.9..72.0).contains(s))
}
