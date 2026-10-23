// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Commands for the browser's library (`docs/spec/browser.md` › Commands, ADR 0012): the
//! folders, searching them with the built-in effects and instruments, and installed plug-ins, what a file is,
//! pins, recent items and colour tags. They belong to Suhina, not to one project, so they run
//! on [`Projects`] like the `plugin.*` commands, and never change a project.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use suhina_device_api::Role;
use suhina_library::{FileKind, Group, Item, Library, Query};

use crate::commands::{CommandKind, CommandSpec, Param, ParamType, optional, param};
use crate::devices::role_name;
use crate::{Agents, CommandError, PathKind, Policy, Projects};

const FOLDER: Param = param("folder", ParamType::Path(PathKind::Folder), "a folder, as library.folders lists them");
const ITEM: Param = param(
    "item",
    ParamType::Path(PathKind::LibraryItem),
    "an item: a file's path, <bank>.syx#<voice>, device:<id> or plugin:<spec>, as library.search gives them",
);

static LIBRARY_COMMANDS: &[CommandSpec<Projects>] = &[
    CommandSpec {
        id: "library.folders",
        label: "List the browser's folders with how each is kept up to date (watching, polling, scanning, missing, idle) and how many files it holds",
        params: &[],
        kind: CommandKind::Library,
        run: folders,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "library.addFolder",
        label: "Add a folder to the browser and index the samples, MIDI files, presets and projects in it",
        params: &[FOLDER],
        kind: CommandKind::Library,
        run: add_folder,
        policy: Policy { agents: Agents::Denied("the user chooses the browser's folders"), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "library.removeFolder",
        label: "Remove a folder from the browser (the files stay where they are)",
        params: &[FOLDER],
        kind: CommandKind::Library,
        run: remove_folder,
        policy: Policy { agents: Agents::Denied("the user chooses the browser's folders"), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "library.refresh",
        label: "Read a folder, or every folder, again",
        params: &[optional("folder", ParamType::Text, "a folder (default: every folder)")],
        kind: CommandKind::Library,
        run: refresh,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "library.list",
        label: "List what is directly in a folder: sub-folders first, then samples, MIDI files, presets and projects",
        params: &[param("folder", ParamType::Path(PathKind::Folder), "any folder")],
        kind: CommandKind::Library,
        run: list,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "library.search",
        label: "Search the folders' files, the built-in effects and instruments, and the installed plug-ins; every word must appear in an item's name or path, best matches first",
        params: &[
            optional("text", ParamType::Text, "the words to find (default: everything)"),
            optional(
                "category",
                ParamType::Text,
                "all (default), samples, midi, instruments, effects, presets or plugins",
            ),
            optional(
                "place",
                ParamType::Path(PathKind::Place),
                "pinned, recent:all, recent:<group> (projects, samples, midi, instruments, effects, plugins, presets), or a folder",
            ),
            optional("tags", ParamType::Texts, "only items carrying any of these tags"),
            optional("limit", ParamType::Integer, "at most this many (default and most 500)"),
        ],
        kind: CommandKind::Library,
        run: search,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "library.info",
        label: "What an item is: a sample's length, rate, channels, tempo (and whether it was detected) and key; a MIDI file's tracks; a preset bank's voices; a device's or plug-in's role",
        params: &[ITEM],
        kind: CommandKind::Library,
        run: info,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "library.pinned",
        label: "List the pinned items, oldest first",
        params: &[],
        kind: CommandKind::Library,
        run: pinned,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "library.pin",
        label: "Pin an item, so it is listed under Pinned",
        params: &[ITEM],
        kind: CommandKind::Library,
        run: pin,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "library.unpin",
        label: "Unpin an item",
        params: &[ITEM],
        kind: CommandKind::Library,
        run: unpin,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "library.recent",
        label: "List the items used lately, newest first, by group (projects, samples, midi, instruments, effects, plugins, presets)",
        params: &[optional("group", ParamType::Text, "one group (default: every group)")],
        kind: CommandKind::Library,
        run: recent,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "library.tags",
        label: "List the colour tags (name, colour), the items carrying them and the most tags there may be",
        params: &[],
        kind: CommandKind::Library,
        run: tags,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "library.setTag",
        label: "Add a colour tag, or change a tag's colour or name; or change the most tags there may be",
        params: &[
            optional("name", ParamType::Text, "the tag"),
            optional("color", ParamType::Text, "its colour, #RRGGBB (needed for a new tag)"),
            optional("rename", ParamType::Text, "a new name for it"),
            optional("limit", ParamType::Integer, "the most tags there may be, from 1 to 1024 (default 64)"),
        ],
        kind: CommandKind::Library,
        run: set_tag,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "library.removeTag",
        label: "Remove a colour tag from the list and from every item",
        params: &[param("name", ParamType::Text, "the tag")],
        kind: CommandKind::Library,
        run: remove_tag,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "library.tag",
        label: "Set or clear a colour tag on an item",
        params: &[
            ITEM,
            param("tag", ParamType::Text, "the tag's name"),
            param("on", ParamType::Bool, "true sets it, false clears it"),
        ],
        kind: CommandKind::Library,
        run: tag,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "library.findFiles",
        label: "Find files by name (without regard to case), best first: in the browser's folders and the active project's folder, or by walking a folder; for missing files",
        params: &[
            param("names", ParamType::Texts, "file names, such as kick.wav; at most 4096"),
            optional(
                "folder",
                ParamType::Path(PathKind::Folder),
                "walk this folder instead (depth 32, at most 1 000 000 entries)",
            ),
        ],
        kind: CommandKind::Library,
        run: crate::relink::find_files,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "library.setPluginsView",
        label: "Choose which installed plug-ins the browser's Plug-ins category lists, kept for the next start: the instruments (the default) or the effects",
        params: &[param("view", ParamType::Text, "instruments (default) or effects")],
        kind: CommandKind::Library,
        run: set_plugins_view,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
];

/// The library commands, as `command.list` shows them.
pub fn library_commands() -> &'static [CommandSpec<Projects>] {
    LIBRARY_COMMANDS
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

fn folders(projects: &mut Projects, _: &Value) -> Result<Value, CommandError> {
    let library = projects.file_library_mut();
    library.poll();
    let list: Vec<Value> = library
        .folders()
        .into_iter()
        .map(|f| json!({ "folder": f.path.display().to_string(), "state": f.state.name(), "files": f.files, "note": f.note }))
        .collect();
    Ok(json!({ "folders": list }))
}

fn add_folder(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let folder = text(params, "folder")?;
    let added = projects.file_library_mut().add_folder(Path::new(folder)).map_err(CommandError)?;
    Ok(json!({ "folder": added.display().to_string() }))
}

fn remove_folder(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let folder = text(params, "folder")?;
    projects.file_library_mut().remove_folder(Path::new(folder)).map_err(CommandError)?;
    Ok(json!({ "removed": folder }))
}

fn refresh(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let folder = optional_text(params, "folder")?;
    projects.file_library_mut().refresh(folder.map(Path::new)).map_err(CommandError)?;
    Ok(json!({ "refreshing": folder.unwrap_or("every folder") }))
}

fn list(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let folder = text(params, "folder")?;
    let entries = projects.file_library_mut().list(Path::new(folder)).map_err(CommandError)?;
    let items: Vec<Value> = entries
        .into_iter()
        .map(|e| {
            json!({
                "item": e.path.display().to_string(),
                "name": e.name,
                "kind": e.kind.map_or("folder", FileKind::name),
                "size": e.size,
            })
        })
        .collect();
    Ok(json!({ "items": items }))
}

/// The browser's categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    All,
    Samples,
    Midi,
    Instruments,
    Effects,
    Presets,
    Plugins,
}

impl Category {
    pub const ALL: [Category; 7] =
        [Self::All, Self::Samples, Self::Midi, Self::Instruments, Self::Effects, Self::Presets, Self::Plugins];

    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "all" => Self::All,
            "samples" => Self::Samples,
            "midi" => Self::Midi,
            "instruments" => Self::Instruments,
            "effects" => Self::Effects,
            "presets" => Self::Presets,
            "plugins" | "plug-ins" => Self::Plugins,
            _ => return None,
        })
    }

    /// The name on its filter button.
    pub fn title(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Samples => "Samples",
            Self::Midi => "MIDI",
            Self::Instruments => "Instruments",
            Self::Effects => "Effects",
            Self::Presets => "Presets",
            Self::Plugins => "Plug-ins",
        }
    }

    /// The kinds of file it shows (none for the device categories).
    pub fn file_kinds(self) -> &'static [FileKind] {
        match self {
            Self::All => &[
                FileKind::Sample,
                FileKind::Stems,
                FileKind::Midi,
                FileKind::Preset,
                FileKind::Project,
                FileKind::Effect,
            ],
            Self::Samples => &[FileKind::Sample, FileKind::Stems],
            Self::Midi => &[FileKind::Midi],
            Self::Presets => &[FileKind::Preset],
            Self::Effects => &[FileKind::Effect],
            Self::Instruments | Self::Plugins => &[],
        }
    }
}

