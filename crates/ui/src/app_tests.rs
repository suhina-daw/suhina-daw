// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn the_map_window_stays_above_suhina_and_minimises_with_it() {
    use egui::WindowLevel::{AlwaysOnTop, Normal};
    assert_eq!(map_window_level(true, false), AlwaysOnTop, "a click in the main window keeps it above");
    assert_eq!(map_window_level(false, true), AlwaysOnTop);
    assert_eq!(map_window_level(false, false), Normal, "another program in use covers it");
    assert_eq!(follow_main_minimised(true, false), Some(true));
    assert_eq!(follow_main_minimised(true, true), None, "minimised once");
    assert_eq!(follow_main_minimised(false, true), Some(false), "restored with the main window");
    assert_eq!(follow_main_minimised(false, false), None, "minimised by hand stays so");
}

#[test]
fn a_pending_quit_check_continues_after_the_close_event_is_gone() {
    let mut app = App::new();
    app.set_session(Session::new(crate::demo::demo_project()), &[]);
    let id = app.projects.active().unwrap();
    app.projects.get_mut(id).unwrap().execute("track.delete", &json!({ "track": 1 })).unwrap();
    app.quit_checks = Some(vec![id]);
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.guard_quit(ui.ctx()));
    output.textures_delta.clear();
    assert!(app.quit_checks.is_none());
    assert!(matches!(app.unsaved_prompt, Some(Leaving { then: AfterLeaving::Quit(_), .. })));
    assert!(app.quitting.is_none(), "the save prompt must precede releasing the project");
}

/// One frame with the main window's content at `main` on the desktop; returns where a drag
/// is in the main window.
fn drag_pointer_in(app: &App, main: Rect, pointer: Option<Pos2>) -> Option<Pos2> {
    let ctx = egui::Context::default();
    let mut input = egui::RawInput::default();
    if let Some(info) = input.viewports.get_mut(&egui::ViewportId::ROOT) {
        info.inner_rect = Some(main);
    }
    if let Some(p) = pointer {
        input.events.push(egui::Event::PointerMoved(p));
    }
    let mut found = None;
    let mut output = ctx.run_ui(input, |ui| found = app.drag_pointer(ui.ctx()));
    output.textures_delta.clear();
    found
}

#[test]
fn files_from_the_system_outline_the_target_they_hover_and_change_nothing_until_dropped() {
    use egui_kittest::kittest::Queryable;
    let mut app = App::new();
    app.set_session(suhina_session::Session::new(crate::demo::demo_project()), &[]);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(Vec2::new(1280.0, 800.0))
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    harness.run();
    let before = harness.state().session().map(|s| s.project().clone());
    let at = harness.get_by_label("Stop Keys (scene 1)").rect().center();
    harness.hover_at(at);
    harness.run();
    let file = egui::HoveredFile { path: Some(PathBuf::from("/x/Groove.mid")), ..Default::default() };
    harness.input_mut().hovered_files.push(file);
    harness.step();
    let target = harness.state().view.drop.as_ref().and_then(|p| p.target);
    assert!(matches!(target, Some(DropTarget::Slot { scene: 0, .. })), "{target:?}");
    // A project or a theme hovering is not a drag into the project.
    let file = egui::HoveredFile { path: Some(PathBuf::from("/x/Song.suhina")), ..Default::default() };
    harness.input_mut().hovered_files = vec![file];
    harness.step();
    assert_eq!(harness.state().view.drop, None);
    // Taken away again (the windowing layer clears them): nothing outlined, nothing added.
    harness.input_mut().hovered_files.clear();
    harness.step();
    assert_eq!(harness.state().view.drop, None);
    assert_eq!(harness.state().session().map(|s| s.project().clone()), before);
}

#[test]
fn a_menu_bar_item_chosen_while_the_question_about_a_clips_edits_is_open_does_nothing() {
    let mut app = App::new();
    app.set_session(suhina_session::Session::new(crate::demo::demo_project()), &[]);
    app.shown = View::Session;
    app.ara_prompt = Some((Action::new("clip.moveMany", serde_json::json!({})), "Plug-in".into()));
    let mut harness = egui_kittest::Harness::builder()
        .with_size(Vec2::new(1280.0, 800.0))
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    harness.run();
    harness.state_mut().queue_action("view.toggle");
    harness.run();
    assert_eq!(harness.state().shown, View::Session);
    assert!(harness.state().ara_prompt.is_some(), "the question is still asked");
    // Answered, the menu bar works again; the item chosen meanwhile is not run late.
    harness.state_mut().ara_prompt = None;
    harness.run();
    assert_eq!(harness.state().shown, View::Session);
    harness.state_mut().queue_action("view.toggle");
    harness.run();
    assert_eq!(harness.state().shown, View::Arrangement);
}

