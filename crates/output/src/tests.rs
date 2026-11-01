// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// Two ring frames: main (0.5, -0.5) with cue (0.25, -0.25), then main (1, 0) with cue (0.1, 0.2).
const RINGED: [f32; 8] = [0.5, -0.5, 0.25, -0.25, 1.0, 0.0, 0.1, 0.2];

#[test]
fn control_panel_targets_the_current_owner_once_and_rejects_other_hosts() {
    let (owner, requests) = mpsc::channel();
    let mut live = Live {
        owner,
        panel_pending: Arc::new(AtomicBool::new(false)),
        progress: Arc::new(Progress::default()),
        host: "ASIO".into(),
        device: "Any installed driver".into(),
        sample_rate: 48_000,
        block: 2048,
        exclusive: true,
        fallback: false,
        channels: 2,
        cue_pair: None,
        input: None,
    };
    assert!(live.open_control_panel().is_ok());
    assert!(matches!(requests.try_recv(), Ok(OwnerRequest::ControlPanel)));
    assert!(live.open_control_panel().is_err());
    assert!(requests.try_recv().is_err());
    live.panel_pending.store(false, Ordering::Release);
    live.host = "WASAPI".into();
    assert!(live.open_control_panel().is_err());
    assert!(requests.try_recv().is_err());
    live.host = "ASIO".into();
    drop(requests);
    assert!(live.open_control_panel().is_err());
    assert!(!live.panel_pending.load(Ordering::Acquire));
}

#[test]
fn a_missing_callback_or_failed_device_does_not_wait_forever_for_the_close_fade() {
    let control = transition::Control::default();
    let start = std::time::Instant::now();
    assert!(!control.finish(Duration::from_millis(10), || false));
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(!control.finish(Duration::from_secs(60), || true));
}

