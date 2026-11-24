// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! PDC against independently specified gain steps, with allocation-free test processors.

#[path = "pdc_context_tests.rs"]
mod context;

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

/// A self-authored processor using VST3's piecewise-linear parameter-queue semantics.
struct InterpolatingGain(HostedGain);

impl PluginProcessor for InterpolatingGain {
    fn automate(&mut self, events: &[ParamEvent]) {
        self.0.automate(events);
    }

    fn process(&mut self, buffer: &mut [f32], _: &[(usize, NoteEvent)], _: Duration) -> bool {
        // These fixtures must fit the real host's per-parameter queue without compaction.
        assert!(self.0.len <= 16);
        let mut events = self.0.events[..self.0.len].iter().peekable();
        let (mut at, mut value) = (-1.0, f64::from(self.0.value));
        for (frame, pair) in buffer.chunks_mut(2).enumerate() {
            while let Some(event) = events.next_if(|event| event.frame <= frame) {
                assert_eq!(event.id, 0);
                (at, value) = (event.frame as f64, event.value);
            }
            let gain = events
                .peek()
                .map_or(value, |next| value + (next.value - value) * (frame as f64 - at) / (next.frame as f64 - at));
            for sample in pair {
                *sample *= gain as f32;
            }
            self.0.value = gain as f32;
        }
        true
    }
}

fn interpolating_gain() -> InterpolatingGain {
    InterpolatingGain(HostedGain { value: 1.0, events: [ParamEvent { frame: 0, id: 0, value: 0.0 }; 512], len: 0 })
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
    let plugins = processors(&project, latency);
    (project, audio, plugins)
}

