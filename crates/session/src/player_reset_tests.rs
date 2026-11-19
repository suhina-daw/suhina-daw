// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::{Projects, Session};
use std::sync::mpsc;

struct Output(Box<suhina_engine::Renderer>);

impl OutputHandle for Output {
    fn can_exchange_renderer(&self) -> bool {
        true
    }
    fn exchange_renderer(
        &mut self,
        next: Box<suhina_engine::Renderer>,
    ) -> Result<Box<suhina_engine::Renderer>, String> {
        Ok(std::mem::replace(&mut self.0, next))
    }
    fn close(self: Box<Self>) -> Option<(u32, Plugins)> {
        None
    }
}

fn prepared() -> Player {
    Player::prepare(
        &Project::default(),
        &suhina_engine::AudioPool::new(),
        &suhina_engine::DeviceAssets::default(),
        44100,
    )
    .unwrap()
}

fn pending_project() -> (Projects, crate::ProjectId, mpsc::Sender<Result<Box<suhina_engine::Renderer>, String>>) {
    let mut projects = Projects::new();
    let first = projects.add(Session::new(Project::default()), None).unwrap();
    let mut player = prepared();
    player.keep_alive = Some(Box::new(Output(player.parked.take().unwrap())));
    projects.get_mut(first).unwrap().attach_player(player);
    projects.add(Session::new(Project::default()), None).unwrap();
    let (send, receive) = mpsc::channel();
    // Model a sandbox whose reset has not answered. No clocks or scheduling assumptions.
    let player = projects.get_mut(first).unwrap().sync.suspended.as_mut().unwrap();
    player.parked = None;
    player.resetting = Some(receive);
    (projects, first, send)
}

#[test]
fn returning_to_a_resetting_project_keeps_it_silent_until_the_reply() {
    let (mut projects, first, send) = pending_project();
    projects.switch(first).unwrap();
    projects.open_output(None).unwrap();
    assert!(projects.get(first).unwrap().player().is_none());
    projects.poll_plugins();
    assert!(projects.get(first).unwrap().player().is_none());
    send.send(Ok(prepared().parked.take().unwrap())).unwrap();
    projects.poll_plugins();
    assert!(projects.get(first).unwrap().player().is_some());
    assert!(projects.get(first).unwrap().sync.suspended.is_none());
    assert_eq!(projects.outputs_opened(), 0, "the retained device was not reopened");
}

#[test]
fn a_failed_reset_is_reported_once_and_never_resumed() {
    let (mut projects, first, send) = pending_project();
    projects.switch(first).unwrap();
    send.send(Err("reset failed".into())).unwrap();
    assert!(projects.poll_plugins().iter().any(|s| s.contains("reset failed")));
    assert!(projects.get(first).unwrap().player().is_none());
    assert!(projects.poll_plugins().is_empty());
}

#[test]
fn a_disconnected_reset_worker_is_an_error_without_waiting() {
    let (mut projects, first, send) = pending_project();
    projects.switch(first).unwrap();
    drop(send);
    assert!(projects.poll_plugins().iter().any(|s| s.contains("reset worker failed")));
    assert!(projects.get(first).unwrap().player().is_none());
}

#[test]
fn closing_a_project_during_reset_does_not_wait_for_its_reply() {
    let (mut projects, first, send) = pending_project();
    let closed = projects.close(first).unwrap();
    drop(closed);
    // The close worker owns the receiver temporarily; no timing assumption about its drop.
    let _ = send.send(Ok(prepared().parked.take().unwrap()));
    assert!(projects.get(first).is_none());
}

#[test]
fn parking_returns_while_a_plugin_is_still_resetting() {
    struct Held(mpsc::Receiver<()>);
    impl suhina_engine::PluginProcessor for Held {
        fn process(
            &mut self,
            _: &mut [f32],
            _: &[(usize, suhina_device_api::NoteEvent)],
            _: std::time::Duration,
        ) -> bool {
            true
        }
        fn reset_audio(&mut self, _: std::time::Duration) -> Result<(), String> {
            self.0.recv().map_err(|e| e.to_string())
        }
    }
    let mut project = crate::blank_project();
    project.channels[0].devices.push(suhina_model::Device {
        id: suhina_model::DeviceId::NONE,
        name: "Held reset".into(),
        kind: suhina_model::DeviceKind::Vst3 { id: "test".into() },
        enabled: true,
        state: None,
        params: vec![],
        automation: vec![],
        clip_host: false,
        inputs: vec![],
        instrument: false,
        extensions: Default::default(),
    });
    project.assign_ids().unwrap();
    let (release, wait) = mpsc::channel();
    let mut plugins = Plugins::new();
    plugins.insert(
        suhina_engine::plugin_devices(&project)[0].key,
        suhina_engine::PluginSlotInfo { instrument: false, latency: 0 },
        Box::new(Held(wait)),
    );
    let mut player = Player::prepare_with_plugins(
        &project,
        &suhina_engine::AudioPool::new(),
        &suhina_engine::DeviceAssets::default(),
        44100,
        plugins,
    )
    .unwrap();
    player.keep_alive = Some(Box::new(Output(player.parked.take().unwrap())));
    let idle = player.park().unwrap();
    assert!(idle.can_park(), "the device is available before the sandbox replies");
    assert!(!player.reset_ready().unwrap());
    release.send(()).unwrap();
    // Explicitly receive the completion, without polling or a wall-clock assertion.
    let renderer = player.resetting.take().unwrap().recv().unwrap().unwrap();
    assert!(player.parked.is_none());
    drop(renderer);
}

#[test]
fn changing_the_output_during_reset_keeps_the_retained_device_available() {
    let (mut projects, first, send) = pending_project();
    projects.switch(first).unwrap();
    assert!(projects.reopen_output().is_err());
    send.send(Ok(prepared().parked.take().unwrap())).unwrap();
    projects.poll_plugins();
    assert!(projects.get(first).unwrap().player().is_some());
}
