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
    let fine = store.tags.first().map(|e| e.id()).unwrap_or(0);
    assert_eq!(store.tags.first().map(|e| e.name().to_string()), Some("Fine".to_string()));
    assert_eq!(store.tagged["/a"], [fine]);
    assert_eq!(store.tag_limit, DEFAULT_TAG_LIMIT);
    assert_eq!(warnings.len(), 7, "{warnings:#?}");
    std::fs::write(&file, vec![b' '; (MAX_FILE_BYTES + 1) as usize]).unwrap();
    assert_eq!(Store::load(&file).1.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
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
    let red = store.tags.first().map(|e| e.id()).unwrap_or(0); // Red is the first default
    store.tagged.insert("/a/kick.wav".into(), vec![red]);
    assert!(store.relocate("/a/kick.wav", "/b/kick.wav"));
    assert_eq!(store.pins, ["/b/kick.wav"]);
    assert_eq!(store.recent.get(&Group::Samples).cloned().unwrap_or_default(), ["/b/kick.wav"]);
    assert_eq!(store.tagged.get("/b/kick.wav").cloned().unwrap_or_default(), [red]);
    assert!(!store.tagged.contains_key("/a/kick.wav"));
    assert!(!store.relocate("/a/kick.wav", "/c/kick.wav"), "nothing left to move");
}

/// The id of the tag `name`, after `set_tag` ran ([#1254] items refer to the tag by its
/// id). The store's own ids are handed out in order, so the tests find the tag by its
/// name where the helper does.
fn set_tag_then_id_of(store: &mut Store, name: &str, color: Option<&str>) -> u32 {
    store.set_tag(name, color, None, None).unwrap();
    tags_iter(&store.tags).find(|t| t.name == name).map_or(FIRST_TAG_ID, |t| t.id)
}