/// A search result.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub item: Item,
    pub name: String,
    /// `sample`, `midi`, `preset`, `project`, `instrument`, `effect`, or `audio_effect`.
    pub kind: &'static str,
    /// For a plug-in: its format and vendor.
    pub detail: Option<String>,
    pub plugin: bool,
    /// For a file: the library folder it is in.
    pub folder: Option<PathBuf>,
}

impl Found {
    pub fn to_json(&self, library: &mut Library) -> Value {
        let key = self.item.key();
        json!({
            "item": key,
            "name": self.name,
            "kind": self.kind,
            "plugin": self.plugin,
            "detail": self.detail,
            "folder": self.folder.as_ref().map(|f| f.display().to_string()),
            "tags": library.item_tags(&key),
        })
    }
}

fn file_kind_name(kind: FileKind) -> &'static str {
    if kind == FileKind::Effect { "audio_effect" } else { kind.name() }
}

/// What an item key is, for a search over pins or recent items.
fn describe(projects: &mut Projects, item: Item) -> Option<Found> {
    match &item {
        Item::File(path) | Item::Voice(path, _) => {
            let kind = item.file_kind()?;
            let mut name = path.file_name()?.to_string_lossy().into_owned();
            if let Item::Voice(_, voice) = &item {
                name = format!("{name} #{voice}");
            }
            Some(Found { name, kind: file_kind_name(kind), detail: None, plugin: false, folder: None, item })
        }
        Item::Device(id) => {
            let info = suhina_device_registry::info(id)?;
            let kind = role_name(info.role);
            Some(Found { name: info.name.to_string(), kind, detail: None, plugin: false, folder: None, item })
        }
        Item::Plugin(spec) => {
            let env = projects.plugin_env();
            let plugin = env.catalog.plugins().find(|p| p.spec() == *spec || p.name == *spec)?;
            let kind = if crate::is_instrument(plugin) { "instrument" } else { "effect" };
            let detail = Some(format!("{} {}", plugin.format(), plugin.vendor).trim().to_string());
            Some(Found { name: plugin.name.clone(), kind, detail, plugin: true, folder: None, item })
        }
    }
}

