// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::{Session, blank_project};
use serde_json::json;

/// The threads backups were written on.
pub(super) static WRITER_THREADS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Projects whose backups and journal go to a folder of their own, with one blank project
/// open, changed once.
fn changed_projects(settings: &Path) -> (Projects, ProjectId) {
    let mut projects = Projects::new();
    projects.set_backup_folder(Some(settings.to_path_buf()));
    let id = projects.add(Session::new(blank_project()), None).unwrap();
    projects.execute("transport.setTempo", &json!({ "bpm": 133 })).unwrap();
    (projects, id)
}

fn files_in(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .map(|d| d.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.sort();
    names
}

fn wait_for_backup(projects: &mut Projects, id: ProjectId) -> Backup {
    assert!(projects.backups.flush(Duration::from_secs(10)));
    projects.tick_backups();
    projects.get(id).and_then(|s| s.last_backup().cloned()).expect("a backup was written")
}

#[test]
fn plug_in_changes_make_a_backup_due_without_project_commands_and_again_after_a_backup() {
    let settings = temporary("plugin-revisions");
    let mut projects = Projects::new();
    projects.set_backup_folder(Some(settings.clone()));
    let id = projects.add(Session::new(blank_project()), None).unwrap();
    let read = |projects: &mut Projects, byte| {
        projects.get_mut(id).unwrap().plugins.edits.read(1, &suhina_plugins::SavedState::from(vec![byte]));
    };
    read(&mut projects, 0);
    read(&mut projects, 1);
    assert!(!projects.backup_due(id), "one differing read does not count");
    read(&mut projects, 1);
    assert!(projects.back_up(id).unwrap(), "plug-in-only changes need a backup");
    wait_for_backup(&mut projects, id);
    read(&mut projects, 1);
    assert!(!projects.backup_due(id), "unchanged reads do not repeat a backup");
    read(&mut projects, 2);
    read(&mut projects, 2);
    assert!(projects.backup_due(id), "a later plug-in change needs another backup");
    assert_eq!(projects.get(id).unwrap().history.changes, 0, "no project command helped");
}

#[test]
fn unsaved_changes_are_backed_up_once_the_interval_has_passed() {
    let settings = temporary("interval");
    let (mut projects, id) = changed_projects(&settings);
    projects.set_backup_settings(Interval::Minutes(1), true);
    let tick = projects.tick_backups();
    assert!(tick.next.is_some_and(|n| n <= Duration::from_secs(60)), "{tick:?}");
    assert!(projects.get(id).unwrap().last_backup().is_none(), "not before the interval");
    // A minute later.
    projects.get_mut(id).unwrap().backup.since = Instant::now() - Duration::from_secs(61);
    let tick = projects.tick_backups();
    assert!(tick.notices.is_empty(), "{tick:?}");
    let backup = wait_for_backup(&mut projects, id);
    assert_eq!(backup.path.parent(), Some(settings.as_path()), "a project never saved backs up to settings");
    assert_eq!(
        project_name_of_file(&backup.path.file_name().unwrap().to_string_lossy()).as_deref(),
        Some("Untitled 1")
    );
    let copy = suhina_project_file::read_file(&backup.path).unwrap();
    assert_eq!(copy.transport.tempo_bpm, 133.0);
    // Nothing changed since: no second backup, however long it waits.
    projects.get_mut(id).unwrap().backup.since = Instant::now() - Duration::from_secs(600);
    assert_eq!(projects.tick_backups().next, None);
    assert!(!projects.backup_due(id));
    assert!(projects.backups.flush(Duration::from_secs(10)));
    assert_eq!(files_in(&settings).iter().filter(|n| n.ends_with(".suhina")).count(), 1);
    // The backups were written on the backup thread, never on this one.
    let threads = WRITER_THREADS.lock().unwrap().clone();
    assert!(!threads.is_empty() && threads.iter().all(|t| t == "suhina-backup"), "{threads:?}");
    let _ = std::fs::remove_dir_all(settings);
}

#[test]
fn never_and_critical_write_nothing_on_a_timer() {
    let settings = temporary("never");
    let (mut projects, id) = changed_projects(&settings);
    for interval in [Interval::Never, Interval::Critical] {
        projects.set_backup_settings(interval, true);
        projects.get_mut(id).unwrap().backup.since = Instant::now() - Duration::from_secs(100_000);
        assert_eq!(projects.tick_backups(), Tick::default());
    }
    assert!(projects.backups.writer.is_none(), "nothing was sent");
    let _ = std::fs::remove_dir_all(settings);
}

#[test]
fn a_saved_project_backs_up_beside_its_file_and_a_save_takes_the_offer_away() {
    let settings = temporary("save-settings");
    let music = temporary("save-music");
    let (mut projects, id) = changed_projects(&settings);
    projects.set_backup_settings(Interval::Critical, true);
    // A backup while it had no file, in the settings folder.
    let first = projects.execute("project.autosave", &json!({ "now": true })).unwrap();
    let in_settings = PathBuf::from(first["written"].as_str().unwrap());
    assert!(in_settings.starts_with(&settings));
    let file = music.join("Song.suhina");
    projects.execute("project.save", &json!({ "path": file.display().to_string() })).unwrap();
    assert!(projects.backups.flush(Duration::from_secs(10)));
    assert!(!in_settings.exists(), "the save took away the copy in the settings folder");
    assert!(read_journal(&settings.join(JOURNAL)).is_empty(), "nothing to offer after a save");
    // Saved, so deleting a track needs no backup before it.
    projects.execute("track.delete", &json!({ "track": 1 })).unwrap();
    assert!(projects.backups.flush(Duration::from_secs(10)));
    assert!(!music.join(FOLDER).exists());
    projects.execute("edit.undo", &json!({})).unwrap();
    projects.execute("project.save", &json!({})).unwrap();
    // Changed: deleting a track is critical, so a backup goes to .backup first.
    projects.execute("transport.setTempo", &json!({ "bpm": 120 })).unwrap();
    projects.execute("track.delete", &json!({ "track": 1 })).unwrap();
    let backup = wait_for_backup(&mut projects, id);
    assert_eq!(backup.path.parent(), Some(music.join(FOLDER).as_path()));
    assert!(project_name_of_file(&backup.path.file_name().unwrap().to_string_lossy()).is_some_and(|n| n == "Song"));
    assert_eq!(suhina_project_file::read_file(&backup.path).unwrap().tracks.len(), blank_project().tracks.len());
    assert_eq!(read_journal(&settings.join(JOURNAL)).len(), 1);
    let info = projects.execute("project.autosave", &json!({})).unwrap();
    assert_eq!(info["interval"], "critical");
    assert_eq!(info["folder"], music.join(FOLDER).display().to_string());
    assert_eq!(info["last_backup"]["path"], backup.path.display().to_string());
    assert_eq!(info["due"], true, "the delete itself is not backed up yet");
    // Saving again leaves the history in .backup but takes the journal entry away.
    projects.execute("project.save", &json!({})).unwrap();
    assert!(projects.backups.flush(Duration::from_secs(10)));
    assert!(backup.path.exists());
    assert!(read_journal(&settings.join(JOURNAL)).is_empty());
    let _ = std::fs::remove_dir_all(settings);
    let _ = std::fs::remove_dir_all(music);
}

#[test]
fn a_backup_left_by_a_crash_is_offered_and_opens_unsaved_tied_to_its_file() {
    let settings = temporary("crash-settings");
    let music = temporary("crash-music");
    let file = music.join("Song.suhina");
    {
        let mut session = Session::new(blank_project());
        session.save(&file, false).unwrap();
    }
    std::thread::sleep(Duration::from_millis(20));
    let backup = {
        // The run that crashes: opened, changed, backed up, never closed.
        let mut projects = Projects::new();
        projects.set_backup_folder(Some(settings.clone()));
        let (session, _) = Session::open(&file).unwrap();
        let id = projects.add(session, None).unwrap();
        projects.execute("transport.setTempo", &json!({ "bpm": 99 })).unwrap();
        projects.execute("project.autosave", &json!({ "now": true })).unwrap();
        wait_for_backup(&mut projects, id).path
    };
    let mut next_run = Projects::new();
    next_run.set_backup_folder(Some(settings.clone()));
    let offers = next_run.leftover_backups();
    assert_eq!(offers.len(), 1, "{offers:?}");
    let offer = offers.first().unwrap();
    assert_eq!(
        (offer.backup.clone(), offer.file.clone(), offer.name()),
        (backup.clone(), Some(file.clone()), "Song".into())
    );
    let (session, warnings) = Session::recover(&offer.backup, offer.file.as_deref()).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(session.source(), Some(file.as_path()));
    assert!(session.has_unsaved_changes());
    assert_eq!(session.project().transport.tempo_bpm, 99.0);
    let id = next_run.add(session, None).unwrap();
    assert_eq!(next_run.name(id), Some("Song"));
    assert!(!next_run.backup_due(id), "the backup already holds it");
    next_run.forget_backup(&offer.backup);
    assert!(next_run.backups.flush(Duration::from_secs(10)));
    assert!(next_run.leftover_backups().is_empty(), "offered once");
    // Saving writes the original file, and the backup stays.
    next_run.execute("project.save", &json!({})).unwrap();
    assert_eq!(suhina_project_file::read_file(&file).unwrap().transport.tempo_bpm, 99.0);
    assert!(backup.exists());
    let _ = std::fs::remove_dir_all(settings);
    let _ = std::fs::remove_dir_all(music);
}

#[test]
fn closing_and_quitting_normally_leave_nothing_to_offer() {
    let settings = temporary("quit");
    let (mut projects, id) = changed_projects(&settings);
    projects.set_backup_settings(Interval::Critical, true);
    // Closing is critical: the changes are backed up, then the entry goes.
    projects.execute("project.close", &json!({ "project": id.0 })).unwrap();
    assert!(projects.backups.flush(Duration::from_secs(10)));
    assert_eq!(files_in(&settings).iter().filter(|n| n.ends_with(".suhina")).count(), 1);
    assert!(read_journal(&settings.join(JOURNAL)).is_empty());
    let other = projects.add(Session::new(blank_project()), None).unwrap();
    projects.execute("transport.setTempo", &json!({ "bpm": 90 })).unwrap();
    assert!(projects.finish_backups(Duration::from_secs(10)));
    assert_eq!(files_in(&settings).iter().filter(|n| n.ends_with(".suhina")).count(), 2, "quitting is critical");
    assert!(read_journal(&settings.join(JOURNAL)).is_empty());
    assert!(projects.get(other).is_some());
    let mut next_run = Projects::new();
    next_run.set_backup_folder(Some(settings.clone()));
    assert!(next_run.leftover_backups().is_empty());
    let _ = std::fs::remove_dir_all(settings);
}

#[test]
fn autosave_checks_its_parameters_and_reports_without_a_project() {
    let mut projects = Projects::new();
    let info = projects.execute("project.autosave", &json!({ "interval": "4h", "cleanUp": false })).unwrap();
    assert_eq!(
        (info["interval"].clone(), info["clean_up"].clone(), info["due"].clone()),
        (json!("4h"), json!(false), json!(false))
    );
    assert_eq!(info["last_backup"], Value::Null);
    assert!(projects.execute("project.autosave", &json!({ "interval": "2m" })).is_err());
    assert!(projects.execute("project.autosave", &json!({ "cleanUp": "yes" })).is_err());
    assert!(projects.execute("project.autosave", &json!({ "now": true })).is_err(), "no project is open");
    // Untitled with no settings folder: nowhere to go, said so.
    projects.add(Session::new(blank_project()), None).unwrap();
    projects.execute("transport.setTempo", &json!({ "bpm": 100 })).unwrap();
    assert!(projects.execute("project.autosave", &json!({ "now": true })).is_err());
    // Unchanged: nothing needs writing.
    let mut clean = Projects::new();
    clean.add(Session::new(blank_project()), None).unwrap();
    assert_eq!(clean.execute("project.autosave", &json!({ "now": true })).unwrap()["written"], Value::Null);
}

#[test]
fn intervals_have_names_and_titles_and_read_back() {
    let names: Vec<String> = Interval::ALL.iter().map(|i| i.name()).collect();
    assert_eq!(names, ["critical", "1m", "5m", "10m", "15m", "30m", "60m", "4h", "never"]);
    for interval in Interval::ALL {
        assert_eq!(Interval::parse(&interval.name()), Some(interval));
    }
    assert_eq!(Interval::Minutes(240).title(), "Every 4 hours");
    assert_eq!(Interval::Minutes(1).title(), "Every minute");
    assert_eq!(Interval::Minutes(60).title(), "Every 60 minutes");
    assert_eq!(Interval::Minutes(5).period(), Some(Duration::from_secs(300)));
    assert_eq!(Interval::Critical.period(), None);
    assert_eq!(Interval::parse("7m"), None);
    assert_eq!(Interval::default(), Interval::Minutes(5));
}

#[test]
fn backup_names_are_recognised_and_others_are_not() {
    assert_eq!(project_name_of_file("Song 2026-10-15 14-05-09.suhina").as_deref(), Some("Song"));
    assert_eq!(project_name_of_file("My Song 2026-10-15 14-05-09 (12).suhina").as_deref(), Some("My Song"));
    assert_eq!(project_name_of_file("Song 2026-10-15 14:05:09.suhina"), None);
    assert_eq!(project_name_of_file("Song.suhina"), None);
    assert_eq!(project_name_of_file("Song 2026-10-15 14-05-09.wav"), None);
    assert_eq!(project_name_of_file(" 2026-10-15 14-05-09.suhina"), None);
    assert_eq!(project_name_of_file("journal.json"), None);
    assert_eq!(project_name_of_file("é 2026-10-15 14-05-0é.suhina"), None);
    assert_eq!(file_name_part("a/b: c?"), "a_b_ c_");
    assert_eq!(file_name_part("  ..  "), "Untitled");
}

#[test]
fn a_broken_or_foreign_journal_is_empty() {
    assert!(parse_journal("not json").is_empty());
    assert!(parse_journal(r#"{"format":"other","backups":[]}"#).is_empty());
    let entries = parse_journal(
        r#"{"format":"suhina-backups","version":1,"backups":[
                {"run":"a","project":1,"file":null,"backup":"/x/Untitled 1 2026-10-15 14-05-09.suhina"},
                {"run":"a","project":-3,"backup":"/y"},
                {"run":"a","project":2,"file":"/m/Song.suhina","backup":"/m/.backup/Song 2026-10-15 14-05-09.suhina"}
            ]}"#,
    );
    assert_eq!(entries.len(), 2);
    assert_eq!(parse_journal(&journal_text(&entries)), entries);
}

fn temporary(label: &str) -> PathBuf {
    let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!("suhina-backup-{label}-{nanos}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn clean_up_keeps_the_newest_of_one_project_and_touches_nothing_else() {
    let dir = temporary("clean");
    for second in 0..5 {
        std::fs::write(dir.join(format!("Song 2026-10-15 14-05-0{second}.suhina")), "x").unwrap();
        std::thread::sleep(Duration::from_millis(15));
    }
    std::fs::write(dir.join("Song B 2026-10-15 14-05-09.suhina"), "x").unwrap();
    std::fs::write(dir.join("notes.txt"), "x").unwrap();
    clean_up(&dir, "Song", 2);
    let mut left: Vec<String> =
        std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    left.sort();
    assert_eq!(
        left,
        [
            "Song 2026-10-15 14-05-03.suhina",
            "Song 2026-10-15 14-05-04.suhina",
            "Song B 2026-10-15 14-05-09.suhina",
            "notes.txt"
        ]
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_entry_is_offered_only_while_its_backup_is_newer_than_its_file() {
    let dir = temporary("offer");
    let file = dir.join("Song.suhina");
    let backup = dir.join("Song 2026-10-15 14-05-09.suhina");
    std::fs::write(&file, "old").unwrap();
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(&backup, "new").unwrap();
    let entry = Entry { run: "r".into(), project: 1, file: Some(file.clone()), backup: backup.clone() };
    assert!(offered(&entry).is_some());
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(&file, "saved").unwrap();
    assert!(offered(&entry).is_none(), "saved after the backup");
    std::fs::remove_file(&file).unwrap();
    assert!(offered(&entry).is_some(), "the project file is gone");
    std::fs::remove_file(&backup).unwrap();
    assert!(offered(&entry).is_none(), "the backup is gone");
    let _ = std::fs::remove_dir_all(dir);
}
