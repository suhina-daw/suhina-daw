// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Path confinement for the commands an agent runs. `params` for every command is checked
//! here before it runs, so a file whose path an agent can name may only be one the
//! [`Workspace`] lets that agent at ([`docs/spec/browser.md`](../docs/spec/browser.md) ›
//! Commands): files read are inside a read root and made real, files written inside the write
//! root. Every caller of [`Session::execute`](suhina_session::Session::execute) under this crate's servers runs [`confine`] on
//! the params first: the MCP tools alone once, and a launch script's steps when its caller has
//! a workspace to confine with.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use suhina_session::{ParamType, PathKind};

use crate::workspace::{Workspace, display_path};

/// `params` with every path a command reads checked against the read root and made real
/// (`docs/spec/browser.md` › Commands), found by the type of each parameter
/// ([`ParamType::Path`]): files, folders, library items and places. The files and folders a
/// command writes are checked against the write root by that command's own rule
/// ([`confine_save`], [`confine_export`], [`confine_autosave`], [`confine_collect`]); `source` is
/// the file the project was opened from or last saved to.
pub fn confine(workspace: &Workspace, id: &str, mut params: Value, source: Option<&Path>) -> Result<Value, String> {
    match id {
        "project.save" => confine_save(workspace, &mut params, source)?,
        "project.export" => confine_export(workspace, &mut params)?,
        "project.autosave" => confine_autosave(workspace, &params, source)?,
        "project.collect" => confine_collect(workspace, &mut params, source)?,
        _ => {}
    }
    for param in suhina_session::params_of(id).unwrap_or_default() {
        let ParamType::Path(kind) = param.ty else { continue };
        let name = param.name;
        match kind {
            PathKind::File => confine_one_file(workspace, &mut params, name)?,
            PathKind::Files => {
                if let Some(paths) = params.get_mut(name).and_then(Value::as_array_mut) {
                    for path in paths.iter_mut() {
                        let given = path.as_str().ok_or_else(|| format!("{name} must be a list of file paths"))?;
                        *path = json!(display_path(&workspace.readable(given)?));
                    }
                }
            }
            PathKind::Folder => {
                if let Some(folder) = params.get(name).and_then(Value::as_str) {
                    params[name] = json!(display_path(&workspace.readable_folder(folder)?));
                }
            }
            PathKind::Place => confine_place(workspace, &mut params, name)?,
            PathKind::LibraryItem => confine_item(workspace, &mut params, name)?,
            // Checked by the command's own rule above.
            PathKind::WriteFile | PathKind::WriteFolder => {}
        }
    }
    Ok(params)
}

/// A library item is a file, maybe a voice in a bank (`bank.syx#4`), unless it names a device or
/// a plug-in, which are not on disk.
fn confine_item(workspace: &Workspace, params: &mut Value, name: &str) -> Result<(), String> {
    if let Some(item) = params.get(name).and_then(Value::as_str)
        && !item.starts_with("device:")
        && !item.starts_with("plugin:")
    {
        let (file, voice) = match item.rsplit_once('#') {
            Some((file, voice)) if voice.parse::<usize>().is_ok() => (file, Some(voice.to_string())),
            _ => (item, None),
        };
        let file = display_path(&workspace.readable(file)?);
        params[name] = json!(voice.map_or(file.clone(), |v| format!("{file}#{v}")));
    }
    Ok(())
}

