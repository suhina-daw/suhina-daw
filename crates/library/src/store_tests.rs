// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("suhina-store-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_long_list_is_read_up_to_its_limit_without_comparing_every_entry() {
    // More different pins than the limit, then entries that are not text: each one read after the
    // limit is reached would add a warning, so one warning shows the read stopped there.
    let mut pins: Vec<Value> = (0..=MAX_PINS).map(|i| json!(format!("/p/{i}.wav"))).collect();
    pins.extend((0..1000).map(|_| json!(7)));
    let mut warnings = Vec::new();
    let kept = texts(Some(&json!(pins)), MAX_PINS, "pinned", &mut warnings);
    assert_eq!(kept.len(), MAX_PINS);
    assert_eq!(kept.first().map(String::as_str), Some("/p/0.wav"));
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    // Repeats do not count towards the limit.
    let repeated = json!(["a", "a", "b", "a", "c"]);
    assert_eq!(texts(Some(&repeated), 2, "pinned", &mut Vec::new()), ["a", "b"]);
    let mut warnings = Vec::new();
    assert_eq!(texts(Some(&json!(["a", "a", "b"])), 2, "pinned", &mut warnings), ["a", "b"]);
    assert!(warnings.is_empty(), "exactly the limit is no warning: {warnings:?}");
}

#[test]
fn a_store_round_trips_and_a_missing_file_is_empty() {
    let dir = temp("round");
    let file = dir.join("browser.json");
    let (empty, warnings) = Store::load(&file);
    assert_eq!((empty.clone(), warnings.len()), (Store::default(), 0));
    assert_eq!(empty.tags.len(), 7);
    let mut store = Store::default();
    store.folders.push(PathBuf::from("/samples"));
    store.pin("/samples/kick.wav");
    store.record(Group::Samples, "/samples/a.wav");
    store.record(Group::Samples, "/samples/b.wav");
    store.record(Group::Samples, "/samples/a.wav");
    store.set_tag("Drums", Some("#112233"), None).unwrap();
    store.tag("/samples/kick.wav", "Drums", true).unwrap();
    store.plugins_view = PluginsView::Effects;
    store.save(&file).unwrap();
    let (loaded, warnings) = Store::load(&file);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(loaded, store);
    assert_eq!(loaded.plugins_view, PluginsView::Effects);
    assert_eq!(loaded.recent[&Group::Samples], ["/samples/a.wav", "/samples/b.wav"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_plugins_view_that_is_not_one_falls_back_to_instruments() {
    let dir = temp("view");
    let file = dir.join("browser.json");
    for junk in ["all", "7", "{", ""] {
        std::fs::write(&file, format!("{{ \"plugins_view\": \"{junk}\" }}")).unwrap();
        let (store, warnings) = Store::load(&file);
        assert_eq!(store.plugins_view, PluginsView::Instruments, "{junk}");
        assert_eq!(warnings.len(), 1, "{junk}");
    }
    // A view an earlier version kept is read as the default, quietly.
    for earlier in ["grouped", "type", "flat"] {
        std::fs::write(&file, format!("{{ \"plugins_view\": \"{earlier}\" }}")).unwrap();
        let (store, warnings) = Store::load(&file);
        assert_eq!((store.plugins_view, warnings.len()), (PluginsView::Instruments, 0), "{earlier}");
    }
    std::fs::write(&file, r#"{}"#).unwrap();
    assert_eq!(Store::load(&file).0.plugins_view, PluginsView::Instruments, "missing is the default, quietly");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hostile_files_give_warnings_not_failures() {
    let dir = temp("hostile");
    let file = dir.join("browser.json");
    std::fs::write(&file, "not json").unwrap();
    let (store, warnings) = Store::load(&file);
    assert_eq!((store, warnings.len()), (Store::default(), 1));
    std::fs::write(&file, "[1,2,3]").unwrap();
    assert_eq!(Store::load(&file).1.len(), 1);
    let many: Vec<String> = (0..2000).map(|i| format!("/p/{i}")).collect();
    let text = json!({
            "folders": "nope",
            "pinned": many,
            "recent": { "samples": ["/a"], "videos": ["/v"] },
            "tags": [{ "name": "", "color": "#000000" }, { "name": "Ok", "color": "teal" }, { "name": "Fine", "color": "#AABBCC" }],
            "tagged": { "/a": ["Fine", "Unknown"] },
            "tag_limit": 99999,
        })
        .to_string();
    std::fs::write(&file, text).unwrap();
    let (store, warnings) = Store::load(&file);
    assert_eq!(store.pins.len(), MAX_PINS);
    assert!(store.folders.is_empty());
    assert_eq!(store.tags, [Tag { name: "Fine".into(), color: "#AABBCC".into() }]);
    assert_eq!(store.tagged["/a"], ["Fine"]);
    assert_eq!(store.tag_limit, DEFAULT_TAG_LIMIT);
    assert_eq!(warnings.len(), 6, "{warnings:#?}");
    std::fs::write(&file, vec![b' '; (MAX_FILE_BYTES + 1) as usize]).unwrap();
    assert_eq!(Store::load(&file).1.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tags_are_renamed_removed_and_limited() {
    let mut store = Store::default();
    store.tag("x", "Red", true).unwrap();
    store.set_tag("Red", None, Some("Hot")).unwrap();
    assert_eq!(store.tagged["x"], ["Hot"]);
    assert!(store.set_tag("Hot", Some("red"), None).is_err());
    assert!(store.set_tag("New", None, None).is_err(), "a new tag needs a colour");
    assert!(store.tag("x", "Nothing", true).is_err());
    store.remove_tag("Hot").unwrap();
    assert!(store.tagged.is_empty());
    assert!(store.set_tag_limit(3).is_err(), "six tags are left");
    store.set_tag_limit(6).unwrap();
    assert!(store.set_tag("Seventh", Some("#000000"), None).is_err());
    store.set_tag_limit(10).unwrap();
    store.set_tag("Seventh", Some("#000000"), None).unwrap();
    assert!(store.set_tag_limit(MAX_TAG_LIMIT + 1).is_err());
}

#[test]
fn pins_and_recent_items_keep_their_limits() {
    let mut store = Store::default();
    for i in 0..MAX_PINS + 5 {
        store.pin(&format!("/{i}"));
    }
    assert_eq!(store.pins.len(), MAX_PINS);
    assert_eq!(store.pins[0], "/5", "the oldest went");
    assert!(!store.pin("/7"), "already pinned");
    assert!(store.unpin("/7") && !store.unpin("/7"));
    for i in 0..80 {
        store.record(Group::Midi, &format!("/m{i}"));
    }
    assert_eq!(store.recent[&Group::Midi].len(), MAX_RECENT);
    assert_eq!(store.recent[&Group::Midi][0], "/m79");
}

#[test]
fn a_moved_files_pin_recent_place_and_tags_go_with_it() {
    let mut store = Store::default();
    store.pin("/a/kick.wav");
    store.record(Group::Samples, "/a/kick.wav");
    store.tagged.insert("/a/kick.wav".into(), vec!["Red".into()]);
    assert!(store.relocate("/a/kick.wav", "/b/kick.wav"));
    assert_eq!(store.pins, ["/b/kick.wav"]);
    assert_eq!(store.recent.get(&Group::Samples).cloned().unwrap_or_default(), ["/b/kick.wav"]);
    assert_eq!(store.tagged.get("/b/kick.wav").cloned().unwrap_or_default(), ["Red"]);
    assert!(!store.tagged.contains_key("/a/kick.wav"));
    assert!(!store.relocate("/a/kick.wav", "/c/kick.wav"), "nothing left to move");
}