fn fits(found: &Found, category: Category) -> bool {
    match category {
        Category::All => true,
        Category::Samples => found.kind == "sample",
        Category::Midi => found.kind == "midi",
        Category::Presets => found.kind == "preset",
        Category::Instruments => found.kind == "instrument",
        Category::Effects => matches!(found.kind, "effect" | "audio_effect" | "midi_effect"),
        Category::Plugins => found.plugin,
    }
}

/// The keys a place that is a list holds, in its order: `pinned`, `recent:all` or
/// `recent:<group>`; `None` for every other place (a folder, or everything).
pub(crate) fn listed_keys(projects: &mut Projects, place: Option<&str>) -> Result<Option<Vec<String>>, CommandError> {
    Ok(match place {
        Some("pinned") => Some(projects.file_library_mut().pinned()),
        // Every group's recent items, each group in its own order, an item once (#711).
        Some("recent:all") => {
            let library = projects.file_library_mut();
            let mut keys: Vec<String> = Vec::new();
            for group in Group::ALL {
                for key in library.recent(group) {
                    if !keys.contains(&key) {
                        keys.push(key);
                    }
                }
            }
            Some(keys)
        }
        Some(p) if p.starts_with("recent:") => {
            let group = Group::parse(&p["recent:".len()..]).ok_or_else(|| format!("unknown recent group in {p:?}"))?;
            Some(projects.file_library_mut().recent(group))
        }
        _ => None,
    })
}