#[test]
fn a_store_round_trips_and_a_missing_file_is_empty() {
    let dir = temp("round");
    let file = dir.join("browser.json");
    let (empty, warnings) = Store::load(&file);
    assert_eq!((empty.clone(), warnings.len()), (Store::default(), 0));
    assert_eq!(tag_count(&empty.tags), 7);
    let mut store = Store::default();
    store.folders.push(PathBuf::from("/samples"));
    store.pin("/samples/kick.wav");
    store.record(Group::Samples, "/samples/a.wav");
    store.record(Group::Samples, "/samples/b.wav");
    store.record(Group::Samples, "/samples/a.wav");
    store.set_tag("Drums", Some("#112233"), None, None).unwrap();
    let drums = set_tag_then_id_of(&mut store, "Drums", Some("#112233"));
    store.tag("/samples/kick.wav", drums, true).unwrap();
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
fn a_legacy_browser_json_is_read_in_with_its_tags_and_tagged_items() {
    let dir = temp("legacy");
    let file = dir.join("browser.json");
    let text = json!({
        "tags": [
            { "name": "Drums", "color": "#112233" },
            { "name": "Hot", "color": "#336699" },
        ],
        "tagged": { "/a/kick.wav": ["Drums", "Hot", "Ghost"] },
    })
    .to_string();
    std::fs::write(&file, text).unwrap();
    let (store, warnings) = Store::load(&file);
    // The tag "Ghost" nothing in the file names is skipped with the refusers' warning.
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(tag_ids(&store.tags), [1, 2]);
    let drums = tag_by_id(&store.tags, 1).unwrap();
    assert_eq!((drums.name.as_str(), drums.color.as_str()), ("Drums", "#112233"));
    assert_eq!(store.tagged["/a/kick.wav"], [1, 2], "the names became ids");
    store.save(&file).unwrap();
    let (again, warnings) = Store::load(&file);
    assert_eq!(warnings.len(), 0);
    assert_eq!(again, store, "the new shape round-trips");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn folders_hold_tags_nest_to_the_cap_and_remove_moves_up() {
    let mut store = Store::default();
    let drums = store.add_tag_folder(Some("Drums"), None).unwrap();
    store.move_tag_node(1, Some(drums), 0).unwrap();
    eprintln!("DBG tree: {:#?}", store.tags);
    eprintln!(
        "DBG before kick: ids={:?} next_free_id={} drums_folder={}",
        tag_ids(&store.tags),
        next_free_id(&store.tags),
        drums
    );
    let kick_folder = store.add_tag_folder(Some("Kick"), Some(drums)).unwrap();
    eprintln!(
        "DBG kick folder id: {}
 tree: {:#?}",
        kick_folder, store.tags
    );
    assert_eq!(entry_depth(&store.tags, kick_folder), 2);
    let deep = store.add_tag_folder(Some("Very"), Some(kick_folder)).unwrap();
    assert_eq!(entry_depth(&store.tags, deep), 3);
    // The depth the folder's children add up to, its row counted in.
    assert_eq!(entry_height(&store.tags, drums), 3, "Drums reaches its tags' children");
    // A fourth level's folder is the deepest that nests; one more above it is refused.
    let mut parent = None;
    let mut level_store = {
        let mut s = Store::default();
        for depth in 1..=MAX_TAG_DEPTH {
            parent = Some(s.add_tag_folder(Some(&format!("Level {depth}")), parent).unwrap());
        }
        s
    };
    assert!(
        level_store.add_tag_folder(Some("Over the cap"), parent).is_err(),
        "the fourth folder's level is the deepest a folder nests"
    );
    // A folder never moves under itself, and a tag returns to the tree's own top.
    let deepest = parent.unwrap();
    assert!(level_store.move_tag_node(deepest, Some(deepest), 0).is_err(), "self is its own descendant");
    // A tag returns to the tree's own top.
    level_store.move_tag_node(2, None, 0).unwrap();
    assert_eq!(entry_depth(&level_store.tags, 2), 1);
    // Removing a folder keeps its tags: the ones under it move up ([#1254]). The
    // deepest folder holds nothing; the one above it holds the levels' chain.
    let empty = level_store.remove_tag_folder(deepest).unwrap();
    assert_eq!(empty, 0, "the deepest folder holds nothing");
    // The folder above the removed one is Level 3 — its removal lets Level 4's old
    // chain's folder move up with its own rows ([#1254]).
    let holder = folders_iter(&level_store.tags).map(|f| f.id).collect::<Vec<_>>();
    let three = { holder.iter().rev().nth(1).copied().unwrap() };
    let one = level_store.remove_tag_folder(three).unwrap();
    assert!(one >= 1, "the folder it held moved up with its own rows");
    assert_eq!(tag_count(&level_store.tags), 7, "removing a folder never removes tags");
}

// The count the menus ask first with is the count the removal moves up: a folder holding
// nothing but an empty folder carries that one row ([#1254] review: the helper counted the
// tags alone and said 0).
#[test]
fn a_folders_carried_count_is_what_its_removal_moves_up() {
    let mut store = Store::default();
    let outer = store.add_tag_folder(Some("Outer"), None).unwrap();
    store.add_tag_folder(Some("Empty"), Some(outer)).unwrap();
    assert_eq!(folder_carried(&store.tags, outer), 1, "the empty folder is one row moving up");
    let holder = store.add_tag_folder(Some("Holder"), None).unwrap();
    let inner = store.add_tag_folder(Some("Inner"), Some(holder)).unwrap();
    store.move_tag_node(1, Some(inner), 0).unwrap();
    store.move_tag_node(2, Some(holder), 0).unwrap();
    assert_eq!(folder_carried(&store.tags, holder), 3, "Inner, its tag and the folder's own tag");
    assert_eq!(folder_carried(&store.tags, 9999), 0, "no such folder carries nothing");
    for folder in [outer, holder] {
        let asked = folder_carried(&store.tags, folder);
        assert_eq!(store.remove_tag_folder(folder).unwrap(), asked, "the removal moves up what was asked");
    }
}

// The ids a file keeps are kept once, and the ones handed out start past all of them: an
// id-less tag after `{"id": 1}` took 1 too and the two answered as one, a folder could keep
// a tag's id, and an id near `u32::MAX` overflowed the next one made ([#1254] review).
#[test]
fn a_files_ids_are_kept_once_and_the_handed_out_ones_take_none_of_them() {
    let dir = temp("file-ids");
    let file = dir.join("browser.json");
    let text = json!({
        "tags": [
            { "tag": { "id": 1, "name": "Kept", "color": "#112233" } },
            { "tag": { "name": "Idless", "color": "#223344" } },
            { "name": "Legacy", "color": "#334455" },
            { "folder": { "id": 1, "name": "Shares", "open": true }, "tags": [
                { "tag": { "id": 7, "name": "Deep", "color": "#445566" } },
            ] },
            { "tag": { "id": 7, "name": "Twin", "color": "#556677" } },
            { "tag": { "id": u32::MAX, "name": "Huge", "color": "#667788" } },
        ],
        "tagged": { "/a.wav": [1] },
    })
    .to_string();
    std::fs::write(&file, text).unwrap();
    let (mut store, _) = Store::load(&file);
    let id_of = |store: &Store, name: &str| tags_iter(&store.tags).find(|t| t.name == name).map(|t| t.id);
    assert_eq!((id_of(&store, "Kept"), id_of(&store, "Deep")), (Some(1), Some(7)), "usable ids are kept");
    let mut ids = tag_ids(&store.tags);
    ids.extend(folders_iter(&store.tags).map(|f| f.id));
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), ids.len(), "tags and folders share the id space, each id once: {ids:?}");
    assert_eq!(ids.len(), 7, "six tags and the folder: {ids:?}");
    for name in ["Idless", "Legacy", "Twin", "Huge"] {
        let id = id_of(&store, name).unwrap_or(0);
        assert!(id > 7, "{name} took {id}: an id the file keeps, or under them");
    }
    assert_eq!(store.tagged["/a.wav"], [1], "the item carries Kept alone");
    // Removing the id-less tag leaves Kept and what carries it.
    store.remove_tag(id_of(&store, "Idless").unwrap()).unwrap();
    assert_eq!(id_of(&store, "Kept"), Some(1));
    assert_eq!(store.tagged["/a.wav"], [1]);
    // The next one made takes a free id, past the highest.
    let made = set_tag_then_id_of(&mut store, "Made", None);
    assert!(!ids.contains(&made) && made > 7, "{made}");
    let _ = std::fs::remove_dir_all(&dir);
}

// A file that keeps the highest id still reads its id-less tags in under ids a reload reads
// back: the lowest free ones, never one past the cap ([#1254] review: the id-less tag took
// one past it, and once the highest went a reload gave it another and its items lost it).
#[test]
fn a_file_keeping_the_highest_id_hands_out_ids_a_reload_keeps() {
    let dir = temp("highest-kept");
    let file = dir.join("browser.json");
    let text = json!({
        "tags": [
            { "tag": { "id": MAX_FILE_TAG_ID, "name": "Top", "color": "#112233" } },
            { "tag": { "name": "Idless", "color": "#223344" } },
        ],
    })
    .to_string();
    std::fs::write(&file, text).unwrap();
    let (mut store, _) = Store::load(&file);
    let idless = tags_iter(&store.tags).find(|t| t.name == "Idless").map(|t| t.id).unwrap_or(0);
    assert_eq!(idless, FIRST_TAG_ID, "the lowest free id, within the cap");
    store.tag("/a.wav", idless, true).unwrap();
    store.remove_tag(MAX_FILE_TAG_ID).unwrap();
    store.save(&file).unwrap();
    let (again, _) = Store::load(&file);
    assert_eq!(tags_iter(&again.tags).find(|t| t.name == "Idless").map(|t| t.id), Some(idless));
    assert_eq!(again.tagged["/a.wav"], [idless], "the item keeps its tag through the reload");
    let _ = std::fs::remove_dir_all(&dir);
}

// A legacy tag or folder whose name spells a selector is kept under a name of its own, with
// what carries it ([#1254] review: #1250's file could hold a tag named `#1`, and the reader
// dropped it with its assignments, the next save making the loss for good).
#[test]
fn a_legacy_selector_name_is_renamed_not_dropped() {
    let dir = temp("legacy-selector");
    let file = dir.join("browser.json");
    let text = json!({
        "tags": [
            { "name": "#1", "color": "#112233" },
            { "name": "Tag #1", "color": "#223344" },
            { "folder": { "name": "#2", "open": true }, "tags": [
                { "tag": { "name": "Inside", "color": "#334455" } },
            ] },
        ],
        "tagged": { "/a.wav": ["#1", "Tag #1"] },
    })
    .to_string();
    std::fs::write(&file, text).unwrap();
    let (store, warnings) = Store::load(&file);
    let names: Vec<String> = tags_iter(&store.tags).map(|t| t.name.clone()).collect();
    assert!(names.contains(&"Tag #1 2".to_string()), "renamed past the name taken: {names:?} {warnings:?}");
    assert!(names.contains(&"Inside".to_string()), "the folder's tag stays: {names:?}");
    assert!(folders_iter(&store.tags).any(|f| f.name == "Folder #2"), "the folder is renamed, not dropped");
    let renamed = tags_iter(&store.tags).find(|t| t.name == "Tag #1 2").map(|t| t.id).unwrap_or(0);
    let other = tags_iter(&store.tags).find(|t| t.name == "Tag #1").map(|t| t.id).unwrap_or(0);
    let mut carried = store.tagged["/a.wav"].clone();
    carried.sort_unstable();
    let mut wanted = vec![renamed, other];
    wanted.sort_unstable();
    assert_eq!(carried, wanted, "the item carries both, the renamed one by its new id");
    let _ = std::fs::remove_dir_all(&dir);
}

// A legacy name that both spells a selector and sits at the limit: `#` over 63 digits is 64
// bytes, and `Tag ` over it kept nothing within the limit, so the tag went with its
// assignments and the next save made the loss for good ([#1254] review). As much of the
// name as fits is kept instead, numbered past what is taken, and a reload holds it.
#[test]
fn an_overlong_legacy_selector_name_keeps_what_fits() {
    let dir = temp("legacy-selector-long");
    let file = dir.join("browser.json");
    let name = format!("#{}", "1".repeat(63));
    let text = json!({
        "tags": [
            { "name": name, "color": "#112233" },
            { "name": "Tag #1", "color": "#223344" },
        ],
        "tagged": { "/a.wav": [name] },
    })
    .to_string();
    std::fs::write(&file, text).unwrap();
    let (store, warnings) = Store::load(&file);
    // `Tag ` is 4 bytes, the limit 64: the name keeps 60 of its own — the `#` and 59 digits.
    let renamed = format!("Tag #{}", "1".repeat(59));
    assert!(
        tags_iter(&store.tags).any(|t| t.name == renamed),
        "the tag kept as much of its name as the limit holds: {:?} {warnings:?}",
        tags_iter(&store.tags).map(|t| t.name.clone()).collect::<Vec<_>>(),
    );
    // The assignment carried the rename: the item is tagged by the id, under the new name.
    let id = tags_iter(&store.tags).find(|t| t.name == renamed).map(|t| t.id).unwrap_or(0);
    assert_eq!(store.tagged["/a.wav"], vec![id], "the item carries the renamed tag: {warnings:?}");
    // The save writes it, the reload keeps it: no load-and-save cycle loses the entry.
    store.save(&file).unwrap();
    let (again, _) = Store::load(&file);
    assert!(tags_iter(&again.tags).any(|t| t.name == renamed), "the reload keeps the tag");
    assert_eq!(again.tagged["/a.wav"], vec![id], "and its assignment");
    let _ = std::fs::remove_dir_all(&dir);
}

// A second tag of a selector's name is the duplicate a second of any name is: skipped, not
// renamed beside the first, and the legacy assignments naming it carry the first ([#1254]
// review). A second folder of such a name in a level is skipped the same way.
#[test]
fn a_second_selector_name_is_a_duplicate_not_a_second_rename() {
    let dir = temp("selector-twice");
    let file = dir.join("browser.json");
    let text = json!({
        "tags": [
            { "name": "#1", "color": "#112233" },
            { "name": "#1", "color": "#223344" },
            { "folder": { "name": "#2" }, "tags": [] },
            { "folder": { "name": "#2" }, "tags": [] },
        ],
        "tagged": { "/a.wav": ["#1"] },
    })
    .to_string();
    std::fs::write(&file, text).unwrap();
    let (store, _) = Store::load(&file);
    let renamed: Vec<&Tag> = tags_iter(&store.tags).filter(|t| t.name.starts_with("Tag #1")).collect();
    assert_eq!(renamed.len(), 1, "one tag of the name: {renamed:?}");
    assert_eq!(renamed.first().map(|t| t.color.as_str()), Some("#112233"), "the first is kept");
    assert_eq!(store.tagged["/a.wav"], [renamed[0].id]);
    assert_eq!(folders_iter(&store.tags).filter(|f| f.name.starts_with("Folder #2")).count(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

// A tag whose id the reader replaced (one past the cap) keeps what carried it: the
// assignments by that number follow it to its new id ([#1254] review).
#[test]
fn a_replaced_id_keeps_the_items_that_carried_it() {
    let dir = temp("replaced-id");
    let file = dir.join("browser.json");
    let text = json!({
        "tags": [{ "tag": { "id": u32::MAX, "name": "Huge", "color": "#112233" } }],
        "tagged": { "/a.wav": [u32::MAX] },
    })
    .to_string();
    std::fs::write(&file, text).unwrap();
    let (store, _) = Store::load(&file);
    let huge = tags_iter(&store.tags).find(|t| t.name == "Huge").map(|t| t.id).unwrap_or(0);
    assert!((FIRST_TAG_ID..=MAX_FILE_TAG_ID).contains(&huge), "{huge}");
    assert_eq!(store.tagged["/a.wav"], [huge], "the item carries it under its new id");
    let _ = std::fs::remove_dir_all(&dir);
}

// An entry the reader leaves out reserves no id: a broken tag naming an id near the cap
// does not push the ids handed out to the cap, and the store can make tags after the load
// ([#1254] review).
#[test]
fn a_skipped_entry_reserves_no_id() {
    let dir = temp("skipped-id");
    let file = dir.join("browser.json");
    let text = json!({
        "tags": [
            { "tag": { "id": MAX_FILE_TAG_ID - 1, "name": "Broken", "color": "not a colour" } },
            { "name": "Legacy", "color": "#223344" },
        ],
    })
    .to_string();
    std::fs::write(&file, text).unwrap();
    let (mut store, _) = Store::load(&file);
    let legacy = tags_iter(&store.tags).find(|t| t.name == "Legacy").map(|t| t.id);
    assert_eq!(legacy, Some(FIRST_TAG_ID), "the broken tag's id held nothing back");
    assert!(store.set_tag("Made", None, None, None).is_ok(), "the store still makes tags");
    let _ = std::fs::remove_dir_all(&dir);
}

// Thousands of id-less tags after a file's highest id each take a free id of their own;
// the reader keeps its own count of what it gave, never scanning the forest per id
// ([#1254] review: the scan stalled the start).
#[test]
fn many_idless_tags_after_the_highest_id_read_in_with_ids_of_their_own() {
    let mut tags = vec![json!({ "tag": { "id": MAX_FILE_TAG_ID, "name": "Top", "color": "#112233" } })];
    tags.extend((0..3000).map(|n| json!({ "name": format!("T{n}"), "color": "#223344" })));
    let dir = temp("many-idless");
    let file = dir.join("browser.json");
    std::fs::write(&file, json!({ "tags": tags, "tag_limit": MAX_TAG_LIMIT }).to_string()).unwrap();
    let (store, _) = Store::load(&file);
    let mut ids = tag_ids(&store.tags);
    assert_eq!(ids.len(), MAX_TAG_LIMIT, "read up to the limit");
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), MAX_TAG_LIMIT, "every id its own");
    assert!(ids.iter().all(|id| (FIRST_TAG_ID..=MAX_FILE_TAG_ID).contains(id)));
    let _ = std::fs::remove_dir_all(&dir);
}

// A selector's new name is numbered on past 99 while one is taken ([#1254] review: the
// search stopped there and the tag was dropped with its assignments).
#[test]
fn a_selectors_new_name_numbers_on_past_ninety_nine() {
    let taken: std::collections::HashSet<String> =
        std::iter::once("Tag #1".to_string()).chain((2..=150).map(|n| format!("Tag #1 {n}"))).collect();
    assert_eq!(unreserved_name("Tag", "#1", MAX_TAG_NAME, |n| taken.contains(n)).as_deref(), Some("Tag #1 151"));
}

// The store makes no id past the ones a file keeps: an id it made past them would be handed
// out anew on the next load and the items carrying it would come back carrying another tag
// ([#1254] review). Every id a make hands out round-trips as it is.
#[test]
fn the_store_makes_no_id_a_reload_would_hand_out_anew() {
    let dir = temp("id-cap");
    let file = dir.join("browser.json");
    let text = json!({
        "tags": [{ "tag": { "id": MAX_FILE_TAG_ID - 1, "name": "High", "color": "#112233" } }],
    })
    .to_string();
    std::fs::write(&file, text).unwrap();
    let (mut store, _) = Store::load(&file);
    let last = set_tag_then_id_of(&mut store, "Last", None);
    assert_eq!(last, MAX_FILE_TAG_ID, "the last id a file keeps is made");
    assert!(store.set_tag("Past", None, None, None).is_err(), "no tag past it");
    assert!(store.add_tag_folder(Some("Past"), None).is_err(), "nor a folder");
    store.tag("/a.wav", last, true).unwrap();
    store.save(&file).unwrap();
    let (again, _) = Store::load(&file);
    assert_eq!(tags_iter(&again.tags).find(|t| t.name == "Last").map(|t| t.id), Some(MAX_FILE_TAG_ID));
    assert_eq!(again.tagged["/a.wav"], [MAX_FILE_TAG_ID], "the item keeps its tag");
    let _ = std::fs::remove_dir_all(&dir);
}

// The selectors' shape is `#` over digits, alone or joined with `+`: that much is reserved,
// and a bare `#`, a sign or other text after `#` stays a name ([#1254] review: `#` was
// refused and `#+1` read as the id 1).
#[test]
fn only_the_selectors_own_shape_is_reserved() {
    for reserved in ["#1", "#007", "#1+#2", "#3+#3", "#99999999999"] {
        assert!(name_is_id_form(reserved), "{reserved}");
    }
    for name in ["#", "#+1", "#-1", "#1a", "# 1", "#notes", "1", "#1+", "#1+2", "+#1", "#１"] {
        assert!(!name_is_id_form(name), "{name}");
    }
    assert_eq!(id_of_selector("#12"), Some(12));
    for not_one in ["#", "#+1", "#1+#2", "#99999999999", "12", "#1a"] {
        assert_eq!(id_of_selector(not_one), None, "{not_one}");
    }
    let mut store = Store::default();
    for name in ["#", "#+1", "#notes"] {
        store.set_tag(name, None, None, None).unwrap();
        assert!(tags_iter(&store.tags).any(|t| t.name == name), "{name} is a name");
    }
    // The level keeps its names one word apart, so the folder named `#` is another store's.
    Store::default().add_tag_folder(Some("#"), None).unwrap();
    assert!(store.set_tag("#1+#2", None, None, None).is_err(), "a joined selector is no name");
    assert!(store.add_tag_folder(Some("#4+#5"), None).is_err(), "nor a folder's");
}

#[test]
fn the_sections_are_orderable_and_collapsible() {
    let mut store = Store::default();
    assert_eq!(
        store.sections.iter().map(|s| s.kind).collect::<Vec<_>>(),
        [SectionKind::Pinned, SectionKind::Tags, SectionKind::Library],
        "the sidebar's own starting order"
    );
    store.set_sections(Some(&[
        Section { kind: SectionKind::Pinned, open: true },
        Section { kind: SectionKind::Tags, open: false },
    ]));
    assert_eq!(
        store.sections.iter().map(|s| (s.kind, s.open)).collect::<Vec<_>>(),
        [(SectionKind::Pinned, true), (SectionKind::Tags, false), (SectionKind::Library, true),],
        "the order leads; a left-out kind keeps its state; Folders is absent"
    );
    store.set_section_open(SectionKind::Tags, true);
    assert!(store.sections.iter().find(|s| s.kind == SectionKind::Tags).is_some_and(|s| s.open));
}

// A tag reaches the deepest child folder, and the cap counts folder levels, not rows
// ([#1254] review: the lowest folder took no tag — the move counted the tag's own row
// against the folder cap).
#[test]
fn a_tag_reaches_the_deepest_folder_and_the_cap_counts_folder_levels() {
    let mut store = Store::default();
    let mut parent = None;
    for depth in 1..=MAX_TAG_DEPTH {
        parent = Some(store.add_tag_folder(Some(&format!("Level {depth}")), parent).unwrap());
    }
    let lowest = parent.unwrap();
    // The defaults: Red..Grey with ids 1..7. A carried tag dropped in the deepest folder
    // lands: a tag is a leaf, it adds no folder level to count.
    store.move_tag_node(2, Some(lowest), 0).unwrap();
    assert_eq!(entry_depth(&store.tags, 2), MAX_TAG_DEPTH + 1, "the tag sits inside the fourth level's folder");
    // ... and moves back out to the top.
    store.move_tag_node(2, None, 0).unwrap();
    assert_eq!(entry_depth(&store.tags, 2), 1);

    // A folder with only tags inside reaches one folder level: under the third level it
    // nests to the cap, its tag riding below it (a row count would have said no).
    let holder = store.add_tag_folder(None, None).unwrap();
    store.move_tag_node(3, Some(holder), 0).unwrap();
    let three = folders_iter(&store.tags).find(|f| f.name == "Level 3").map_or(0, |f| f.id);
    store.move_tag_node(holder, Some(three), 0).unwrap();
    assert_eq!(entry_depth(&store.tags, holder), MAX_TAG_DEPTH, "the holder nests at the cap");
    assert_eq!(entry_depth(&store.tags, 3), MAX_TAG_DEPTH + 1, "its tag rides under it");
    // Yet a folder itself under the lowest folder would nest a fifth level: refused.
    assert!(store.move_tag_node(holder, Some(lowest), 0).is_err(), "the fifth level's folder is refused");
}

// The deepest folder's tags survive a save and a load; a fifth level's folder in the file
// is past the cap and comes in with the refusers' warning ([#1254]).
#[test]
fn the_deepest_folders_tags_round_trip_and_a_fifth_level_is_left_out() {
    let dir = temp("tag-depth");
    let file = dir.join("browser.json");
    let text = json!({
        "tagged": {},
        "tags": [
            { "folder": { "name": "One" }, "tags": [
                { "folder": { "name": "Two" }, "tags": [
                    { "folder": { "name": "Three" }, "tags": [
                        { "folder": { "name": "Four" }, "tags": [
                            { "tag": { "name": "Deep", "color": "#FF7F50" } },
                            { "folder": { "name": "Five" }, "tags": [
                                { "tag": { "name": "Beyond", "color": "#112233" } },
                            ] },
                        ] },
                    ] },
                ] },
            ] },
        ],
    })
    .to_string();
    std::fs::write(&file, text).unwrap();
    let (mut store, warnings) = Store::load(&file);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("nest more than 4 deep"), "the fifth level's folder warns: {warnings:?}");
    assert!(!tags_iter(&store.tags).any(|t| t.name == "Beyond"), "the fifth level's cortège left out");
    let deep = tags_iter(&store.tags).find(|t| t.name == "Deep").map_or(0, |t| t.id);
    assert!(deep >= FIRST_TAG_ID, "the tag inside the deepest folder read in");
    let four = folders_iter(&store.tags).find(|f| f.name == "Four").map_or(0, |f| f.id);
    assert_eq!(entry_depth(&store.tags, four), MAX_TAG_DEPTH, "the fourth level's folder is where it nests");
    store.save(&file).unwrap();
    let (again, warnings) = Store::load(&file);
    assert_eq!(warnings.len(), 0, "{warnings:?}");
    assert_eq!(again, store, "the deepest folder's tags round-trip");
    // And the deep one still moves out of there.
    store.move_tag_node(deep, None, 0).unwrap();
    assert_eq!(entry_depth(&store.tags, deep), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

// The flattened tags lead with a level's folders' own tags, as the sidebar's rows go:
// the footer, a tag's menu and `library.tags` list them too ([#1254] review).
#[test]
fn the_flattened_tags_lead_with_each_levels_folders() {
    let mut store = Store::default();
    // The defaults, Red..Grey by id, then a folder appended last, Red filed in it.
    let drum = store.add_tag_folder(Some("Drums"), None).unwrap();
    store.move_tag_node(1, Some(drum), 0).unwrap();
    let in_order: Vec<&str> = tags_iter(&store.tags).map(|t| t.name.as_str()).collect();
    assert_eq!(
        in_order,
        ["Red", "Orange", "Yellow", "Green", "Blue", "Purple", "Grey"],
        "the folder leads its level, the tags follow"
    );
    // A tag of a deeper folder leads its own level too.
    let inner = store.add_tag_folder(Some("Kicks"), Some(drum)).unwrap();
    store.move_tag_node(2, Some(inner), 0).unwrap();
    let in_order: Vec<&str> = tags_iter(&store.tags).map(|t| t.name.as_str()).collect();
    assert_eq!(
        in_order,
        ["Orange", "Red", "Yellow", "Green", "Blue", "Purple", "Grey"],
        "a subfolder's tag leads its parent level"
    );
}

// A new tag made without a colour is made under its `rename` still, held to the same
// name checks ([#1254] review).
#[test]
fn a_new_tag_made_without_a_colour_answers_its_rename() {
    let mut store = Store::default();
    store.set_tag("Coral", None, Some("Reef"), None).unwrap();
    let made = tags_iter(&store.tags).find(|t| t.name == "Reef").map_or(0, |t| t.id);
    assert!(made >= FIRST_TAG_ID, "the rename named the made tag");
    assert_eq!(
        tags_iter(&store.tags).find(|t| t.name == "Reef").map(|t| t.color.as_str()),
        Some("#800000"), // Maroon's: the next named colour no tag has
        "the no-colour make took the named colours' next turn from the name it was made under"
    );
    // A rename that another tag has: refused, not silently made under the asked name.
    assert!(store.set_tag("Coral", None, Some("Orange"), None).is_err(), "the taken name refuses");
    // Too long: the same refusal a rename of an existing tag gets.
    let long = "x".repeat(MAX_TAG_NAME + 1);
    assert!(store.set_tag("Gold", None, Some(&long), None).is_err());
    // An empty rename is none: Gold made as asked.
    assert!(store.set_tag("Gold", None, Some("   "), None).is_ok());
    assert!(tags_iter(&store.tags).any(|t| t.name == "Gold"));
}

// A folder's name is unique among its own level alone: a folder of another parent may
// carry it, at a rename and over a reload ([#1254] review).
#[test]
fn a_folders_name_is_unique_in_its_level_alone() {
    let mut store = Store::default();
    let belt = store.add_tag_folder(Some("Belt"), None).unwrap();
    let two = store.add_tag_folder(Some("Two"), None).unwrap();
    store.add_tag_folder(Some("Kick"), Some(belt)).unwrap();
    // Renaming the top level's Two to a name Belt's level holds: fine — different level.
    store.rename_tag_folder(two, "Kick").unwrap();
    // Renaming it to ITS level's own existing name: refused.
    assert!(store.rename_tag_folder(two, "Belt").is_err(), "the level's own name refuses");
    // Making a second Kick under Belt itself: refused there as well.
    assert!(store.add_tag_folder(Some("Kick"), Some(belt)).is_err(), "the level already has Kick");
    // A nested level's own names count too ([#1254]): the check reaches the nested level.
    let one = store.add_tag_folder(Some("One"), None).unwrap();
    store.add_tag_folder(Some("Kick"), Some(one)).unwrap();
    assert!(store.add_tag_folder(Some("Kick"), Some(one)).is_err(), "a nested level refuses a second of its own names");
    // Three Kick-named folders, under three parents (each level's own one): the reload keeps them.
    let dir = temp("folder-level-names");
    let file = dir.join("browser.json");
    store.save(&file).unwrap();
    let (again, warnings) = Store::load(&file);
    assert_eq!(warnings.len(), 0, "{warnings:?}");
    assert_eq!(folders_iter(&again.tags).filter(|f| f.name == "Kick").count(), 3, "nothing was dropped");
    let _ = std::fs::remove_dir_all(&dir);
}

// A second section kind in the order given keeps its first slot, as the load does
// ([#1254] review).
#[test]
fn a_repeated_section_kind_keeps_its_first_slot() {
    let mut store = Store::default();
    store.set_section_open(SectionKind::Tags, false);
    store.set_sections(Some(&[
        Section { kind: SectionKind::Tags, open: true },
        Section { kind: SectionKind::Pinned, open: true },
        Section { kind: SectionKind::Tags, open: true },
    ]));
    assert_eq!(
        store.sections.iter().map(|s| (s.kind, s.open)).collect::<Vec<_>>(),
        [(SectionKind::Tags, true), (SectionKind::Pinned, true), (SectionKind::Library, true)],
        "the repeated kind kept its first slot, the rest of the order led"
    );
    store.set_section_open(SectionKind::Tags, false);
    assert_eq!(store.sections.iter().filter(|s| s.kind == SectionKind::Tags).count(), 1, "one slot to close");
}

// A tag renamed to a sibling folder's name: refused — the two kinds keep their names apart
// in the forest, so the shape the loader skips cannot be saved ([#1254] review: the tag's
// rename was the shape's own door).
#[test]
fn a_tag_renamed_to_a_folders_name_is_refused() {
    let mut store = Store::default();
    let belt = store.add_tag_folder(Some("Belt"), None).unwrap();
    // The tag Orange, at Belt's own level, cannot take Belt's name.
    assert!(store.rename_tag(2, "Belt").is_err(), "the level's folder name refuses");
    assert_eq!(
        tag_by_id(&store.tags, 2).map(|t| t.name.as_str()),
        Some("Orange"),
        "the refuse leaves the tag as it was"
    );
    // A rename into its own name stays no-op-ish, and other levels' folder names do not stop it.
    store.rename_tag(2, "Belt").unwrap_err();
    let under = store.add_tag_folder(Some("Belt"), Some(belt)).unwrap();
    assert_eq!(under, belt + 1, "the second Belt sits under the first");
    assert!(store.rename_tag(2, "Belt").is_err(), "any level's own folder name refuses");
    // The make takes the same rule: a top-level folder's name is not a made tag's.
    assert!(store.set_tag("Belt", None, None, None).is_err(), "the top-level folder's name refuses the make");
    store.set_tag("Deep", Some("#123123"), None, None).unwrap();
    assert!(tags_iter(&store.tags).any(|t| t.name == "Deep"));
}

// A move cannot sit a tag and a folder of one name together at a level: the reload
// skipped that shape, so the moves hold to the one set of names per level ([#1254]
// review — the find's own case: a same-named tag beside a moved folder).
#[test]
fn a_move_that_would_pair_a_level_s_names_is_refused() {
    let mut store = Store::default();
    // A tag Belt at the top (an old make's), a folder Belt inside Box: both legal on
    // their own levels.
    store.set_tag("Belt", Some("#765432"), None, None).unwrap();
    let box_id = store.add_tag_folder(Some("Box"), None).unwrap();
    let belt_id = store.add_tag_folder(Some("Belt"), Some(box_id)).unwrap();
    // Moving the folder Belt out to the top pairs the level's names: refused, the
    // folder back where it was.
    assert!(store.move_tag_node(belt_id, None, 0).is_err(), "the pair refuse");
    let children =
        folder_children(&store.tags, box_id).map(|c| c.iter().map(|e| e.name().to_string()).collect::<Vec<_>>());
    assert_eq!(children.as_deref(), Some(["Belt".to_string()].as_slice()), "the folder stayed in Box: {children:?}");
    // A move whose destination pairs nothing goes: Orange joins Box beside the folder.
    store.move_tag_node(2, Some(box_id), 0).unwrap();
    let children =
        folder_children(&store.tags, box_id).map(|c| c.iter().map(|e| e.name().to_string()).collect::<Vec<_>>());
    assert_eq!(
        children.as_deref(),
        Some(["Orange".to_string(), "Belt".to_string()].as_slice()),
        "the plain move went: {children:?}"
    );
    // Its own level's reorder is fine: Yellow to the top of the top level's own list.
    store.move_tag_node(3, None, 0).unwrap();
    assert_eq!(store.tags.first().map(|e| e.name()), Some("Yellow"));
    // The rename is the way out: the TAG Belt (the made one) goes as Copper, then the
    // folder's move carries nothing to pair.
    let belt_tag = tags_iter(&store.tags).find(|t| t.name == "Belt").map(|t| t.id).unwrap();
    store.rename_tag(belt_tag, "Copper").unwrap();
    store.move_tag_node(belt_id, None, 0).unwrap();
    let top = store.tags.iter().map(|e| e.name().to_string()).collect::<Vec<_>>();
    assert!(top.contains(&"Belt".to_string()) && top.contains(&"Copper".to_string()), "{top:?}");
}

// Lifting a folder's cortège that holds a name the destination has is refused too
// ([#1254] review): the removal's lift makes the same pair a move would.
#[test]
fn a_lift_that_would_pair_a_level_s_names_is_refused() {
    let mut store = Store::default();
    // Store an already-paired level inside Box: a tag Punch and a folder Punch " below
    // side by side — a shape an old file holds (the make and the rename refuse it now).
    let box_id = store.add_tag_folder(Some("Box"), None).unwrap();
    let inner = store.add_tag_folder(Some("Punch"), Some(box_id)).unwrap();
    store.move_tag_node(1, Some(inner), 0).unwrap();
    store.rename_tag(1, "Punch").unwrap();
    // Top has its own Punch-named folder: lifting Box's cortège would pair it.
    let top_punch = store.add_tag_folder(Some("Punch"), None).unwrap();
    let _ = top_punch;
    // The removal refuses rather than pairing the top.
    let removed = store.remove_tag_folder(box_id);
    assert!(matches!(&removed, Err(e) if e.to_string().contains("at that level already")), "{removed:?}");
    // Box stands; nothing was lifted away.
    assert!(folder_by_id(&store.tags, box_id).is_some(), "the folder came back");
    assert!(folder_children(&store.tags, box_id).is_some_and(|c| !c.is_empty()), "the cortège stayed inside");
}
