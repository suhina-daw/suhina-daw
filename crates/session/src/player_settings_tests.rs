// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Retained projects hear current output settings without reopening the device.

use super::*;
use crate::audio::{AudioSystem, HostInfo, Opened};
use crate::{OutputChoice, Projects, Session};
use serde_json::json;
use std::sync::{Arc, Mutex};
use suhina_engine::{AudioData, AudioPool, LaunchCommand, RenderInputs, RenderOptions, Renderer};
use suhina_model::{AudioRegion, Clip, ContentId};

#[derive(Default)]
struct Interface {
    renderer: Arc<Mutex<Option<Box<Renderer>>>>,
}

struct Output {
    renderer: Arc<Mutex<Option<Box<Renderer>>>>,
    separate: bool,
}

impl OutputHandle for Output {
    fn can_exchange_renderer(&self) -> bool {
        true
    }

    fn exchange_renderer(&mut self, mut next: Box<Renderer>) -> Result<Box<Renderer>, String> {
        next.handle(LaunchCommand::SetCue { gain: None, separate: Some(self.separate) });
        self.renderer.lock().unwrap().replace(next).ok_or_else(|| "no renderer".into())
    }

    fn close(self: Box<Self>) -> Option<(u32, Plugins)> {
        self.renderer.lock().unwrap().take().map(|renderer| (renderer.sample_rate(), renderer.into_plugins()))
    }
}

impl AudioSystem for Interface {
    fn hosts(&self) -> Vec<HostInfo> {
        Vec::new()
    }

    fn open(
        &self,
        project: &Project,
        audio: &AudioPool,
        assets: &suhina_engine::DeviceAssets,
        choice: &OutputChoice,
        previous: Option<Player>,
    ) -> Result<Opened, String> {
        if let Some(previous) = previous
            && let (_, Some(owner)) = previous.into_parts()
        {
            owner.close();
        }
        let mut renderer = Box::new(
            Renderer::build(
                project,
                RenderInputs::from_audio(audio, crate::devices()).with_assets(assets),
                choice.sample_rate.unwrap_or(48_000),
                RenderOptions::STOPPED,
            )
            .map_err(|e| e.to_string())?,
        );
        let separate = choice.cue_pair.is_some();
        renderer.handle(LaunchCommand::SetCue { gain: Some(choice.cue_gain), separate: Some(separate) });
        let controller = renderer.controller();
        *self.renderer.lock().unwrap() = Some(renderer);
        let owner = Output { renderer: Arc::clone(&self.renderer), separate };
        Ok(Opened { player: Player::new(controller, Some(Box::new(owner))), notice: None, plugins_kept: false })
    }
}

fn projects(rate: u32) -> (Projects, Arc<Interface>) {
    let interface = Arc::new(Interface::default());
    let mut projects = Projects::new();
    projects.set_audio_system(interface.clone());
    projects.set_output_choice(OutputChoice { sample_rate: Some(rate), cue_pair: Some(1), ..Default::default() });
    (projects, interface)
}

fn preview_level(projects: &mut Projects, interface: &Interface) -> f32 {
    let player = projects.preview_session_mut().unwrap().player_mut().unwrap();
    let data = Arc::new(AudioData { sample_rate: player.sample_rate(), channels: 2, samples: vec![0.5; 4096] });
    player.controller_mut().preview_sample(data, 0.0, 1.0).unwrap();
    let (mut main, mut cue) = ([0.0; 2048], [0.0; 2048]);
    interface.renderer.lock().unwrap().as_mut().unwrap().process_with_cue(&mut main, &mut cue);
    assert!(main.iter().all(|sample| *sample == 0.0));
    cue[1000]
}

#[test]
fn cue_volume_survives_new_projects_returning_and_preview_transitions() {
    let (mut projects, interface) = projects(48_000);
    let first = projects.add(Session::new(Project::default()), None).unwrap();
    projects.open_output(None).unwrap();
    assert_eq!(preview_level(&mut projects, &interface), 0.5);
    for db in [-6.0, -160.0] {
        projects.execute("output.setCueVolume", &json!({ "db": db })).unwrap();
        let expected = projects.output_choice().cue_gain * 0.5;
        // Switch before the renderer consumes the volume command, too.
        let second = projects.add(Session::new(Project::default()), None).unwrap();
        projects.open_output(None).unwrap();
        assert!((preview_level(&mut projects, &interface) - expected).abs() < 1e-6);
        projects.switch(first).unwrap();
        projects.open_output(None).unwrap();
        assert!((preview_level(&mut projects, &interface) - expected).abs() < 1e-6);
        projects.close(second).unwrap();
    }
    projects.close(first).unwrap();
    assert_eq!(preview_level(&mut projects, &interface), 0.0);
    assert_eq!(projects.outputs_opened(), 1, "volume changes and project exchanges do not reopen the device");
}

