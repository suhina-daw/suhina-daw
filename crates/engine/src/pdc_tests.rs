// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! PDC against independently specified gain steps, with allocation-free test processors.

use std::sync::Arc;
use std::time::Duration;

use suhina_device_api::{Device as Native, DeviceError, DeviceInfo, NoteEvent, ParamInfo, ProcessContext, Role};
use suhina_device_sdk::{DeviceAsset, DeviceFactory, DeviceGraph, DeviceSdkError, GraphDevice};
use suhina_model::*;

use crate::{
    AudioData, AudioPool, ParamEvent, PluginProcessor, PluginSlotInfo, Plugins, RenderInputs, RenderOptions, Renderer,
};

const RATE: u32 = 8_000;
const LOOP: usize = 2048;
static GAIN_PARAMS: [ParamInfo; 1] = [ParamInfo::toggle("0", "On", true)];
static GAIN: DeviceInfo = DeviceInfo::new("pdc-gain", "Gain", Role::Effect, &GAIN_PARAMS);

struct Gain(f32);

impl Native for Gain {
    fn info(&self) -> &DeviceInfo {
        &GAIN
    }
    fn prepare(&mut self, _: f64) -> Result<(), DeviceError> {
        Ok(())
    }
    fn reset(&mut self) {}
    fn set_param(&mut self, index: usize, value: f64) {
        if index == 0 {
            self.0 = value as f32;
        }
    }
    fn param(&self, index: usize) -> Option<f64> {
        (index == 0).then_some(f64::from(self.0))
    }
    fn process(&mut self, _: &ProcessContext, buffer: &mut [f32]) {
        for sample in buffer {
            *sample *= self.0;
        }
    }
}

struct Devices;

impl DeviceFactory for Devices {
    fn create(&self, id: &str, rate: f64, asset: Option<&DeviceAsset>) -> Option<Box<dyn Native>> {
        if id == "pdc-gain" {
            Some(Box::new(Gain(1.0)))
        } else {
            suhina_device_registry::factory().create(id, rate, asset)
        }
    }
    fn graph_device(
        &self,
        graph: &DeviceGraph,
        rate: f64,
        samples: suhina_device_sdk::sample_resource::PreparedSamples,
    ) -> Result<GraphDevice, DeviceSdkError> {
        suhina_device_registry::factory().graph_device(graph, rate, samples)
    }
    fn runs_a_file(&self, _: &str) -> bool {
        false
    }
    fn preview_instrument(&self, _: f64) -> Option<Box<dyn Native>> {
        None
    }
}

struct Delay {
    samples: Vec<f32>,
    cursor: usize,
}

impl PluginProcessor for Delay {
    fn process(&mut self, buffer: &mut [f32], _: &[(usize, NoteEvent)], _: Duration) -> bool {
        if !self.samples.is_empty() {
            for sample in buffer {
                std::mem::swap(sample, &mut self.samples[self.cursor]);
                self.cursor = (self.cursor + 1) % self.samples.len();
            }
        }
        true
    }
}

struct HostedGain {
    value: f32,
    events: [ParamEvent; 512],
    len: usize,
}

impl PluginProcessor for HostedGain {
    fn automate(&mut self, events: &[ParamEvent]) {
        self.len = events.len().min(self.events.len());
        self.events[..self.len].copy_from_slice(&events[..self.len]);
    }
    fn process(&mut self, buffer: &mut [f32], _: &[(usize, NoteEvent)], _: Duration) -> bool {
        let mut events = self.events[..self.len].iter().peekable();
        for (frame, pair) in buffer.chunks_mut(2).enumerate() {
            while let Some(event) = events.next_if(|event| event.frame <= frame) {
                if event.id == 0 {
                    self.value = event.value as f32;
                }
            }
            for sample in pair {
                *sample *= self.value;
            }
        }
        true
    }
}

fn delay() -> Device {
    Device::new("Delay", DeviceKind::Clap { id: "test:pdc-delay".into() })
}

fn gain(hosted: bool, automated: bool) -> Device {
    let kind =
        if hosted { DeviceKind::Clap { id: "test:pdc-gain".into() } } else { DeviceKind::Builtin("pdc-gain".into()) };
    let mut device = Device::new("Gain", kind);
    if automated {
        device.automation.push(AutomationLane {
            param: "0".into(),
            enabled: true,
            points: [(0, 1.0), (1000, 1.0), (1000, 0.0), (1500, 0.0), (1500, 1.0)]
                .into_iter()
                .map(|(frame, value)| AutomationPoint::new(frame as f64 / 4000.0, value))
                .collect(),
        });
    }
    device
}

