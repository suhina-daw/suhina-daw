// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Native audio effects follow their input's transport without needing parameter lanes.

use super::*;

struct BeatGate {
    rate: f64,
    context: ProcessContext,
}

impl Native for BeatGate {
    fn info(&self) -> &DeviceInfo {
        &GAIN
    }
    fn prepare(&mut self, rate: f64) -> Result<(), DeviceError> {
        self.rate = rate;
        Ok(())
    }
    fn reset(&mut self) {}
    fn set_param(&mut self, _: usize, _: f64) {}
    fn param(&self, _: usize) -> Option<f64> {
        Some(1.0)
    }
    fn set_context(&mut self, context: &ProcessContext) {
        self.context = *context;
    }
    fn process(&mut self, context: &ProcessContext, buffer: &mut [f32]) {
        assert_eq!(*context, self.context);
        for (frame, pair) in buffer.chunks_mut(2).enumerate() {
            let beat =
                context.beat + if context.playing { frame as f64 * context.tempo / (60.0 * self.rate) } else { 0.0 };
            // Snap the independently defined gate to a 1/4000-beat grid. This avoids a
            // floating-point value just below a boundary moving an edge by one sample.
            if (1000..1500).contains(&((beat * 4000.0).round() as i64)) {
                pair.fill(0.0);
            }
        }
    }
}

struct TimedDevices;

/// Encodes the context in the output so sub-block dispatch, not only its prepared plan,
/// can be compared with the declared historical records.
struct ContextProbe {
    rate: f64,
    context: ProcessContext,
}

impl Native for ContextProbe {
    fn info(&self) -> &DeviceInfo {
        &GAIN
    }
    fn prepare(&mut self, rate: f64) -> Result<(), DeviceError> {
        self.rate = rate;
        Ok(())
    }
    fn reset(&mut self) {}
    fn set_param(&mut self, _: usize, _: f64) {}
    fn param(&self, _: usize) -> Option<f64> {
        Some(1.0)
    }
    fn set_context(&mut self, context: &ProcessContext) {
        self.context = *context;
    }
    fn process(&mut self, context: &ProcessContext, buffer: &mut [f32]) {
        assert_eq!(*context, self.context);
        for (frame, pair) in buffer.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            pair[0] = (context.beat
                + if context.playing { frame as f64 * context.tempo / (60.0 * self.rate) } else { 0.0 })
                as f32;
            pair[1] = (if context.playing { context.tempo } else { -context.tempo }) as f32;
        }
    }
}

impl DeviceFactory for TimedDevices {
    fn create(&self, id: &str, rate: f64, asset: Option<&DeviceAsset>) -> Option<Box<dyn Native>> {
        if id == "pdc-gain" {
            Some(Box::new(BeatGate { rate, context: ProcessContext::default() }))
        } else {
            Devices.create(id, rate, asset)
        }
    }
    fn graph_device(
        &self,
        graph: &DeviceGraph,
        rate: f64,
        samples: suhina_device_sdk::sample_resource::PreparedSamples,
    ) -> Result<GraphDevice, DeviceSdkError> {
        Devices.graph_device(graph, rate, samples)
    }
    fn runs_a_file(&self, _: &str) -> bool {
        false
    }
    fn preview_instrument(&self, _: f64) -> Option<Box<dyn Native>> {
        None
    }
}

