// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The sample map's commands (`docs/spec/sample-map.md` › Commands, ADR 0013): analysis of the
//! library's samples, layouts of a scope, similar items, kinds set by hand. Like the library they belong to
//! Suhina, not to one project, so they run on [`Projects`] and never change a project.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use suhina_library::{FileKind, Item};
use suhina_soundmap::cache::Analysis;
use suhina_soundmap::kinds::Kind;
use suhina_soundmap::{DEFAULT_MAX_SECONDS, MAX_SECONDS_RANGE, MAX_SIMILAR, Map, SoundMap, SourceFile};

use crate::commands::{CommandKind, CommandSpec, ParamType, optional, param};
use crate::{Agents, CommandError, PathKind, Policy, Projects};

const PLACE: &str = "pinned, recent:all, recent:samples, or a folder (default: every folder)";

static SOUND_MAP_COMMANDS: &[CommandSpec<Projects>] = &[
    CommandSpec {
        id: "soundmap.status",
        label: "How far the sample map's analysis of the browser's samples is, per folder: analysed, queued, failed",
        params: &[],
        kind: CommandKind::Library,
        run: status,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "soundmap.analyse",
        label: "Analyse a folder's samples, or every folder's, again for the sample map, ignoring what was kept",
        params: &[optional(
            "folder",
            ParamType::Path(PathKind::Folder),
            "a folder, as library.folders lists them (default: every folder)",
        )],
        kind: CommandKind::Library,
        run: analyse,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "soundmap.layout",
        label: "Lay out samples by how they sound and their kind: each analysed sample's position (0–1), kind (kick, snare, hat…, whether it was guessed from the sound and whether it was set by hand) and whether it matches the text and tags; and how many are not analysed yet, too long or unreadable",
        params: &[
            optional("place", ParamType::Path(PathKind::Place), PLACE),
            optional("text", ParamType::Text, "words a matching sample's name or path holds"),
            optional("tags", ParamType::Texts, "a matching sample carries any of these tags"),
            optional("grid", ParamType::Bool, "also give each sample's cell in a grid (column, row)"),
            optional("aspect", ParamType::Number, "the grid's width over its height, 0.1 to 10 (default 1, square)"),
            optional("maxSeconds", ParamType::Number, "leave out samples longer than this, 1 to 60 (default 10)"),
        ],
        kind: CommandKind::Library,
        run: layout,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "soundmap.similar",
        label: "The samples that sound most like a sample, nearest first, with their distance",
        params: &[
            param("item", ParamType::Path(PathKind::File), "a sample's path"),
            optional("count", ParamType::Integer, "how many, 1 to 100 (default 16)"),
            optional("place", ParamType::Path(PathKind::Place), PLACE),
        ],
        kind: CommandKind::Library,
        run: similar,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "soundmap.info",
        label: "What the sample map knows of a sample: its kind and where it comes from (set by hand, its name, or a guess from its sound), its length and its features",
        params: &[param("item", ParamType::Path(PathKind::File), "a sample's path")],
        kind: CommandKind::Library,
        run: info,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "soundmap.setKind",
        label: "Set a sample's kind by hand, over its name and its sound, or give it back to them (auto); kept with the library and through new analyses and layouts",
        params: &[
            param("item", ParamType::Path(PathKind::File), "a sample's path"),
            param("kind", ParamType::Text, KIND_PARAM),
        ],
        kind: CommandKind::Library,
        run: set_kind,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
];

const KIND_PARAM: &str = "kick, snare, clap, closedHat, openHat, hat, rim, tom, crash, ride, cymbal, percussion, bass, tonal, vocal, effect or other; auto to let the name and the sound decide again";

/// The sample map commands, as `command.list` shows them.
pub fn sound_map_commands() -> &'static [CommandSpec<Projects>] {
    SOUND_MAP_COMMANDS
}

impl Projects {
    /// The sample map's analysis, kept beside the library's indexes.
    pub fn sound_map_mut(&mut self) -> &mut SoundMap {
        let folder = self.file_library_mut().data_folder();
        self.browser.sound_map.get_or_insert_with(|| SoundMap::new(folder, SoundMap::default_threads()))
    }