#[test]
fn the_main_output_maps_onto_any_channel_count() {
    let mut mono = [0.0f32; 2];
    write_frames(&mut mono, 1, &RINGED, None);
    assert_eq!(mono, [0.0, 0.5]);

    let mut two = [0.0f32; 4];
    write_frames(&mut two, 2, &RINGED, None);
    assert_eq!(two, [0.5, -0.5, 1.0, 0.0]);

    // Without a cue pair, the cue is not written: the engine has put the preview in the main.
    let mut four = [9.0f32; 8];
    write_frames(&mut four, 4, &RINGED, None);
    assert_eq!(four, [0.5, -0.5, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);

    let mut ints = [0i16; 4];
    write_frames(&mut ints, 2, &RINGED, None);
    assert_eq!(ints[2], i16::MAX);

    let mut none: [f32; 0] = [];
    write_frames(&mut none, 0, &RINGED, None);
}

/// A fake eight-channel interface with the cue on pair 2 (channels 5 and 6): the main output
/// stays on 1/2, the cue goes to 5/6, everything else is silent.
#[test]
fn the_cue_goes_to_its_pair_on_a_multi_channel_device() {
    let cue = cue_channel(Some(2), 8);
    assert_eq!(cue, Some(4));
    let mut eight = [9.0f32; 16];
    write_frames(&mut eight, 8, &RINGED, cue);
    assert_eq!(eight[..8], [0.5, -0.5, 0.0, 0.0, 0.25, -0.25, 0.0, 0.0]);
    assert_eq!(eight[8..], [1.0, 0.0, 0.0, 0.0, 0.1, 0.2, 0.0, 0.0]);
    // The last pair of the device.
    let mut last = [9.0f32; 8];
    write_frames(&mut last, 4, &RINGED[..4], cue_channel(Some(1), 4));
    assert_eq!(last[..4], [0.5, -0.5, 0.25, -0.25]);
    assert_eq!(last[4..], [9.0; 4], "frames past the ring's are left alone");
}

#[test]
fn a_cue_pair_the_device_does_not_have_plays_with_the_main_output() {
    assert_eq!(cue_channel(None, 8), None);
    assert_eq!(cue_channel(Some(0), 8), None, "pair 0 is the main output");
    assert_eq!(cue_channel(Some(1), 2), None);
    assert_eq!(cue_channel(Some(1), 3), None, "half a pair is not a pair");
    assert_eq!(cue_channel(Some(3), 8), Some(6));
    assert_eq!(cue_channel(Some(4), 8), None);
    assert_eq!(cue_channel(Some(u16::MAX), u16::MAX), None);
    // Opening more channels than the default gives, when the device offers them.
    assert_eq!(channels_for_cue(1, 2, &[2, 8, 4]), Some(4));
    assert_eq!(channels_for_cue(1, 6, &[2]), Some(6), "the default is enough");
    assert_eq!(channels_for_cue(3, 2, &[2, 4, 6]), None);
    assert_eq!(channels_for_cue(u16::MAX, 2, &[u16::MAX]), None);
    assert!(cue_missing("Interface", 3, 4).contains("not 7/8"));
}

#[test]
fn the_engine_block_follows_the_chosen_buffer() {
    assert_eq!(block_for(None, None), ENGINE_BLOCK);
    assert_eq!(block_for(Some(256), None), 256);
    assert_eq!(block_for(Some(32), None), 64, "bounded below");
    assert_eq!(block_for(Some(10_000), None), 4096, "bounded above");
    // The test override wins when it makes sense, and is ignored when it does not.
    assert_eq!(block_for(Some(256), Some("512")), 512);
    assert_eq!(block_for(Some(256), Some("16")), 256);
    assert_eq!(block_for(None, Some("lots")), ENGINE_BLOCK);
}

#[test]
fn the_ring_keeps_four_blocks_or_a_device_buffer_and_a_block() {
    assert_eq!(ring_target(512, 0, false), 2048);
    assert_eq!(ring_target(512, 128, false), 2048);
    assert_eq!(ring_target(256, 1024, false), 1280);
    // A device asking for absurd amounts is capped.
    assert_eq!(ring_target(64, usize::MAX, false), MAX_DEVICE_FRAMES + 64);
    // The ring has room for its target and a block more, for every block size, live or not.
    for block in [64, 256, 1024, 4096] {
        let (producer, _consumer) = ring(block);
        for live in [false, true] {
            assert!(
                producer.buffer().capacity() >= (ring_target(block, MAX_DEVICE_FRAMES, live) + block) * RING_CHANNELS
            );
        }
    }
}

#[test]
fn while_a_track_is_armed_the_ring_keeps_one_block_or_a_device_buffer() {
    assert_eq!(ring_target(128, 0, true), 128, "before the first callback");
    assert_eq!(ring_target(128, 128, true), 128);
    assert_eq!(ring_target(128, 64, true), 128);
    assert_eq!(ring_target(128, 512, true), 512, "a device taking more at once gets it");
    assert_eq!(ring_target(64, usize::MAX, true), MAX_DEVICE_FRAMES);
    // Never more than without anyone playing.
    for block in [64, 128, 256, 1024, 4096] {
        for device in [0, 64, 128, 441, 1024, 8192] {
            assert!(ring_target(block, device, true) < ring_target(block, device, false), "{block} {device}");
        }
    }
}

#[test]
fn a_keys_latency_is_the_measured_latency_and_the_fixed_delay() {
    let progress = Progress::default();
    progress.set_latency(Duration::from_millis(5));
    progress.block.store(480, Ordering::Relaxed);
    // Two blocks: the longest wait between blocks, and the block the note lands in (#269).
    assert_eq!(progress.live_latency(48_000), Duration::from_millis(25));
    assert_eq!(progress.live_latency(0), Duration::from_millis(5) + Duration::from_secs(960), "a rate of 0");
    // A device that takes more than a block at once makes the wait longer.
    progress.set_callback_frames(960);
    assert_eq!(progress.live_latency(48_000), Duration::from_millis(35));
    assert!(!progress.live());
}

#[test]
fn reading_the_ring_counts_what_was_missing() {
    let (mut producer, mut consumer) = rtrb::RingBuffer::<f32>::new(16);
    let progress = Progress::default();
    for sample in [0.1, 0.2, 0.3, 0.4] {
        producer.push(sample).unwrap();
    }
    let mut out = [9.0f32; 4];
    read_ring(&mut consumer, &mut out, &progress);
    assert_eq!(out, [0.1, 0.2, 0.3, 0.4]);
    assert_eq!(progress.underruns(), 0);

    for sample in [0.5, 0.6, 0.7, 0.8, 0.9, 1.0] {
        producer.push(sample).unwrap();
    }
    let mut out = [9.0f32; 12];
    read_ring(&mut consumer, &mut out, &progress);
    // Whole frames only: the half frame waits for the rest of it.
    assert_eq!(out, [0.5, 0.6, 0.7, 0.8, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    assert_eq!(progress.underruns(), 1);
    assert_eq!(consumer.slots(), 2);
}

#[test]
fn latency_is_the_ring_and_the_device() {
    let latency = measure_latency(441, Duration::from_millis(5), 44_100);
    assert_eq!(latency, Duration::from_millis(15));
    assert_eq!(measure_latency(100, Duration::ZERO, 0), Duration::from_secs(100), "a rate of 0 does not divide by 0");
    let progress = Progress::default();
    progress.set_latency(Duration::from_micros(23_456));
    assert_eq!(progress.latency(), Duration::from_micros(23_456));
}

/// A choice that names nothing on this system falls back to the default with notices, or
/// fails cleanly where there is no audio device at all (CI).
#[test]
fn a_choice_of_missing_things_falls_back_to_the_default() {
    let choice = OutputChoice {
        host: Some("No Such Host".into()),
        device: Some("No Such Device".into()),
        sample_rate: Some(12_345),
        buffer_frames: Some(300),
        exclusive: false,
        cue_pair: Some(31),
        cue_gain: 1.0,
        input: suhina_session::InputChoice::Off,
    };
    match Output::choose(&choice) {
        Ok(output) => {
            assert!(output.notices().iter().any(|n| n.contains("No Such Host")), "{:?}", output.notices());
            assert!(output.notices().iter().any(|n| n.contains("No Such Device")));
            assert!(output.sample_rate() > 0 && output.sample_rate() != 12_345);
            assert!((64..=4096).contains(&output.block()));
            // No default device has 64 channels: the cue plays with the main output, and says so.
            assert_eq!(output.cue_pair(), None);
            assert!(output.notices().iter().any(|n| n.contains("63/64")), "{:?}", output.notices());
        }
        Err(e) => assert!(!e.is_empty()),
    }
}

#[test]
fn the_priority_notice_is_told_once() {
    let told = AtomicBool::new(false);
    assert_eq!(tell_once(&told, None), None, "nothing to tell leaves it untold");
    assert_eq!(tell_once(&told, Some("first".into())), Some("first".into()));
    assert_eq!(tell_once(&told, Some("again".into())), None);
}

/// The render thread says what priority it got before it renders anything, so the stream can
/// tell the user before the device starts; it plays on whatever it got.
#[test]
fn the_render_thread_says_its_priority_first() {
    let said = std::thread::spawn(|| {
        let renderer = Renderer::stopped(&suhina_model::Project::default(), &AudioPool::new(), 48_000).unwrap();
        let (producer, _consumer) = ring(256);
        let (stop, progress) = (AtomicBool::new(true), Progress::default());
        let (joined, said) = mpsc::sync_channel(1);
        let _renderer = render_ahead(renderer, producer, 256, &stop, &progress, &joined);
        said.try_recv().unwrap()
    })
    .join()
    .unwrap();
    let (priority, notice) = said;
    if priority.is_real_time() || priority == Priority::Off {
        assert_eq!(notice, None);
    }
    if let Some(notice) = notice {
        assert!(notice.contains(priority.describe()), "{notice}");
    }
}

#[test]
fn an_output_plays_in_place_of_a_chosen_device_that_is_not_there() {
    let choice = |host: Option<&str>, device: Option<&str>| OutputChoice {
        host: host.map(String::from),
        device: device.map(String::from),
        ..OutputChoice::default()
    };
    assert!(!in_place_of(&OutputChoice::default(), "ALSA", "default"), "nothing chosen");
    assert!(!in_place_of(&choice(Some("alsa "), Some("USB Audio")), "ALSA", "usb audio"), "names as compared");
    assert!(in_place_of(&choice(None, Some("USB Audio")), "ALSA", "default"), "another device");
    assert!(in_place_of(&choice(Some("ASIO"), None), "WASAPI", "Speakers"), "another host");
}

#[test]
fn the_chosen_device_is_looked_for_in_the_background() {
    let system = DeviceOutput::new();
    let gone = OutputChoice { host: Some("No such host".into()), ..OutputChoice::default() };
    // The first ask starts a look and does not wait for it.
    assert_eq!(system.chosen_present(&gone), None);
    let mut answer = None;
    for _ in 0..500 {
        std::thread::sleep(Duration::from_millis(10));
        answer = system.chosen_present(&gone);
        if answer.is_some() {
            break;
        }
    }
    assert_eq!(answer, Some(false));
    // An answer is for the choice it was asked for: another choice waits for its own look.
    let other = OutputChoice { device: Some("Elsewhere".into()), ..gone.clone() };
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(system.chosen_present(&other), None);
}

/// Waits up to two seconds for `done`, looking every millisecond.
fn eventually(mut done: impl FnMut() -> bool) -> bool {
    let until = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < until {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    done()
}

#[test]
fn the_render_thread_keeps_less_ahead_while_a_track_is_armed() {
    use suhina_engine::{LaunchCommand, model};
    const BLOCK: usize = 128;
    let mut renderer = Renderer::stopped(&model::Project::default(), &AudioPool::new(), 48_000).unwrap();
    let mut controller = renderer.controller();
    let (producer, mut consumer) = ring(BLOCK);
    let progress = Arc::new(Progress::default());
    let stop = Arc::new(AtomicBool::new(false));
    let (stopping, watching) = (Arc::clone(&stop), Arc::clone(&progress));
    let (joined, _said) = mpsc::sync_channel(1);
    let render = std::thread::spawn(move || render_ahead(renderer, producer, BLOCK, &stopping, &watching, &joined));
    let queued = |c: &Ring| c.slots() / RING_CHANNELS;

    // Nobody plays: two blocks ahead.
    assert!(eventually(|| queued(&consumer) >= 2 * BLOCK));
    assert!(!progress.live());

    // Armed: the device drains the queue down to one block, which the thread keeps.
    controller
        .send(LaunchCommand::SetArmed { track: model::TrackId(1), armed: true, input: Default::default() })
        .unwrap();
    let mut device = vec![0.0f32; BLOCK * RING_CHANNELS];
    assert!(eventually(|| {
        read_ring(&mut consumer, &mut device, &progress);
        progress.live() && queued(&consumer) < 2 * BLOCK
    }));
    std::thread::sleep(Duration::from_millis(20));
    assert!(queued(&consumer) >= BLOCK && queued(&consumer) < 2 * BLOCK, "{}", queued(&consumer));

    // Disarmed: back to two blocks.
    controller
        .send(LaunchCommand::SetArmed { track: model::TrackId(1), armed: false, input: Default::default() })
        .unwrap();
    assert!(eventually(|| {
        if !progress.live() && queued(&consumer) >= 2 * BLOCK {
            return true;
        }
        read_ring(&mut consumer, &mut device, &progress);
        false
    }));
    stop.store(true, Ordering::Release);
    progress.wake_render();
    assert!(render.join().is_ok());
}

#[test]
fn the_first_device_callback_has_audio_before_the_render_thread_runs() {
    for (block, device, live) in
        [(1024, 2048, false), (2048, 444, false), (256, 2048, true), (64, MAX_DEVICE_FRAMES, true)]
    {
        let (mut producer, mut consumer) = ring(block);
        prime_ring(&mut producer, block, device, live);
        let progress = Progress::default();
        let mut output = vec![1.0; device * RING_CHANNELS];
        read_ring(&mut consumer, &mut output, &progress);
        assert_eq!(progress.underruns(), 0, "first callback: block {block}, device {device}");
        assert!(output.iter().all(|sample| *sample == 0.0));
        // ASIO may request both halves immediately before hardware streaming starts.
        read_ring(&mut consumer, &mut output, &progress);
        assert_eq!(progress.underruns(), 0, "second initial callback: block {block}, device {device}");
        assert_eq!(consumer.slots() / RING_CHANNELS, ring_target(block, device, live).max(2 * device) - 2 * device);
    }
}
#[cfg(all(windows, feature = "asio"))]
#[test]
#[ignore = "requires SUHINA_ASIO_TEST_DRIVER naming an installed connected driver; emits silence"]
fn installed_asio_driver_delivers_more_than_one_callback() {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let name = std::env::var("SUHINA_ASIO_TEST_DRIVER").expect("test driver required");
    let host = cpal::host_from_id(cpal::HostId::Asio).expect("ASIO host");
    let device = host.output_devices().expect("devices").find(|d| d.to_string() == name).expect("test device");
    let supported = device.default_output_config().expect("output config");
    let mut config = supported.config();
    config.channels = config.channels.min(2);
    let count = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let seen = std::sync::Arc::clone(&count);
    // CPAL zero-fills the output before invoking this callback. No generated audio.
    let stream = device
        .build_output_stream_raw(
            config,
            supported.sample_format(),
            move |_, _| {
                seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            },
            |_| {},
            None,
        )
        .expect("open stream");
    stream.play().expect("start stream");
    std::thread::sleep(std::time::Duration::from_secs(3));
    let callbacks = count.load(std::sync::atomic::Ordering::Relaxed);
    drop(stream);
    assert!(callbacks > 5, "stream stalled after {callbacks} callbacks");
}

#[test]
fn device_switch_fades_the_actual_ring_including_cue_before_acknowledging_close() {
    for rate in [8_000, 44_100, 48_000, 96_000] {
        for frames in [32, 480, 2048] {
            let (mut producer, mut ring) = rtrb::RingBuffer::new(frames * RING_CHANNELS);
            let progress = Progress::default();
            progress.set_callback_frames(frames);
            let mut envelope = transition::Envelope::new(rate);
            let delay = Duration::from_secs_f64(frames as f64 / f64::from(rate));
            let mut buffer = vec![0.0; frames * RING_CHANNELS];
            let mut previous = 0.0f32;
            let mut biggest_step = 0.0f32;
            // Startup silence must not consume the onset fade. Then open and close
            // through the very same ring reader both device backends use.
            for block in 0..240 {
                if block == 120 {
                    progress.transition.request_close();
                }
                for _ in 0..frames {
                    for sample in [0.5, -0.5, 0.25, -0.25] {
                        producer.push(if block < 60 { 0.0 } else { sample }).unwrap();
                    }
                }
                read_device_ring(&mut ring, &mut buffer, &progress, &mut envelope, delay);
                for frame in buffer.as_chunks::<RING_CHANNELS>().0 {
                    biggest_step = biggest_step.max((frame[0] - previous).abs());
                    previous = frame[0];
                    assert_eq!(frame[2], frame[0] * 0.5);
                    assert_eq!(frame[1], -frame[0]);
                }
                if block == 119 {
                    assert_eq!(previous, 0.5);
                }
                if block == 120 {
                    assert!(!progress.transition.drained());
                }
            }
            assert!(biggest_step < 0.02, "rate={rate}, frames={frames}, step={biggest_step}");
            assert_eq!(previous, 0.0);
            assert!(progress.transition.drained());
        }
    }
}
