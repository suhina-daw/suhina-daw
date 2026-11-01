// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The library as a whole: folders added and kept on disk, searching, listing, the background
//! thread and its watching.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::*;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("suhina-library-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn touch(path: &Path) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, b"x").unwrap();
}

/// A sample pack: drums, loops and MIDI.
fn pack(root: &Path) {
    for file in [
        "Drums/Kick 1.wav",
        "Drums/Kick 2.wav",
        "Drums/Kick 10.wav",
        "Drums/Snare Tight.wav",
        "Loops/Funky Kick Loop 120bpm.wav",
        "Loops/kick.wav",
        "MIDI/Bass Line.mid",
        "Presets/bells.syx",
        "Docs/readme.txt",
    ] {
        touch(&root.join(file));
    }
}

#[test]
fn folders_are_added_kept_and_read_back_with_their_index() {
    let settings = temp("settings");
    let samples = temp("samples");
    pack(&samples);
    let mut library = Library::new(Some(settings.clone()));
    let added = library.add_folder(&samples).unwrap();
    assert_eq!(added, samples);
    assert!(library.add_folder(&samples).is_err(), "twice");
    assert!(library.add_folder(&samples.join("Docs/readme.txt")).is_err(), "not a folder");
    assert!(library.add_folder(&samples.join("nothing")).is_err());
    let status = library.folders();
    assert_eq!((status.len(), status[0].files), (1, 8));

    // A new library on the same settings reads the folder and its index from disk.
    let mut again = Library::new(Some(settings.clone()));
    assert_eq!(again.folders()[0].files, 8);
    assert_eq!(again.files(Some(FileKind::Midi)).map(|f| f.rel).collect::<Vec<_>>(), ["MIDI/Bass Line.mid"]);
    again.remove_folder(&samples).unwrap();
    assert!(Library::new(Some(settings.clone())).folders().is_empty());
    assert!(again.remove_folder(&samples).is_err());
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&samples);
}

#[test]
fn search_ranks_exact_then_prefix_then_word_start_then_the_rest() {
    let samples = temp("search");
    pack(&samples);
    let mut library = Library::default();
    library.add_folder(&samples).unwrap();
    let names = |library: &mut Library, query: Query| -> Vec<String> {
        library.search(&query).iter().map(|f| f.name().to_string()).collect()
    };
    let found = names(&mut library, Query { text: "kick".into(), ..Query::default() });
    assert_eq!(found, ["kick.wav", "Kick 1.wav", "Kick 2.wav", "Kick 10.wav", "Funky Kick Loop 120bpm.wav"]);
    // Every word must match, in the name or the path below the folder.
    assert_eq!(names(&mut library, Query { text: "drums snare".into(), ..Query::default() }), ["Snare Tight.wav"]);
    assert_eq!(names(&mut library, Query { text: "KICK loop".into(), ..Query::default() }).len(), 2);
    assert!(names(&mut library, Query { text: "nothing like it".into(), ..Query::default() }).is_empty());
    // Kinds, a folder inside and the limit narrow it.
    assert_eq!(names(&mut library, Query { kinds: vec![FileKind::Midi], ..Query::default() }), ["Bass Line.mid"]);
    let within = Query { text: "kick".into(), within: Some(samples.join("Drums")), ..Query::default() };
    assert_eq!(names(&mut library, within).len(), 3);
    assert_eq!(names(&mut library, Query { limit: 2, ..Query::default() }).len(), 2);
    // Tags.
    let kick = samples.join("Loops").join("kick.wav").display().to_string();
    library.tag(&kick, "Red", true).unwrap();
    assert_eq!(names(&mut library, Query { tags: vec!["Red".into()], ..Query::default() }), ["kick.wav"]);
    let _ = std::fs::remove_dir_all(&samples);
}

#[test]
fn listing_shows_folders_first_in_natural_order_and_only_known_kinds() {
    let samples = temp("list");
    pack(&samples);
    let library = Library::default();
    let drums = library.list(&samples.join("Drums")).unwrap();
    assert_eq!(
        drums.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(),
        ["Kick 1.wav", "Kick 2.wav", "Kick 10.wav", "Snare Tight.wav"]
    );
    let top = library.list(&samples).unwrap();
    assert!(top.iter().all(|l| l.kind.is_none()), "only folders at the top");
    assert!(top.iter().any(|l| l.name == "Docs"), "folders are shown even when empty of known files");
    assert!(library.list(&samples.join("missing")).is_err());
    let _ = std::fs::remove_dir_all(&samples);
}

#[test]
fn holds_tells_whether_a_folder_has_files_of_a_kind() {
    let samples = temp("holds");
    pack(&samples);
    let mut library = Library::default();
    library.add_folder(&samples).unwrap();
    assert_eq!(library.holds(&samples.join("Drums"), &[FileKind::Sample]), Some(true));
    assert_eq!(library.holds(&samples.join("Drums"), &[FileKind::Midi]), Some(false));
    assert_eq!(library.holds(&samples.join("Docs"), &[]), Some(false));
    assert_eq!(library.holds(&std::env::temp_dir(), &[]), None, "not in the library");
    let _ = std::fs::remove_dir_all(&samples);
}

#[test]
fn natural_order_compares_numbers_as_numbers() {
    let mut names = vec!["Kick 10", "kick 2", "Kick 1", "Kick 02b", "Hat", "Kick"];
    names.sort_by(|a, b| natural_cmp(a, b));
    assert_eq!(names, ["Hat", "Kick", "Kick 1", "kick 2", "Kick 02b", "Kick 10"]);
}

