// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use suhina_session::{Session, blank_project};

fn temp(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!("suhina-map-panel-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn wav(path: &Path, hz: f32, seconds: f32) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 22_050,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..(seconds * 22_050.0) as usize {
        let t = i as f32 / 22_050.0;
        let s = (std::f32::consts::TAU * hz * t).sin() * (-t * 6.0).exp();
        writer.write_sample((s * 20_000.0) as i16).unwrap();
    }
    writer.finalize().unwrap();
}

/// Projects whose library holds a folder of analysed samples: kicks (low) and hats (high).
fn analysed() -> (Projects, PathBuf, PathBuf) {
    let settings = temp("settings");
    let samples = temp("samples");
    for i in 0..8 {
        wav(&samples.join(format!("kick {i}.wav")), 50.0 + i as f32 * 3.0, 0.5);
        wav(&samples.join(format!("hat {i}.wav")), 6000.0 + i as f32 * 100.0, 0.1);
    }
    wav(&samples.join("long kick.wav"), 55.0, 3.0);
    let mut projects = Projects::new();
    projects.set_file_library(suhina_library::Library::new(Some(settings.clone())));
    projects.add(Session::new(blank_project()), None).unwrap();
    projects.execute("library.addFolder", &json!({ "folder": samples.display().to_string() })).unwrap();
    projects.start_sound_map();
    projects.wait_sound_map();
    (projects, settings, samples)
}

#[test]
fn the_map_shows_the_place_and_acts_on_the_browser() {
    let (mut projects, settings_dir, samples) = analysed();
    let mut panel = Panel::default();
    let mut state = BrowserState { mode: ResultsMode::Other, ..BrowserState::default() };
    let mut settings = Settings { map_max_seconds: 1, ..Settings::default() };
    let ctx = egui::Context::default();
    let draw = |panel: &mut Panel, state: &mut BrowserState, projects: &mut Projects, settings: &mut Settings| {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let rect = ui.max_rect();
            panel.show(ui, rect, state, projects, settings, 0);
        });
        output.textures_delta.clear();
    };
    draw(&mut panel, &mut state, &mut projects, &mut settings);
    // The layout runs on a thread of its own; it arrives on a later frame.
    for _ in 0..200 {
        if panel.map.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        draw(&mut panel, &mut state, &mut projects, &mut settings);
    }
    let map = panel.map.as_ref().expect("laid out");
    assert_eq!(map.points.len(), 16);
    assert_eq!(map.too_long, 1);
    assert!(panel.status().starts_with("16 samples"), "{}", panel.status());
    assert_eq!(panel.points.len(), 16);
    let kick = panel.points.iter().position(|p| p.name == "kick 0.wav").unwrap();
    assert_eq!(panel.points[kick].kind, suhina_soundmap::kinds::Kind::Kick);

    // Selecting on the map selects in the browser; the search dims what does not match.
    panel.handle(MapEvent::Select { items: vec![kick], add: false }, &mut state, &mut projects, &mut settings);
    assert_eq!(state.selected().len(), 1);
    assert!(state.selected()[0].key.ends_with("kick 0.wav"));
    state.search = "hat".into();
    draw(&mut panel, &mut state, &mut projects, &mut settings);
    assert!(panel.points.iter().all(|p| p.matches == p.name.starts_with("hat")));
    assert!(panel.points[kick].selected);

    // Pins, tags, similar items, drags and double-clicks.
    panel.handle(MapEvent::Pin(kick), &mut state, &mut projects, &mut settings);
    assert_eq!(projects.file_library_mut().pinned().len(), 1);
    panel.handle(MapEvent::Similar(kick), &mut state, &mut projects, &mut settings);
    let (_, similar) = panel.view.similar().unwrap();
    assert!(similar.iter().take(5).all(|i| panel.points[*i].name.contains("kick")));
    panel.handle(MapEvent::StartDrag(vec![kick]), &mut state, &mut projects, &mut settings);
    assert_eq!(state.drag.as_ref().map(Vec::len), Some(1));
    panel.handle(MapEvent::Add(kick), &mut state, &mut projects, &mut settings);
    assert_eq!(panel.add.len(), 1);

    // The layout switch is a setting; the large view hands the results back to the list.
    assert!(panel.handle(MapEvent::Layout(Layout::Grid), &mut state, &mut projects, &mut settings));
    assert!(settings.map_grid);
    panel.handle(MapEvent::ToggleLarge, &mut state, &mut projects, &mut settings);
    assert_eq!((panel.view.large, state.mode), (true, ResultsMode::List));
    panel.handle(MapEvent::ShowInFolder(kick), &mut state, &mut projects, &mut settings);
    assert_eq!(state.place, Some(Place::Folder(samples.clone())));
    assert!(!panel.view.large);

    // A longer limit takes in the long sample.
    settings.map_max_seconds = 10;
    state.place = None;
    for _ in 0..200 {
        draw(&mut panel, &mut state, &mut projects, &mut settings);
        if panel.map.as_ref().is_some_and(|m| m.points.len() == 17) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(panel.map.as_ref().unwrap().points.len(), 17);

    // A kind set by hand shows at once and the scope is laid out again with it; Automatic
    // gives it back to the name and the sound.
    use suhina_soundmap::kinds::Kind;
    let index =
        |panel: &Panel| panel.map.as_ref().unwrap().points.iter().position(|p| p.path.ends_with("hat 3.wav")).unwrap();
    let hat = index(&panel);
    let key = panel.shown.as_ref().map(|k| k.layout_key);
    panel.handle(MapEvent::SetKind { item: hat, kind: Some(Kind::Tom) }, &mut state, &mut projects, &mut settings);
    assert_eq!(panel.map.as_ref().unwrap().points[hat].kind, Kind::Tom);
    assert!(panel.map.as_ref().unwrap().points[hat].by_hand);
    for _ in 0..200 {
        draw(&mut panel, &mut state, &mut projects, &mut settings);
        if panel.shown.as_ref().map(|k| k.layout_key) != key && panel.job.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_ne!(panel.shown.as_ref().map(|k| k.layout_key), key);
    let hat = index(&panel);
    assert_eq!(panel.points[hat].kind, Kind::Tom);
    assert!(panel.points[hat].by_hand);
    panel.handle(MapEvent::SetKind { item: hat, kind: None }, &mut state, &mut projects, &mut settings);
    for _ in 0..200 {
        draw(&mut panel, &mut state, &mut projects, &mut settings);
        if panel.map.as_ref().is_some_and(|m| m.points[index(&panel)].kind == Kind::Hat) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let hat = index(&panel);
    assert_eq!((panel.points[hat].kind, panel.points[hat].by_hand), (Kind::Hat, false));
    drop(projects);
    let _ = std::fs::remove_dir_all(settings_dir);
    let _ = std::fs::remove_dir_all(samples);
}
