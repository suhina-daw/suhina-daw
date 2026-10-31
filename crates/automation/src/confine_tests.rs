// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn roots(name: &str) -> (std::path::PathBuf, Workspace) {
    let dir = std::env::temp_dir().join(format!("suhina-confine-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("inside/pack")).unwrap();
    std::fs::create_dir_all(dir.join("outside")).unwrap();
    for file in ["inside/pack/kick.wav", "inside/bank.syx", "outside/secret.wav"] {
        std::fs::write(dir.join(file), b"x").unwrap();
    }
    let workspace = Workspace::new(Some(&dir.join("inside")), None).unwrap();
    (dir, workspace)
}

#[test]
fn paths_agents_give_must_be_inside_the_read_root() {
    let (dir, workspace) = roots("paths");
    let ok = confine(&workspace, "clip.addFiles", json!({ "paths": ["pack/kick.wav"], "start": 0.0 }), None).unwrap();
    assert!(ok["paths"][0].as_str().unwrap().ends_with("kick.wav"));
    assert!(Path::new(ok["paths"][0].as_str().unwrap()).is_absolute());
    let update = confine(&workspace, "device.updateSd", json!({"path":"pack/kick.wav"}), None).unwrap();
    assert!(Path::new(update["path"].as_str().unwrap()).is_absolute());
    let outside = dir.join("outside").join("secret.wav").display().to_string();
    for (id, params) in [
        ("clip.addFiles", json!({ "paths": [outside.clone()] })),
        ("clip.addFiles", json!({ "paths": ["../outside/secret.wav"] })),
        ("preview.play", json!({ "path": outside.clone() })),
        ("device.updateSd", json!({ "track":1,"device":0,"path": outside.clone() })),
        ("device.add", json!({ "track": 1, "preset": outside.clone() })),
        ("library.list", json!({ "folder": dir.join("outside").display().to_string() })),
        ("library.search", json!({ "place": dir.join("outside").display().to_string() })),
        ("library.info", json!({ "item": outside.clone() })),
        ("library.tag", json!({ "item": format!("{outside}#3"), "tag": "Red", "on": true })),
        ("soundmap.layout", json!({ "place": dir.join("outside").display().to_string() })),
        ("soundmap.similar", json!({ "item": outside.clone() })),
        ("soundmap.info", json!({ "item": outside.clone() })),
        ("soundmap.setKind", json!({ "item": outside.clone(), "kind": "kick" })),
        ("soundmap.analyse", json!({ "folder": dir.join("outside").display().to_string() })),
        ("project.relinkFiles", json!({ "files": ["a.wav"], "to": [outside.clone()] })),
        ("library.findFiles", json!({ "names": ["a.wav"], "folder": dir.join("outside").display().to_string() })),
    ] {
        assert!(confine(&workspace, id, params.clone(), None).is_err(), "{id} {params}");
    }
    let kept = json!({ "item": "device:overdrive" });
    assert_eq!(confine(&workspace, "library.pin", kept.clone(), None).unwrap(), kept);
    let place = json!({ "place": "recent:samples" });
    assert_eq!(confine(&workspace, "library.search", place.clone(), None).unwrap(), place);
    assert_eq!(confine(&workspace, "soundmap.layout", place.clone(), None).unwrap(), place);
    let voice = confine(&workspace, "library.info", json!({ "item": "bank.syx#4" }), None).unwrap();
    assert!(voice["item"].as_str().unwrap().ends_with("bank.syx#4"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn project_collect_writes_only_below_the_write_root() {
    let (dir, read_only) = roots("collect");
    std::fs::create_dir_all(dir.join("out/song")).unwrap();
    let workspace = Workspace::new(Some(&dir.join("inside")), Some(&dir.join("out"))).unwrap();
    let collect = |w: &Workspace, params: Value, source: Option<&Path>| confine(w, "project.collect", params, source);
    let outside = dir.join("outside").join("collected");

    // A folder outside the write root, by an absolute path or by '..', is refused, and nothing is made.
    assert!(collect(&workspace, json!({ "folder": outside.display().to_string() }), None).is_err());
    assert!(collect(&workspace, json!({ "folder": "../outside/collected" }), None).is_err());
    assert!(!outside.exists());
    // Without a write root nothing is written, whatever the folder.
    assert!(collect(&read_only, json!({ "folder": "collected" }), None).is_err());
    // Without a folder the project's own folder is used, which must be below the write root.
    let source = dir.join("inside").join("own.suhina");
    assert!(collect(&workspace, json!({}), Some(&source)).is_err());
    let source = dir.join("out").join("song").join("own.suhina");
    assert!(collect(&read_only, json!({}), Some(&source)).is_err());
    let ok = collect(&workspace, json!({}), Some(&source)).unwrap();
    assert!(ok["folder"].as_str().unwrap().ends_with("song"), "{ok}");
    // A project with no file is left to the command, which asks for a folder.
    assert_eq!(collect(&workspace, json!({}), None).unwrap(), json!({}));

    // Below the write root it works, in a folder that is new or one that is there.
    let ok = collect(&workspace, json!({ "folder": "fresh" }), None).unwrap();
    assert!(Path::new(ok["folder"].as_str().unwrap()).is_absolute());
    assert!(ok["folder"].as_str().unwrap().ends_with("fresh"));
    assert!(collect(&workspace, json!({ "folder": "song" }), None).is_ok());
    assert!(collect(&workspace, json!({ "folder": 3 }), None).is_err());
    // A blank folder is refused, with or without a project file.
    for blank in ["", "  "] {
        assert!(collect(&workspace, json!({ "folder": blank }), None).is_err());
        assert!(collect(&workspace, json!({ "folder": blank }), Some(&source)).is_err());
    }
    #[cfg(unix)]
    {
        // A Samples folder or a folder that leads outside is refused.
        std::os::unix::fs::symlink(dir.join("outside"), dir.join("out").join("link")).unwrap();
        assert!(collect(&workspace, json!({ "folder": "link" }), None).is_err());
        std::os::unix::fs::symlink(dir.join("outside"), dir.join("out").join("song").join("Samples")).unwrap();
        assert!(collect(&workspace, json!({ "folder": "song" }), None).is_err());
        assert!(collect(&workspace, json!({}), Some(&source)).is_err());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_nam_model_or_impulse_response_is_read_below_the_read_root_only() {
    let (dir, workspace) = roots("nam");
    std::fs::write(dir.join("inside/amp.nam"), b"x").unwrap();
    std::fs::write(dir.join("outside/amp.nam"), b"x").unwrap();
    let set =
        |path: &str| confine(&workspace, "device.setNamFile", json!({ "track": 1, "device": 0, "path": path }), None);
    let ok = set("amp.nam").unwrap();
    assert!(Path::new(ok["path"].as_str().unwrap()).is_absolute());
    assert_eq!(ok["track"], 1);
    assert!(set(&dir.join("outside/amp.nam").display().to_string()).is_err());
    assert!(set("../outside/amp.nam").is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn project_autosave_writes_only_below_the_write_root_and_never_unasked() {
    let (dir, read_only) = roots("autosave");
    std::fs::create_dir_all(dir.join("out")).unwrap();
    let workspace = Workspace::new(Some(&dir.join("inside")), Some(&dir.join("out"))).unwrap();
    let autosave = |w: &Workspace, params: Value, source: Option<&Path>| confine(w, "project.autosave", params, source);
    let inside = dir.join("inside").join("song.suhina");
    let out = dir.join("out").join("song.suhina");
    // Reporting is always fine.
    assert!(autosave(&read_only, json!({}), Some(&inside)).is_ok());
    assert!(autosave(&workspace, json!({ "interval": "5m", "cleanUp": false }), None).is_ok());
    // Backups before critical operations would write beside the project unasked.
    assert!(autosave(&workspace, json!({ "interval": "critical" }), Some(&out)).is_err());
    // Now writes beside the project's file, which must be below the write root.
    assert!(autosave(&workspace, json!({ "now": true }), None).is_err(), "no file of its own");
    assert!(autosave(&workspace, json!({ "now": true }), Some(&inside)).is_err());
    assert!(autosave(&read_only, json!({ "now": true }), Some(&out)).is_err());
    assert!(autosave(&workspace, json!({ "now": true }), Some(&out)).is_ok());
    #[cfg(unix)]
    {
        // A .backup that leads outside is refused.
        std::os::unix::fs::symlink(dir.join("inside"), dir.join("out").join(".backup")).unwrap();
        assert!(autosave(&workspace, json!({ "now": true }), Some(&out)).is_err());
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn project_save_writes_only_below_the_write_root() {
    let (dir, read_only) = roots("save");
    std::fs::create_dir_all(dir.join("out")).unwrap();
    let workspace = Workspace::new(Some(&dir.join("inside")), Some(&dir.join("out"))).unwrap();
    let save = |w: &Workspace, params: Value, source: Option<&Path>| confine(w, "project.save", params, source);

    // Outside the write root, by an absolute path or by '..'.
    let outside = dir.join("outside").join("song.suhina").display().to_string();
    assert!(save(&workspace, json!({ "path": outside }), None).is_err());
    assert!(save(&workspace, json!({ "path": "../outside/song" }), None).is_err());
    // Without a write root nothing is written, with a path or over the project's own file.
    assert!(save(&read_only, json!({ "path": "song.suhina" }), None).is_err());
    let own = dir.join("inside").join("own.suhina");
    std::fs::write(&own, b"x").unwrap();
    let own = own.canonicalize().unwrap();
    assert!(save(&read_only, json!({}), Some(&own)).is_err());
    assert!(save(&read_only, json!({ "overwrite": true }), Some(&own)).is_err());
    // Nor over a project file outside the write root.
    assert!(save(&workspace, json!({ "overwrite": true }), Some(&own)).is_err());

    // Below the write root it works; .suhina is added and `overwrite` is taken out.
    let ok = save(&workspace, json!({ "path": "song", "copy": true }), None).unwrap();
    let written = ok["path"].as_str().unwrap();
    assert!(written.ends_with("song.suhina"), "{written}");
    assert!(Path::new(written).is_absolute());
    assert_eq!(ok["copy"], true);

    // An existing file is replaced only with overwrite: true, also the project's own file.
    let mine = dir.join("out").join("song.suhina");
    std::fs::write(&mine, b"x").unwrap();
    assert!(save(&workspace, json!({ "path": "song.suhina" }), None).is_err());
    let ok = save(&workspace, json!({ "path": "song.suhina", "overwrite": true }), None).unwrap();
    assert!(ok.get("overwrite").is_none());
    let mine = mine.canonicalize().unwrap();
    assert!(save(&workspace, json!({}), Some(&mine)).is_err());
    let ok = save(&workspace, json!({ "overwrite": true }), Some(&mine)).unwrap();
    assert!(ok["path"].as_str().unwrap().ends_with("song.suhina"));
    assert!(save(&workspace, json!({ "path": "x", "overwrite": "yes" }), None).is_err());

    // A project without a project file and no path is left to the command, which asks for one.
    assert_eq!(save(&workspace, json!({}), None).unwrap(), json!({}));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn project_export_writes_only_below_the_write_root() {
    let (dir, read_only) = roots("export");
    std::fs::create_dir_all(dir.join("out")).unwrap();
    let workspace = Workspace::new(Some(&dir.join("inside")), Some(&dir.join("out"))).unwrap();
    let export = |w: &Workspace, params: Value| confine(w, "project.export", params, None);

    // Outside the write root, by an absolute path or by '..'.
    let outside = dir.join("outside").join("mix.wav").display().to_string();
    assert!(export(&workspace, json!({ "path": outside })).is_err());
    assert!(export(&workspace, json!({ "path": "../outside/mix.wav" })).is_err());
    // Without a write root nothing is written.
    assert!(export(&read_only, json!({ "path": "mix.wav" })).is_err());
    // An existing file below the write root is replaced only when overwrite says so.
    let mine = dir.join("out").join("mix.wav");
    std::fs::write(&mine, b"x").unwrap();
    assert!(export(&workspace, json!({ "path": "mix.wav" })).is_err());
    assert!(export(&workspace, json!({ "path": "mix.wav", "overwrite": "yes" })).is_err());
    // Below the write root it works by either alias, and the checked path is put in both.
    let ok = export(&workspace, json!({ "output": "mix.wav", "overwrite": true })).unwrap();
    assert!(Path::new(ok["path"].as_str().unwrap()).is_absolute());
    assert!(Path::new(ok["output"].as_str().unwrap()).is_absolute());
    // Without a path the command's own text is left to say it.
    assert_eq!(export(&workspace, json!({})).unwrap(), json!({}));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn library_move_to_takes_files_only_from_below_the_write_root() {
    let (dir, _) = roots("move");
    std::fs::create_dir_all(dir.join("inside/out")).unwrap();
    // Read and write roots both inside: the pack's kick may move into out.
    let workspace = Workspace::new(Some(&dir.join("inside")), Some(&dir.join("inside"))).unwrap();
    let kick = dir.join("inside/pack/kick.wav").display().to_string();
    let out = dir.join("inside/out").display().to_string();
    let ok = confine(&workspace, "library.moveTo", json!({ "items": [kick], "folder": out }), None);
    assert!(ok.is_ok(), "{ok:?}");
    // With the write root only out, the kick cannot be taken from the pack.
    let narrow = Workspace::new(Some(&dir.join("inside")), Some(&dir.join("inside/out"))).unwrap();
    let out = dir.join("inside/out").display().to_string();
    assert!(confine(&narrow, "library.moveTo", json!({ "items": [kick], "folder": out }), None).is_err());
}

#[test]
fn sd_sample_path_obeys_the_same_read_boundary_as_the_graph() {
    let (dir, workspace) = roots("sd-sample");
    std::fs::write(dir.join("inside/graph.sd"), b"{}").unwrap();
    std::fs::write(dir.join("inside/sample.wav"), b"wav").unwrap();
    std::fs::write(dir.join("outside/private.wav"), b"private").unwrap();
    let graph = dir.join("inside/graph.sd");
    for (id, key) in [("device.addSd", "path"), ("track.add", "sd")] {
        let mut request = json!({"sample":dir.join("outside/private.wav")});
        request[key] = json!(graph);
        assert!(confine(&workspace, id, request.clone(), None).is_err());
        request["sample"] = json!(dir.join("inside/sample.wav"));
        let ok = confine(&workspace, id, request, None).unwrap();
        assert!(Path::new(ok["sample"].as_str().unwrap()).is_absolute());
    }
    let _ = std::fs::remove_dir_all(dir);
}