fn cabinet_session(rate: u32) -> Session {
    let project = suhina_test_support::ProjectBuilder::new()
        .audio_track(1, "Impulse")
        .audio(1, "Impulse", 4.0, AudioRegion::new("impulse"))
        .clip(1, Clip::new(ContentId(1), 0.0, 4.0))
        .build();
    let mut session = Session::new(project);
    let mut samples = vec![0.0; 8192];
    samples[1024] = 0.5;
    session.audio.pool.files.insert("impulse".into(), Arc::new(AudioData { sample_rate: rate, channels: 1, samples }));
    session.execute("device.add", &json!({ "track": 1, "id": "nam-cab-loader" })).unwrap();
    session
        .execute(
            "device.setNamFile",
            &json!({ "track": 1, "device": 0, "path": "builtin:nam-cab-loader/Synthetic Cabinet Plain.wav" }),
        )
        .unwrap();
    session
}

fn render_start(projects: &mut Projects, interface: &Interface) -> Vec<f32> {
    projects.execute("transport.setPosition", &json!({ "beats": 0.0 })).unwrap();
    projects.execute("transport.play", &json!({})).unwrap();
    let mut rendered = Vec::new();
    let mut block = [0.0; 1024];
    for _ in 0..16 {
        interface.renderer.lock().unwrap().as_mut().unwrap().process(&mut block);
        rendered.extend_from_slice(&block);
    }
    rendered
}

#[test]
fn background_graph_edits_keep_cabinet_audio_at_the_retained_rate() {
    for rate in [44_100, 96_000] {
        let (mut projects, interface) = projects(rate);
        let first = projects.add(cabinet_session(rate), None).unwrap();
        projects.open_output(None).unwrap();
        let expected = render_start(&mut projects, &interface);
        assert!(expected.iter().any(|sample| sample.abs() > 1e-4), "the cabinet is heard");
        let loads = projects.get(first).unwrap().device_file_loads();
        let identity = projects.get(first).unwrap().player().unwrap().meter_monitor_arc();
        projects.add(Session::new(Project::default()), None).unwrap();
        projects.open_output(None).unwrap();
        projects.execute_in(Some(first), "track.add", &json!({ "kind": "audio" })).unwrap();
        assert_eq!(projects.get(first).unwrap().device_file_loads(), loads, "suspending does not change asset rate");
        projects.switch(first).unwrap();
        projects.open_output(None).unwrap();
        assert!(Arc::ptr_eq(&identity, &projects.get(first).unwrap().player().unwrap().meter_monitor_arc()));
        let actual = render_start(&mut projects, &interface);
        assert_eq!(expected, actual, "an unrelated background edit must preserve the impulse at {rate} Hz");
        assert_eq!(projects.outputs_opened(), 1);
    }
}

#[test]
fn retained_devices_prepare_cabinet_assets_before_a_new_rate_plays() {
    for retained in [false, true] {
        let (mut projects, interface) = projects(48_000);
        let first = projects.add(cabinet_session(48_000), None).unwrap();
        if retained {
            projects.open_output(None).unwrap();
        }
        projects.add(Session::new(Project::default()), None).unwrap();
        projects.open_output(None).unwrap();
        projects.execute("audio.setOutput", &json!({ "sampleRate": 96_000 })).unwrap();
        projects.switch(first).unwrap();
        projects.open_output(None).unwrap();
        let actual = render_start(&mut projects, &interface);
        let (mut reference, reference_output) = self::projects(96_000);
        reference.add(cabinet_session(48_000), None).unwrap();
        reference.open_output(None).unwrap();
        let expected = render_start(&mut reference, &reference_output);
        assert!(expected.iter().any(|sample| sample.abs() > 1e-4));
        assert_eq!(actual, expected, "assets must have the device's rate from the first block, retained={retained}");
    }
}

#[test]
fn returning_projects_use_the_current_process_render_pool() {
    // A separate process keeps this global setting and the pool's limited lanes
    // independent of other tests, including a parallel cargo test run.
    const CHILD: &str = "SUHINA_TEST_RETAINED_RENDER_POOL";
    if std::env::var_os(CHILD).is_none() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "player::settings_tests::returning_projects_use_the_current_process_render_pool",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env_remove(suhina_engine::RENDER_THREADS_VAR)
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    suhina_engine::set_process_render_threads(2);
    let (mut projects, interface) = projects(48_000);
    let first = projects.add(Session::new(Project::default()), None).unwrap();
    projects.open_output(None).unwrap();
    let identity = projects.get(first).unwrap().player().unwrap().meter_monitor_arc();
    let second = projects.add(Session::new(Project::default()), None).unwrap();
    projects.open_output(None).unwrap();
    for threads in [3, 1] {
        projects.execute("audio.setRenderThreads", &json!({ "threads": threads })).unwrap();
        projects.switch(first).unwrap();
        projects.open_output(None).unwrap();
        assert_eq!(
            interface.renderer.lock().unwrap().as_ref().unwrap().render_threads(),
            suhina_engine::render_threads_for(threads),
        );
        assert!(Arc::ptr_eq(&identity, &projects.get(first).unwrap().player().unwrap().meter_monitor_arc()));
        projects.switch(second).unwrap();
        projects.open_output(None).unwrap();
    }
    assert_eq!(projects.outputs_opened(), 3, "only the settings changes reopen the device");
}