    /// Starts analysing the library's samples in the background, and keeps doing so as files
    /// come and go. The desktop app and the MCP server start it, as they start the library;
    /// tests and the CLI never analyse a user's folders unasked.
    pub fn start_sound_map(&mut self) {
        self.browser.sound_map_started = true;
        // A large cache is read off the caller's thread (the window's).
        self.sound_map_mut().read_in_background(true);
        self.poll_sound_map();
    }

    /// Waits until every queued sample is analysed (tests, the CLI).
    pub fn wait_sound_map(&mut self) {
        self.poll_sound_map();
        if let Some(map) = &mut self.browser.sound_map {
            map.wait();
        }
    }

    /// Takes in finished analyses and, once started, queues new and changed samples when the
    /// library changed. Call it often (the window calls it every frame); returns whether
    /// anything changed.
    pub fn poll_sound_map(&mut self) -> bool {
        let mut changed = false;
        if self.browser.sound_map_started {
            let library = self.file_library_mut();
            library.poll();
            let revision = library.revision();
            // While the update before is worked out, the index is not copied for the next: that
            // would be refused, and copied again every frame until it was not.
            if self.browser.sound_map_revision != Some(revision) && !self.sound_map_mut().updating() {
                if self.sound_map_mut().reads_in_background() {
                    // Listed and compared on a thread of its own: a large library held the
                    // window up for a second here.
                    let snapshot = self.file_library_mut().index_snapshot();
                    let started = self.sound_map_mut().update_in_background(move || {
                        suhina_library::files_in(&snapshot, Some(FileKind::Sample))
                            .map(|f| SourceFile { folder: f.folder, path: f.path, size: f.size, modified: f.modified })
                            .collect()
                    });
                    if started {
                        self.browser.sound_map_revision = Some(revision);
                    }
                } else {
                    let files = sample_files(self);
                    self.sound_map_mut().update(&files);
                    self.browser.sound_map_revision = Some(revision);
                }
                changed = true;
            }
        }
        if let Some(map) = &mut self.browser.sound_map {
            changed |= suhina_plugins::trace::timed("sound map: results", || map.poll());
        }
        changed
    }
}

fn sample_files(projects: &mut Projects) -> Vec<SourceFile> {
    projects
        .file_library_mut()
        .files(Some(FileKind::Sample))
        .map(|f| SourceFile { folder: f.folder, path: f.path, size: f.size, modified: f.modified })
        .collect()
}

/// The samples of a place: `None` for every folder, `pinned`, `recent:all`, `recent:samples`,
/// or a folder.
pub fn scope_files(projects: &mut Projects, place: Option<&str>) -> Result<Vec<SourceFile>, CommandError> {
    let files = sample_files(projects);
    if place.is_none() {
        return Ok(files);
    }
    let keys = crate::library::listed_keys(projects, place)?;
    Ok(match (keys, place) {
        (Some(keys), _) => {
            let wanted: HashSet<PathBuf> = keys
                .iter()
                .filter_map(|k| match Item::parse(k)? {
                    Item::File(path) => Some(path),
                    _ => None,
                })
                .collect();
            files.into_iter().filter(|f| wanted.contains(&f.path)).collect()
        }
        (None, Some(folder)) => {
            let folder = Path::new(folder);
            files.into_iter().filter(|f| f.path.starts_with(folder)).collect()
        }
        (None, None) => files,
    })
}

fn text<'a>(params: &'a Value, name: &str) -> Result<&'a str, CommandError> {
    params
        .get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| CommandError(format!("{name} must be a text")))
}

fn optional_text<'a>(params: &'a Value, name: &str) -> Result<Option<&'a str>, CommandError> {
    match params.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => text(params, name).map(Some),
    }
}