#[test]
fn an_agent_asking_to_connect_waits_for_the_question_about_a_clips_edits() {
    use egui_kittest::kittest::Queryable;
    let mut app = App::new();
    app.set_session(suhina_session::Session::new(crate::demo::demo_project()), &[]);
    app.ara_prompt = Some((Action::new("clip.moveMany", serde_json::json!({})), "Plug-in".into()));
    app.ask_agent(1, "Helper".into());
    let mut harness = egui_kittest::Harness::builder()
        .with_size(Vec2::new(1280.0, 800.0))
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    harness.run();
    assert!(harness.query_by_label("Allow Helper").is_none(), "one dialog at a time");
    harness.state_mut().ara_prompt = None;
    harness.run();
    assert!(harness.query_by_label("Allow Helper").is_some());
}

#[test]
fn a_drag_from_the_map_window_lands_where_the_pointer_is_on_the_main_window() {
    let mut app = App::new();
    let main = Rect::from_min_size(Pos2::new(1920.0, 100.0), Vec2::new(1280.0, 800.0));
    // A drag in the main window itself: its own pointer.
    assert_eq!(drag_pointer_in(&app, main, Some(Pos2::new(40.0, 50.0))), Some(Pos2::new(40.0, 50.0)));
    // From the map's window: the desktop position, moved onto the main window.
    app.map_window.drag =
        Some(RemoteDrag { screen: Some(Pos2::new(2020.0, 400.0)), released: false, escape: false, ctrl: false });
    assert_eq!(drag_pointer_in(&app, main, None), Some(Pos2::new(100.0, 300.0)));
    // Where the map's window cannot tell (no pointer over it yet), nothing is under the drag.
    app.map_window.drag = Some(RemoteDrag { screen: None, released: false, escape: false, ctrl: false });
    assert_eq!(drag_pointer_in(&app, main, None), None);
}

#[test]
fn failed_sampler_decode_leaves_the_empty_track_unchanged() {
    let mut app = App::new();
    app.set_session(Session::new(suhina_session::blank_project()), &[]);
    let item = drop::system_file(Path::new("/invalid.wav")).unwrap();
    let plan = app.plan_drop(&[item], DropTarget::Chain(TrackId(1)), false).unwrap();
    let before = app.session().unwrap().project().clone();
    for short in [false, true] {
        let (send, decoded) = std::sync::mpsc::channel();
        let data = std::sync::Arc::new(suhina_engine::AudioData { sample_rate: 48000, channels: 1, samples: vec![0.] });
        let result = if short {
            vec![(PathBuf::from("/invalid.wav"), None, data.clone(), crate::kit::waveform::peaks(&data))]
        } else {
            vec![]
        };
        send.send(result).unwrap();
        app.pending_drop =
            Some(PendingDrop { project: app.projects.active(), commands: plan.commands.clone(), decoded });
        app.poll_drop(&egui::Context::default());
        assert_eq!(app.session().unwrap().project(), &before);
        assert!(matches!(app.status, Status::Error(_)));
    }
}

#[test]
fn sampler_source_uses_the_shown_track_when_two_tracks_share_a_channel() {
    let mut project = suhina_session::blank_project();
    project.tracks[1].channel = project.tracks[0].channel;
    let mut session = Session::new(project);
    session.execute("device.add", &json!({"track":2,"id":"sampler"})).unwrap();
    let mut project = session.project().clone();
    project.channels[0].devices[0].set_sample_file("shared.wav");
    let mut session = Session::new(project);
    let mut audio = suhina_engine::AudioPool::new();
    audio.files.insert(
        "shared.wav".into(),
        std::sync::Arc::new(suhina_engine::AudioData { sample_rate: 48000, channels: 1, samples: vec![0.25; 480] }),
    );
    session.set_audio(audio);
    let mut app = App::new();
    app.set_session(session, &[]);
    app.view.selected = Some(TrackId(2));
    app.detail_open = true;
    app.detail_showing = detail_view::Showing::Devices;
    let mut harness = egui_kittest::Harness::builder()
        .with_size(Vec2::new(1280., 800.))
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    harness.step();
    harness.step();
    harness.ctx.data(|data| {
        assert!(
            data.get_temp::<std::sync::Arc<suhina_engine::AudioData>>(crate::detail::device_canvas::sample_source_id(
                TrackId(2),
                0
            ))
            .is_some()
        );
        assert!(
            data.get_temp::<std::sync::Arc<suhina_engine::AudioData>>(crate::detail::device_canvas::sample_source_id(
                TrackId(1),
                0
            ))
            .is_none()
        );
    });
}

