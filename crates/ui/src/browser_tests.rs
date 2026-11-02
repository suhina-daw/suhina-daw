// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn a_large_search_answers_from_its_own_thread_and_the_next_key_replaces_it() {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let folder = std::env::temp_dir().join(format!("suhina-browser-search-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    for name in ["kick one.wav", "kick two.wav", "hat.wav"] {
        std::fs::write(folder.join(name), b"x").unwrap();
    }
    let mut projects = Projects::new();
    projects.file_library_mut().add_folder(&folder).unwrap();
    let names = |state: &BrowserState| -> Vec<String> {
        state.rows.iter().filter(|r| r.item.name.ends_with(".wav")).map(|r| r.item.name.clone()).collect()
    };

    // Small enough: there at once.
    let mut state = BrowserState { search: "hat".into(), ..BrowserState::default() };
    state.refresh_rows(&mut projects).unwrap();
    assert!(!state.searching());
    assert_eq!(names(&state), ["hat.wav"]);

    // Large: what was listed stays until the search answers.
    state.search_inline_files = 0;
    state.search = "kick".into();
    state.refresh_rows(&mut projects).unwrap();
    assert!(state.searching());
    assert_eq!(names(&state), ["hat.wav"]);
    state.take_search_answer(true);
    assert!(!state.searching());
    assert_eq!(names(&state), ["kick one.wav", "kick two.wav"]);

    // A search overtaken by the next key never shows.
    state.search = "hat".into();
    state.refresh_rows(&mut projects).unwrap();
    let overtaken = state.search_job.as_ref().map(|job| Arc::clone(&job.cancel)).unwrap();
    state.search = "kick two".into();
    state.refresh_rows(&mut projects).unwrap();
    assert!(overtaken.load(Ordering::Relaxed), "the next key ends it");
    state.take_search_answer(true);
    assert_eq!(names(&state), ["kick two.wav"]);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_preview_not_heard_yet_keeps_its_stop_control_until_it_ends() {
    let mut state = BrowserState::default();
    // Waiting to be read: the row's control already stops it, a second click does not restart it.
    state.want_preview(PathBuf::from("kick.wav"), 0.0);
    assert!(state.preview_active_for(&PathBuf::from("kick.wav").display().to_string()));
    state.pending_preview = None;
    state.preview_started("kick.wav".into());
    // The audio thread has not started it in the first frames: it has not ended.
    state.observe_preview(false, 0);
    assert!(state.preview_active_for("kick.wav"), "not dropped before the voice starts");
    state.observe_preview(true, 1);
    assert!(state.preview_active_for("kick.wav"));
    state.observe_preview(false, 1);
    assert!(!state.preview_active_for("kick.wav"), "ended at the sample's end");
    assert_eq!(state.previewing, None);
    // A short one that starts and ends between two looks: the count shows it played.
    state.preview_started("hat.wav".into());
    state.observe_preview(false, 2);
    assert!(!state.preview_active_for("hat.wav"), "a preview over before it was seen playing");
    // Stopping forgets it at once.
    state.preview_started("snare.wav".into());
    state.cancel_preview();
    assert!(!state.preview_active_for("snare.wav"));
}

#[test]
fn durations_read_as_minutes_and_seconds() {
    assert_eq!(duration_text(4.2), "0:04.2");
    assert_eq!(duration_text(0.06), "0:00.1");
    assert_eq!(duration_text(201.7), "3:21");
    assert_eq!(duration_text(f64::NAN), "");
}

/// A sample previewed without being selected (the sample map previews what the pointer is
/// over) keeps its decoded audio until it plays.
#[test]
fn an_unselected_sample_waiting_to_preview_keeps_its_audio() {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!("suhina-browser-preview-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hat.wav");
    let spec =
        hound::WavSpec { channels: 1, sample_rate: 8000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut writer = hound::WavWriter::create(&path, spec).unwrap();
    for i in 0..800 {
        writer.write_sample(((i % 50) * 400) as i16).unwrap();
    }
    writer.finalize().unwrap();

    let mut state = BrowserState::default();
    assert!(state.selected().is_empty());
    let _ = state.info(&path, true);
    state.want_preview(path.clone(), 0.0);
    let mut ready = None;
    for _ in 0..400 {
        state.poll();
        ready = state.take_ready_preview();
        if ready.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let (got, from, data) = ready.expect("the preview's audio arrives");
    assert_eq!((got, from, data.frames()), (path, 0.0, 800));
    let _ = std::fs::remove_dir_all(dir);
}

/// A loop's audio arrives for its preview, and its tempo is detected and taken in too (it is
/// sent after the audio, so the preview does not wait for it).
#[test]
fn a_previewed_loop_gets_its_tempo_too() {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!("suhina-browser-tempo-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("loop.wav");
    let rate = 44_100u32;
    let spec =
        hound::WavSpec { channels: 1, sample_rate: rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut writer = hound::WavWriter::create(&path, spec).unwrap();
    // Four bars of clicks at 120 BPM: 8 seconds, a beat every half second.
    for i in 0..(rate * 8) {
        let since_beat = i % (rate / 2);
        let s = if since_beat < 400 { ((since_beat as f32 * 0.3).sin() * 20_000.0) as i16 } else { 0 };
        writer.write_sample(s).unwrap();
    }
    writer.finalize().unwrap();

    let mut state = BrowserState::default();
    let started = std::time::Instant::now();
    let _ = state.info(&path, true);
    state.want_preview(path.clone(), 0.0);
    let mut ready_after = None;
    for _ in 0..2000 {
        state.poll();
        if ready_after.is_none() && state.take_ready_preview().is_some() {
            ready_after = Some(started.elapsed());
        }
        if ready_after.is_some() && state.info.get(&path).and_then(|i| i.tempo).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(ready_after.is_some(), "the preview's audio arrives");
    let detected = state.info.get(&path).and_then(|i| i.tempo).expect("the tempo follows");
    assert!((detected.bpm - 120.0).abs() < 1.0, "{}", detected.bpm);
    assert_eq!(detected.source, TempoSource::Detected);
    println!("audio ready after {:?}, tempo after {:?}", ready_after.unwrap_or_default(), started.elapsed());
    let _ = std::fs::remove_dir_all(dir);
}