fn status(projects: &mut Projects, _: &Value) -> Result<Value, CommandError> {
    projects.poll_sound_map();
    let files = sample_files(projects);
    let started = projects.browser.sound_map_started;
    let folders: Vec<Value> = projects
        .sound_map_mut()
        .status(&files)
        .into_iter()
        .map(|s| {
            json!({ "folder": s.folder.display().to_string(), "analysed": s.analysed, "queued": s.queued, "failed": s.failed })
        })
        .collect();
    Ok(json!({ "started": started, "folders": folders }))
}

fn analyse(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let folder = optional_text(params, "folder")?;
    projects.start_sound_map();
    let files = sample_files(projects);
    let folders: Vec<PathBuf> = match folder {
        Some(f) => {
            let f = PathBuf::from(f);
            if !files.iter().any(|file| file.folder == f) {
                return Err(format!("{} is not a browser folder with samples", f.display()).into());
            }
            vec![f]
        }
        None => {
            let mut all: Vec<PathBuf> = files.iter().map(|f| f.folder.clone()).collect();
            all.sort();
            all.dedup();
            all
        }
    };
    let map = projects.sound_map_mut();
    for folder in &folders {
        map.reanalyse(folder, &files);
    }
    let queued = files.iter().filter(|f| folders.contains(&f.folder)).count();
    Ok(json!({ "queued": queued }))
}

fn max_seconds(params: &Value) -> Result<f32, CommandError> {
    match params.get("maxSeconds") {
        None | Some(Value::Null) => Ok(DEFAULT_MAX_SECONDS),
        Some(v) => v
            .as_f64()
            .map(|s| s as f32)
            .filter(|s| (MAX_SECONDS_RANGE.0..=MAX_SECONDS_RANGE.1).contains(s))
            .ok_or_else(|| "maxSeconds must be a number from 1 to 60".into()),
    }
}

fn tags(params: &Value) -> Result<Vec<String>, CommandError> {
    match params.get("tags") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(v) => v
            .as_array()
            .ok_or("tags must be a list of tag names")?
            .iter()
            .map(|t| t.as_str().map(str::to_string).ok_or_else(|| "tags must be a list of tag names".into()))
            .collect(),
    }
}

/// The map of a place, laid out here when it was not before.
fn map_of(projects: &mut Projects, place: Option<&str>, max_seconds: f32) -> Result<Map, CommandError> {
    projects.poll_sound_map();
    let files = scope_files(projects, place)?;
    Ok(projects.sound_map_mut().map(&files, max_seconds))
}

/// Whether a sample matches search words and tags, as the browser's search decides.
pub fn matches(projects: &mut Projects, path: &Path, words: &[String], tags: &[String]) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let key = path.display().to_string();
    (words.is_empty() || suhina_library::rank(words, &name, &key).is_some())
        && (tags.is_empty() || projects.file_library_mut().item_tags(&key).iter().any(|t| tags.contains(t)))
}

fn layout(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let place = optional_text(params, "place")?;
    let words = suhina_library::words(optional_text(params, "text")?.unwrap_or(""));
    let tags = tags(params)?;
    let grid = match params.get("grid") {
        None | Some(Value::Null) => false,
        Some(v) => v.as_bool().ok_or("grid must be true or false")?,
    };
    let aspect = match params.get("aspect") {
        None | Some(Value::Null) => 1.0,
        Some(v) => {
            v.as_f64().filter(|a| (0.1..=10.0).contains(a)).ok_or("aspect must be a number from 0.1 to 10")? as f32
        }
    };
    let map = map_of(projects, place, max_seconds(params)?)?;
    let cells = grid.then(|| map.grid(aspect));
    let mut items = Vec::with_capacity(map.points.len());
    for (i, point) in map.points.iter().enumerate() {
        let mut item = json!({
            "item": point.path.display().to_string(),
            "x": point.position[0],
            "y": point.position[1],
            "kind": point.kind.id(),
            "guessed": point.guessed,
            "byHand": point.by_hand,
            "matches": matches(projects, &point.path, &words, &tags),
        });
        if let Some((column, row)) = cells.as_ref().and_then(|g| g.cells.get(i)) {
            item["column"] = json!(column);
            item["row"] = json!(row);
        }
        items.push(item);
    }
    let mut out = json!({
        "items": items,
        "pending": map.pending,
        "tooLong": map.too_long,
        "failed": map.failed,
        "overLimit": map.over_limit,
    });
    if let Some(g) = &cells {
        out["columns"] = json!(g.columns);
        out["rows"] = json!(g.rows);
    }
    Ok(out)
}