fn timed_render(latency: usize, block: usize, layout: Layout, seek: bool) -> Vec<f32> {
    let (mut project, audio, plugins) = fixture(false, false, latency, layout);
    project.transport.loop_enabled = !seek;
    let mut renderer = Renderer::build(
        &project,
        RenderInputs::from_audio(&audio, Arc::new(TimedDevices)).with_plugins(plugins),
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

#[test]
fn native_effect_contexts_follow_delayed_audio_across_loops_and_routing() {
    for layout in [Layout::Serial, Layout::Nested, Layout::Parallel, Layout::Bus, Layout::Return, Layout::Main] {
        for latency in [0, 1, 257, 1500] {
            for block in [1, 31, 1024] {
                let plain = render(false, false, latency, block, true, layout);
                let actual = timed_render(latency, block, layout, false);
                let total = latency * if matches!(layout, Layout::Nested) { 2 } else { 1 };
                assert!(plain.iter().any(|sample| *sample > 0.01));
                for (frame, (got, input)) in actual.as_chunks::<2>().0.iter().zip(plain.as_chunks::<2>().0).enumerate()
                {
                    let source = frame.saturating_sub(total) % LOOP;
                    let gain = if (1000..1500).contains(&source) { 0.0 } else { 1.0 };
                    for channel in 0..2 {
                        assert!(
                            (got[channel] - input[channel] * gain).abs() < 1e-6,
                            "{layout:?}, latency {latency}, block {block}, frame {frame}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn native_effect_contexts_keep_the_delayed_seek_boundary() {
    for block in [1, 31, 1024] {
        let actual = timed_render(257, block, Layout::Serial, true);
        // A constant source is still sounding on both sides of these gate edges; seek's
        // own fade is far from them. The delayed seek reaches the effect at 1536 + 257.
        for source in [999, 1000, 1499, 1500, 1536 + 999, 1536 + 1000, 1536 + 1499, 1536 + 1500] {
            let silent = (1000..1500).contains(&(if source >= 1536 { source - 1536 } else { source }));
            let value = actual[(source + 257) * 2];
            assert_eq!(value.abs() < 1e-6, silent, "block {block}, source {source}: {value}");
        }
    }
}

#[test]
fn native_effect_contexts_preserve_tempo_stop_resume_and_subblock_offsets() {
    for rate in [8000, 44100, 48000, 96000] {
        let mut history = crate::plugin::SongHistory::new(rate);
        let records = [
            (0, 4.0, 120.0, true),
            (16, 4.25, 60.0, true),
            (400, 10.0, 90.0, true),
            (416, 10.5, 90.0, false),
            (416, 10.5, 150.0, true),
            (432, 11.0, 150.0, true),
        ];
        for &(position, beat, tempo, playing) in &records {
            history.record(crate::SongPosition { position, beat, tempo, playing, ..Default::default() }, 16);
        }
        let mut device: Box<dyn Native> =
            Box::new(ContextProbe { rate: f64::from(rate), context: ProcessContext::default() });
        let mut lanes = crate::automation::DeviceLanes::none();
        lanes.add(0, device.as_ref(), &[], 4000.0);
        lanes.delay_slot(0, 63);
        lanes.plan_delayed(&history, 7, 64);
        let fallback = ProcessContext { tempo: 999.0, beat: 999.0, playing: false };
        let mut audio = [0.0; 128];
        let events = [(17, NoteEvent::On { key: 60, velocity: 1.0 }), (41, NoteEvent::AllNotesOff)];
        crate::chain::run_device(&mut device, 0, &mut audio, 5, 64, &events, &lanes, &fallback, f64::from(rate));
        // Current record starts at processed frame 80; the effect hears frame 10 onward.
        for frame in 5..64 {
            let source = 10 + frame;
            let (_, beat, tempo, playing) = records[source / 16];
            let expected = beat + if playing { (source % 16) as f64 * tempo / (60.0 * f64::from(rate)) } else { 0.0 };
            let (got, end) = lanes.context_span(0, frame, 64, &fallback, f64::from(rate));
            assert!((got.beat - expected).abs() < 1e-12, "rate {rate}, frame {frame}: {got:?}");
            assert_eq!((got.tempo, got.playing), (tempo, playing));
            assert!(end > frame && end <= 64);
            assert!(
                (f64::from(audio[frame * 2]) - expected).abs() < 1e-6,
                "rate {rate}, frame {frame}: dispatched context"
            );
            assert_eq!(audio[frame * 2 + 1], (if playing { tempo } else { -tempo }) as f32);
        }
    }
}

#[test]
fn native_effect_context_priming_and_one_frame_history_fit_reserved_storage() {
    let rate = 8000;
    let mut history = crate::plugin::SongHistory::new(rate);
    let device = BeatGate { rate: f64::from(rate), context: ProcessContext::default() };
    let mut lanes = crate::automation::DeviceLanes::none();
    lanes.add(0, &device, &[], 4000.0);
    lanes.delay_slot(0, 1024);
    history.record(crate::SongPosition { playing: true, ..Default::default() }, 1);
    lanes.plan_delayed(&history, 0, 1);
    let fallback = ProcessContext::default();
    let (priming, _) = lanes.context_span(0, 0, 1, &fallback, f64::from(rate));
    assert_eq!(priming.beat, -1024.0 / 4000.0);
    for frame in 1..=1024 {
        history.record(
            crate::SongPosition { playing: frame % 2 == 0, position: frame, beat: frame as f64, ..Default::default() },
            1,
        );
    }
    let allocation = allocation_counter::measure(|| lanes.plan_delayed(&history, 0, 1024));
    assert_eq!(allocation.count_total, 0, "{allocation:?}");
    for frame in 0..1024 {
        let (context, end) = lanes.context_span(0, frame, 1024, &fallback, f64::from(rate));
        assert_eq!(context.beat, frame as f64);
        assert_eq!(context.playing, frame % 2 == 0);
        assert_eq!(end, frame + 1);
    }
}

#[test]
fn native_effect_contexts_adopt_new_path_delay_without_replacing_the_device() {
    for layout in [Layout::Serial, Layout::Nested, Layout::Bus, Layout::Return, Layout::Main] {
        let (project, audio, mut plugins) = fixture(false, false, 257, layout);
        let reported = plugins.info.clone();
        for info in plugins.info.values_mut() {
            info.latency = 0;
        }
        let mut renderer = Renderer::build(
            &project,
            RenderInputs::from_audio(&audio, Arc::new(TimedDevices)).with_plugins(plugins),
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
        assert!(output[(999 + total) * 2] > 0.01, "{layout:?}: early gate");
        assert!(
            output[(1000 + total) * 2..(1500 + total) * 2].iter().all(|sample| sample.abs() < 1e-6),
            "{layout:?}: wrong input time"
        );
        assert!(output[(1500 + total) * 2] > 0.01, "{layout:?}: late gate");
    }
}

#[test]
fn native_effect_context_planning_and_processing_allocate_nothing() {
    let (project, audio, plugins) = fixture(false, false, 257, Layout::Nested);
    let mut renderer = Renderer::build(
        &project,
        RenderInputs::from_audio(&audio, Arc::new(TimedDevices)).with_plugins(plugins),
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
    assert_eq!(allocation.count_total, 0, "{allocation:?}");
    assert_eq!(allocation.count_current, 0, "{allocation:?}");
}

fn stock_pan_render(reference: bool, rate: u32, block: usize) -> Vec<f32> {
    let (mut project, audio, _) = fixture(false, false, 257, Layout::Serial);
    let mut pan = Device::new("Auto Pan", DeviceKind::Builtin("auto-pan".into()));
    pan.params = [("amount", 1.0), ("sync", 1.0), ("syncedRate", 12.0), ("phase", 0.0)]
        .into_iter()
        .map(|(id, value)| DeviceParam { id: id.into(), value })
        .collect();
    project.channels[0].devices = if reference { vec![pan, delay()] } else { vec![delay(), pan] };
    let key = crate::plugin_devices(&project).into_iter().next().unwrap().key;
    let mut plugins = Plugins::new();
    plugins.insert(
        key,
        PluginSlotInfo { instrument: false, latency: 257 },
        Box::new(Delay { samples: vec![0.0; 514], cursor: 0 }),
    );
    let mut renderer = Renderer::build(
        &project,
        RenderInputs::from_audio(&audio, Arc::new(Devices)).with_plugins(plugins),
        rate,
        RenderOptions::PLAYING,
    )
    .unwrap();
    renderer.set_offline(true);
    let mut output = vec![0.0; rate as usize]; // half a second, stereo
    for chunk in output.chunks_mut(block * 2) {
        renderer.process(chunk);
    }
    output
}

#[test]
fn stock_synced_auto_pan_matches_processing_before_the_delay() {
    for rate in [44100, 48000] {
        for block in [31, 1024] {
            let reference = stock_pan_render(true, rate, block);
            let actual = stock_pan_render(false, rate, block);
            // The gain filter has a different initial state during negative pre-roll.
            // Compare once that short startup has settled, retaining several LFO cycles.
            for (frame, (got, want)) in
                actual.as_chunks::<2>().0.iter().zip(reference.as_chunks::<2>().0).enumerate().skip(4096)
            {
                for channel in 0..2 {
                    assert!(
                        (got[channel] - want[channel]).abs() < 1e-6,
                        "rate {rate}, block {block}, frame {frame}: {} != {}",
                        got[channel],
                        want[channel]
                    );
                }
            }
        }
    }
}
