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
        models: vec![Set { name: "Test amp".into(), files: vec![("p/Test amp/test.nam".into(), "Test amp".into())] }],
        irs: vec![],
        names: HashMap::from([("p/Test amp/test.nam".into(), "Fender Super Reverb".into())]),
        notes: HashMap::new(),
    };
    // The pack a file sits in, by its parent folder; a file outside the folder's packs is none of them.
    assert_eq!(pack_of_current("p/Test amp/test.nam", &lists.models).map(|p| p.name.as_str()), Some("Test amp"));
    assert_eq!(pack_of_current("elsewhere/x.nam", &lists.models), None);
    // The shown name from the session's names, else the path's own.
    assert_eq!(file_shown("p/Test amp/test.nam", &lists), "Fender Super Reverb");
    assert_eq!(file_shown("p/Other/loose.nam", &lists), "loose.nam");
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