#[test]
fn the_background_thread_reads_new_folders_and_refreshes() {
    let settings = temp("thread-settings");
    let samples = temp("thread");
    pack(&samples);
    let mut library = Library::new(Some(settings.clone()));
    library.start().unwrap();
    library.add_folder(&samples).unwrap();
    assert!(library.wait_idle(Duration::from_secs(20)));
    let status = library.folders();
    assert_eq!(status[0].files, 8);
    assert!(matches!(status[0].state, FolderState::Watching | FolderState::Polling), "{:?}", status[0].state);

    touch(&samples.join("Drums/Clap.wav"));
    library.refresh(Some(&samples)).unwrap();
    assert!(library.wait_idle(Duration::from_secs(20)));
    assert_eq!(library.folders()[0].files, 9);
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&samples);
}

#[test]
fn a_watched_folder_notices_new_files_by_itself() {
    let samples = temp("watch");
    pack(&samples);
    let mut library = Library::default();
    library.start().unwrap();
    library.add_folder(&samples).unwrap();
    assert!(library.wait_idle(Duration::from_secs(20)));
    if library.folders()[0].state != FolderState::Watching {
        return; // A system without file-system events (a network drive) polls instead.
    }
    touch(&samples.join("Loops/new loop.wav"));
    let until = Instant::now() + Duration::from_secs(10);
    while Instant::now() < until && library.folders()[0].files != 9 {
        library.poll();
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(library.folders()[0].files, 9);
    let _ = std::fs::remove_dir_all(&samples);
}

#[test]
fn a_saved_index_is_brought_up_to_date_when_the_library_starts() {
    let settings = temp("start-settings");
    let samples = temp("start");
    pack(&samples);
    Library::new(Some(settings.clone())).add_folder(&samples).unwrap();
    // While Suhina was closed: a file went and a folder came.
    std::fs::remove_file(samples.join("Drums/Kick 1.wav")).unwrap();
    touch(&samples.join("New/hit.wav"));
    // Times have a second's resolution on some systems; make the saved ones old.
    let mut library = Library::new(Some(settings.clone()));
    let file = library.index_file(&samples).unwrap();
    let mut saved = FolderIndex::load(&samples, &file).unwrap();
    for time in saved.dirs.values_mut() {
        *time = 0;
    }
    saved.save(&samples, &file).unwrap();
    assert_eq!(library.folders()[0].files, 8, "the saved index, as it was");
    library.start().unwrap();
    assert!(library.wait_idle(Duration::from_secs(20)));
    let rels: Vec<String> = library.files(None).map(|f| f.rel).collect();
    assert!(rels.contains(&"New/hit.wav".to_string()) && !rels.contains(&"Drums/Kick 1.wav".to_string()), "{rels:?}");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&samples);
}

#[test]
fn pins_recent_and_tags_are_kept() {
    let settings = temp("places");
    let mut library = Library::new(Some(settings.clone()));
    library.pin("device:overdrive").unwrap();
    library.record(Group::Instruments, "device:fm-synth").unwrap();
    library.set_tag("Vocals", Some("#AA00AA"), None).unwrap();
    library.tag("/x.wav", "Vocals", true).unwrap();
    let mut again = Library::new(Some(settings.clone()));
    assert_eq!(again.pinned(), ["device:overdrive"]);
    assert_eq!(again.recent(Group::Instruments), ["device:fm-synth"]);
    assert_eq!(again.item_tags("/x.wav"), ["Vocals"]);
    assert!(again.tags().iter().any(|t| t.name == "Vocals"));
    // The count the footer's dot hovers with, without copying the tagged items.
    assert_eq!(again.tagged_count("Vocals"), 1);
    assert_eq!(again.tagged_count("Nothing"), 0);
    again.tag("/y.wav", "Vocals", true).unwrap();
    assert_eq!(again.tagged_count("Vocals"), 2);
    again.tag("/y.wav", "Vocals", false).unwrap();
    assert_eq!(again.tagged_count("Vocals"), 1);
    assert!(again.take_warnings().is_empty());
    let _ = std::fs::remove_dir_all(&settings);
}

#[test]
fn network_paths_are_polled() {
    if cfg!(windows) {
        assert!(on_network_drive(Path::new(r"\\server\share\samples")));
        assert!(!on_network_drive(Path::new(r"C:\samples")));
        assert!(!on_network_drive(Path::new(r"\\?\C:\samples")));
    }
    assert!(!on_network_drive(&std::env::temp_dir()));
}

#[test]
fn a_prepared_search_answers_as_a_search_does_and_a_cancelled_one_finds_nothing() {
    let samples = temp("prepared");
    pack(&samples);
    let mut library = Library::default();
    library.add_folder(&samples).unwrap();
    let query = Query { text: "kick".into(), ..Query::default() };
    let search = library.prepare_search(&query);
    assert!(search.files() > 0);
    let looking = std::sync::atomic::AtomicBool::new(false);
    assert_eq!(search.run(&looking), library.search(&query));
    looking.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(search.run(&looking).is_empty());
    // Fewer results than matches keep the best ones, in order.
    let two = Query { text: "kick".into(), limit: 2, ..Query::default() };
    assert_eq!(library.search(&two), library.search(&query)[..2]);
}
