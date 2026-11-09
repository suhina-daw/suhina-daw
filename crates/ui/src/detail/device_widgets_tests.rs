// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn a_file_steps_through_its_pack_wrapping() {
    let files = vec![("a.nam".into(), "A".into()), ("b.nam".into(), "B".into()), ("c.nam".into(), "C".into())];
    let by = |current: &str, step: i64| step_through(&files, current, step);
    assert_eq!(by("a.nam", 1).as_deref(), Some("b.nam"));
    assert_eq!(by("c.nam", 1).as_deref(), Some("a.nam"), "the last file wraps to the first");
    assert_eq!(by("a.nam", -1).as_deref(), Some("c.nam"));
    assert_eq!(by("not here.nam", 1).as_deref(), Some("a.nam"), "a lost file steps from the first");
    assert_eq!(by("not here.nam", -1).as_deref(), Some("c.nam"), "…and back from the last");
    assert_eq!(by("", 1).as_deref(), Some("a.nam"), "a file none of the lists holds steps from the first");
    assert_eq!(step_through(&[], "", 1), None, "no files, no step");
}

#[test]
fn the_note_names_what_the_file_faulted_with() {
    let lists = Lists {
        models: vec![Set { name: "Pack".into(), files: vec![("m.nam".into(), "M".into())] }],
        irs: vec![],
        names: HashMap::from([("m.nam".into(), "Pack".into()), ("gone.nam".into(), "Gone amp".into())]),
        notes: HashMap::from([("broken.nam".into(), "bad weights".into())]),
        ..Lists::default()
    };
    let state =
        |file: &'static str| PanelState { name: "NAM Amp Modeler".into(), enabled: true, file, lists: Some(&lists) };
    assert_eq!(note_of(&state("")), None, "no file, no note");
    assert_eq!(note_of(&state("broken.nam")).as_deref(), Some("bad weights"), "the load's fault");
    assert_eq!(
        note_of(&state("missing.nam")).as_deref(),
        Some("Model missing: missing.nam"),
        "a file the folders do not know"
    );
    assert_eq!(note_of(&state("m.nam")), None, "a known file runs well: no note");
    assert_eq!(
        note_of(&PanelState { name: String::new(), enabled: true, file: "m.nam", lists: None }),
        None,
        "without the lists, nothing to say"
    );
}

#[test]
fn a_file_shows_its_own_name_and_a_pack_holds_it() {
    let lists = Lists {
        models: vec![
            Set { name: "Test amp".into(), files: vec![("p/Test amp/test.nam".into(), "Test amp".into())] },
            // The loose files' pack beside the subfolder packs (nam spec › Folders › Packs).
            Set { name: LOOSE_PACK.into(), files: vec![("p/NAM models/loose.nam".into(), "loose capture".into())] },
        ],
        irs: vec![],
        names: HashMap::from([("p/Test amp/test.nam".into(), "Fender Super Reverb".into())]),
        notes: HashMap::new(),
        ..Lists::default()
    };
    // The pack that lists the file, by membership; a file somewhere else that shares a pack's
    // folder name lists nowhere, and a file no pack lists is none of the loose pack's either.
    assert_eq!(pack_of_current("p/Test amp/test.nam", &lists.models).map(|p| p.name.as_str()), Some("Test amp"));
    assert_eq!(pack_of_current("p/NAM models/loose.nam", &lists.models).map(|p| p.name.as_str()), Some(LOOSE_PACK));
    assert_eq!(pack_of_current("elsewhere/Test amp/false.nam", &lists.models), None);
    assert_eq!(pack_of_current("p/Another/wave.wav", &lists.irs), None);
    // The shown name from the session's names, else the path's own without its extension.
    assert_eq!(file_shown("p/Test amp/test.nam", &lists), "Fender Super Reverb");
    assert_eq!(file_shown("p/NAM models/loose.nam", &lists), "loose");
    assert_eq!(file_shown("", &lists), "");
}

