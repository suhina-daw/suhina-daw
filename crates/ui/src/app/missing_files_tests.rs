// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn row(file: &str, last: &str) -> Row {
    Row {
        file: file.into(),
        last_path: PathBuf::from(last),
        clips: 1,
        samplers: 0,
        candidates: Vec::new(),
        chosen: None,
        by_hand: false,
    }
}

/// A folder of its own under the system's temporary folder, empty.
fn temp(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!("suhina-missing-ui-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn file(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"x").unwrap();
}

#[test]
fn the_best_candidate_is_chosen_unless_the_user_chose() {
    let mut r = row("kick.wav", "/old/Drums/kick.wav");
    r.offer(vec![PathBuf::from("/new/Other/kick.wav"), PathBuf::from("/new/Drums/kick.wav")]);
    assert_eq!(r.chosen, Some(PathBuf::from("/new/Drums/kick.wav")));
    assert_eq!(r.state(), State::Several(2));
    r.choose(PathBuf::from("/new/Other/kick.wav"));
    r.offer(vec![PathBuf::from("/x/Drums/kick.wav")]);
    assert_eq!(r.chosen, Some(PathBuf::from("/new/Other/kick.wav")));
    assert_eq!(r.state(), State::Chosen);
    assert_eq!(r.candidates.len(), 3);
    assert_eq!(r.name(), "kick.wav");
}

#[test]
fn a_row_says_what_plays_it() {
    let mut r = row("kick.wav", "/old/kick.wav");
    assert_eq!(r.uses(), "1 clip");
    (r.clips, r.samplers) = (38, 0);
    assert_eq!(r.uses(), "38 clips");
    (r.clips, r.samplers) = (0, 1);
    assert_eq!(r.uses(), "1 sampler");
    (r.clips, r.samplers) = (2, 3);
    assert_eq!(r.uses(), "2 clips, 3 samplers");
}

#[test]
fn searches_give_rows_their_candidates_and_the_chosen_ones_are_applied() {
    let mut missing = MissingFiles {
        rows: vec![row("a/Kick.wav", "/old/a/Kick.wav"), row("snare.wav", "/old/snare.wav")],
        ..MissingFiles::default()
    };
    missing.take(vec![("kick.wav".into(), vec![PathBuf::from("/new/a/kick.wav")]), ("snare.wav".into(), vec![])]);
    assert_eq!(missing.chosen(), vec![("a/Kick.wav".to_string(), PathBuf::from("/new/a/kick.wav"))]);
    assert!(missing.bar_shows());
    missing.bar_hidden = true;
    assert!(!missing.bar_shows());
}

#[test]
fn the_filter_and_the_words_choose_the_rows_shown() {
    let mut missing = MissingFiles {
        rows: vec![row("kick.wav", "/old/Drums/kick.wav"), row("chop.wav", "/old/Vox/chop.wav")],
        ..MissingFiles::default()
    };
    missing.rows[0].chosen = Some(PathBuf::from("/new/kick.wav"));
    assert_eq!(missing.shown(), vec![0, 1]);
    missing.filter = Filter::Missing;
    assert_eq!(missing.shown(), vec![1]);
    missing.filter = Filter::Found;
    assert_eq!(missing.shown(), vec![0]);
    missing.filter = Filter::All;
    missing.search = "vox".into();
    assert_eq!(missing.shown(), vec![1]);
}

/// Where a folder moved to is the part of the two paths before what they end in alike.
#[test]
fn a_moved_folder_is_what_the_old_and_new_path_do_not_share() {
    let moved = moved_folder(Path::new("C:/Old/Lib/Drums/kick.wav"), Path::new("D:/Samples/Lib/drums/Kick.wav"));
    assert_eq!(moved, Some((PathBuf::from("C:/Old"), PathBuf::from("D:/Samples"))));
    // Only the name alike: its folder moved.
    let moved = moved_folder(Path::new("C:/A/kick.wav"), Path::new("D:/B/kick.wav"));
    assert_eq!(moved, Some((PathBuf::from("C:/A"), PathBuf::from("D:/B"))));
    // A renamed file, or a path that is all alike, tells nothing.
    assert_eq!(moved_folder(Path::new("C:/A/kick.wav"), Path::new("D:/B/kick 2.wav")), None);
    assert_eq!(moved_folder(Path::new("C:/A/kick.wav"), Path::new("C:/A/kick.wav")), None);
}

#[test]
fn a_pasted_path_loses_its_quotes_and_the_lines_after_it() {
    assert_eq!(pasted_path("\"C:\\Samples\\kick.wav\"\r\n"), Some(PathBuf::from("C:\\Samples\\kick.wav")));
    assert_eq!(pasted_path("  /samples/kick.wav\n/other.wav"), Some(PathBuf::from("/samples/kick.wav")));
    assert_eq!(pasted_path(" \n "), None);
}

/// Locating one file finds the others beside it and where its old folder moved to, and leaves
/// a file chosen by hand as it was.
#[test]
fn locating_one_file_finds_the_others_where_it_was() {
    let old = temp("old");
    let new = temp("new");
    for name in ["Lib/Drums/kick.wav", "Lib/Drums/snare.wav", "Lib/Vox/chop.wav", "Lib/Vox/hand.wav"] {
        file(&new.join(name));
    }
    file(&new.join("elsewhere/hand.wav"));
    let mut missing = MissingFiles {
        rows: vec![
            row("kick.wav", &old.join("Lib/Drums/kick.wav").display().to_string()),
            row("snare.wav", &old.join("Lib/Drums/snare.wav").display().to_string()),
            row("chop.wav", &old.join("Lib/Vox/chop.wav").display().to_string()),
            row("hand.wav", &old.join("Lib/Vox/hand.wav").display().to_string()),
        ],
        ..MissingFiles::default()
    };
    missing.rows[3].choose(new.join("elsewhere/hand.wav"));
    missing.locate("kick.wav", new.join("Lib/Drums/kick.wav"));
    assert_eq!(missing.rows[0].chosen, Some(new.join("Lib/Drums/kick.wav")));
    assert_eq!(missing.rows[0].state(), State::Chosen);
    assert_eq!(missing.rows[1].chosen, Some(new.join("Lib/Drums/snare.wav")), "beside it");
    assert_eq!(missing.rows[2].chosen, Some(new.join("Lib/Vox/chop.wav")), "where its folder moved");
    assert_eq!(missing.rows[3].chosen, Some(new.join("elsewhere/hand.wav")), "chosen by hand");
    assert_eq!(missing.note.as_deref(), Some("Located kick.wav, found 2 more beside it"));
    assert_eq!(missing.selected.as_deref(), Some("kick.wav"));
    // Nothing is missing, so no folder is walked.
    assert!(missing.walk.is_none());
    for dir in [old, new] {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// A file dropped or pasted goes to the row it was meant for, or else to the file of its name;
/// a folder is searched, and the files still missing are found in its subfolders.
#[test]
fn a_dropped_path_goes_to_its_row_or_its_name_and_a_folder_is_searched() {
    let dir = temp("dropped");
    let kick = dir.join("one/kick.wav");
    let other = dir.join("one/other.wav");
    let snare = dir.join("deep/er/snare.wav");
    for path in [&kick, &other, &snare] {
        file(path);
    }
    let mut missing = MissingFiles {
        rows: vec![row("kick.wav", "/gone/kick.wav"), row("snare.wav", "/gone/snare.wav")],
        ..MissingFiles::default()
    };
    // Dropped onto the snare's row: the snare is that file, whatever its name.
    missing.take_path(other.clone(), Some("snare.wav".into()));
    assert_eq!(missing.rows[1].chosen, Some(other.clone()));
    // Dropped elsewhere: the file of its name.
    missing.take_path(kick.clone(), None);
    assert_eq!(missing.rows[0].chosen, Some(kick));
    missing.take_path(dir.join("one/nothing.wav"), None);
    assert!(missing.note.as_deref().is_some_and(|n| n.starts_with("No file at")));
    missing.take_path(other, None);
    assert_eq!(missing.note.as_deref(), Some("other.wav is not one of the missing files"));

    // A folder: walked for the files still missing.
    missing.rows[1] = row("snare.wav", "/gone/snare.wav");
    missing.take_path(dir.clone(), None);
    assert!(missing.walk.is_some());
    missing.wait_for_walk();
    assert_eq!(missing.rows[1].chosen, Some(snare));
    assert!(missing.note.as_deref().is_some_and(|n| n.ends_with("1 found")), "{:?}", missing.note);
    let _ = std::fs::remove_dir_all(dir);
}

/// A pasted file goes to the selected row, unless it has another missing file's name and not
/// the selected one's.
#[test]
fn a_pasted_file_goes_to_the_row_of_its_name() {
    let missing = MissingFiles {
        rows: vec![row("kick.wav", "/gone/kick.wav"), row("Drums/snare.wav", "/gone/snare.wav")],
        selected: Some("kick.wav".into()),
        ..MissingFiles::default()
    };
    assert_eq!(missing.paste_target(Path::new("/x/Snare.wav")).as_deref(), Some("Drums/snare.wav"));
    assert_eq!(missing.paste_target(Path::new("/x/kick.wav")).as_deref(), Some("kick.wav"));
    assert_eq!(missing.paste_target(Path::new("/x/renamed kick.wav")).as_deref(), Some("kick.wav"));
}

/// A file located while a folder is walked has its own folder walked after it; a project
/// opened meanwhile stops the walk and forgets the ones waiting.
#[test]
fn a_folder_located_during_a_walk_is_walked_after_it() {
    let dir = temp("queued");
    let kick = dir.join("one/kick.wav");
    let snare = dir.join("one/deep/snare.wav");
    file(&kick);
    file(&snare);
    file(&dir.join("other/x.wav"));
    let mut missing = MissingFiles {
        rows: vec![row("kick.wav", "/gone/a/kick.wav"), row("snare.wav", "/gone/b/snare.wav")],
        ..MissingFiles::default()
    };
    missing.search_folder(dir.join("other"));
    assert!(missing.walk.is_some());
    missing.locate("kick.wav", kick);
    assert_eq!(missing.queued, vec![dir.join("one")]);
    missing.wait_for_walk();
    assert!(missing.queued.is_empty());
    assert_eq!(missing.walk.as_ref().map(|w| w.folder.clone()), Some(dir.join("one")));
    missing.wait_for_walk();
    assert_eq!(missing.rows[1].chosen, Some(snare));
    let _ = std::fs::remove_dir_all(dir);
}

/// A sampler inside a device group names its file as one on the track does: changing it
/// changes the key the list is rebuilt by.
#[test]
fn a_grouped_sampler_s_file_is_in_the_key() {
    use suhina_engine::model::{Branch, Device, DeviceGroup, DeviceKind};
    let mut project = crate::demo::demo_project();
    let sampler = Device::new("Sampler", DeviceKind::Builtin("sampler".into()));
    let mut branch = Branch::new("Branch 1");
    branch.devices.push(sampler);
    project.channels[0].devices.push(Device::group("Group", DeviceGroup::with_branches(vec![branch])));
    let before = super::audio_files_key(&project);
    let group = project.channels[0].devices.last_mut().and_then(Device::as_group_mut).unwrap();
    group.branches[0].devices[0].set_sample_file("samples/Kick.wav");
    assert_ne!(super::audio_files_key(&project), before);
}

/// The panel in a window: Locate… asks for the file dialog for its row, a filter narrows the
/// table, and the selected file's panel copies its name.
#[test]
fn the_panel_s_buttons_ask_for_what_they_say() {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    let mut found = row("Drums/Kick 909.wav", "D:/Samples/Drums/Kick 909.wav");
    found.offer(vec![PathBuf::from("E:/Library/Drums/Kick 909.wav")]);
    let lost = row("bass loop.wav", "D:/Projects/Song/bass loop.wav");
    let state =
        (MissingFiles { rows: vec![found, lost], open: true, ..MissingFiles::default() }, Vec::<Outcome>::new());
    let mut harness = Harness::builder().allow_missing_glyphs().with_size(egui::vec2(1280.0, 760.0)).build_ui_state(
        |ui, (missing, asked): &mut (MissingFiles, Vec<Outcome>)| {
            crate::kit::theme::install(ui.ctx());
            let area = ui.max_rect();
            if let Some(outcome) = missing.panel(ui, area, "Song") {
                asked.push(outcome);
            }
        },
        state,
    );
    harness.run();
    harness.get_by_label("Locate bass loop.wav").click();
    harness.run();
    assert_eq!(harness.state().1, vec![Outcome::PickFile]);
    assert_eq!(harness.state().0.selected.as_deref(), Some("bass loop.wav"));

    harness.get_by_label("Show missing (1)").click();
    harness.run();
    assert_eq!(harness.state().0.filter, Filter::Missing);
    assert!(harness.query_by_label("Locate Kick 909.wav").is_none());

    harness.get_by_label("Copy the name of bass loop.wav").click();
    harness.run();
    assert_eq!(harness.state().0.note.as_deref(), Some("Copied bass loop.wav"));

    harness.get_by_label("Point the clips at the files found, one undo step").click();
    harness.run();
    let applied = vec![("Drums/Kick 909.wav".to_string(), PathBuf::from("E:/Library/Drums/Kick 909.wav"))];
    assert_eq!(harness.state().1.last(), Some(&Outcome::Apply(applied)));
    assert!(!harness.state().0.open);
}