/// Searches as `library.search` does; the window's results come from here too.
pub fn search_items(
    projects: &mut Projects,
    text: &str,
    category: Category,
    place: Option<&str>,
    tags: &[String],
    limit: usize,
) -> Result<Vec<Found>, CommandError> {
    let limit = if limit == 0 { suhina_library::MAX_RESULTS } else { limit.min(suhina_library::MAX_RESULTS) };
    let words = suhina_library::words(text);
    let tagged =
        |library: &mut Library, key: &str| tags.is_empty() || library.item_tags(key).iter().any(|t| tags.contains(t));
    if let Some(keys) = listed_keys(projects, place)? {
        let mut out = Vec::new();
        for key in keys {
            let Some(item) = Item::parse(&key) else { continue };
            let Some(found) = describe(projects, item) else { continue };
            let path = match &found.item {
                Item::File(p) | Item::Voice(p, _) => p.display().to_string(),
                _ => String::new(),
            };
            if fits(&found, category)
                && suhina_library::rank(&words, &found.name, &path).is_some()
                && tagged(projects.file_library_mut(), &key)
            {
                out.push(found);
            }
            if out.len() >= limit {
                break;
            }
        }
        return Ok(out);
    }
    let within = place.map(PathBuf::from);
    let mut ranked: Vec<(u8, Found)> = Vec::new();
    // Built-in effects and instruments, and plug-ins, unless a folder is the place.
    if within.is_none() {
        for info in suhina_device_registry::DEVICES {
            let kind = role_name(info.role);
            let found = Found {
                item: Item::Device(info.id.to_string()),
                name: info.name.to_string(),
                kind,
                detail: None,
                plugin: false,
                folder: None,
            };
            if let Some(rank) = suhina_library::rank(&words, info.name, info.id)
                && fits(&found, category)
                && tagged(projects.file_library_mut(), &found.item.key())
            {
                ranked.push((rank, found));
            }
        }
        let env = projects.plugin_env();
        for plugin in env.catalog.plugins() {
            let kind = if crate::is_instrument(plugin) { "instrument" } else { "effect" };
            let found = Found {
                item: Item::Plugin(plugin.spec()),
                name: plugin.name.clone(),
                kind,
                detail: Some(format!("{} {}", plugin.format(), plugin.vendor).trim().to_string()),
                plugin: true,
                folder: None,
            };
            let haystack = format!("{} {}", plugin.vendor, plugin.format());
            if let Some(rank) = suhina_library::rank(&words, &plugin.name, &haystack)
                && fits(&found, category)
                && tagged(projects.file_library_mut(), &found.item.key())
            {
                ranked.push((rank, found));
            }
        }
    }
    let kinds = category.file_kinds();
    if !kinds.is_empty() {
        let query = Query { text: text.to_string(), kinds: kinds.to_vec(), within, tags: tags.to_vec(), limit };
        let library = projects.file_library_mut();
        library.poll();
        for file in library.search(&query) {
            let rank = suhina_library::rank(&words, file.name(), &file.rel).unwrap_or(3);
            ranked.push((
                rank,
                Found {
                    name: file.name().to_string(),
                    kind: file_kind_name(file.kind),
                    detail: None,
                    plugin: false,
                    folder: Some(file.folder.clone()),
                    item: Item::File(file.path),
                },
            ));
        }
    }
    ranked.sort_by(|a, b| {
        a.0.cmp(&b.0).then(a.1.name.len().cmp(&b.1.name.len())).then(suhina_library::natural_cmp(&a.1.name, &b.1.name))
    });
    ranked.truncate(limit);
    Ok(ranked.into_iter().map(|(_, f)| f).collect())
}