#[derive(Clone, Copy, Debug)]
enum Layout {
    Serial,
    Nested,
    Parallel,
    Bus,
    Return,
    Main,
}

fn group(branches: Vec<Vec<Device>>) -> Device {
    Device::group(
        "Group",
        DeviceGroup::with_branches(
            branches.into_iter().map(|devices| Branch { devices, ..Branch::new("Branch") }).collect(),
        ),
    )
}

fn fixture(hosted: bool, automated: bool, latency: usize, layout: Layout) -> (Project, AudioPool, Plugins) {
    let source = crate::fade_tests::region("pdc.wav", false, StretchMode::Repitch, 2.0);
    let mut project = crate::fade_tests::project(source, 4.0, 0.0, 0.0);
    let gate = || gain(hosted, automated);
    project.channels[0].devices = match layout {
        Layout::Serial => vec![delay(), gate()],
        Layout::Nested => vec![delay(), group(vec![vec![delay(), group(vec![vec![gate()]])]])],
        Layout::Parallel => vec![group(vec![vec![delay(), gate()], vec![gate()]])],
        Layout::Bus | Layout::Return | Layout::Main => vec![delay()],
    };
    if matches!(layout, Layout::Bus | Layout::Return | Layout::Main) {
        let role = match layout {
            Layout::Main => ChannelRole::Master,
            Layout::Return => ChannelRole::Effect,
            _ => ChannelRole::Regular,
        };
        project.channels.push(Channel { role, devices: vec![gate()], ..Channel::new(ChannelId(2), "Sum") });
        if !matches!(layout, Layout::Main) {
            project.tracks.push(Track { channel: Some(ChannelId(2)), ..Track::new(TrackId(2), "Sum") });
        }
        if matches!(layout, Layout::Bus) {
            project.channels[0].output = Some(ChannelId(2));
            project.tracks[0].parent = Some(TrackId(2));
        } else if matches!(layout, Layout::Return) {
            project.channels[0].volume = 0.0;
            project.channels[0].sends.push(Send { target: ChannelId(2), level: 1.0, pre_fader: true });
        }
    }
    project.transport.loop_range = Some(LoopRange { start: 0.0, end: LOOP as f64 / 4000.0 });
    let mut audio = AudioPool::new();
    audio
        .files
        .insert("pdc.wav".into(), Arc::new(AudioData { sample_rate: RATE, channels: 1, samples: vec![0.25; 16000] }));
    let mut plugins = Plugins::new();
    for found in crate::plugin_devices(&project) {
        let channel = project.channels.iter().find(|channel| channel.id == found.channel).unwrap();
        let stored = suhina_model::device_at(&channel.devices, &found.path).unwrap();
        let is_delay = matches!(&stored.kind, DeviceKind::Clap { id } if id == "test:pdc-delay");
        let processor: Box<dyn PluginProcessor> = if is_delay {
            Box::new(Delay { samples: vec![0.0; latency * 2], cursor: 0 })
        } else {
            Box::new(HostedGain { value: 1.0, events: [ParamEvent { frame: 0, id: 0, value: 0.0 }; 512], len: 0 })
        };
        plugins.insert(
            found.key,
            PluginSlotInfo { instrument: false, latency: if is_delay { latency as u32 } else { 0 } },
            processor,
        );
    }
    (project, audio, plugins)
}

fn render(hosted: bool, automated: bool, latency: usize, block: usize, looping: bool, layout: Layout) -> Vec<f32> {
    let (mut project, audio, plugins) = fixture(hosted, automated, latency, layout);
    project.transport.loop_enabled = looping;
    let mut renderer = Renderer::build(
        &project,
        RenderInputs::from_audio(&audio, Arc::new(Devices)).with_plugins(plugins),
        RATE,
        RenderOptions::PLAYING,
    )
    .unwrap();
    renderer.set_offline(true);
    let mut out = vec![0.0; 6144 * 2];
    for chunk in out.chunks_mut(block * 2) {
        renderer.process(chunk);
    }
    out
}