#[test]
fn sample_drops_into_empty_device_area_load_sampler() {
    #[derive(Debug)]
    struct TestFile(PathBuf);
    impl egui::DroppedFile for TestFile {
        fn path(&self) -> &Path {
            &self.0
        }
        fn bytes(&self) -> Result<Vec<u8>, String> {
            Ok(Vec::new())
        }
    }
    let path = std::env::temp_dir().join(format!("suhina-device-drop-{}.wav", std::process::id()));
    let mut writer = hound::WavWriter::create(
        &path,
        hound::WavSpec { channels: 1, sample_rate: 8000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int },
    )
    .unwrap();
    for _ in 0..800 {
        writer.write_sample(1000i16).unwrap();
    }
    writer.finalize().unwrap();
    for system in [true, false] {
        let mut app = App::new();
        app.set_session(Session::new(suhina_session::blank_project()), &[]);
        app.view.selected = Some(TrackId(1));
        app.detail_open = true;
        app.detail_showing = detail_view::Showing::Devices;
        let mut harness = egui_kittest::Harness::builder()
            .with_size(Vec2::new(1280., 800.))
            .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
        harness.run();
        let target = harness.state().detail_drop.unwrap().0.center();
        harness.hover_at(target);
        harness.step();
        if system {
            harness.input_mut().dropped_files.push(std::sync::Arc::new(TestFile(path.clone())));
        } else {
            harness.drag_at(target);
            harness.step();
            harness.state_mut().browser.drag = Some(vec![drop::system_file(&path).unwrap()]);
            harness.drop_at(target);
        }
        harness.step();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < end {
            harness.step();
            if harness.state().session().unwrap().project().channels[0]
                .devices
                .first()
                .is_some_and(|d| d.sample_file().is_some())
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let session = harness.state().session().unwrap();
        let devices = &session.project().channels[0].devices;
        assert_eq!(devices.len(), 1, "system={system}: {:?}", harness.state().status);
        assert_eq!(devices[0].kind, suhina_engine::model::DeviceKind::Builtin("sampler".into()));
        assert!(devices[0].sample_file().is_some());
        assert!(session.project().arrangement.lanes[0].clips.is_empty());
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn empty_midi_track_drop_adds_sampler_before_loading_source() {
    use suhina_engine::model::*;
    let mut app = App::new();
    app.set_session(
        Session::new(Project {
            tracks: vec![Track {
                content: ContentKinds { notes: true, audio: false },
                channel: Some(ChannelId(1)),
                ..Track::new(TrackId(1), "MIDI")
            }],
            channels: vec![Channel::new(ChannelId(1), "MIDI")],
            ..Project::default()
        }),
        &[],
    );
    let sample = drop::system_file(Path::new("/test.wav")).unwrap();
    // On the track's devices (the detail area): a Sampler that plays it.
    let plan = app.plan_drop(std::slice::from_ref(&sample), DropTarget::Chain(TrackId(1)), false).unwrap();
    assert_eq!(
        plan.commands,
        vec![
            ("device.add", json!({"track":1,"id":"sampler"})),
            ("device.loadSample", json!({"track":1,"device":0,"path":"/test.wav"}))
        ]
    );
    // On its slot or lane: a clip, as on any track (the empty MIDI track becomes an audio one).
    for target in [DropTarget::Slot { track: TrackId(1), scene: 0 }, DropTarget::Lane { track: TrackId(1), beat: 0. }] {
        let plan = app.plan_drop(std::slice::from_ref(&sample), target, false).unwrap();
        assert_eq!(plan.commands[0].0, "clip.addFiles", "{target:?}");
    }
    let stems = drop::system_file(Path::new("/test.stem.mp4")).unwrap();
    let plan = app.plan_drop(&[stems], DropTarget::Lane { track: TrackId(1), beat: 0. }, false).unwrap();
    assert_eq!(plan.commands[0].0, "clip.addFiles");
    app.run_drop(vec![("device.add", json!({"track":1,"id":"sampler"}))]);
    assert!(app.plan_drop(&[sample], DropTarget::Chain(TrackId(1)), false).is_none());
}

#[test]
fn the_browser_slides_out_and_in_with_animations_on() {
    let mut app = App::new();
    app.set_session(suhina_session::Session::new(crate::demo::demo_project()), &[]);
    app.set_browser_open(true);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(Vec2::new(1280.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    harness.run();
    assert_eq!(harness.state().browser_part, 1.0);
    harness.state_mut().set_browser_open(false);
    harness.step();
    harness.step();
    let part = harness.state().browser_part;
    assert!(part > 0.0 && part < 1.0, "half way out: {part}");
    harness.run_steps(30);
    assert_eq!(harness.state().browser_part, 0.0, "all the way out");
}

#[test]
fn the_large_sample_map_closes_with_its_button_and_with_escape() {
    use egui_kittest::kittest::Queryable;
    let mut app = App::new();
    app.set_session(suhina_session::Session::new(crate::demo::demo_project()), &[]);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(Vec2::new(1280.0, 800.0))
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    harness.run();
    // Stepped while the map shows: it asks for frames while it lays out the files, which on
    // a slow machine outlasts what `run` waits for.
    harness.state_mut().do_action("view.sampleMap", &mut Vec::new());
    harness.run_steps(3);
    assert!(harness.state().sample_map.view.large);
    harness.get_by_label("Close sample map").click();
    harness.run_steps(3);
    assert!(!harness.state().sample_map.view.large, "the button closes it");
    harness.state_mut().do_action("view.sampleMap", &mut Vec::new());
    harness.run_steps(3);
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(!harness.state().sample_map.view.large, "Escape closes it");
}

#[test]
fn a_ctrl_drag_copy_to_another_track_selects_the_copy() {
    let mut app = App::new();
    app.set_session(suhina_session::Session::new(crate::demo::demo_project()), &[]);
    let project = app.session().unwrap().project().clone();
    let lanes: Vec<_> = project.arrangement.lanes.iter().filter(|l| l.track.is_some()).collect();
    let from = lanes.iter().find(|l| !l.clips.is_empty()).expect("the demo has an arrangement clip");
    let (track, clip) = (from.track.unwrap(), from.clips[0].id);
    let notes = |t: TrackId| project.tracks.iter().any(|x| x.id == t && x.content.notes);
    let to = lanes
        .iter()
        .filter_map(|l| l.track)
        .find(|t| *t != track && notes(*t) == notes(track))
        .expect("another track of the same kind");
    app.arrangement.select_clip(track, clip);
    let params = json!({ "clips": [{ "clip": clip.0 }], "beats": 64.0, "copy": true, "toTrack": to.0 });
    app.run(&Action::new("clip.moveMany", params));
    let picked = app.arrangement.selected_clips();
    assert_eq!(picked.len(), 1, "{picked:?}");
    assert_eq!(picked[0].0, to, "the copy on the other track is selected");
    assert_ne!(picked[0].1, clip);
}

#[test]
fn delete_after_a_header_click_removes_the_track_not_a_clip() {
    use egui_kittest::kittest::Queryable;
    let mut app = App::new();
    app.set_session(suhina_session::Session::new(crate::demo::demo_project()), &[]);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(Vec2::new(1280.0, 800.0))
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    harness.run();
    let count = |h: &egui_kittest::Harness<'_, App>| h.state().session().map_or(0, |s| s.project().tracks.len());
    let clips = |h: &egui_kittest::Harness<'_, App>| {
        h.state().session().map_or(0, |s| {
            s.project().launcher.columns.iter().flat_map(|c| &c.slots).filter(|s| s.clip.is_some()).count()
        })
    };
    let (tracks, slots) = (count(&harness), clips(&harness));
    harness.get_by_label("Track Keys").click();
    harness.run();
    harness.key_press(egui::Key::Delete);
    harness.run();
    assert_eq!(count(&harness), tracks - 1);
    assert!(harness.state().session().unwrap().project().tracks.iter().all(|t| t.name != "Keys"));
    assert!(clips(&harness) < slots, "the track's clips went with it, no other clip did");
}

/// The ARA edits prompt and the missing files panel are drawn over the window: plug-in
/// editors hide, and the keys that edit the project wait until they close.
#[test]
fn the_ara_prompt_and_the_missing_files_panel_hold_the_keys_and_hide_the_editors() {
    use egui_kittest::kittest::Queryable;
    let mut app = App::new();
    app.set_session(suhina_session::Session::new(crate::demo::demo_project()), &[]);
    assert!(!app.over_the_window());
    app.missing.open = true;
    assert!(app.over_the_window());
    app.missing.open = false;
    let mut harness = egui_kittest::Harness::builder()
        .with_size(Vec2::new(1280.0, 800.0))
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    harness.run();
    let count = |h: &egui_kittest::Harness<'_, App>| h.state().session().map_or(0, |s| s.project().tracks.len());
    let tracks = count(&harness);
    harness.get_by_label("Track Keys").click();
    harness.run();
    harness.state_mut().ara_prompt = Some((Action::new("clip.moveMany", json!({})), "Editor".into()));
    assert!(harness.state().over_the_window());
    harness.key_press(egui::Key::Delete);
    harness.run();
    assert_eq!(count(&harness), tracks, "Delete waits for the prompt");
}

#[test]
fn a_device_dropped_on_a_return_column_goes_on_the_return() {
    use egui_kittest::kittest::Queryable;
    let mut app = App::new();
    app.set_session(suhina_session::Session::new(crate::demo::demo_project()), &[]);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(Vec2::new(1280.0, 800.0))
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    harness.run();
    let at = harness.get_by_label("A Reverb channel strip").rect().center();
    let delay = BrowserItem {
        key: "device:delay".into(),
        name: "Delay".into(),
        kind: crate::interaction::drop::ItemKind::AudioEffect,
        plugin: false,
    };
    harness.state_mut().browser.drag = Some(vec![delay]);
    harness.hover_at(at);
    harness.run();
    let project = harness.state().session().map(|s| s.project().clone()).unwrap();
    let channel = project.tracks.iter().find(|t| t.name == "A Reverb").and_then(|t| t.channel).unwrap();
    let devices = &project.channel(channel).unwrap().devices;
    assert!(devices.iter().any(|d| d.name == "Delay"), "{devices:?}");
}

#[test]
fn pending_drop_is_cancelled_when_another_project_is_active() {
    let mut app = App::new();
    app.set_session(Session::new(crate::demo::demo_project()), &[]);
    let project = app.projects.active();
    let (send, decoded) = std::sync::mpsc::channel();
    send.send(Vec::new()).unwrap();
    app.pending_drop =
        Some(PendingDrop { project, decoded, commands: vec![("device.add", json!({"track":1,"id":"sampler"}))] });
    app.open_project(Session::new(crate::demo::demo_project()), &[], OpenAs::NewTab);
    let before = app.session().unwrap().project().clone();
    app.poll_drop(&egui::Context::default());
    assert!(app.pending_drop.is_none());
    assert_eq!(app.session().unwrap().project(), &before);
}

#[test]
fn ram_is_shown_in_megabytes_or_gigabytes() {
    assert_eq!(ram_text(0), "0 MB");
    assert_eq!(ram_text(512 * 1024 * 1024), "512 MB");
    assert_eq!(ram_text(700_000_000), "668 MB");
    // From a gibibyte on, gigabytes with a decimal.
    assert_eq!(ram_text(1024 * 1024 * 1024), "1.0 GB");
    assert_eq!(ram_text(2 * 1024 * 1024 * 1024 + 555_745_280), "2.5 GB");
    // The measurement itself answers on this machine.
    assert!(memory_stats::memory_stats().is_some_and(|s| s.physical_mem > 0));
}

/// One export runs on its worker at a time: a second is refused with where it is said, and
/// a dialog opened over the running one shows it busy again instead of starting over the
/// running job's receiver. What runs is the one whose result is taken, and closing the
/// dialog while it goes never means the window lost track of it.
#[test]
fn a_second_export_is_refused_while_one_runs_and_a_reopened_dialog_shows_it() {
    let mut app = App::new();
    app.set_session(Session::new(crate::demo::demo_project()), &[]);
    let dir = std::env::temp_dir().join(format!("suhina-export-one-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("one.wav");
    let job = ExportJob {
        path: path.clone(),
        format: None,
        sample_rate: 48_000,
        bit_depth: None,
        normalize: false,
        dither: None,
        mp3_rate: None,
        overwrite: false,
    };

    // The first export goes to its worker: its receiver is in the app from now on, whether
    // the render is at its first or its last block.
    app.start_export(job.clone());
    assert!(app.export.is_some(), "the first export's worker started");

    // File › Export… opened again over the running export shows it busy, its own settings.
    app.open_export();
    let dialog = app.export_dialog.as_ref().unwrap();
    assert!(dialog.busy, "the reopened dialog shows the running export");
    assert!(!dialog.path.is_empty(), "the dialog still names its file: {:?}", dialog.path);

    // A second export is refused, and nothing took the first one's result away.
    app.start_export(job);
    assert!(
        matches!(&app.status, Status::Error(text) if text.contains("already running")),
        "the second export is refused: {:?}",
        app.status
    );
    assert!(app.export_dialog.as_ref().is_some_and(|d| d.busy), "the dialog shows the running one");
    assert!(app.export.is_some(), "the first export's result is still coming");

    // The first export lands its result; the dialog closes with it.
    let ctx = egui::Context::default();
    for _ in 0..600 {
        app.poll_export(&ctx);
        if matches!(app.status, Status::Done(_)) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(matches!(&app.status, Status::Done(text) if text.contains("Exported")), "{:?}", app.status);
    assert!(app.export.is_none(), "the finished export's receiver went with it");
    assert!(app.export_dialog.is_none(), "the finished export closed the dialog");
    assert!(path.exists(), "the first export wrote {}", path.display());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The project closed while the export's states were being gathered: the export stops,
/// said with the name it set out under (the feedback keeps naming a closed project), and
/// no worker is left running or a dialog stuck on Exporting….
#[test]
fn a_project_closed_during_the_gather_stops_the_export_with_its_name() {
    let mut app = App::new();
    app.set_session(Session::new(crate::demo::demo_project()), &[]);
    let id = app.projects.active().unwrap();
    let dir = std::env::temp_dir().join(format!("suhina-export-closed-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let job = ExportJob {
        path: dir.join("one.wav"),
        format: None,
        sample_rate: 48_000,
        bit_depth: None,
        normalize: false,
        dither: None,
        mp3_rate: None,
        overwrite: false,
    };

    // The export is gathering its states over the frames (a read pending): the app holds
    // the job, and the dialog is up showing Exporting… with the gather's working Cancel.
    let name = app.projects.name(id).unwrap_or("the mix").to_string();
    app.export_gather = Some((id, job, std::time::Instant::now() + std::time::Duration::from_secs(1), name.clone()));
    app.open_export();
    assert!(app.export_dialog.as_ref().is_some_and(|d| d.busy && d.gathering), "the dialog shows the gather");

    // The project's tab closes (nothing unsaved here, so it closes without asking).
    app.run(&Action::new("project.close", json!({ "project": id.0 })));
    assert!(app.projects.get(id).is_none(), "the project is gone");

    // The next frame's gather finds no project: the export is said with its name, no
    // worker started, and the dialog closed instead of saying Exporting… for ever.
    app.gather_export();
    assert!(app.export_gather.is_none(), "the gather's job went with it");
    assert!(app.export.is_none(), "no worker started");
    assert!(app.export_dialog.is_none(), "the stuck dialog closed with the export");
    assert!(
        matches!(&app.status, Status::Error(text) if text.contains(&format!("the export of {name} stopped: the project closed"))),
        "{:?}",
        app.status
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_panel_the_theme_lays_over_the_view_is_drawn_above_it() {
    let ctx = egui::Context::default();
    let mut app = App::new();
    app.set_session(Session::new(crate::demo::demo_project()), &[]);
    app.settings.animations = false;
    app.settings.browser_open = true;
    app.use_test_skin(crate::kit::skin::Skin {
        css: Some(std::sync::Arc::from(
            "#workspace { display: grid; grid-template-columns: minmax(0,1fr); grid-template-rows: minmax(0,1fr); } \
             #panel-main { grid-column: 1; grid-row: 1; } \
             #panel-browser { grid-column: 1; grid-row: 1; width: 260px; justify-self: start; }",
        )),
        ..Default::default()
    });
    let layer = egui::LayerId::new(egui::Order::Middle, Id::new(("workspace-overlay", "panel-browser")));
    let mut drawn = false;
    for _ in 0..4 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))),
                ..Default::default()
            },
            |ui| {
                app.ui(ui);
                drawn = ui.ctx().graphics(|layers| layers.get(layer).is_some_and(|list| !list.is_empty()));
            },
        );
        output.textures_delta.clear();
    }
    app.use_test_skin(crate::kit::skin::Skin::default());
    assert!(drawn, "the browser is drawn on its own layer over the view");
}

#[test]
fn a_bar_draws_its_separator_in_its_own_border_colour() {
    let ctx = egui::Context::default();
    let mut app = App::new();
    app.set_session(Session::new(crate::demo::demo_project()), &[]);
    app.settings.animations = false;
    app.use_test_skin(crate::kit::skin::Skin {
        css: Some(std::sync::Arc::from(".menu-bar { border-color: #FF0000; }")),
        ..Default::default()
    });
    let red = egui::Color32::from_rgb(0xFF, 0, 0);
    let mut found = false;
    for _ in 0..4 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))),
                ..Default::default()
            },
            |ui| app.ui(ui),
        );
        output.textures_delta.clear();
        // The separator runs the window's whole width; the toolbar's own lines are short.
        found = output.shapes.iter().any(|shape| match &shape.shape {
            egui::Shape::LineSegment { points, stroke } => {
                stroke.color == red && (points[1].x - points[0].x).abs() > 1000.0
            }
            egui::Shape::Rect(rect) => rect.fill == red && rect.rect.width() > 1000.0 && rect.rect.height() <= 2.0,
            _ => false,
        });
    }
    app.use_test_skin(crate::kit::skin::Skin::default());
    assert!(found, "the line under the menu bar is red");
}