fn search(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let words = match params.get("text") {
        None | Some(Value::Null) => "",
        Some(v) => v.as_str().ok_or("text must be a text")?,
    };
    let category = match optional_text(params, "category")? {
        None => Category::All,
        Some(c) => {
            Category::parse(c).ok_or("category must be all, samples, midi, instruments, effects, presets or plugins")?
        }
    };
    let place = optional_text(params, "place")?;
    let tags: Vec<String> = match params.get("tags") {
        None | Some(Value::Null) => Vec::new(),
        Some(v) => v
            .as_array()
            .ok_or("tags must be a list of tag names")?
            .iter()
            .map(|t| t.as_str().map(str::to_string).ok_or("tags must be a list of tag names"))
            .collect::<Result<_, _>>()?,
    };
    let limit = match params.get("limit") {
        None | Some(Value::Null) => 0,
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| *n >= 1)
            .ok_or("limit must be a whole number from 1")?,
    };
    let found = search_items(projects, words, category, place, &tags, limit)?;
    let library = projects.file_library_mut();
    let items: Vec<Value> = found.iter().map(|f| f.to_json(library)).collect();
    Ok(json!({ "items": items }))
}

fn info(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let key = text(params, "item")?;
    let item = Item::parse(key).ok_or("item must be a path or an item key")?;
    let tags = projects.file_library_mut().item_tags(&item.key());
    let mut out = match &item {
        Item::Device(id) => {
            let info =
                suhina_device_registry::info(id).ok_or_else(|| format!("no built-in effect or instrument {id:?}"))?;
            json!({ "kind": role_name(info.role), "name": info.name })
        }
        Item::Plugin(spec) => {
            let env = projects.plugin_env();
            let plugin = env
                .catalog
                .plugins()
                .find(|p| p.spec() == *spec || p.name == *spec)
                .ok_or_else(|| format!("no plug-in {spec:?} in the catalog"))?;
            json!({
                "kind": if crate::is_instrument(plugin) { "instrument" } else { "effect" },
                "name": plugin.name, "vendor": plugin.vendor, "format": plugin.format(), "version": plugin.version,
            })
        }
        Item::File(path) | Item::Voice(path, _) => file_info(path, &item)?,
    };
    out["item"] = json!(item.key());
    out["tags"] = json!(tags);
    Ok(out)
}

/// Samples longer than this are not analysed for their tempo.
const DETECT_SECONDS: f64 = 60.0;

