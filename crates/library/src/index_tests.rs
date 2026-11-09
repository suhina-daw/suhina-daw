// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("suhina-index-{name}-{}", std::process::id()));
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

#[test]
fn indexes_files_of_known_kinds_and_every_folder() {
    let root = temp("scan");
    touch(&root.join("Kick.wav"));
    touch(&root.join("Loops/Drums/break 120bpm.flac"));
    touch(&root.join("Loops/readme.txt"));
    touch(&root.join("MIDI/groove.MID"));
    touch(&root.join("bank.syx"));
    std::fs::create_dir_all(root.join("Empty")).unwrap();
    let index = FolderIndex::scan(&root);
    let files: Vec<(&str, FileKind)> = index.files.iter().map(|(k, e)| (k.as_str(), e.kind)).collect();
    assert_eq!(
        files,
        [
            ("Kick.wav", FileKind::Sample),
            ("Loops/Drums/break 120bpm.flac", FileKind::Sample),
            ("MIDI/groove.MID", FileKind::Midi),
            ("bank.syx", FileKind::Preset),
        ]
    );
    let dirs: Vec<&str> = index.dirs.keys().map(String::as_str).collect();
    assert_eq!(dirs, ["", "Empty", "Loops", "Loops/Drums", "MIDI"]);
    assert_eq!(index.note, None);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn saves_and_loads_and_refuses_another_folders_or_a_damaged_index() {
    let root = temp("save");
    touch(&root.join("a/b.wav"));
    let index = FolderIndex::scan(&root);
    let file = root.join("index.tsv");
    index.save(&root, &file).unwrap();
    assert_eq!(FolderIndex::load(&root, &file), Some(index.clone()));
    assert_eq!(FolderIndex::load(&root.join("a"), &file), None, "another folder's");
    std::fs::write(&file, "something else\n").unwrap();
    assert_eq!(FolderIndex::load(&root, &file), None);
    std::fs::write(
        &file,
        format!("{HEADER}\nroot\t{}\nf\tx.wav\tsample\tnot-a-size\t0\nf\ty.wav\tsample\t3\t4\nzzz\n", root.display()),
    )
    .unwrap();
    let loaded = FolderIndex::load(&root, &file).unwrap();
    assert_eq!(loaded.files.keys().collect::<Vec<_>>(), ["y.wav"], "bad lines are skipped");
    let saved = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, saved.replacen(HEADER, OLD_HEADER, 1)).unwrap();
    assert_eq!(FolderIndex::load(&root, &file), Some(loaded), "an index from before the rename");
    let _ = std::fs::remove_dir_all(&root);
}

/// A save that cannot write (its folder made read-only) leaves the old index whole and no
/// partial file beside it.
#[cfg(unix)]
#[test]
fn a_failed_save_keeps_the_old_index_and_leaves_nothing_beside_it() {
    use std::os::unix::fs::PermissionsExt;
    let root = temp("failed-save");
    touch(&root.join("a.wav"));
    let index = FolderIndex::scan(&root);
    let folder = root.join("indexes");
    let file = folder.join("index.tsv");
    index.save(&root, &file).unwrap();
    let old = std::fs::read(&file).unwrap();
    let mut grown = index.clone();
    grown.note = Some("a new note".into());
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).unwrap();
    let failed = grown.save(&root, &file);
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(failed.is_err());
    assert_eq!(std::fs::read(&file).unwrap(), old, "the old index is whole");
    let names: Vec<_> = std::fs::read_dir(&folder).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names, ["index.tsv"], "nothing is left beside it");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn refresh_reads_changed_folders_and_drops_removed_ones() {
    let root = temp("refresh");
    touch(&root.join("keep/one.wav"));
    touch(&root.join("gone/two.wav"));
    let mut index = FolderIndex::scan(&root);
    assert!(!index.refresh(&root), "nothing changed");
    std::fs::remove_dir_all(root.join("gone")).unwrap();
    touch(&root.join("new/deep/three.mid"));
    // Folder times have a resolution of a second on some systems; the root's entry list
    // changed, so make sure its time moves.
    index.dirs.insert(String::new(), 0);
    assert!(index.refresh(&root));
    let files: Vec<&str> = index.files.keys().map(String::as_str).collect();
    assert_eq!(files, ["keep/one.wav", "new/deep/three.mid"]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_event_updates_the_folder_it_names() {
    let root = temp("event");
    touch(&root.join("pack/a.wav"));
    let mut index = FolderIndex::scan(&root);
    touch(&root.join("pack/b.wav"));
    touch(&root.join("pack/sub/c.wav"));
    assert!(index.update_paths(&root, &[root.join("pack/b.wav"), root.join("pack/sub")]));
    assert_eq!(index.files.keys().collect::<Vec<_>>(), ["pack/a.wav", "pack/b.wav", "pack/sub/c.wav"]);
    std::fs::remove_file(root.join("pack/a.wav")).unwrap();
    assert!(index.update_paths(&root, &[root.join("pack/a.wav"), std::env::temp_dir().join("elsewhere")]));
    assert!(!index.files.contains_key("pack/a.wav"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn deep_and_huge_folders_stop_at_the_limits() {
    let root = temp("deep");
    let mut deep = root.clone();
    for i in 0..MAX_DEPTH + 3 {
        deep = deep.join(format!("d{i}"));
    }
    touch(&deep.join("lost.wav"));
    touch(&root.join("d0/found.wav"));
    let index = FolderIndex::scan(&root);
    assert!(index.files.contains_key("d0/found.wav"));
    assert!(!index.files.keys().any(|k| k.ends_with("lost.wav")));
    assert!(index.note.is_some());
    assert!(index.dirs.keys().all(|k| depth(k) <= MAX_DEPTH));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_missing_folder_has_an_empty_index() {
    let root = std::env::temp_dir().join("suhina-index-does-not-exist");
    let mut index = FolderIndex::scan(&root);
    assert!(index.files.is_empty() && index.dirs.is_empty());
    assert!(!index.refresh(&root));
}