#[test]
fn a_bar_without_a_line_fills_the_line_with_its_background() {
    let ctx = egui::Context::default();
    let mut app = App::new();
    app.set_session(Session::new(crate::demo::demo_project()), &[]);
    app.settings.animations = false;
    app.use_test_skin(crate::kit::skin::Skin {
        css: Some(std::sync::Arc::from(".menu-bar { background-color: #00FF00; border-width: 0; }")),
        ..Default::default()
    });
    let green = egui::Color32::from_rgb(0, 0xFF, 0);
    let mut found = false;
    for _ in 0..4 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))),
                ..Default::default()
            },
            |ui| app.ui(ui),
        );
        output.textures_delta.clear();
        found = output.shapes.iter().any(|shape| match &shape.shape {
            egui::Shape::LineSegment { points, stroke } => {
                stroke.color == green && (points[1].x - points[0].x).abs() > 1000.0
            }
            egui::Shape::Rect(rect) => rect.fill == green && rect.rect.width() > 1000.0 && rect.rect.height() <= 2.0,
            _ => false,
        });
    }
    app.use_test_skin(crate::kit::skin::Skin::default());
    assert!(found, "the menu bar's line is its own green");
}

#[test]
fn a_theme_keeps_the_header_column_at_its_least_width() {
    let ctx = egui::Context::default();
    let mut app = App::new();
    app.set_session(Session::new(crate::demo::demo_project()), &[]);
    app.settings.animations = false;
    app.shown = View::Arrangement;
    // As a theme file is read: the sheet's --suhina-* values are the app's.
    let css = ".arrangement { --suhina-header-min-width: 330px; }";
    let mut theme = crate::kit::theme::Theme::default();
    assert!(crate::kit::theme_css::read_host_rules(&mut theme, css).is_empty());
    theme.skin.css = Some(std::sync::Arc::from(css));
    app.use_test_skin(theme.skin);
    app.arrangement.header_width = 200.0;
    for _ in 0..3 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))),
                ..Default::default()
            },
            |ui| app.ui(ui),
        );
        output.textures_delta.clear();
    }
    app.use_test_skin(crate::kit::skin::Skin::default());
    assert_eq!(app.arrangement.header_width, 330.0);
}