/// `project.export` writes the audio file `path` (or `output`, the same thing) names, below
/// the write root only, an existing one replaced only with `overwrite: true` — the rules of
/// [`Workspace::writable`], the same as `project.render`'s file. The checked path goes back
/// in both aliases, so the command sees the one that was checked.
///
/// The alias is resolved as `project.export` itself resolves it in
/// [`suhina_session::export_from`]: the first of `path` and `output` that is a non-null
/// string decides, and nothing is trimmed — the checks see exactly what the command will use,
/// so no shape of input can have confinement resolve a different file than the command writes.
/// A value the command refuses on its own (a non-null non-text `path`, or nothing at all) is
/// left for the command to refuse with its own text.
fn confine_export(workspace: &Workspace, params: &mut Value) -> Result<(), String> {
    let overwrite = match params.get("overwrite") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("overwrite must be true or false".into()),
    };
    let resolved = params
        .get("path")
        .filter(|value| !value.is_null())
        .or_else(|| params.get("output").filter(|value| !value.is_null()))
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty());
    let Some(text) = resolved else {
        // The command itself asks for a path, or refuses what was given.
        return Ok(());
    };
    let target = workspace.writable(text, overwrite)?;
    let shown = json!(display_path(&target));
    params["path"] = shown.clone();
    params["output"] = shown;
    Ok(())
}

/// `project.save` writes below the write root only. Its `path` (with `.suhina` added as the
/// session adds it) must be there; without a path it saves over the project's own `.suhina` file,
/// which must be there too. Either way an existing file is
/// replaced only with `overwrite: true`, an MCP parameter that is taken out before the command
/// runs.
fn confine_save(workspace: &Workspace, params: &mut Value, source: Option<&Path>) -> Result<(), String> {
    let overwrite = match params.get("overwrite") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("overwrite must be true or false".into()),
    };
    if let Some(object) = params.as_object_mut() {
        object.remove("overwrite");
    }
    let mut path = match params.get("path") {
        Some(Value::String(path)) if !path.trim().is_empty() => PathBuf::from(path.trim()),
        Some(Value::String(_)) | None | Some(Value::Null) => match source {
            Some(source) if suhina_session::is_project_file(source) => source.to_path_buf(),
            // The command itself says it needs a path.
            _ => return Ok(()),
        },
        Some(_) => return Err("path must be text".into()),
    };
    if !suhina_session::is_project_file(&path) {
        let mut name = path.file_name().map(|n| n.to_os_string()).ok_or("give the project a file name")?;
        name.push(".");
        name.push(suhina_session::PROJECT_EXTENSION);
        path.set_file_name(name);
    }
    let text = path.to_str().ok_or("the path is not valid text")?;
    let target = workspace.writable(text, overwrite)?;
    params["path"] = json!(display_path(&target));
    Ok(())
}

/// `project.autosave` writes a backup into `.backup` beside the project's file
/// (`docs/spec/backups.md`), so `now: true` needs that folder below the write root; backups before
/// critical operations would write there unasked, so agents cannot choose them.
fn confine_autosave(workspace: &Workspace, params: &Value, source: Option<&Path>) -> Result<(), String> {
    if params.get("interval").and_then(Value::as_str).is_some_and(|i| i.trim() == "critical") {
        return Err("agents cannot choose backups before critical operations; use now: true".into());
    }
    if params.get("now") == Some(&Value::Bool(true)) {
        let source = source.ok_or("over MCP only a project with a file below the write root can be backed up")?;
        let folder = source.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
        workspace.writable_folder(folder)?;
        let backups = folder.join(suhina_session::backup::FOLDER);
        if std::fs::symlink_metadata(&backups).is_ok() {
            workspace.writable_folder(&backups)?;
        }
    }
    Ok(())
}

/// `project.collect` copies files into `Samples/Collected` of its folder, so that folder is below
/// the write root: the given `folder`, which may be new, or without one the folder of the
/// project's own file. A `Samples` or `Collected` that is there already must be below it too, so
/// a symlink there cannot lead the copies out.
fn confine_collect(workspace: &Workspace, params: &mut Value, source: Option<&Path>) -> Result<(), String> {
    let folder = match params.get("folder") {
        Some(Value::String(folder)) if !folder.trim().is_empty() => workspace.writable_new_folder(folder.trim())?,
        // The command would read a blank folder as the working folder, which nothing checked.
        Some(Value::String(_)) => return Err("folder must not be blank".into()),
        None | Some(Value::Null) => match source {
            Some(source) => {
                let folder = source.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
                workspace.writable_folder(folder)?
            }
            // The command itself says it needs a folder.
            None => return Ok(()),
        },
        Some(_) => return Err("folder must be text".into()),
    };
    let mut inside = folder.clone();
    for part in suhina_session::COLLECTED {
        inside = inside.join(part);
        if std::fs::symlink_metadata(&inside).is_ok() {
            workspace.writable_folder(&inside)?;
        }
    }
    params["folder"] = json!(display_path(&folder));
    Ok(())
}