fn check(hosted: bool, layout: Layout) {
    for looping in [false, true] {
        for latency in [0, 1, 257, 1500] {
            for block in [1, 31, 64, 257, 1024] {
                let plain = render(hosted, false, latency, block, looping, layout);
                let actual = render(hosted, true, latency, block, looping, layout);
                assert!(plain.iter().any(|sample| *sample > 0.01));
                for (frame, (got, input)) in actual.as_chunks::<2>().0.iter().zip(plain.as_chunks::<2>().0).enumerate()
                {
                    let total = latency * if matches!(layout, Layout::Nested) { 2 } else { 1 };
                    let source = frame.saturating_sub(total);
                    let source = if looping { source % LOOP } else { source };
                    let value = if (1000..1500).contains(&source) { 0.0 } else { 1.0 };
                    for channel in 0..2 {
                        let want = input[channel] * value;
                        assert!(
                            (got[channel] - want).abs() < 1e-6,
                            "{layout:?}, hosted {hosted}, loop {looping}, latency {latency}, block {block}, frame {frame}: {} != {want}",
                            got[channel]
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn pdc_hosted_automation_follows_delayed_audio() {
    check(true, Layout::Serial);
}

#[test]
fn pdc_native_automation_follows_delayed_audio() {
    check(false, Layout::Serial);
}

#[test]
fn pdc_automation_in_groups_buses_returns_and_main_follows_the_input_sum() {
    for layout in [Layout::Nested, Layout::Parallel, Layout::Bus, Layout::Return, Layout::Main] {
        for hosted in [false, true] {
            check(hosted, layout);
        }
    }
}

#[test]
fn pdc_manual_changes_remain_immediate_after_a_delaying_device() {
    for hosted in [false, true] {
        let (project, audio, plugins) = fixture(hosted, false, 1500, Layout::Serial);
        let mut renderer = Renderer::build(
            &project,
            RenderInputs::from_audio(&audio, Arc::new(Devices)).with_plugins(plugins),
            RATE,
            RenderOptions::PLAYING,
        )
        .unwrap();
        renderer.set_offline(true);
        let mut controller = renderer.controller();
        let mut block = vec![0.0; 256 * 2];
        for _ in 0..8 {
            renderer.process(&mut block);
        }
        assert!(block.iter().all(|sample| *sample > 0.01));
        controller.set_device_param(TrackId(1), 1, 0, 0.0).unwrap();
        renderer.process(&mut block);
        assert!(block.iter().all(|sample| *sample == 0.0), "hosted {hosted}: manual value waited for PDC");
    }
}

#[test]
fn pdc_history_keeps_seek_and_stop_offsets_inside_a_delayed_block() {
    use crate::plugin::SongHistory;
    let mut songs = SongHistory::new(RATE);
    let playing = |position| crate::SongPosition { position, playing: true, ..Default::default() };
    songs.record(playing(900), 64);
    songs.record(playing(4000), 32); // seek
    songs.record(crate::SongPosition { position: 100, ..Default::default() }, 16); // stop
    songs.record(playing(100), 128); // resume
    let spans: Vec<_> = songs.spans(97, 128).map(|s| (s.offset, s.frames, s.time, s.parked)).collect();
    assert_eq!(
        spans,
        [(0, 49, Some(915), 915), (49, 32, Some(4000), 4000), (81, 16, None, 100), (97, 31, Some(100), 100)]
    );
}

#[test]
fn pdc_history_joins_old_block_boundaries_and_holds_negative_positions() {
    use crate::plugin::SongHistory;
    let mut songs = SongHistory::new(RATE);
    for position in 0..1024 {
        songs.record(crate::SongPosition { position, playing: true, ..Default::default() }, 1);
    }
    songs.record(crate::SongPosition { position: 1024, playing: true, ..Default::default() }, 1024);
    let spans: Vec<_> = songs.spans(1024, 1024).collect();
    assert_eq!(spans.len(), 1, "history boundaries must not consume parameter event capacity");
    assert_eq!((spans[0].time, spans[0].frames), (Some(0), 1024));
    songs.clear();
    songs.record(crate::SongPosition { playing: true, ..Default::default() }, 128);
    let spans: Vec<_> = songs.spans(33, 128).map(|s| (s.offset, s.frames, s.time, s.advancing)).collect();
    assert_eq!(spans, [(0, 33, Some(0), false), (33, 95, Some(0), true)]);
}

#[test]
fn pdc_delayed_automation_planning_allocates_nothing() {
    for hosted in [false, true] {
        let (project, audio, plugins) = fixture(hosted, true, 257, Layout::Nested);
        let mut renderer = Renderer::build(
            &project,
            RenderInputs::from_audio(&audio, Arc::new(Devices)).with_plugins(plugins),
            RATE,
            RenderOptions::PLAYING,
        )
        .unwrap();
        renderer.set_offline(true);
        let mut block = [0.0; 62];
        let allocation = allocation_counter::measure(|| {
            for _ in 0..128 {
                renderer.process(&mut block);
            }
        });
        assert_eq!(allocation.count_total, 0, "hosted {hosted}: {allocation:?}");
        assert_eq!(allocation.count_current, 0, "hosted {hosted}: {allocation:?}");
    }
}

#[test]
fn pdc_parameter_events_keep_discontinuity_and_subblock_offsets() {
    let mut songs = crate::plugin::SongHistory::new(RATE);
    let playing = |position| crate::SongPosition { position, playing: true, ..Default::default() };
    songs.record(playing(975), 64);
    songs.record(playing(4000), 32);
    songs.record(crate::SongPosition { position: 100, ..Default::default() }, 16);
    songs.record(playing(1000), 128);
    let mut params = crate::plugin_params::SlotParams::new(&gain(true, true), 4000.0, &[]);
    params.set(17, 0.5);
    params.plan_delayed(&songs, 97, 128);
    assert_eq!(params.block().iter().filter(|event| event.id == 17).count(), 1, "manual value sent once");
    for (frame, value) in [(0, 1.0), (10, 0.0), (49, 1.0), (97, 0.0)] {
        assert!(
            params.block().iter().any(|event| event.id == 0 && event.frame == frame && event.value == value),
            "missing {frame}: {value} in {:?}",
            params.block()
        );
    }
    assert!(params.span(80, 128).iter().any(|event| event.id == 0 && event.frame == 17 && event.value == 0.0));

    let mut lanes = crate::automation::DeviceLanes::none();
    lanes.add(0, &Gain(1.0), &gain(false, true).automation, 4000.0);
    lanes.plan_delayed(&songs, 97, 128);
    let changes: Vec<_> = lanes.changes(0, 0, 128).map(|change| (change.frame, change.value)).collect();
    assert_eq!(changes, [(0, 1.0), (10, 0.0), (49, 1.0), (97, 0.0)]);
}

#[test]
fn pdc_stopped_editor_values_do_not_wait_for_silent_history() {
    let mut songs = crate::plugin::SongHistory::new(RATE);
    let mut params = crate::plugin_params::SlotParams::new(&gain(true, true), 4000.0, &[]);
    songs.record(crate::SongPosition::default(), 1024);
    params.plan_delayed(&songs, 500, 1024);
    assert!(params.block().is_empty(), "opening keeps the saved state");
    songs.record(crate::SongPosition { position: 1100, ..Default::default() }, 64);
    params.plan_delayed(&songs, 500, 64);
    assert_eq!(params.block(), [ParamEvent { frame: 0, id: 0, value: 0.0 }]);
}

#[test]
fn pdc_automation_adopts_new_upstream_latency_with_running_instances() {
    for hosted in [false, true] {
        for layout in [Layout::Serial, Layout::Nested, Layout::Bus, Layout::Return, Layout::Main] {
            let (project, audio, mut plugins) = fixture(hosted, true, 257, layout);
            let reported = plugins.info.clone();
            for info in plugins.info.values_mut() {
                info.latency = 0;
            }
            let mut renderer = Renderer::build(
                &project,
                RenderInputs::from_audio(&audio, Arc::new(Devices)).with_plugins(plugins),
                RATE,
                RenderOptions::PLAYING,
            )
            .unwrap();
            renderer.set_offline(true);
            let mut controller = renderer.controller();
            let mut output = vec![0.0; 4096 * 2];
            renderer.process(&mut output[..512 * 2]);
            controller.set_plugin_info(reported);
            controller.update(&project).unwrap();
            for block in output[512 * 2..].chunks_mut(31 * 2) {
                renderer.process(block);
            }
            let total = if matches!(layout, Layout::Nested) { 514 } else { 257 };
            let off = (1000 + total) * 2;
            let on = (1500 + total) * 2;
            assert!(output[off - 2] > 0.01, "{layout:?} hosted {hosted}: early transition");
            assert!(output[off..on].iter().all(|sample| *sample == 0.0), "{layout:?} hosted {hosted}: late transition");
            assert!(output[on] > 0.01, "{layout:?} hosted {hosted}: late return");
        }
    }
}
