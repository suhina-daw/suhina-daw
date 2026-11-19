// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The metronome's sounds: the built-in families, the library's folders of sound files and the
//! bar's own sound, through `transport.metronomeSounds` and `transport.setMetronome`.
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use std::path::Path;

use serde_json::{Value, json};
use suhina_engine::{AudioPool, Renderer};
use suhina_model::Project;
use suhina_session::{Player, Session};

use crate::support::run;
use crate::temp_dir::TempDir;

const SR: u32 = 48_000;
const BEAT: usize = SR as usize / 2;

/// A mono 16-bit WAV file of `frames` frames at `level`, at the engine's rate.
fn write_wav(path: &Path, level: f32, frames: usize) {
    let data = (frames * 2) as u32;
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&SR.to_le_bytes());
    bytes.extend_from_slice(&(SR * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    let sample = (level * 32_767.0) as i16;
    for _ in 0..frames {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap();
}

/// A library holding `Sounds/Metronome/Claps` (two sounds, a text file and a hidden sound),
/// an empty folder and a hidden one.
fn library() -> TempDir {
    let base = TempDir::new("metronome-sounds");
    let claps = suhina_session::metronome_folder(&base).join("Claps");
    std::fs::create_dir_all(&claps).unwrap();
    write_wav(&claps.join("b.wav"), 0.25, 400);
    write_wav(&claps.join("A.wav"), 0.5, 400);
    write_wav(&claps.join(".hidden.wav"), 0.5, 400);
    std::fs::write(claps.join("notes.txt"), "not a sound").unwrap();
    std::fs::create_dir_all(suhina_session::metronome_folder(&base).join("Empty")).unwrap();
    std::fs::create_dir_all(suhina_session::metronome_folder(&base).join(".git")).unwrap();
    base
}

fn playing(base: &Path) -> (Session, Renderer) {
    let project = Project::default();
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR, suhina_session::devices()).unwrap();
    let mut session = crate::support::toned(project);
    session.set_library_base(base.to_path_buf());
    session.attach_player(Player::new(renderer.controller(), None));
    (session, renderer)
}

fn names(families: &Value) -> Vec<(String, Vec<String>)> {
    families["families"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            let sounds = f["sounds"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap().to_string());
            (f["name"].as_str().unwrap().to_string(), sounds.collect())
        })
        .collect()
}

#[test]
fn the_sounds_are_the_built_in_families_then_each_library_folder() {
    let base = library();
    let (mut session, _renderer) = playing(&base);
    let listed = run(&mut session, "transport.metronomeSounds", json!({})).unwrap();
    let strings = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        names(&listed),
        [
            ("Tones".to_string(), strings(&["beep", "wood"])),
            ("Soft".to_string(), strings(&["soft", "muted", "deep", "tick", "knock"])),
            ("Claps".to_string(), strings(&["library/Claps/A.wav", "library/Claps/b.wav"])),
        ],
        "an empty folder, a hidden one, a hidden file and a file that is no sound are left out"
    );
    assert_eq!(listed["families"][2]["folder"], "Claps");
    assert_eq!(listed["families"][2]["sounds"][0]["name"], "A");
    assert_eq!(listed["families"][0]["folder"], Value::Null);
    // The menu reads the same, and a folder added later is found when they are looked for again.
    assert_eq!(session.metronome().families.len(), 3);
    let kicks = suhina_session::metronome_folder(&base).join("Kicks");
    std::fs::create_dir_all(&kicks).unwrap();
    write_wav(&kicks.join("k.wav"), 0.5, 100);
    assert_eq!(session.metronome().families.len(), 3, "not looked for yet");
    run(&mut session, "transport.metronomeSounds", json!({})).unwrap();
    assert_eq!(session.metronome().families.last().map(|f| f.name.as_str()), Some("Kicks"));
}

