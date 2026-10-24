// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! File › Export: the dialog, its worker and the file it writes.

use super::*;

/// The demo project with its arrangement stretched to 512 beats (a stretch of silence after
/// the clips): an export of it takes seconds, so a test can step through the window while it
/// is inside the running export, not at its race's end.
fn long_demo_project() -> suhina_engine::model::Project {
    let mut project = demo_project();
    // The Drums lane's own content (11), placed again out at 256 beats, so the arrangement
    // ends at 512: the export takes seconds, and a test steps through the window while it
    // is inside the running export, not at its race's end.
    project.arrangement.lanes[0].clips.push(suhina_engine::model::Clip::new(
        suhina_engine::model::ContentId(11),
        256.0,
        256.0,
    ));
    project
}

fn long_demo_app() -> App {
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(long_demo_project()), &[]);
    app
}

#[test]
fn the_export_dialog_writes_the_wav_the_demo_names() {
    let mut harness = window(demo_app());
    let dir = crate::temp_dir::TempDir::new("export-dialog");
    let path = dir.join("Demo mix.wav");

    // File › Export… opens the dialog over the window.
    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label("Export…").click();
    harness.run();
    assert!(harness.query_by_label("File to export to").is_some(), "the export dialog is open");

    // The file to write, typed over what the dialog filled in.
    harness.get_by_label("File to export to").click();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("File to export to").type_text(path.display().to_string().as_str());
    harness.run();

    // Export runs on its worker; the window keeps living while it renders (the wait runs
    // frames beside the real thread), and the result lands in the status.
    harness.get_by_label("Export the mix to the file").click();
    waits_for_export(&mut harness);
    assert!(path.exists(), "the export wrote {}", path.display());
    assert!(
        matches!(harness.state().status(), Status::Done(text) if text.contains("Exported")),
        "{:?}",
        harness.state().status()
    );
    assert!(harness.query_by_label("File to export to").is_none(), "the finished export closed the dialog");
    // A wav: the RIFF and WAVE marks of the file's first bytes.
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");

    // An export over it is refused until overwrite is on.
    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label("Export…").click();
    harness.run();
    harness.get_by_label("File to export to").click();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("File to export to").type_text(path.display().to_string().as_str());
    harness.get_by_label("Export the mix to the file").click();
    waits_for_export(&mut harness);
    assert!(
        matches!(harness.state().status(), Status::Error(text) if text.contains("already exists")),
        "{:?}",
        harness.state().status()
    );
    // A failed export keeps the dialog open with its settings; Overwrite makes it go through.
    assert!(harness.query_by_label("File to export to").is_some(), "the dialog stays open after a failure");
    harness.get_by_label("Overwrite").click();
    harness.run();
    harness.get_by_label("Export the mix to the file").click();
    waits_for_export(&mut harness);
    assert!(
        matches!(harness.state().status(), Status::Done(text) if text.contains("Exported")),
        "{:?}",
        harness.state().status()
    );
}

#[test]
fn a_file_named_mp3_shows_the_mp3_rate_and_exports_an_mp3() {
    let mut harness = window(demo_app());
    let dir = crate::temp_dir::TempDir::new("export-mp3-dialog");
    let path = dir.join("Demo mix.mp3");

    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label("Export…").click();
    harness.run();
    // A wav name shows no mp3 rate.
    assert!(harness.query_by_label_contains("MP3 rate").is_none());

    // From the file name: an .mp3 name is an mp3, its rate row shows, and 48 000 stays.
    harness.get_by_label("File to export to").click();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("File to export to").type_text(path.display().to_string().as_str());
    harness.run();
    assert!(harness.query_by_label("Sample rate 48000 Hz").is_some(), "48 000 is kept for mp3");
    harness.get_by_label("MP3 rate 320 kbps").click();
    harness.run();
    harness.get_by_label("VBR V2").click();
    harness.run();
    assert!(harness.query_by_label("MP3 rate VBR V2").is_some(), "the rate row took the choice");

    harness.get_by_label("Export the mix to the file").click();
    waits_for_export(&mut harness);
    assert!(
        matches!(harness.state().status(), Status::Done(text) if text.contains("Exported")),
        "{:?}",
        harness.state().status()
    );
    // An mp3: the demo was never saved, so no ID3 header; the stream starts at a frame's sync.
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes[0], 0xFF);
    assert_eq!(bytes[1] & 0xE0, 0xE0);
}