#[test]
fn an_arrangement_header_and_name_draw_the_sheet_s_backgrounds() {
    let ctx = egui::Context::default();
    let mut app = App::new();
    app.set_session(Session::new(crate::demo::demo_project()), &[]);
    app.settings.animations = false;
    app.shown = View::Arrangement;
    app.use_test_skin(crate::kit::skin::Skin {
        css: Some(std::sync::Arc::from(
            ".arrangement .track-header { background-color: #102030; } \
             .arrangement .track-name { background-color: #FF0000; }",
        )),
        ..Default::default()
    });
    let (mut header, mut name) = (false, false);
    for _ in 0..4 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))),
                ..Default::default()
            },
            |ui| app.ui(ui),
        );
        output.textures_delta.clear();
        for shape in &output.shapes {
            if let egui::Shape::Rect(rect) = &shape.shape {
                header |= rect.fill == egui::Color32::from_rgb(0x10, 0x20, 0x30);
                name |= rect.fill == egui::Color32::from_rgb(0xFF, 0, 0);
            }
        }
    }
    app.use_test_skin(crate::kit::skin::Skin::default());
    assert!(header && name, "header {header}, name {name}");
}

#[test]
fn a_clip_takes_its_radius_opacity_and_name_colour_from_the_sheet() {
    let ctx = egui::Context::default();
    let mut app = App::new();
    app.set_session(Session::new(crate::demo::demo_project()), &[]);
    app.settings.animations = false;
    app.shown = View::Arrangement;
    app.use_test_skin(crate::kit::skin::Skin {
        css: Some(std::sync::Arc::from(
            ".clip { border-radius: 9px; opacity: 0.5; background-color: #2040FF; } \
             .clip-name { color: #FF0000; }",
        )),
        ..Default::default()
    });
    let (mut round, mut faded, mut red_name) = (false, false, false);
    for _ in 0..4 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))),
                ..Default::default()
            },
            |ui| app.ui(ui),
        );
        output.textures_delta.clear();
        for shape in &output.shapes {
            match &shape.shape {
                egui::Shape::Rect(rect) if rect.corner_radius.nw == 9 => {
                    round = true;
                    faded |= rect.fill.a() < 200;
                }
                egui::Shape::Text(text) => {
                    red_name |= text.galley.job.sections.iter().any(|s| {
                        let c = s.format.color;
                        c.r() > 0 && c.g() == 0 && c.b() == 0
                    });
                }
                _ => {}
            }
        }
    }
    app.use_test_skin(crate::kit::skin::Skin::default());
    assert!(round && faded && red_name, "round {round}, faded {faded}, red name {red_name}");
}