fn similar(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let item = PathBuf::from(text(params, "item")?);
    let count = match params.get("count") {
        None | Some(Value::Null) => 16,
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| (1..=MAX_SIMILAR).contains(n))
            .ok_or("count must be a whole number from 1 to 100")?,
    };
    let place = optional_text(params, "place")?;
    let map = map_of(projects, place, MAX_SECONDS_RANGE.1)?;
    if !map.points.iter().any(|p| p.path == item) {
        return Err(format!("{} is not an analysed sample of this place", item.display()).into());
    }
    let items: Vec<Value> = map
        .similar(&item, count)
        .into_iter()
        .map(|(path, distance)| json!({ "item": path.display().to_string(), "distance": distance }))
        .collect();
    Ok(json!({ "items": items }))
}

/// The sample in the browser's folders at `path`.
fn sample_file(projects: &mut Projects, path: &Path) -> Result<SourceFile, CommandError> {
    sample_files(projects)
        .into_iter()
        .find(|f| f.path == path)
        .ok_or_else(|| format!("{} is not a sample in the browser's folders", path.display()).into())
}

fn info(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let path = PathBuf::from(text(params, "item")?);
    projects.poll_sound_map();
    let file = sample_file(projects, &path)?;
    let hand = projects.sound_map_mut().kind_by_hand(&file.path);
    let named = suhina_soundmap::classify::named(&file.path, &file.folder);
    let analysis = projects.sound_map_mut().analysis(&file);
    let mut out = json!({ "item": path.display().to_string() });
    match analysis {
        None => out["state"] = json!("queued"),
        Some(Analysis::Failed(reason)) => {
            out["state"] = json!("failed");
            out["reason"] = json!(reason);
        }
        Some(Analysis::Analysed { features, seconds }) => {
            out["state"] = json!("analysed");
            out["seconds"] = json!(seconds);
            out["features"] = json!(features.to_vec());
            out["featureVersion"] = json!(suhina_soundmap::features::FEATURE_VERSION);
        }
    }
    match (hand, named) {
        (Some(kind), _) => {
            out["kind"] = json!(kind.id());
            out["kindFrom"] = json!("hand");
        }
        (None, Some((kind, suhina_soundmap::classify::Evidence::FileName))) => {
            out["kind"] = json!(kind.id());
            out["kindFrom"] = json!("name");
        }
        _ => {
            // A folder's kind may give way to the sound, and a guess needs the kinds learned
            // from the sample's library folder: its map.
            let folder = file.folder.display().to_string();
            let map = map_of(projects, Some(&folder), MAX_SECONDS_RANGE.1)?;
            let point = map.points.iter().find(|p| p.path == path);
            let kind = point.map(|p| p.kind).or(named.map(|(k, _)| k)).unwrap_or(Kind::Other);
            let from = match point {
                Some(p) if p.guessed => "guess",
                Some(_) | None if named.is_some() => "name",
                _ => "none",
            };
            out["kind"] = json!(kind.id());
            out["kindFrom"] = json!(from);
        }
    }
    Ok(out)
}

fn set_kind(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let path = PathBuf::from(text(params, "item")?);
    let id = text(params, "kind")?;
    let kind = match id {
        "auto" => None,
        id => Some(Kind::from_id(id).ok_or_else(|| format!("unknown kind {id:?}: {KIND_PARAM}"))?),
    };
    let file = sample_file(projects, &path)?;
    projects.sound_map_mut().set_kind(&file.path, kind)?;
    Ok(json!({ "item": path.display().to_string(), "kind": kind.map_or("auto", Kind::id) }))
}