/// Runs frames until the export's result is in the status (the export renders on a real
/// worker thread); the deadline only guards against a hang, not how long a busy runner takes.
fn waits_for_export(harness: &mut Harness<'static, App>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while std::time::Instant::now() < deadline {
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let exporting = matches!(harness.state().status(), Status::Hint(text) if text.starts_with("Exporting"));
        if !exporting {
            return;
        }
    }
    assert!(
        !matches!(harness.state().status(), Status::Hint(text) if text.starts_with("Exporting")),
        "the export did not finish: {:?}",
        harness.state().status()
    );
}

/// Runs frames until the status says Exporting…, at most two seconds.
fn waits_while_exporting(harness: &mut Harness<'static, App>) {
    for _ in 0..200 {
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(10));
        if matches!(harness.state().status(), Status::Hint(text) if text.starts_with("Exporting")) {
            return;
        }
    }
    assert!(
        matches!(harness.state().status(), Status::Hint(text) if text.starts_with("Exporting")),
        "the export did not start: {:?}",
        harness.state().status()
    );
}

/// The frames a WAV file holds, walked from its `data` chunk, and the depth its `fmt ` chunk
/// names: the export's length is checked against the arrangement's end.
#[allow(clippy::unwrap_used, clippy::panic)] // a test helper outside #[test] functions
fn wav_frames(path: &std::path::Path) -> u64 {
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(&bytes[..4], b"RIFF", "a wav");
    assert_eq!(&bytes[8..12], b"WAVE", "a wav");
    let (mut at, mut width) = (12_usize, 0_u64);
    while at + 8 <= bytes.len() {
        let id: [u8; 4] = bytes[at..at + 4].try_into().unwrap();
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        if id == *b"fmt " {
            let bits = u16::from_le_bytes(bytes[at + 8 + 14..at + 8 + 16].try_into().unwrap());
            let channels = u16::from_le_bytes(bytes[at + 8 + 2..at + 8 + 4].try_into().unwrap());
            width = u64::from(bits) * u64::from(channels) / 8;
        }
        if id == *b"data" {
            assert!(width > 0, "the fmt chunk came before the data chunk");
            return u64::from(size as u32) / width;
        }
        at += 8 + size + (size & 1);
    }
    panic!("no data chunk in {}", path.display());
}

/// Closing an export's busy dialog mid-run leaves its worker running; File › Export… again
/// shows the running job (Exporting…, no second Export to click), and the first export's
/// result is the one that lands.
#[test]
fn a_closed_export_dialogs_worker_still_runs_and_the_reopened_dialog_shows_it() {
    let mut harness = window(long_demo_app());
    let dir = crate::temp_dir::TempDir::new("export-running");
    let path = dir.join("Demo mix.wav");

    // File › Export…, the file typed, Export clicked: the dialog turns into Exporting….
    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label("Export…").click();
    harness.run();
    harness.get_by_label("File to export to").click();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("File to export to").type_text(path.display().to_string().as_str());
    harness.run();
    harness.get_by_label("Export the mix to the file").click();
    waits_while_exporting(&mut harness);

    // The dialog closed while the export renders: the worker goes on, the status holds. The
    // frames here are bounded steps: the window repaints every frame while the export runs.
    harness.get_by_label("Close Export audio").click();
    harness.run_steps(2);
    assert!(harness.query_by_label("File to export to").is_none(), "the dialog is closed");
    assert!(
        matches!(harness.state().status(), Status::Hint(text) if text.starts_with("Exporting")),
        "the export renders on: {:?}",
        harness.state().status()
    );

    // Reopening shows the running job again: Exporting…, and no Export button to start another.
    harness.get_by_label("File").click();
    harness.run_steps(2);
    harness.get_by_label("Export…").click();
    harness.run_steps(2);
    assert!(
        matches!(harness.state().status(), Status::Hint(text) if text.starts_with("Exporting")),
        "the reopened dialog shows the running export: {:?}",
        harness.state().status()
    );
    assert!(
        harness.query_by_label("Export the mix to the file").is_none(),
        "a second export is not offered while one runs"
    );

    // The export the user closed the window of is the one whose result lands.
    waits_for_export(&mut harness);
    assert!(path.exists(), "the export wrote {}", path.display());
    assert!(
        matches!(harness.state().status(), Status::Done(text) if text.contains("Exported")),
        "{:?}",
        harness.state().status()
    );
}