#[test]
fn a_rename_in_place_covers_the_name_drawn_under_it() {
    use egui_kittest::kittest::Queryable;
    let mut app = App::new();
    app.set_session(Session::new(crate::demo::demo_project()), &[]);
    app.settings.animations = false;
    app.shown = View::Arrangement;
    let track = app.session().unwrap().project().tracks[0].id.0;
    let mut harness =
        egui_kittest::Harness::builder().with_size((1280.0, 800.0)).build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    harness.run();
    harness.state_mut().rename = Some((json!({ "rename_track": track }), "Renamed".into(), false));
    harness.run();
    let field = harness.get_by_label("Track name").rect();
    // The back is the name's own colour (#916), opaque, so nothing drawn under it shows through.
    assert!(
        harness.output().shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Rect(rect) if rect.fill.a() == u8::MAX && rect.rect.contains_rect(field))),
        "an opaque back under the field"
    );
}

#[test]
fn css_workspace_moves_native_controls_and_preserves_project_state() {
    use egui_kittest::kittest::Queryable;
    let mut app = App::new();
    app.set_session(Session::new(crate::demo::demo_project()), &[]);
    app.settings.animations = false;
    app.settings.browser_open = true;
    app.detail_open = true;
    let mut harness =
        egui_kittest::Harness::builder().with_size((1280.0, 800.0)).build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    harness.run();
    // Let the normal first-frame project/theme initialization finish before
    // checking that changing presentation does not edit the project.
    let before = harness.state().session().unwrap().project().clone();
    harness.state_mut().use_test_skin(crate::kit::skin::Skin {css:Some(std::sync::Arc::from("#workspace {display:grid;grid-template-columns:minmax(0,1fr) 240px;grid-template-rows:minmax(0,1fr) 160px;gap:8px;} #panel-browser {grid-column:2;grid-row:1 / -1;} #panel-main {grid-column:1;grid-row:1;} #panel-detail {grid-column:1;grid-row:2;}")),..Default::default()});
    harness.run();
    let slot = harness.get_by_label("Stop Keys (scene 1)").rect();
    assert!(slot.right() < 1032.0, "native slot uses CSS main rectangle: {slot:?}");
    assert!(harness.query_by_label("Detail area splitter").is_none());
    assert_eq!(harness.state().session().unwrap().project(), &before);
    harness.state_mut().use_test_skin(crate::kit::skin::Skin::default());
    harness.run();
    let native_slot = harness.get_by_label("Stop Keys (scene 1)").rect();
    assert!(
        native_slot.left() > slot.left() + 100.0,
        "removing CSS restores the native browser beside the view: {slot:?} -> {native_slot:?}"
    );
    assert!(harness.query_by_label("Detail area splitter").is_some());
    assert_eq!(harness.state().session().unwrap().project(), &before);
    crate::kit::skin::configure(&crate::kit::skin::Skin::default());
}