#[test]
fn the_words_name_what_a_panel_calls_its_file() {
    let model = words("model").unwrap();
    assert_eq!(model.pack, "No model");
    assert_eq!(model.file, "Choose a model file…");
    let cab = words("cab").unwrap();
    assert_eq!(cab.pack, "No IR");
    assert_eq!(words("wave"), None, "a file kind the panels do not name draws nothing");
    let mut label = PanelState { name: "NAM Amp Modeler".into(), ..PanelState::none() }.label();
    assert_eq!(label, "Amp Modeler");
    label = PanelState { name: "Utility".into(), ..PanelState::none() }.label();
    assert_eq!(label, "Utility", "the prefix is only dropped where it led");
}

#[test]
fn a_state_relative_to_the_library_reads_the_lists_files() {
    // The device's state says the library-relative name a project carries ([ADR 0036] › 4);
    // the lists carry the same file as a whole path. The pack row and the file row read the
    // file either way: the pack lookup canonicalises the state against the lists' root.
    let lists = Lists {
        models: vec![Set {
            name: "Suhina".into(),
            files: vec![("/mine/Suhina/Amp models/Suhina/lead.nam".into(), "lead".into())],
        }],
        irs: vec![],
        names: HashMap::new(),
        notes: HashMap::new(),
        library: Some(PathBuf::from("/mine")),
    };
    let state = PanelState {
        name: "NAM Amp Modeler".into(),
        enabled: true,
        file: "Suhina/Amp models/Suhina/lead.nam",
        lists: Some(&lists),
    };
    assert_eq!(
        pack_of_current(&state_as_listed(state.file, &lists), &lists.models).map(|pack| pack.name.as_str()),
        Some("Suhina"),
        "the relative state reads the pack whole"
    );
    assert_eq!(file_shown(state.file, &lists), "lead", "the shown name falls to the file's own");
}

#[test]
fn a_shipped_key_reads_the_pack_and_the_unpacked_copy_of_its_file() {
    // The shipped pack's key beside the unpacked copy: the scan's set lists whole paths, so
    // the key lists the pack only by its file's own name in there ([ADR 0036] › the pack).
    let lists_with_unpacked = Lists {
        models: vec![Set {
            name: "Suhina".into(),
            files: vec![("/library/Amp models/Suhina/Suhina Overdrive.nam".into(), "Suhina Overdrive".into())],
        }],
        ..Lists::default()
    };
    let scanned = lists_with_unpacked.models.clone();
    let key = "builtin:nam-amp-modeler/Suhina Overdrive.nam";
    assert_eq!(pack_of_current(key, &scanned).map(|pack| pack.name.as_str()), Some("Suhina"), "the unpacked copy");
    assert_eq!(pack_of_current("builtin:nam-amp-modeler/Nothing.nam", &scanned), None);
    // And without the unpacked copy, the shipped set's own rows list the key as they are.
    let shipped = vec![Set {
        name: "Suhina".into(),
        files: vec![("builtin:nam-amp-modeler/Suhina Overdrive.nam".into(), "Suhina Overdrive".into())],
    }];
    assert_eq!(pack_of_current(key, &shipped).map(|pack| pack.name.as_str()), Some("Suhina"));

    // A stock preset's key beside the unpacked copy reads its disk row: the steps and the
    // dropdown's own row match the disk path, so Next file steps to the successor, not the
    // pack's first.
    let next_to = Lists {
        models: vec![Set {
            name: "Suhina".into(),
            files: vec![
                ("/library/Amp models/Suhina/Suhina Overdrive.nam".into(), "Suhina Overdrive".into()),
                ("/library/Amp models/Suhina/Suhina Clean.nam".into(), "Suhina Clean".into()),
            ],
        }],
        ..Lists::default()
    };
    let rows = &next_to.models;
    let listed = listed_of_disk_row(key.to_owned(), pack_of_current(key, rows));
    assert_eq!(listed.as_str(), "/library/Amp models/Suhina/Suhina Overdrive.nam", "the key reads its disk row");
    assert_eq!(
        step_through(&rows[0].files, &listed, 1).as_deref(),
        Some("/library/Amp models/Suhina/Suhina Clean.nam"),
        "the key's successor, not the pack's first"
    );
    // A key with no unpacked copy stands whole: the shipped rows are the keys.
    let rows = &shipped;
    assert_eq!(listed_of_disk_row(key.to_owned(), pack_of_current(key, rows)).as_str(), key);
    // And a state that is no key stands as it is.
    assert_eq!(listed_of_disk_row("/library/other.nam".to_owned(), None), "/library/other.nam");
}