/// A folder `place` must be below the read root; `pinned` and `recent:…` are the library's own.
fn confine_place(workspace: &Workspace, params: &mut Value, name: &str) -> Result<(), String> {
    if let Some(place) = params.get(name).and_then(Value::as_str)
        && place != "pinned"
        && !place.starts_with("recent:")
    {
        params[name] = json!(display_path(&workspace.readable_folder(place)?));
    }
    Ok(())
}

fn confine_one_file(workspace: &Workspace, params: &mut Value, name: &str) -> Result<(), String> {
    if let Some(given) = params.get(name).and_then(Value::as_str) {
        let path = workspace.readable(given)?;
        params[name] = json!(display_path(&path));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A read root, a write root below it, and a folder outside both, in a temp folder.
    fn roots(name: &str) -> (std::path::PathBuf, Workspace) {
        let dir = std::env::temp_dir().join(format!("suhina-confine-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("in")).unwrap();
        std::fs::create_dir_all(dir.join("elsewhere")).unwrap();
        let workspace = Workspace::new(Some(&dir.join("in")), Some(&dir.join("in"))).unwrap();
        (dir, workspace)
    }

    /// The alias resolution matches `project.export`'s own ([`suhina_session`]'s `export_from`):
    /// the first of `path` and `output` that is a non-null non-empty text decides, untrimmed, so
    /// no input shape has confinement resolve a different file than the command writes.
    #[test]
    fn project_export_writes_only_what_was_checked_by_either_alias() {
        let (dir, workspace) = roots("export-alias");
        let confine_export = |params: Value| confine(&workspace, "project.export", params, None);
        // A null `path` with an `output` outside the root: confined through `output`.
        let outside = dir.join("elsewhere").join("mix.wav").display().to_string();
        assert!(confine_export(json!({ "path": Value::Null, "output": outside })).is_err());
        // A value in one alias that the command refuses leaves the command to refuse it: a
        // non-null non-text `path` is the one the command reads, whatever `output` says. Those
        // go through unchanged, to fail at the command with its own text.
        for params in [
            json!({}),
            json!({ "path": Value::Null, "output": Value::Null }),
            json!({ "path": 5, "output": "mix.wav" }),
            json!({ "path": true }),
            json!({ "path": "" }),
        ] {
            assert_eq!(confine_export(params.clone()).unwrap_or_default(), params, "{params}");
        }
        // The checked destination is put in both aliases, so neither names what was not checked.
        let ok = confine_export(json!({ "path": Value::Null, "output": "mix.wav" })).unwrap();
        assert!(Path::new(ok["path"].as_str().unwrap()).is_absolute());
        assert!(Path::new(ok["output"].as_str().unwrap()).is_absolute());
        assert_eq!(ok["output"], ok["path"]);
        // A name of spaces stays a name all the same — nothing is trimmed before the check —
        // and joins the write root like any relative path.
        let spaced = confine_export(json!({ "path": "  " })).unwrap();
        assert!(Path::new(spaced["path"].as_str().unwrap()).is_absolute(), "{spaced}");
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod from_the_tool_lists {
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
        let ok =
            confine(&workspace, "clip.addFiles", json!({ "paths": ["pack/kick.wav"], "start": 0.0 }), None).unwrap();
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
        let collect =
            |w: &Workspace, params: Value, source: Option<&Path>| confine(w, "project.collect", params, source);
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
        let set = |path: &str| {
            confine(&workspace, "device.setNamFile", json!({ "track": 1, "device": 0, "path": path }), None)
        };
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
        let autosave =
            |w: &Workspace, params: Value, source: Option<&Path>| confine(w, "project.autosave", params, source);
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
}