/// A rename in the launcher view's header reads as the name itself: the name's size, centred in
/// the header as the name is, and no selection colour of the theme's, which reads as a box of
/// its own on a coloured header (#916).
#[test]
fn a_rename_in_a_session_header_reads_as_the_name() {
    use egui_kittest::kittest::Queryable;
    let mut app = App::new();
    app.set_session(Session::new(crate::demo::demo_project()), &[]);
    app.settings.animations = false;
    app.shown = View::Session;
    let track = app.session().unwrap().project().tracks[0].id.0;
    let mut harness =
        egui_kittest::Harness::builder().with_size((1280.0, 800.0)).build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    harness.run();
    harness.state_mut().rename = Some((json!({ "rename_track": track }), "Renamed".into(), false));
    harness.run();
    harness.run();
    let field = harness.get_by_label("Track name").rect();
    let text = harness
        .output()
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == "Renamed" => Some(text.clone()),
            _ => None,
        })
        .expect("the field's text");
    let size = text.galley.job.sections.first().map(|s| s.format.font_id.size);
    assert_eq!(size, Some(theme::TEXT), "the name's own size");
    let middle = text.pos.y + text.galley.size().y / 2.0;
    assert!((middle - field.center().y).abs() <= 1.5, "centred: text at {middle}, field {field:?}");
}

/// Every copy in Suhina puts a line on the system clipboard, automation, devices and tracks too,
/// so Ctrl+V reaches the keymap after it even when the clipboard held no text (#916).
#[test]
fn copies_put_a_line_on_the_system_clipboard() {
    for command in ["automation.copy", "device.copy", "track.copy", "track.cut", "clip.copy", "note.cut"] {
        assert!(clipboard_line(command).is_some_and(|line| line.starts_with("Suhina: ")), "{command}");
    }
    assert_eq!(clipboard_line("automation.paste"), None);
}
