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
    store.set_tag(name, color, None).unwrap();
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
    store.set_tag("Drums", Some("#112233"), None).unwrap();
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
    store.set_tag("Coral", None, Some("Reef")).unwrap();
    let made = tags_iter(&store.tags).find(|t| t.name == "Reef").map_or(0, |t| t.id);
    assert!(made >= FIRST_TAG_ID, "the rename named the made tag");
    assert_eq!(
        tags_iter(&store.tags).find(|t| t.name == "Reef").map(|t| t.color.as_str()),
        Some("#800000"), // Maroon's: the next named colour no tag has
        "the no-colour make took the named colours' next turn from the name it was made under"
    );
    // A rename that another tag has: refused, not silently made under the asked name.
    assert!(store.set_tag("Coral", None, Some("Orange")).is_err(), "the taken name refuses");
    // Too long: the same refusal a rename of an existing tag gets.
    let long = "x".repeat(MAX_TAG_NAME + 1);
    assert!(store.set_tag("Gold", None, Some(&long)).is_err());
    // An empty rename is none: Gold made as asked.
    assert!(store.set_tag("Gold", None, Some("   ")).is_ok());
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
    assert!(store.set_tag("Belt", None, None).is_err(), "the top-level folder's name refuses the make");
    store.set_tag("Deep", Some("#123123"), None).unwrap();
    assert!(tags_iter(&store.tags).any(|t| t.name == "Deep"));
}