#[test]
fn a_library_sound_clicks_on_every_beat_and_the_bar_plays_its_own_sound() {
    let base = library();
    let (mut session, mut renderer) = playing(&base);
    let set = run(
        &mut session,
        "transport.setMetronome",
        json!({ "on": true, "db": 0, "sound": "library/Claps/A.wav", "accent": "library/Claps/b.wav" }),
    )
    .unwrap();
    assert_eq!((&set["sound"], &set["accent"]), (&json!("library/Claps/A.wav"), &json!("library/Claps/b.wav")));
    let state = session.metronome();
    assert_eq!((state.sound.as_str(), state.accent.as_deref()), ("library/Claps/A.wav", Some("library/Claps/b.wav")));
    run(&mut session, "transport.play", json!({})).unwrap();
    let peaks = beat_peaks(&mut renderer, 2);
    // The bar's first beat plays b (0.25), the next beat A (0.5).
    assert!((peaks[0] - 0.25).abs() < 0.01 && (peaks[1] - 0.5).abs() < 0.01, "{peaks:?}");
    // Back to the beat's sound, higher, on the bar; the sound is kept.
    let higher = run(&mut session, "transport.setMetronome", json!({ "on": true, "accent": "higher" })).unwrap();
    assert_eq!((&higher["sound"], &higher["accent"]), (&json!("library/Claps/A.wav"), &json!("higher")));
    assert_eq!(session.metronome().accent, None);
}

#[test]
fn replacing_the_player_restores_the_click_sounds_and_cue_route() {
    let base = library();
    let (mut session, _previous) = playing(&base);
    run(
        &mut session,
        "transport.setMetronome",
        json!({
            "on": true, "db": 0, "sound": "library/Claps/A.wav",
            "accent": "library/Claps/b.wav", "route": "cue"
        }),
    )
    .unwrap();

    let mut renderer =
        Renderer::stopped(&Project::default(), &AudioPool::new(), SR, suhina_session::devices()).unwrap();
    renderer.handle(suhina_engine::LaunchCommand::SetCue { gain: Some(1.0), separate: Some(true) });
    session.attach_player(Player::new(renderer.controller(), None));
    run(&mut session, "transport.play", json!({})).unwrap();
    let mut main = vec![0.0; 2 * BEAT];
    let mut cue = vec![0.0; 2 * BEAT];
    for expected in [0.25, 0.5] {
        renderer.process_with_cue(&mut main, &mut cue);
        assert!(main.iter().all(|sample| *sample == 0.0), "the click stays off Main after replacing the output");
        let peak = cue.iter().fold(0.0f32, |peak, sample| peak.max(sample.abs()));
        assert!((peak - expected).abs() < 0.01, "the bar and beat keep their library sounds: {peak} != {expected}");
    }
}

#[test]
fn a_sound_outside_the_library_folder_or_not_a_sound_is_refused_and_changes_nothing() {
    let base = library();
    std::fs::write(base.join("secret.wav"), "x").unwrap();
    let (mut session, _renderer) = playing(&base);
    run(&mut session, "transport.setMetronome", json!({ "on": true, "sound": "knock", "accent": "wood" })).unwrap();
    for bad in [
        "cowbell",
        "library/Claps",
        "library/Claps/missing.wav",
        "library/Claps/notes.txt",
        "library/Claps/.hidden.wav",
        "library/../secret.wav",
        "library/Claps/../../../secret.wav",
        "library/../../secret.wav",
        "library//A.wav",
        "/etc/passwd",
    ] {
        for key in ["sound", "accent"] {
            let mut params = json!({ "on": true });
            params[key] = json!(bad);
            assert!(run(&mut session, "transport.setMetronome", params).is_err(), "{key} {bad}");
        }
    }
    assert!(run(&mut session, "transport.setMetronome", json!({ "on": true, "accent": 3 })).is_err());
    let state = session.metronome();
    assert_eq!((state.sound.as_str(), state.accent.as_deref()), ("knock", Some("wood")), "an error changes nothing");
}

#[test]
fn without_a_library_only_the_built_in_sounds_are_there() {
    let mut session = crate::support::toned(Project::default());
    let listed = run(&mut session, "transport.metronomeSounds", json!({})).unwrap();
    assert_eq!(names(&listed).len(), 2);
    assert_eq!(listed["library"], Value::Null);
}

fn beat_peaks(renderer: &mut Renderer, beats: usize) -> Vec<f32> {
    let mut buffer = vec![0.0f32; 2 * BEAT];
    (0..beats)
        .map(|_| {
            renderer.process(&mut buffer);
            buffer.iter().fold(0.0f32, |m, s| m.max(s.abs()))
        })
        .collect()
}