/// What `library.info` says about a file; the window's preview strip shows the same.
pub fn file_info(path: &Path, item: &Item) -> Result<Value, CommandError> {
    let kind = item.file_kind().ok_or_else(|| format!("{}: not a file the browser lists", path.display()))?;
    let size = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?.len();
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut out = json!({ "kind": file_kind_name(kind), "name": name, "size": size });
    match kind {
        FileKind::Sample | FileKind::Stems => {
            let info = suhina_media::info::sample_info(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let mut tempo = info.tempo;
            if tempo.is_none() && info.duration_seconds.is_some_and(|d| d <= DETECT_SECONDS) {
                tempo = suhina_media::decode_file(path).ok().and_then(|data| {
                    suhina_media::tempo::detect(&data)
                        .map(|bpm| suhina_media::info::Tempo { bpm, source: suhina_media::info::TempoSource::Detected })
                });
            }
            out["seconds"] = json!(info.duration_seconds);
            out["sample_rate"] = json!(info.sample_rate);
            out["channels"] = json!(info.channels);
            out["bpm"] = json!(tempo.map(|t| t.bpm));
            out["bpm_from"] = json!(tempo.map(|t| t.source.name()));
            out["key"] = json!(info.key);
            if kind == FileKind::Stems {
                let stems = suhina_media::stems::read_info(path).map_err(|e| format!("{}: {e}", path.display()))?;
                out["stems"] = json!(stems
                    .stems
                    .iter()
                    .map(|s| json!({ "name": s.name, "color": s.color.map(|[r, g, b]| format!("#{r:02x}{g:02x}{b:02x}")) }))
                    .collect::<Vec<_>>());
                out["mastering"] = json!(
                    suhina_media::stems::mastering_devices(&stems)
                        .iter()
                        .map(|d| json!({ "device": d.id, "enabled": d.enabled }))
                        .collect::<Vec<_>>()
                );
            }
        }
        FileKind::Midi => {
            let midi = suhina_media::midi::read_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let tracks: Vec<Value> = midi
                .tracks
                .iter()
                .map(|t| json!({ "name": t.name, "notes": t.notes.len(), "beats": t.end() }))
                .collect();
            out["bpm"] = json!(midi.tempo_bpm);
            out["tracks"] = json!(tracks);
        }
        FileKind::Preset => {
            let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let voices =
                suhina_device_registry::preset_voices(&data).map_err(|e| format!("{}: {e}", path.display()))?;
            out["device"] = json!("fm-synth");
            out["voices"] =
                json!(voices.iter().map(|(n, name)| json!({ "voice": n, "name": name })).collect::<Vec<_>>());
            if let Item::Voice(_, voice) = item {
                out["voice"] = json!(voice);
            }
        }
        FileKind::Project => {}
        FileKind::Effect => {
            let graph =
                suhina_device_sdk::DeviceGraph::read_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
            out["id"] = json!(graph.id);
            out["role"] = json!("audio_effect");
        }
    }
    Ok(out)
}

fn pinned(projects: &mut Projects, _: &Value) -> Result<Value, CommandError> {
    Ok(json!({ "items": projects.file_library_mut().pinned() }))
}

fn pin(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let key = text(params, "item")?;
    let item = Item::parse(key).ok_or("item must be a path or an item key")?;
    let pinned = projects.file_library_mut().pin(&item.key()).map_err(CommandError)?;
    Ok(json!({ "item": item.key(), "pinned": true, "changed": pinned }))
}

fn set_plugins_view(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let name = text(params, "view")?;
    let view = suhina_library::store::PluginsView::parse(name)
        .ok_or_else(|| CommandError("view must be instruments or effects".into()))?;
    projects.file_library_mut().set_plugins_view(view).map_err(CommandError)?;
    Ok(json!({ "view": view.name() }))
}

fn unpin(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let key = text(params, "item")?;
    let item = Item::parse(key).ok_or("item must be a path or an item key")?;
    let changed = projects.file_library_mut().unpin(&item.key()).map_err(CommandError)?;
    Ok(json!({ "item": item.key(), "pinned": false, "changed": changed }))
}

fn recent(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let library = projects.file_library_mut();
    match optional_text(params, "group")? {
        Some(name) => {
            let group = Group::parse(name)
                .ok_or("group must be projects, samples, midi, instruments, effects, plugins or presets")?;
            Ok(json!({ group.name(): library.recent(group) }))
        }
        None => {
            let mut out = serde_json::Map::new();
            for group in Group::ALL {
                out.insert(group.name().to_string(), json!(library.recent(group)));
            }
            Ok(Value::Object(out))
        }
    }
}

fn tags(projects: &mut Projects, _: &Value) -> Result<Value, CommandError> {
    let library = projects.file_library_mut();
    let list: Vec<Value> = library.tags().into_iter().map(|t| json!({ "name": t.name, "color": t.color })).collect();
    let tagged: serde_json::Map<String, Value> = library.tagged().into_iter().map(|(k, v)| (k, json!(v))).collect();
    Ok(json!({ "tags": list, "limit": library.tag_limit(), "tagged": tagged }))
}

fn set_tag(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let library = projects.file_library_mut();
    let mut done = false;
    if let Some(limit) = params.get("limit").filter(|v| !v.is_null()) {
        let limit = limit.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or("limit must be a whole number")?;
        library.set_tag_limit(limit).map_err(CommandError)?;
        done = true;
    }
    if let Some(name) = optional_text(params, "name")? {
        library
            .set_tag(name, optional_text(params, "color")?, optional_text(params, "rename")?)
            .map_err(CommandError)?;
        done = true;
    }
    if !done {
        return Err("give a tag's name (with a color or a new name), or a limit".into());
    }
    tags(projects, &Value::Null)
}

fn remove_tag(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let name = text(params, "name")?;
    projects.file_library_mut().remove_tag(name).map_err(CommandError)?;
    Ok(json!({ "removed": name }))
}

fn tag(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let key = text(params, "item")?;
    let item = Item::parse(key).ok_or("item must be a path or an item key")?;
    let name = text(params, "tag")?;
    let on = params.get("on").and_then(Value::as_bool).ok_or("on must be true or false")?;
    let library = projects.file_library_mut();
    library.tag(&item.key(), name, on).map_err(CommandError)?;
    Ok(json!({ "item": item.key(), "tags": library.item_tags(&item.key()) }))
}

/// Records what a successful command added to a project among the recent items
/// (`docs/spec/browser.md` › Sidebar).
pub(crate) fn record_use(library: &mut Library, id: &str, params: &Value, result: &Value) {
    let mut record = |group: Group, item: String| {
        let _ = library.record(group, &item);
    };
    let device = |record: &mut dyn FnMut(Group, String), id: &str| {
        if let Some(info) = suhina_device_registry::info(id) {
            let group = if info.role == Role::Instrument { Group::Instruments } else { Group::Effects };
            record(group, format!("device:{id}"));
        }
    };
    let preset = |record: &mut dyn FnMut(Group, String), params: &Value| {
        if let Some(path) = params.get("preset").and_then(Value::as_str) {
            let path = crate::files::absolute(Path::new(path)).display().to_string();
            let voice = params.get("voice").and_then(Value::as_u64).unwrap_or(1);
            record(Group::Presets, if voice > 1 { format!("{path}#{voice}") } else { path });
        }
    };
    match id {
        "clip.addFiles" => {
            let skipped: Vec<&str> = result["skipped"]
                .as_array()
                .map(|s| s.iter().filter_map(|x| x["path"].as_str()).collect())
                .unwrap_or_default();
            for path in params["paths"].as_array().into_iter().flatten().filter_map(Value::as_str).rev() {
                if skipped.contains(&path) {
                    continue;
                }
                let absolute = crate::files::absolute(Path::new(path));
                let group = match suhina_media::info::kind_of(&absolute) {
                    Some(FileKind::Sample | FileKind::Stems) => Group::Samples,
                    Some(FileKind::Midi) => Group::Midi,
                    _ => continue,
                };
                record(group, absolute.display().to_string());
            }
        }
        "device.add" | "track.add" => {
            if let Some(path) = params.get("sd").and_then(Value::as_str) {
                record(Group::Effects, path.to_string());
            } else if params.get("preset").is_some_and(|p| !p.is_null()) {
                preset(&mut record, params);
            } else if let Some(id) = params.get("id").or_else(|| params.get("device")).and_then(Value::as_str) {
                if suhina_device_registry::info(id).is_some() {
                    device(&mut record, id);
                } else if id == params.get("device").and_then(Value::as_str).unwrap_or("") {
                    record(Group::Plugins, format!("plugin:{id}"));
                }
            }
        }
        "device.addSd" => {
            if let Some(path) = params.get("path").and_then(Value::as_str) {
                record(Group::Effects, path.to_string());
            }
        }
        "device.addPlugin" => {
            if let Some(name) = params.get("plugin").and_then(Value::as_str) {
                record(Group::Plugins, format!("plugin:{name}"));
            }
        }
        _ => {}
    }
}