/// Switching to another project while the export renders changes nothing about it: the
/// operation is the first project's, its file the first project's sound, and the feedback
/// names it whatever tab is showing (the spec's tabbed workflow, two projects open).
#[test]
fn switching_projects_while_the_export_renders_keeps_the_export_and_names_it() {
    // The second project is the long demo (the export takes seconds, so the switch lands
    // inside the render); the first is switched to while it runs.
    let mut app = App::new();
    app.set_browser_open(false);
    app.open_project(Session::new(demo_project()), &[], OpenAs::NewTab);
    app.open_project(Session::new(long_demo_project()), &[], OpenAs::NewTab);
    let mut harness = window(app);
    let ids = harness.state().projects().ids();
    assert_eq!(harness.state().projects().active(), ids.get(1).copied());
    let first = ids.first().copied();
    // The export belongs to the project active when Export was clicked (the second tab); its
    // name is what the feedback carries.
    let export_name = harness.state().projects().name(ids.get(1).copied().unwrap_or(ids[0])).unwrap_or("").to_string();
    let dir = crate::temp_dir::TempDir::new("export-switch");
    let path = dir.join("Long mix.wav");

    // File › Export…, the file typed, Export clicked: Exporting…, and the name is the long
    // project's.
    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label("Export…").click();
    harness.run();
    harness.get_by_label("File to export to").click();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("File to export to").type_text(path.display().to_string().as_str());
    harness.run();
    harness.get_by_label("Export the mix to the file").click();
    waits_while_exporting(&mut harness);
    assert!(
        matches!(harness.state().status(), Status::Hint(text) if text.contains(&format!("Exporting {export_name}"))),
        "the export's project is named while it runs: {:?}",
        harness.state().status()
    );

    // The dialog closes; the worker goes on. Then the other tab shows: the export goes on,
    // still the second project's. The frames are steps: the window repaints every frame
    // while the export runs.
    harness.get_by_label("Close Export audio").click();
    harness.run_steps(2);
    assert!(harness.query_by_label("File to export to").is_none(), "the dialog closed; the export renders on");
    harness.get_by_label("Tab Untitled 1").click();
    harness.run_steps(2);
    assert_eq!(harness.state().projects().active(), first, "the other project shows");
    assert!(
        matches!(harness.state().status(), Status::Hint(text) if text.contains(&format!("Exporting {export_name}"))),
        "the status still names the export's project: {:?}",
        harness.state().status()
    );

    waits_for_export(&mut harness);
    assert!(path.exists(), "the export wrote {}", path.display());
    assert!(
        matches!(harness.state().status(), Status::Done(text) if text.contains(&format!("Exported {export_name} to"))),
        "the finished export names the project it belongs to: {:?}",
        harness.state().status()
    );
    // And the file is the second project's render, not the tab now showing: its frames are
    // what the engine renders of the same project (the same length the whole-mix export
    // writes, the demo project's own render being far shorter).
    let project = long_demo_project();
    let expected = suhina_engine::Renderer::stopped(&project, &suhina_engine::AudioPool::new(), 48_000)
        .expect("the long project renders")
        .length();
    let frames = wav_frames(&path);
    assert_eq!(frames, expected, "the exported file is the second project's render");
}