/// The test processors of `project`'s plug-ins: each delay `latency` frames late, each gain at
/// 1 until its events move it.
fn processors(project: &Project, latency: usize) -> Plugins {
    let mut plugins = Plugins::new();
    for found in crate::plugin_devices(project) {
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
    plugins
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

fn render_interpolated_gain(automated: bool, latency: usize, block: usize, seek: bool) -> Vec<f32> {
    let (mut project, audio, mut plugins) = fixture(true, false, latency, Layout::Serial);
    project.transport.loop_enabled = !seek;
    if automated {
        // A continuous ramp over the first sample, then a plateau. At a loop/seek to zero
        // the old plateau must last until the delayed source actually reaches zero.
        project.channels[0].devices[1].automation.push(AutomationLane {
            param: "0".into(),
            enabled: true,
            points: vec![AutomationPoint::new(0.0, 0.0), AutomationPoint::new(1.0 / 4000.0, 1.0)].into(),
        });
    }
    let key = crate::plugin_devices(&project).into_iter().find(|device| device.path == [1]).unwrap().key;
    plugins.insert(key, PluginSlotInfo::default(), Box::new(interpolating_gain()));
    let mut renderer = Renderer::build(
        &project,
        RenderInputs::from_audio(&audio, Arc::new(Devices)).with_plugins(plugins),
        RATE,
        RenderOptions::PLAYING,
    )
    .unwrap();
    renderer.set_offline(true);
    let mut out = vec![0.0; 6144 * 2];
    for (part, samples) in out.chunks_mut(1536 * 2).enumerate() {
        if seek && part == 1 {
            renderer.handle(crate::LaunchCommand::SetPosition { beat: 0.0 });
        }
        for chunk in samples.chunks_mut(block * 2) {
            renderer.process(chunk);
        }
    }
    out
}

fn check_interpolated_gain(seek: bool) {
    for latency in [0, 1, 257, 1500] {
        for block in [1, 31, 257, 512, 1024] {
            let plain = render_interpolated_gain(false, latency, block, seek);
            let actual = render_interpolated_gain(true, latency, block, seek);
            assert!(plain.iter().any(|sample| *sample > 0.01));
            for (frame, (got, input)) in actual.as_chunks::<2>().0.iter().zip(plain.as_chunks::<2>().0).enumerate() {
                let source = frame.saturating_sub(latency);
                let source = if seek { if source >= 1536 { source - 1536 } else { source } } else { source % LOOP };
                let gain = if source == 0 { 0.0 } else { 1.0 };
                for channel in 0..2 {
                    let want = input[channel] * gain;
                    assert!(
                        (got[channel] - want).abs() < 1e-6,
                        "seek {seek}, latency {latency}, block {block}, frame {frame}: {} != {want}",
                        got[channel]
                    );
                }
            }
        }
    }
}

#[test]
fn pdc_interpolated_gain_preserves_delayed_loop_boundaries() {
    check_interpolated_gain(false);
}

#[test]
fn pdc_interpolated_gain_preserves_delayed_seek_boundaries() {
    check_interpolated_gain(true);
}

#[test]
fn pdc_interpolated_gain_keeps_boundary_values_in_subblocks() {
    let mut songs = crate::plugin::SongHistory::new(RATE);
    let playing = |position| crate::SongPosition { position, playing: true, ..Default::default() };
    songs.record(playing(512), 512);
    songs.record(playing(0), 512);
    let mut device = gain(true, false);
    device.automation.push(AutomationLane {
        param: "0".into(),
        enabled: true,
        points: vec![AutomationPoint::new(0.0, 0.0), AutomationPoint::new(100.0 / 4000.0, 1.0)].into(),
    });
    let mut params = crate::plugin_params::SlotParams::new(&device, 4000.0, &[]);
    params.plan_delayed(&songs, 257, 512);
    for split in [0, 128, 256, 257, 258, 512] {
        let mut processor = interpolating_gain();
        let mut output = [1.0; 512 * 2];
        for (from, to) in [(0, split), (split, 512)] {
            processor.automate(params.span(from, to));
            processor.process(&mut output[from * 2..to * 2], &[], Duration::ZERO);
        }
        // The independent reference is the end of the old plateau followed by the ramp.
        // Check the plateau and the exact jump; ordinary ramp subdivision is separate.
        assert!(output[..257 * 2].iter().all(|sample| *sample == 1.0), "split {split}: early ramp");
        assert_eq!(&output[257 * 2..258 * 2], &[0.0, 0.0], "split {split}: late jump");
    }
}

#[test]
fn pdc_historical_span_endpoint_stays_within_the_lane_event_budget() {
    let mut songs = crate::plugin::SongHistory::new(RATE);
    songs.record(crate::SongPosition { playing: true, ..Default::default() }, 128);
    songs.record(crate::SongPosition { position: 1000, playing: true, ..Default::default() }, 128);
    let mut device = gain(true, false);
    device.automation.push(AutomationLane {
        param: "0".into(),
        enabled: true,
        points: (0..200).map(|frame| AutomationPoint::new(f64::from(frame), f64::from(frame) / 256.0)).collect(),
    });
    let mut params = crate::plugin_params::SlotParams::new(&device, 1.0, &[]);
    params.plan_delayed(&songs, 65, 128);
    let before: Vec<_> = params.block().iter().filter(|event| event.frame < 65).collect();
    assert!(before.len() <= 32, "the boundary uses the existing per-lane event budget");
    assert_eq!(before.last().copied(), Some(&ParamEvent { frame: 64, id: 0, value: 127.0 / 256.0 }));
}

#[test]
fn pdc_later_span_starts_survive_a_full_parameter_buffer() {
    let mut songs = crate::plugin::SongHistory::new(RATE);
    songs.record(crate::SongPosition { playing: true, ..Default::default() }, 64);
    songs.record(crate::SongPosition { position: 1000, playing: true, ..Default::default() }, 64);
    for dense in [false, true] {
        let mut device = gain(true, false);
        device.automation = (0..256)
            .map(|id| AutomationLane {
                param: id.to_string(),
                enabled: true,
                points: if dense {
                    (0..64).map(|frame| AutomationPoint::new(f64::from(frame), f64::from(frame % 2))).collect()
                } else {
                    vec![
                        AutomationPoint::new(0.0, 0.0),
                        AutomationPoint::new(64.0, 0.0),
                        AutomationPoint::new(65.0, 1.0),
                    ]
                    .into()
                },
            })
            .collect();
        let mut params = crate::plugin_params::SlotParams::new(&device, 1.0, &[]);
        let allocation = allocation_counter::measure(|| params.plan_delayed(&songs, 33, 64));
        assert_eq!(allocation.count_total, 0);
        assert_eq!(params.block().len(), crate::MAX_BLOCK_PARAMS);
        for id in 0..256 {
            assert!(params.block().iter().any(|event| event.id == id && event.frame == 0));
            assert!(params.block().contains(&ParamEvent { frame: 33, id, value: 1.0 }), "dense {dense}, lane {id}");
        }
    }
}

#[test]
fn pdc_already_sent_stopped_value_holds_until_resume() {
    for delay in [1, 33] {
        let mut songs = crate::plugin::SongHistory::new(RATE);
        let mut device = gain(true, false);
        device.automation.push(AutomationLane {
            param: "0".into(),
            enabled: true,
            points: vec![AutomationPoint::new(0.0, 0.0), AutomationPoint::new(100.0, 1.0)].into(),
        });
        let mut params = crate::plugin_params::SlotParams::new(&device, 1.0, &[]);
        let mut processor = interpolating_gain();
        // Open stopped at another position, then move to 100 and send its value once.
        params.plan(None, 0, 64);
        params.plan(None, 100, 64);
        processor.automate(params.block());
        processor.process(&mut [1.0; 128], &[], Duration::ZERO);
        songs.record(crate::SongPosition { position: 100, ..Default::default() }, 64);
        songs.record(crate::SongPosition { playing: true, ..Default::default() }, 64);
        params.plan_delayed(&songs, delay, 64);
        processor.automate(params.block());
        let mut output = [1.0; 128];
        processor.process(&mut output, &[], Duration::ZERO);
        let boundary = delay as usize * 2;
        assert!(output[..boundary].iter().all(|sample| *sample == 1.0), "delay {delay}: early ramp");
        assert_eq!(&output[boundary..boundary + 2], &[0.0, 0.0], "delay {delay}: late resume");
    }
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
    for (frame, value) in [(0, 1.0), (10, 0.0), (48, 0.0), (49, 1.0), (80, 1.0), (96, 1.0), (97, 0.0)] {
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

/// Every frame deliberately has a different source position, tempo or transport state.
/// An implementation that extrapolates an evicted record cannot reproduce this sequence.
fn history_song(frame: u32) -> crate::SongPosition {
    crate::SongPosition {
        playing: !frame.is_multiple_of(17),
        position: u64::from((frame * 7) % 4096 + 1000),
        tempo: if frame.is_multiple_of(3) { 90.0 } else { 150.0 },
        beat: f64::from(frame % 32),
        bar: frame % 8,
        bar_start: f64::from(frame % 8) * 4.0,
        numerator: if frame.is_multiple_of(5) { 3 } else { 4 },
        denominator: 4,
        cycle: Some((0.0, 32.0)),
        cycle_seconds: Some((0.0, 16.0)),
    }
}

#[test]
fn pdc_history_covers_the_horizon_after_one_frame_callbacks_and_ring_wraps() {
    for rate in [8_000, 44_100, 48_000, 96_000, 192_000, crate::MAX_SAMPLE_RATE] {
        let horizon = rate * crate::MAX_LATENCY_SECONDS;
        let mut history = crate::plugin::SongHistory::new(rate);
        // Cross the full ring twice. The exact horizon must still address its real record.
        for now in 0..=horizon * 2 + 3 {
            history.record(history_song(now), 1);
            if now >= horizon && (now.is_multiple_of(8192) || now == horizon * 2 + 3 || now == horizon) {
                let allocations = allocation_counter::measure(|| {
                    for delay in [0, 1, 8192, horizon - 1, horizon] {
                        let expected = history_song(now - delay);
                        assert_eq!(history.heard(delay), expected, "rate {rate}, now {now}, delay {delay}");
                        let span = history.spans(delay, 1).next().unwrap();
                        assert_eq!(span.parked, expected.position);
                        assert_eq!(span.time, expected.playing.then_some(expected.position));
                    }
                });
                assert_eq!(allocations.count_total, 0);
            }
        }
    }
}

#[test]
fn pdc_empty_callbacks_cannot_evict_history_and_clear_restarts_priming() {
    let horizon = RATE * crate::MAX_LATENCY_SECONDS;
    let mut history = crate::plugin::SongHistory::new(RATE);
    let allocations = allocation_counter::measure(|| {
        for frame in 0..=horizon {
            history.record(history_song(frame), 1);
        }
        for _ in 0..=horizon {
            history.record(crate::SongPosition::default(), 0);
        }
        assert_eq!(history.heard(horizon), history_song(0));
        assert_eq!(history.heard(0), history_song(horizon));
        history.clear();
        assert_eq!(history.heard(horizon), crate::SongPosition::default());
        let initial = crate::SongPosition { playing: true, position: 100, ..Default::default() };
        history.record(initial, 1);
        assert_eq!(history.heard(horizon), initial.earlier(horizon, f64::from(RATE)));
        let span = history.spans(horizon, 1).next().unwrap();
        assert_eq!(span.time, Some(0));
        assert!(!span.advancing);
    });
    assert_eq!(allocations.count_total, 0);
}

fn render_at_horizon(hosted: bool, automated: bool, layout: Layout, blocks: &[usize]) -> Vec<f32> {
    let total = (RATE * crate::MAX_LATENCY_SECONDS) as usize;
    let latency = if matches!(layout, Layout::Nested) { total / 2 } else { total };
    let (mut project, audio, plugins) = fixture(hosted, automated, latency, layout);
    project.transport.loop_enabled = true;
    let mut renderer = Renderer::build(
        &project,
        RenderInputs::from_audio(&audio, Arc::new(Devices)).with_plugins(plugins),
        RATE,
        RenderOptions::PLAYING,
    )
    .unwrap();
    renderer.set_offline(true);
    let mut out = vec![0.0; (total * 2 + LOOP) * 2];
    let mut at = 0;
    let mut block = 0;
    let allocations = allocation_counter::measure(|| {
        while at < out.len() {
            let end = (at + blocks[block % blocks.len()] * 2).min(out.len());
            renderer.process(&mut out[at..end]);
            at = end;
            block += 1;
        }
    });
    assert_eq!(allocations.count_total, 0);
    assert!(block > 8192, "exercise the former history limit");
    out
}

#[test]
fn pdc_long_delay_automation_matches_reference_after_history_wraps() {
    let horizon = (RATE * crate::MAX_LATENCY_SECONDS) as usize;
    for hosted in [false, true] {
        for (layout, blocks) in [(Layout::Serial, &[1][..]), (Layout::Nested, &[1, 2, 1, 3][..])] {
            let plain = render_at_horizon(hosted, false, layout, blocks);
            let actual = render_at_horizon(hosted, true, layout, blocks);
            assert!(plain[horizon * 2..].iter().any(|sample| *sample > 0.01));
            for (frame, (got, input)) in actual.as_chunks::<2>().0.iter().zip(plain.as_chunks::<2>().0).enumerate() {
                let source = frame.saturating_sub(horizon) % LOOP;
                let gain = if (1000..1500).contains(&source) { 0.0 } else { 1.0 };
                for channel in 0..2 {
                    assert!(
                        (got[channel] - input[channel] * gain).abs() < 1e-6,
                        "{layout:?}, hosted {hosted}, frame {frame}"
                    );
                }
            }
        }
    }
}

fn latency_inputs(project: &Project, audio: &AudioPool, latencies: &[u32]) -> RenderInputs {
    let devices = crate::plugin_devices(project);
    assert_eq!(devices.len(), latencies.len());
    let mut plugins = Plugins::new();
    for (found, &latency) in devices.iter().zip(latencies) {
        plugins.set_info(found.key, PluginSlotInfo { instrument: false, latency });
    }
    RenderInputs::from_audio(audio, Arc::new(Devices)).with_plugins(plugins)
}

#[test]
fn pdc_limit_checks_the_complete_path_without_clamping() {
    let limit = RATE * crate::MAX_LATENCY_SECONDS;
    for layout in [Layout::Serial, Layout::Parallel, Layout::Nested, Layout::Bus, Layout::Return, Layout::Main] {
        let (mut project, audio, _) = fixture(false, false, 0, layout);
        if !matches!(layout, Layout::Nested) {
            project.channels.last_mut().unwrap().devices.push(delay());
        }
        for latencies in
            [[0, limit], [limit, 0], [limit / 2, limit / 2], [1, limit], [0, limit + 1], [u32::MAX, 1], [1, u32::MAX]]
        {
            let total = latencies[0].saturating_add(latencies[1]);
            let result =
                Renderer::build(&project, latency_inputs(&project, &audio, &latencies), RATE, RenderOptions::PLAYING);
            if total <= limit {
                assert!(result.is_ok(), "{layout:?}, {latencies:?}");
            } else {
                let error = result.err().unwrap();
                assert!(
                    matches!(error, crate::RenderError::LatencyLimit { frames, limit: cap, .. } if frames > cap && cap == limit),
                    "{layout:?}, {latencies:?}: {error}"
                );
                let message = error.to_string();
                assert!(message.contains("16000 frames (2 seconds)"));
                assert!(message.contains("reduce device latency or bypass a device"));
                if matches!(layout, Layout::Main) && latencies[0] <= limit {
                    assert!(message.contains("ending at Main"));
                }
            }
        }
    }
}

#[test]
fn pdc_parallel_branches_use_the_maximum_and_disabled_latency_does_not_count() {
    let limit = RATE * crate::MAX_LATENCY_SECONDS;
    let (mut project, audio, _) = fixture(false, false, 0, Layout::Serial);
    project.channels[0].devices = vec![group(vec![vec![delay()], vec![group(vec![vec![delay()]])]])];
    let renderer =
        Renderer::build(&project, latency_inputs(&project, &audio, &[limit, limit]), RATE, RenderOptions::PLAYING)
            .unwrap();
    assert_eq!(renderer.latency(), limit);
    let mut disabled = delay();
    disabled.enabled = false;
    project.channels[0].devices = vec![disabled];
    let renderer =
        Renderer::build(&project, latency_inputs(&project, &audio, &[u32::MAX]), RATE, RenderOptions::PLAYING).unwrap();
    assert_eq!(renderer.latency(), 0);
}

#[test]
fn pdc_latency_limit_scales_with_rate_and_invalid_rates_are_rejected() {
    let (project, audio, _) = fixture(false, false, 0, Layout::Serial);
    for rate in [8_000, 44_100, 48_000, 96_000, 192_000, crate::MAX_SAMPLE_RATE] {
        let limit = rate * crate::MAX_LATENCY_SECONDS;
        let renderer =
            Renderer::build(&project, latency_inputs(&project, &audio, &[limit]), rate, RenderOptions::PLAYING)
                .unwrap();
        assert_eq!(renderer.latency(), limit);
        let error =
            Renderer::build(&project, latency_inputs(&project, &audio, &[limit + 1]), rate, RenderOptions::PLAYING)
                .err()
                .unwrap();
        assert_eq!(error, crate::RenderError::LatencyLimit { track: TrackId(1), frames: limit + 1, limit });
    }
    for rate in [0, 7_999, crate::MAX_SAMPLE_RATE + 1, u32::MAX] {
        let error =
            Renderer::build(&project, latency_inputs(&project, &audio, &[u32::MAX]), rate, RenderOptions::PLAYING)
                .err()
                .unwrap();
        assert_eq!(error, crate::RenderError::InvalidSampleRate(rate));
    }
}

#[test]
fn pdc_rejected_update_keeps_the_running_graph_and_allows_a_corrected_retry() {
    let (project, audio, plugins) = fixture(false, false, 257, Layout::Serial);
    let valid_info = plugins.info.clone();
    let mut renderer = Renderer::build(
        &project,
        RenderInputs::from_audio(&audio, Arc::new(Devices)).with_plugins(plugins),
        RATE,
        RenderOptions::PLAYING,
    )
    .unwrap();
    renderer.set_offline(true);
    let mut controller = renderer.controller();
    let mut out = vec![0.0; 6144 * 2];
    renderer.process(&mut out[..1024 * 2]);
    let mut invalid_info = valid_info.clone();
    for info in invalid_info.values_mut() {
        info.latency = RATE * crate::MAX_LATENCY_SECONDS + 1;
    }
    controller.set_plugin_info(invalid_info);
    assert!(matches!(
        controller.update(&project),
        Err(crate::ControlError::Build(crate::RenderError::LatencyLimit { .. }))
    ));
    renderer.process(&mut out[1024 * 2..2048 * 2]);
    assert_eq!(renderer.latency(), 257);
    controller.set_plugin_info(valid_info);
    controller.update(&project).unwrap();
    renderer.process(&mut out[2048 * 2..]);
    let reference = render(false, false, 257, 1024, false, Layout::Serial);
    assert_eq!(out, reference);
}

/// Where a mixer lane plays, against which device delays the audio it scales.
#[derive(Clone, Copy, Debug)]
enum Fader {
    /// The track's own lane, after its own delay.
    Track,
    /// Main's lane, the delay on the track that plays into it.
    MainAfterTrack,
    /// Main's lane, the delay on Main itself.
    MainAfterMain,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Motion {
    Straight,
    Loop,
    /// A jump back to the start 1536 frames in.
    Seek,
}

/// A mute lane: heard, muted from frame 1008 to 1504, heard again; both points on the mixer's
/// 16-frame grid.
fn mute_lane() -> AutomationLane {
    AutomationLane {
        param: suhina_model::automation::MUTE.into(),
        enabled: true,
        points: [(0, 0.0), (1008, 1.0), (1504, 0.0)]
            .into_iter()
            .map(|(frame, value)| AutomationPoint::new(f64::from(frame) / 4000.0, value))
            .collect(),
    }
}

/// The factor `mute_lane` scales the audio from arrangement frame `source` by, written from the
/// mixer's rule rather than from the engine: the grid's values, in a straight line between.
fn muted(source: usize) -> f32 {
    match source {
        s if s < 992 => 1.0,
        s if s < 1008 => 1.0 - (s - 992) as f32 / 16.0,
        s if s < 1488 => 0.0,
        s if s < 1504 => (s - 1488) as f32 / 16.0,
        _ => 1.0,
    }
}

fn render_mixer(lane: bool, latency: usize, block: usize, motion: Motion, fader: Fader) -> Vec<f32> {
    let source = crate::fade_tests::region("pdc.wav", false, StretchMode::Repitch, 2.0);
    let mut project = crate::fade_tests::project(source, 4.0, 0.0, 0.0);
    let mut main = Channel { role: ChannelRole::Master, ..Channel::new(ChannelId(2), "Main") };
    match fader {
        Fader::Track | Fader::MainAfterTrack => project.channels[0].devices = vec![delay()],
        Fader::MainAfterMain => main.devices = vec![delay()],
    }
    if lane {
        let channel = if matches!(fader, Fader::Track) { &mut project.channels[0] } else { &mut main };
        channel.automation.push(mute_lane());
    }
    project.channels.push(main);
    project.transport.loop_range = Some(LoopRange { start: 0.0, end: LOOP as f64 / 4000.0 });
    project.transport.loop_enabled = motion == Motion::Loop;
    let mut audio = AudioPool::new();
    audio
        .files
        .insert("pdc.wav".into(), Arc::new(AudioData { sample_rate: RATE, channels: 1, samples: vec![0.25; 16000] }));
    let plugins = processors(&project, latency);
    let mut renderer = Renderer::build(
        &project,
        RenderInputs::from_audio(&audio, Arc::new(Devices)).with_plugins(plugins),
        RATE,
        RenderOptions::PLAYING,
    )
    .unwrap();
    renderer.set_offline(true);
    let mut out = vec![0.0; 6144 * 2];
    for (part, samples) in out.chunks_mut(1536 * 2).enumerate() {
        if motion == Motion::Seek && part == 1 {
            renderer.handle(crate::LaunchCommand::SetPosition { beat: 0.0 });
        }
        for chunk in samples.chunks_mut(block * 2) {
            renderer.process(chunk);
        }
    }
    out
}

/// Mixer lanes scale the audio they reach at its own place in the arrangement, however late the
/// devices before them make it: on a track, and on Main after a track's or its own delay, across
/// a loop's wrap and a jump (#1703). The reference is written from the lane, not taken from
/// another engine mode.
#[test]
fn pdc_mixer_lanes_follow_the_delayed_audio() {
    for fader in [Fader::Track, Fader::MainAfterTrack, Fader::MainAfterMain] {
        for motion in [Motion::Straight, Motion::Loop, Motion::Seek] {
            for latency in [0, 1, 257, 1500] {
                for block in [1, 31, 257, 1024] {
                    let plain = render_mixer(false, latency, block, motion, fader);
                    let actual = render_mixer(true, latency, block, motion, fader);
                    assert!(plain.iter().any(|sample| *sample > 0.01));
                    for (frame, (got, input)) in
                        actual.as_chunks::<2>().0.iter().zip(plain.as_chunks::<2>().0).enumerate()
                    {
                        let source = frame.saturating_sub(latency);
                        let source = match motion {
                            Motion::Straight => source,
                            Motion::Loop => source % LOOP,
                            Motion::Seek if source >= 1536 => source - 1536,
                            Motion::Seek => source,
                        };
                        for channel in 0..2 {
                            let want = input[channel] * muted(source);
                            assert!(
                                (got[channel] - want).abs() < 1e-6,
                                "{fader:?}, {motion:?}, latency {latency}, block {block}, frame {frame}: {} != {want}",
                                got[channel]
                            );
                        }
                    }
                }
            }
        }
    }
}
