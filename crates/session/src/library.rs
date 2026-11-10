// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Commands for the browser's library (`docs/spec/browser.md` › Commands, ADR 0012): the
//! folders, searching them with the built-in effects and instruments, and installed plug-ins, what a file is,
//! pins, recent items and colour tags. They belong to Suhina, not to one project, so they run
//! on [`Projects`] like the `plugin.*` commands, and never change a project.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use serde_json::{Value, json};
use suhina_device_api::Role;
use suhina_library::{FileKind, FileSearch, Group, Item, Library, Query};

use crate::commands::{Args, CommandKind, CommandSpec, Param, ParamType, optional, param};
use crate::devices::role_name;
use crate::{Agents, CommandError, PathKind, Policy, Projects, Session};

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
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Read),
        run: folders,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "library.addFolder",
        label: "Add a folder to the browser and index the samples, MIDI files, presets and projects in it",
        params: &[FOLDER],
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Change),
        run: add_folder,
        policy: Policy { agents: Agents::Denied("the user chooses the browser's folders"), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "library.removeFolder",
        label: "Remove a folder from the browser (the files stay where they are)",
        params: &[FOLDER],
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Change),
        run: remove_folder,
        policy: Policy { agents: Agents::Denied("the user chooses the browser's folders"), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "library.refresh",
        label: "Read a folder, or every folder, again",
        params: &[optional("folder", ParamType::Text, "a folder (default: every folder)")],
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Change),
        run: refresh,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "library.list",
        label: "List what is directly in a folder: sub-folders first, then samples, MIDI files, presets and projects",
        params: &[param("folder", ParamType::Path(PathKind::Folder), "any folder")],
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Read),
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
                "pinned, recent:all, recent:<group> (projects, samples, midi, instruments, effects, plugins, presets), tag:<name> or tag:#<id> (the items carrying that tag; several joined by id, tag:#<id>+#<id>…), or a folder",
            ),
            optional("tags", ParamType::Texts, "only items carrying any of these tags"),
            optional("limit", ParamType::Integer, "at most this many (default and most 500)"),
        ],
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Read),
        run: search,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "library.info",
        label: "What an item is: a sample's length, rate, channels, tempo (and whether it was detected) and key; a MIDI file's tracks; a preset bank's voices; a device's or plug-in's role",
        params: &[ITEM],
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Read),
        run: info,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "library.pinned",
        label: "List the pinned items, oldest first",
        params: &[],
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Read),
        run: pinned,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "library.pin",
        label: "Pin an item, so it is listed under Pinned",
        params: &[ITEM],
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Change),
        run: pin,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "library.unpin",
        label: "Unpin an item",
        params: &[ITEM],
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Change),
        run: unpin,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "library.recent",
        label: "List the items used lately, newest first, by group (projects, samples, midi, instruments, effects, plugins, presets)",
        params: &[optional("group", ParamType::Text, "one group (default: every group)")],
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Read),
        run: recent,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "library.tags",
        label: "List the colour tags (name, colour), the items carrying them and the most tags there may be",
        params: &[],
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Read),
        run: tags,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "library.setTag",
        label: "Add a colour tag, or change a tag's colour or name; or change the most tags there may be",
        params: &[
            optional(
                "id",
                ParamType::Text,
                "the tag to change, by its id (#<id>) or name; a name no tag has creates one, coloured from the named colours ([#1254])",
            ),
            optional("name", ParamType::Text, "the tag (the id's form names it too)"),
            optional(
                "color",
                ParamType::Text,
                "its colour, #RRGGBB (new tags take a named colour when it is left out)",
            ),
            optional("rename", ParamType::Text, "a new name for it"),
            optional(
                "folder",
                ParamType::Text,
                "the tag folder a made tag lands in, by its id (#<id>) or name; the top when it is left out ([#1254] review)",
            ),
            optional("limit", ParamType::Integer, "the most tags there may be, from 1 to 1024 (default 64)"),
        ],
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Change),
        run: set_tag,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "library.removeTag",
        label: "Remove a colour tag from the list and from every item",
        params: &[
            optional("id", ParamType::Text, "the tag, by its id (#<id>) or its name ([#1254])"),
            optional("name", ParamType::Text, "the tag"),
        ],
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Change),
        run: remove_tag,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "library.tag",
        label: "Set or clear a colour tag on an item",
        params: &[
            ITEM,
            param("tag", ParamType::Text, "the tag, by its id (#<id>) or its name ([#1254])"),
            param("on", ParamType::Bool, "true sets it, false clears it"),
        ],
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Change),
        run: tag,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "library.addTagFolder",
        label: "Add a folder of tags, unique among its own level's names and named when the name is left out, under a folder or at the top",
        params: &[
            optional("name", ParamType::Text, "its name (the next free \"Folder\" when left out)"),
            optional(
                "parent",
                ParamType::Text,
                "the folder it goes under, by id (#<id>) or name, at the top when left out",
            ),
        ],
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Change),
        run: add_tag_folder,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "library.renameTagFolder",
        label: "Rename a tag folder, unique among the folders of its own level",
        params: &[
            param("folder", ParamType::Text, "the folder, by id (#<id>) or name"),
            param("rename", ParamType::Text, "its new name"),
        ],
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Change),
        run: rename_tag_folder,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "library.removeTagFolder",
        label: "Remove a tag folder: its tags and folders move up to its parent, and the count of them comes with the answer",
        params: &[param("folder", ParamType::Text, "the folder, by id (#<id>) or name")],
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Change),
        run: remove_tag_folder,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "library.setFolderOpen",
        label: "Set whether a tag folder of the browser shows open, remembered between runs",
        params: &[
            param("folder", ParamType::Text, "the folder, by id (#<id>) or name"),
            param("open", ParamType::Bool, "true opens it, false closes it"),
        ],
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Change),
        run: set_folder_open,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "library.moveTag",
        label: "Move a tag or a tag folder: to a folder (or to the top level), at a position of that level",
        params: &[
            param("id", ParamType::Text, "the tag or folder, by its id (#<id>) or name"),
            optional("folder", ParamType::Text, "the folder it goes under (the top level when left out)"),
            optional("position", ParamType::Integer, "at this position of the level (the end when left out)"),
        ],
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Change),
        run: move_tag,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "library.setSections",
        label: "Set the browser's sidebar sections in an order; whether each shows open is kept, and Folders stays last whatever is asked",
        params: &[optional("order", ParamType::Texts, "pinned, tags, library, in the order their titles show")],
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Change),
        run: set_sections,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "library.setSectionOpen",
        label: "Set whether one of the browser's sidebar sections shows open",
        params: &[
            param("section", ParamType::Text, "pinned, tags or library"),
            param("open", ParamType::Bool, "true opens it, false collapses it"),
        ],
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Change),
        run: set_section_open,
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
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Read),
        run: crate::relink::find_files,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "library.moveTo",
        label: "Move files into another folder: a name taken there gets a number (kick 2.wav), nothing is overwritten; the files' pins, recent places and tags go with them and the browser's index follows; a file an open project plays stays where it is",
        params: &[
            param(
                "items",
                ParamType::Path(PathKind::Files),
                "the files' paths, as library.search gives them; at most 4096",
            ),
            param("folder", ParamType::Path(PathKind::WriteFolder), "a folder that exists, to move them into"),
        ],
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Change),
        run: move_to,
        // Moves files on disk: kept from agents, as removing a file from where it was is not
        // something `confine` can undo.
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
    CommandSpec {
        id: "library.copyTo",
        label: "Copy files into a folder, leaving the originals where they are: a name taken there gets a number (kick 2.wav), nothing is overwritten; files that are gone are listed as missing",
        params: &[
            param(
                "items",
                ParamType::Path(PathKind::Files),
                "the files' paths, as library.search gives them; at most 4096",
            ),
            param("folder", ParamType::Path(PathKind::WriteFolder), "a folder that exists, to copy them into"),
        ],
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Change),
        run: copy_to,
        // Copies files on disk: open to agents, below the roots `confine` keeps them in.
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "library.setPluginsView",
        label: "Choose which installed plug-ins the browser's Plug-ins category lists, kept for the next start: the instruments (the default) or the effects",
        params: &[param("view", ParamType::Text, "instruments (default) or effects")],
        kind: CommandKind::on(crate::commands::Target::Library, crate::commands::Effect::Change),
        run: set_plugins_view,
        policy: Policy { agents: Agents::NotWhenAttached(Agents::SETTINGS), ..Policy::DEFAULT },
    },
];

/// The library commands, as `command.list` shows them.
pub fn library_commands() -> &'static [CommandSpec<Projects>] {
    LIBRARY_COMMANDS
}

fn optional_text<'a>(params: &'a Value, name: &str) -> Result<Option<&'a str>, CommandError> {
    match Args::new(params).value(name) {
        None => Ok(None),
        Some(_) => Args::new(params).text(name).map(Some),
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
    let folder = Args::new(params).text("folder")?;
    let added = projects.file_library_mut().add_folder(Path::new(folder)).map_err(CommandError)?;
    Ok(json!({ "folder": added.display().to_string() }))
}

fn remove_folder(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let folder = Args::new(params).text("folder")?;
    projects.file_library_mut().remove_folder(Path::new(folder)).map_err(CommandError)?;
    Ok(json!({ "removed": folder }))
}

fn refresh(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let folder = optional_text(params, "folder")?;
    projects.file_library_mut().refresh(folder.map(Path::new)).map_err(CommandError)?;
    Ok(json!({ "refreshing": folder.unwrap_or("every folder") }))
}

fn list(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let folder = Args::new(params).text("folder")?;
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
    /// `sample`, `midi`, `preset`, `project`, `instrument`, `effect`, `audio_effect`, or `folder`
    /// (a pinned or tagged folder of the library, [#1254] review).
    pub kind: &'static str,
    /// For a plug-in: its format and vendor.
    pub detail: Option<String>,
    pub plugin: bool,
    /// For a plug-in: its format, which a list shows beside its name and filters by.
    pub format: Option<suhina_plugins::scan::PluginFormat>,
    /// For a file: the library folder it is in.
    pub folder: Option<PathBuf>,
}

impl Found {
    pub fn to_json(&self, library: &mut Library) -> Value {
        let key = self.item.key();
        // The tags the item carries (by id, [#1254]), named for a script's reading.
        let item_tags: Vec<Value> = library
            .item_tags(&key)
            .iter()
            .filter_map(|id| library.tag_name_of(*id).map(|name| json!({ "id": id, "name": name })))
            .collect();
        json!({
            "item": key,
            "name": self.name,
            "kind": self.kind,
            "plugin": self.plugin,
            "format": self.format.map(|f| f.name()),
            "detail": self.detail,
            "folder": self.folder.as_ref().map(|f| f.display().to_string()),
            "tags": item_tags,
        })
    }
}

fn file_kind_name(kind: FileKind) -> &'static str {
    if kind == FileKind::Effect { "audio_effect" } else { kind.name() }
}

/// What an item key is, for a search over pins or recent items.
fn describe(projects: &mut Projects, item: Item) -> Option<Found> {
    // A folder of the library, a top one or a sub-folder its index holds: a pinned or tagged
    // folder lists as a folder ([#1254] review: a tagged folder was counted on its tag's row
    // and left out of the tag's place).
    if let Item::File(path) = &item
        && let Some(root) = projects.file_library_mut().folder_root_of(path)
    {
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        return Some(Found {
            name,
            kind: "folder",
            detail: None,
            plugin: false,
            format: None,
            folder: Some(root),
            item,
        });
    }
    match &item {
        Item::File(path) | Item::Voice(path, _) => {
            let kind = item.file_kind()?;
            let mut name = path.file_name()?.to_string_lossy().into_owned();
            if let Item::Voice(_, voice) = &item {
                name = format!("{name} #{voice}");
            }
            Some(Found {
                name,
                kind: file_kind_name(kind),
                detail: None,
                plugin: false,
                format: None,
                folder: None,
                item,
            })
        }
        Item::Device(id) => {
            let info = suhina_device_registry::info(id)?;
            let kind = role_name(info.role);
            Some(Found {
                name: info.name.to_string(),
                kind,
                detail: None,
                plugin: false,
                format: None,
                folder: None,
                item,
            })
        }
        Item::Plugin(spec) => {
            let env = projects.plugin_env();
            let plugin = env.catalog.plugins().find(|p| p.spec() == *spec || p.name == *spec)?;
            let kind = if crate::is_instrument(plugin) { "instrument" } else { "effect" };
            let detail = Some(format!("{} {}", plugin.format(), plugin.vendor).trim().to_string());
            let format = Some(plugin.plugin_format());
            Some(Found { name: plugin.name.clone(), kind, detail, plugin: true, format, folder: None, item })
        }
        Item::Stock(key) => {
            let name = suhina_device_registry::stock_preset(key)?.preset.name.clone();
            Some(Found { name, kind: "preset", detail: None, plugin: false, format: None, folder: None, item })
        }
    }
}

fn fits(found: &Found, category: Category) -> bool {
    match category {
        Category::All => true,
        Category::Samples => matches!(found.kind, "sample" | "stems"),
        Category::Midi => found.kind == "midi",
        Category::Presets => found.kind == "preset",
        Category::Instruments => found.kind == "instrument",
        Category::Effects => matches!(found.kind, "effect" | "audio_effect" | "midi_effect"),
        Category::Plugins => found.plugin,
    }
}

/// The keys a place that is a list holds, in its order: `pinned`, `recent:all`, `recent:<group>`
/// or `tag:<name>`; `None` for every other place (a folder, or everything).
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
        Some(p) if p.starts_with("tag:") => {
            // A tag by its name, or by `tag:#<id>`, so a script keeps working after a rename
            // ([#1254](https://github.com/suhina-daw/suhina-daw/issues/1254)). A joined
            // `tag:#<id>+#<id>…` is a place of several: the ids only, because no two names
            // join into one param unambiguously, and every part names its tag — a part that
            // is not one of the library's ids refuses rather than quietly searching the rest
            // ([#1254] review). The joined shape is the selectors' alone (`#` over digits),
            // which no name may spell, so a name with `+` and `#` in it (`#notes+#rock`)
            // reads as the name it is ([#1254] review).
            let given = &p["tag:".len()..];
            let joined = given.contains('+') && suhina_library::store::name_is_id_form(given);
            let ids = if joined {
                let mut ids: Vec<u32> = Vec::new();
                for part in given.split('+') {
                    let id = suhina_library::store::id_of_selector(part)
                        .ok_or_else(|| format!("there is no tag {part:?}; library.tags lists them"))?;
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
                ids
            } else {
                vec![
                    resolve_tag_param(projects, given)
                        .ok_or_else(|| format!("there is no tag {given:?}; library.tags lists them"))?,
                ]
            };
            if let Some(id) = ids.iter().copied().find(|id| {
                let library = projects.file_library_mut();
                suhina_library::store::tag_by_id(&library.tags(), *id).is_none()
            }) {
                return Err(format!("there is no tag #{id}; library.tags lists them").into());
            }
            let library = projects.file_library_mut();
            // The union in the first tag's order, then each further tag's own items as its
            // own passes them ([#1254] review: a place of several lists each). The set keeps
            // the looks cheap: a hundred thousand claimed keys is no cause for a
            // nested-loop's parade ([#1254] review).
            let mut keys: Vec<String> = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for id in ids {
                for key in library.items_tagged(id) {
                    if seen.insert(key.clone()) {
                        keys.push(key);
                    }
                }
            }
            Some(keys)
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
    tags: &[u32],
    limit: usize,
) -> Result<Vec<Found>, CommandError> {
    Ok(prepare_search(projects, text, category, place, tags, limit)?.run(&AtomicBool::new(false)))
}

/// A search with everything that needs the library looked up, and its files still to look
/// through, which [`PreparedSearch::run`] does on any thread.
pub struct PreparedSearch {
    /// A place that lists its items itself (pinned, recent): the answer, in its order.
    listed: Option<Vec<Found>>,
    ranked: Vec<(u8, Found)>,
    files: Option<FileSearch>,
    words: Vec<String>,
    limit: usize,
}

impl PreparedSearch {
    /// How many files it looks through.
    pub fn files(&self) -> usize {
        self.files.as_ref().map_or(0, FileSearch::files)
    }

    /// What was found, best first; nothing from the files once `cancel` is set.
    pub fn run(self, cancel: &AtomicBool) -> Vec<Found> {
        if let Some(listed) = self.listed {
            return listed;
        }
        let mut ranked = self.ranked;
        for file in self.files.iter().flat_map(|files| files.run(cancel)) {
            let rank = suhina_library::rank(&self.words, file.name(), &file.rel).unwrap_or(3);
            ranked.push((
                rank,
                Found {
                    name: file.name().to_string(),
                    kind: file_kind_name(file.kind),
                    detail: None,
                    plugin: false,
                    format: None,
                    folder: Some(file.folder.clone()),
                    item: Item::File(file.path),
                },
            ));
        }
        ranked.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then(a.1.name.len().cmp(&b.1.name.len()))
                .then(suhina_library::natural_cmp(&a.1.name, &b.1.name))
        });
        ranked.truncate(self.limit);
        ranked.into_iter().map(|(_, f)| f).collect()
    }
}

/// [`search_items`] up to where it looks through the files.
pub fn prepare_search(
    projects: &mut Projects,
    text: &str,
    category: Category,
    place: Option<&str>,
    tags: &[u32],
    limit: usize,
) -> Result<PreparedSearch, CommandError> {
    let limit = if limit == 0 { suhina_library::MAX_RESULTS } else { limit.min(suhina_library::MAX_RESULTS) };
    let words = suhina_library::words(text);
    let tagged =
        |library: &mut Library, key: &str| tags.is_empty() || library.item_tags(key).iter().any(|t| tags.contains(t));
    // A tag's place lists its items named first, whatever kind they are; a pin's place keeps
    // the pins' order, a recent group the recency.
    let sorted_by_name = place.is_some_and(|p| p.starts_with("tag:"));
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
            // A place that does not sort before the cut can stop at it: the order is the
            // keys' own, so the first `limit` that fit are the answer ([#1250] review). A
            // tag's place sorts first, and a cut before the sort would rank `/a/Zebra`
            // over `/z/Apple` (#1196).
            if !sorted_by_name && out.len() >= limit {
                break;
            }
        }
        // A place that sorts by name sorts before the limit: a cut in key order would
        // rank `/a/Zebra` over `/z/Apple` (#1196).
        if sorted_by_name {
            out.sort_by(|a, b| suhina_library::natural_cmp(&a.name, &b.name));
        }
        out.truncate(limit);
        return Ok(PreparedSearch { listed: Some(out), ranked: Vec::new(), files: None, words, limit });
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
                format: None,
                folder: None,
            };
            if let Some(rank) = suhina_library::rank(&words, &info.name, &info.id)
                && fits(&found, category)
                && tagged(projects.file_library_mut(), &found.item.key())
            {
                ranked.push((rank, found));
            }
        }
        for stock in suhina_device_registry::stock_presets() {
            let found = Found {
                item: Item::Stock(stock.key.to_string()),
                name: stock.preset.name.clone(),
                kind: "preset",
                detail: None,
                plugin: false,
                format: None,
                folder: None,
            };
            if let Some(rank) = suhina_library::rank(&words, &stock.preset.name, &stock.tags.join(" "))
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
                format: Some(plugin.plugin_format()),
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
    let files = (!kinds.is_empty()).then(|| {
        let query = Query { text: text.to_string(), kinds: kinds.to_vec(), within, tags: tags.to_vec(), limit };
        let library = projects.file_library_mut();
        library.poll();
        library.prepare_search(&query)
    });
    Ok(PreparedSearch { listed: None, ranked, files, words, limit })
}

fn search(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let words = match Args::new(params).value("text") {
        None => "",
        Some(v) => v.as_str().ok_or("text must be a text")?,
    };
    let category = match optional_text(params, "category")? {
        None => Category::All,
        Some(c) => {
            Category::parse(c).ok_or("category must be all, samples, midi, instruments, effects, presets or plugins")?
        }
    };
    let place = optional_text(params, "place")?;
    let tags: Vec<u32> = match Args::new(params).value("tags") {
        None => Vec::new(),
        Some(v) => v
            .as_array()
            .ok_or("tags must be a list of tag names or ids")?
            .iter()
            .map(|t| match t {
                // A tag by its name, or by `#<id>` ([#1254]).
                Value::String(given) => resolve_tag_param(projects, given)
                    .ok_or_else(|| format!("there is no tag {given:?}; library.tags lists them")),
                Value::Number(n) => {
                    resolve_tag_param(projects, &format!("#{}", n)).ok_or_else(|| format!("there is no tag {};", n))
                }
                _ => Err("tags must be a list of tag names or ids".into()),
            })
            .collect::<Result<_, _>>()?,
    };
    let limit = match Args::new(params).value("limit") {
        None => 0,
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
    let key = Args::new(params).text("item")?;
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
        Item::Stock(key) => {
            let stock = suhina_device_registry::stock_preset(key).ok_or_else(|| format!("no stock preset {key:?}"))?;
            json!({ "kind": "preset", "name": stock.preset.name, "device": stock.preset.device, "preset_tags": stock.tags })
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
            let preset = suhina_device_registry::preset(&data, 1).map_err(|e| format!("{}: {e}", path.display()))?;
            out["device"] = json!(preset.device);
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
    let key = Args::new(params).text("item")?;
    let item = Item::parse(key).ok_or("item must be a path or an item key")?;
    let pinned = projects.file_library_mut().pin(&item.key()).map_err(CommandError)?;
    Ok(json!({ "item": item.key(), "pinned": true, "changed": pinned }))
}

/// The most files one `library.copyTo` copies.
const MAX_COPIES: usize = 4096;
/// The most numbered names tried for one file before it is given up.
const MAX_COPY_NAMES: u32 = 9999;

/// `library.copyTo` (#865): each file into `folder` under its own name, or the first numbered
/// one free there. A copy is made with a file that must not exist yet, so nothing in the folder
/// is ever overwritten, even by a file that appeared meanwhile.
fn copy_to(_projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let folder = PathBuf::from(Args::new(params).text("folder")?);
    if !folder.is_dir() {
        return Err(format!("{} is not a folder that exists", folder.display()).into());
    }
    let items =
        Args::new(params).value("items").and_then(Value::as_array).ok_or("items must be a list of file paths")?;
    if items.is_empty() || items.len() > MAX_COPIES {
        return Err(format!("items must list from 1 to {MAX_COPIES} files").into());
    }
    let (mut copied, mut missing, mut failed) = (Vec::new(), Vec::new(), Vec::new());
    for item in items {
        let from = PathBuf::from(item.as_str().ok_or("items must be a list of file paths")?);
        if !from.is_file() {
            missing.push(from.display().to_string());
            continue;
        }
        match copy_into(&from, &folder) {
            Ok(to) => copied.push(json!({ "from": from.display().to_string(), "to": to.display().to_string() })),
            Err(e) => failed.push(json!({ "file": from.display().to_string(), "error": e })),
        }
    }
    Ok(json!({ "folder": folder.display().to_string(), "copied": copied, "missing": missing, "failed": failed }))
}

/// `library.moveTo` (#943): each file into `folder` under its own name, or the first numbered
/// one free there, as `library.copyTo` names them. A file already in that folder, one gone, or one
/// an open project plays is left where it is and listed.
fn move_to(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let folder = PathBuf::from(Args::new(params).text("folder")?);
    if !folder.is_dir() {
        return Err(format!("{} is not a folder that exists", folder.display()).into());
    }
    let items =
        Args::new(params).value("items").and_then(Value::as_array).ok_or("items must be a list of file paths")?;
    if items.is_empty() || items.len() > MAX_COPIES {
        return Err(format!("items must list from 1 to {MAX_COPIES} files").into());
    }
    let in_use: Vec<PathBuf> =
        projects.ids().into_iter().filter_map(|id| projects.get(id)).flat_map(Session::loaded_audio_paths).collect();
    let (mut moved, mut missing, mut kept, mut failed) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for item in items {
        let from = PathBuf::from(item.as_str().ok_or("items must be a list of file paths")?);
        if !from.is_file() {
            missing.push(from.display().to_string());
            continue;
        }
        let here = from.parent().is_some_and(|p| same_place(p, &folder));
        if here || in_use.iter().any(|p| same_place(p, &from)) {
            kept.push(from.display().to_string());
            continue;
        }
        match move_into(&from, &folder) {
            Ok(to) => {
                projects.file_library_mut().relocate(&from, &to).map_err(CommandError)?;
                moved.push(json!({ "from": from.display().to_string(), "to": to.display().to_string() }));
            }
            Err(e) => failed.push(json!({ "file": from.display().to_string(), "error": e })),
        }
    }
    Ok(json!({
        "folder": folder.display().to_string(),
        "moved": moved,
        "missing": missing,
        "kept": kept,
        "failed": failed,
    }))
}

/// Whether two paths name the same file or folder, as the system resolves them.
fn same_place(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Moves `from` into `folder` under the first of its name, `name 2`, … not taken: renamed when
/// both are on one drive, else copied and the original removed once the copy is whole.
fn move_into(from: &Path, folder: &Path) -> Result<PathBuf, String> {
    let stem = from.file_stem().map(|s| s.to_string_lossy().into_owned()).ok_or("the file has no name")?;
    let extension = from.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let free = (1..=MAX_COPY_NAMES)
        .map(|n| if n == 1 { format!("{stem}{extension}") } else { format!("{stem} {n}{extension}") })
        .map(|name| folder.join(name))
        .find(|to| !to.exists())
        .ok_or_else(|| format!("no free name for {stem}{extension} in {}", folder.display()))?;
    if std::fs::rename(from, &free).is_ok() {
        return Ok(free);
    }
    let to = copy_into(from, folder)?;
    if let Err(e) = std::fs::remove_file(from) {
        let _ = std::fs::remove_file(&to);
        return Err(e.to_string());
    }
    Ok(to)
}

/// Copies `from` into `folder` under the first of its name, `name 2`, `name 3`, … not taken.
fn copy_into(from: &Path, folder: &Path) -> Result<PathBuf, String> {
    let stem = from.file_stem().map(|s| s.to_string_lossy().into_owned()).ok_or("the file has no name")?;
    let extension = from.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let mut source = std::fs::File::open(from).map_err(|e| e.to_string())?;
    for n in 1..=MAX_COPY_NAMES {
        let name = if n == 1 { format!("{stem}{extension}") } else { format!("{stem} {n}{extension}") };
        let to = folder.join(name);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&to) {
            Ok(mut target) => {
                if let Err(e) = std::io::copy(&mut source, &mut target) {
                    drop(target);
                    let _ = std::fs::remove_file(&to);
                    return Err(e.to_string());
                }
                return Ok(to);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err(format!("no free name for {stem}{extension} in {}", folder.display()))
}

fn set_plugins_view(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let name = Args::new(params).text("view")?;
    let view = suhina_library::store::PluginsView::parse(name)
        .ok_or_else(|| CommandError("view must be instruments or effects".into()))?;
    projects.file_library_mut().set_plugins_view(view).map_err(CommandError)?;
    Ok(json!({ "view": view.name() }))
}

fn unpin(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let key = Args::new(params).text("item")?;
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
    // The tags and folders, each with its id, in the order the section shows ([#1254]).
    let library = projects.file_library_mut();
    let forest = library.tags();
    let list: Vec<Value> = collect_tag_json(&forest, None);
    let folders: Vec<Value> = suhina_library::store::folders_iter(&forest)
        .map(|f| {
            json!({
                "id": f.id, "name": f.name, "open": f.open,
                "parent": suhina_library::store::find_entry_parent(&forest, f.id).and_then(|(p, _)| p),
            })
        })
        .collect();
    let tagged: serde_json::Map<String, Value> = library.tagged().into_iter().map(|(k, v)| (k, json!(v))).collect();
    let sections = library.sections();
    Ok(json!({ "tags": list, "folders": folders, "tagged": tagged, "limit": library.tag_limit(),
        "sections": sections.iter().map(|s| json!({ "kind": s.kind.name(), "open": s.open })).collect::<Vec<_>>() }))
}

/// The tags of the forest in the order shown, each with the folder it sits in: a level's
/// folders' tags ahead of its own tags, as the sidebar's rows go ([#1254] › Tag folders).
fn collect_tag_json(tree: &[suhina_library::TagEntry], parent: Option<u32>) -> Vec<Value> {
    let mut out = Vec::new();
    for entry in tree {
        if let suhina_library::TagEntry::Folder { folder, tags } = entry {
            out.extend(collect_tag_json(tags, Some(folder.id)));
        }
    }
    for entry in tree {
        if let suhina_library::TagEntry::Tag(tag) = entry {
            out.push(json!({
                "id": tag.id, "name": tag.name, "color": tag.color, "folder": parent,
            }));
        }
    }
    out
}

fn set_tag(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    // What the command asks is agreed before anything runs: the limit's form, and the
    // folder a make lands in ([#1254] review). The limit goes in before the tag's own
    // work, so a raise makes past the library's own wall — `{"name":"Extra","limit":65}`
    // at the 64 tags makes, where the old limit refused both. A tag work that fails
    // rolls that limit back, so the one command lands whole or refuses whole ([#1254]
    // review: a make behind a lowered limit is refused with nothing half in).
    let limit = Args::new(params).value("limit").filter(|v| !v.is_null());
    let parsed_limit = match &limit {
        Some(limit) => match limit.as_u64().and_then(|n| usize::try_from(n).ok()) {
            Some(n) if (1..=suhina_library::MAX_TAG_LIMIT).contains(&n) => Some(n),
            _ => return Err("the tag limit is from 1 to 1024".into()),
        },
        None => None,
    };
    let folder = match optional_text(params, "folder")? {
        Some(given) => Some(resolve_folder_param(projects, given).map_err(CommandError)?),
        None => None,
    };
    let mut raised = None;
    if let Some(max) = parsed_limit {
        let old = projects.file_library_mut().tag_limit();
        if old != max {
            projects.file_library_mut().set_tag_limit(max).map_err(CommandError)?;
            raised = Some(old);
        }
    }
    let work = |projects: &mut Projects| -> Result<Value, CommandError> {
        // The limit applied before this runs is its own turn: the nothing form refuses
        // when no limit and no tag came with it ([#1254] review).
        let mut done = parsed_limit.is_some();
        match tag_param(projects, params)? {
            // A tag by its id or name, changed, or created from the named colours when the name
            // is not one yet ([#1254]):
            Some(SetTagGiven::Tag((id, rename))) => {
                if folder.is_some() {
                    // A tag the library has takes no `folder`: where it goes is
                    // `library.moveTag`'s ([#1254] review).
                    return Err("a tag the library has takes no folder; give its moves to library.moveTag".into());
                }
                if let Some(renamed) = rename {
                    projects.file_library_mut().rename_tag(id, &renamed).map_err(CommandError)?;
                }
                if let Some(color) = optional_text(params, "color")? {
                    projects.file_library_mut().set_tag_color(id, Some(color)).map_err(CommandError)?;
                }
                done = true;
            }
            // A name the `id` carried that no tag has: made from the store's make, which holds
            // the same turn for `rename` and lands in the `folder` when one is asked ([#1254]
            // review).
            Some(SetTagGiven::Name(name)) => {
                projects
                    .file_library_mut()
                    .set_tag(&name, optional_text(params, "color")?, optional_text(params, "rename")?, folder)
                    .map_err(CommandError)?;
                done = true;
            }
            // The `name` param is the id-keep's own: read as soon as no id gave, so a limit and
            // a make can come in the one command ([#1254] review: the word forms keep their own
            // work beside the limit's).
            None => {
                if let Some(name) = optional_text(params, "name")? {
                    projects
                        .file_library_mut()
                        .set_tag(name, optional_text(params, "color")?, optional_text(params, "rename")?, folder)
                        .map_err(CommandError)?;
                    done = true;
                }
            }
        }
        if !done {
            return Err("give a tag's id or name, a tag folder's, or a limit".into());
        }
        tags(projects, &Value::Null)
    };
    match work(projects) {
        Ok(reply) => Ok(reply),
        Err(e) => {
            if let Some(old) = raised {
                let _ = projects.file_library_mut().set_tag_limit(old);
            }
            Err(e)
        }
    }
}

/// A `set_tag`'s `id` names a tag the library has, with its rename untouched, or a name
/// nothing has — created from rather than looked up ([#1254] review: the help keeps its
/// promise; a `#<id>` that is not one stays an error).
enum SetTagGiven {
    Tag((u32, Option<String>)),
    Name(String),
}

fn tag_param(projects: &mut Projects, params: &Value) -> Result<Option<SetTagGiven>, CommandError> {
    let rename = Args::new(params).value("rename").and_then(Value::as_str).map(str::to_string);
    match Args::new(params).value("id") {
        // An id's own, by number or `#<id>` ([#1254]: the UIs send the text form). A name
        // nothing has makes one instead; an id's own form that is not one refuses.
        Some(Value::String(given)) => {
            if let Some(id) = resolve_tag_param(projects, given) {
                return Ok(Some(SetTagGiven::Tag((id, rename))));
            }
            // The id's own shape (`#` over digits and nothing else, [#1254] review) that
            // is not one of the library's stays an error; other leading-# forms are
            // names, made and reached as names.
            if suhina_library::store::name_is_id_form(given) {
                return Err(format!("there is no tag {given:?}; library.tags lists them").into());
            }
            Ok(Some(SetTagGiven::Name(given.to_string())))
        }
        Some(given) => {
            let id = given.as_u64().ok_or("id must be a whole number")?;
            let id = u32::try_from(id).map_err(|_| "id must be a whole number")?;
            Ok(Some(SetTagGiven::Tag((id, rename))))
        }
        None => Ok(None),
    }
}

fn remove_tag(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let given: String = match Args::new(params).value("id").filter(|v| !v.is_null()) {
        Some(v) => v.as_str().map(|s| s.to_string()).ok_or_else(|| "id must be a tag's id or name".to_string())?,
        None => Args::new(params).text("name")?.to_string(),
    };
    let id = resolve_tag_param(projects, &given)
        .ok_or_else(|| format!("there is no tag {given:?}; library.tags lists them"))?;
    projects.file_library_mut().remove_tag(id).map_err(CommandError)?;
    Ok(json!({ "removed": given }))
}

fn tag(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let key = Args::new(params).text("item")?;
    let item = Item::parse(key).ok_or("item must be a path or an item key")?;
    let given = Args::new(params).text("tag")?;
    let on = Args::new(params).bool("on")?;
    let id = resolve_tag_param(projects, given)
        .ok_or_else(|| format!("there is no tag {given:?}; library.tags lists them"))?;
    let library = projects.file_library_mut();
    library.tag(&item.key(), id, on).map_err(CommandError)?;
    let tags = library.item_tags(&item.key());
    Ok(json!({ "item": item.key(), "tags": tags }))
}

/// The tag a `tag`-sounding param names: `#<id>` for the id's own, else the name (names are
/// unique, [#1254](https://github.com/suhina-daw/suhina-daw/issues/1254)). The id's form is
/// `#` over ASCII digits and nothing else, the shape no name may spell ([#1254] review:
/// `#+1` parsed as the id 1 and reached Red instead of the tag named so); any other text,
/// leading-# or not, is a name.
pub(crate) fn resolve_tag_param(projects: &mut Projects, given: &str) -> Option<u32> {
    if suhina_library::store::name_is_id_form(given) {
        let id = suhina_library::store::id_of_selector(given)?;
        let library = projects.file_library_mut();
        suhina_library::store::tag_by_id(&library.tags(), id).map(|_| id)
    } else {
        projects.file_library_mut().tag_id_of(given)
    }
}

/// Records what a successful command added to a project among the recent items
/// (`docs/spec/browser.md` › Sidebar).
pub(crate) fn record_use(library: &mut Library, recent: crate::Recent, params: &Value, result: &Value) {
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
        if let Some(path) = Args::new(params).raw_str("preset") {
            let path = crate::files::absolute(Path::new(path)).display().to_string();
            let voice = Args::new(params).value("voice").and_then(Value::as_u64).unwrap_or(1);
            record(Group::Presets, if voice > 1 { format!("{path}#{voice}") } else { path });
        }
    };
    match recent {
        crate::Recent::Files => {
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
        crate::Recent::Device => {
            if let Some(key) = Args::new(params).raw_str("stock") {
                record(Group::Presets, format!("stock:{key}"));
            } else if let Some(path) = Args::new(params).raw_str("sd") {
                record(Group::Effects, path.to_string());
            } else if Args::new(params).has("preset") {
                preset(&mut record, params);
            } else if let Some(id) =
                Args::new(params).value("id").or_else(|| Args::new(params).value("device")).and_then(Value::as_str)
            {
                if suhina_device_registry::info(id).is_some() {
                    device(&mut record, id);
                } else if id == Args::new(params).raw_str("device").unwrap_or("") {
                    record(Group::Plugins, format!("plugin:{id}"));
                }
            }
        }
        crate::Recent::SdFile => {
            if let Some(path) = Args::new(params).raw_str("path") {
                record(Group::Effects, path.to_string());
            }
        }
        crate::Recent::Plugin => {
            if let Some(name) = Args::new(params).raw_str("plugin") {
                record(Group::Plugins, format!("plugin:{name}"));
            }
        }
    }
}
/// The folder a folder-sounding param names: `#<id>` for the id's own, else the name
/// ([#1254](https://github.com/suhina-daw/suhina-daw/issues/1254) review: folder params know
/// folders only, so a tag of the same name does not shadow the folder). A name that more
/// than one folder has is refused: the forest order would pick one of them, and a
/// reordering would change which ([#1254] review), the id's own says which. The id's form is
/// the selectors' shape alone (`#` over ASCII digits); a folder named `#notes` or `#` is
/// reached by its name ([#1254] review). The paths' errors are the callers'.
pub(crate) fn resolve_folder_param(projects: &mut Projects, given: &str) -> Result<u32, String> {
    if suhina_library::store::name_is_id_form(given) {
        let id = suhina_library::store::id_of_selector(given)
            .ok_or_else(|| format!("there is no tag folder {given:?}; library.tags lists the folders"))?;
        let library = projects.file_library_mut();
        if suhina_library::store::folder_by_id(&library.tags(), id).is_some() {
            Ok(id)
        } else {
            Err(format!("there is no tag folder {given:?}; library.tags lists the folders"))
        }
    } else {
        let library = projects.file_library_mut();
        let matches: Vec<u32> =
            suhina_library::store::folders_iter(&library.tags()).filter(|f| f.name == given).map(|f| f.id).collect();
        match matches.as_slice() {
            [id] => Ok(*id),
            [] => Err(format!("there is no tag folder {given:?}; library.tags lists the folders")),
            more => Err(format!("there are {} tag folders named {given:?}; name it by its id (#<id>)", more.len())),
        }
    }
}

/// `library.addTagFolder` ([#1254](https://github.com/suhina-daw/suhina-daw/issues/1254)):
/// a folder of tags, unique among its own level's names, named when not asked.
pub(crate) fn add_tag_folder(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let parent = match optional_text(params, "parent")? {
        Some(given) => Some(resolve_folder_param(projects, given).map_err(CommandError)?),
        None => None,
    };
    let id =
        projects.file_library_mut().add_tag_folder(optional_text(params, "name")?, parent).map_err(CommandError)?;
    Ok(json!({ "folder": id }))
}

/// `library.renameTagFolder` ([#1254]: the name is unique among its own level).
pub(crate) fn rename_tag_folder(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let given = Args::new(params).text("folder")?;
    let id = resolve_folder_param(projects, given).map_err(CommandError)?;
    let renamed = Args::new(params).text("rename")?;
    projects.file_library_mut().rename_tag_folder(id, renamed).map_err(CommandError)?;
    Ok(json!({ "folder": id, "renamed": renamed }))
}

/// `library.removeTagFolder` ([#1254]: its tags and folders move up to its parent).
pub(crate) fn remove_tag_folder(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let given = Args::new(params).text("folder")?;
    let id = resolve_folder_param(projects, given).map_err(CommandError)?;
    let up = projects.file_library_mut().remove_tag_folder(id).map_err(CommandError)?;
    Ok(json!({ "removed": given, "up": up }))
}

/// `library.setFolderOpen` ([#1254]: each folder remembers whether it shows open).
pub(crate) fn set_folder_open(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let given = Args::new(params).text("folder")?;
    let id = resolve_folder_param(projects, given).map_err(CommandError)?;
    let open = Args::new(params).bool("open")?;
    projects.file_library_mut().set_tag_folder_open(id, open).map_err(CommandError)?;
    Ok(json!({ "folder": id, "open": open }))
}

/// `library.moveTag` ([#1254]): a tag or a tag folder moves under `folder` — the top
/// level when left out — at `position` of that level, clamped to its length.
pub(crate) fn move_tag(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let given = Args::new(params).text("id")?;
    // The id's own: the tag's own first, then the folder's — an ambiguous folder name
    // still refuses ([#1254] review).
    let id = match resolve_tag_param(projects, given) {
        Some(id) => id,
        None => resolve_folder_param(projects, given).map_err(CommandError)?,
    };
    let parent = match optional_text(params, "folder")? {
        Some(given) => Some(resolve_folder_param(projects, given).map_err(CommandError)?),
        None => None,
    };
    let position = match Args::new(params).value("position") {
        None => usize::MAX,
        Some(v) => v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or("position must be a whole number")?,
    };
    projects.file_library_mut().move_tag_node(id, parent, position).map_err(CommandError)?;
    Ok(json!({ "moved": given, "folder": parent }))
}

/// `library.setSections` ([#1254] › Sidebar sections): the three movable sections in
/// the order given, each whether it shows open — as it already showed, so a collapse
/// survives a reordering ([#1254] review); a section left out of the command keeps what it
/// had, and Folders stays last whatever is asked.
pub(crate) fn set_sections(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let mut order: Vec<suhina_library::Section> = Vec::new();
    let current = projects.file_library_mut().sections();
    if let Some(list) = Args::new(params).value("order").and_then(Value::as_array) {
        for kind in list {
            let Some(kind) = kind.as_str().and_then(suhina_library::SectionKind::parse) else {
                return Err("order is the sections by name (pinned, tags, library), each once".into());
            };
            if order.iter().any(|s| s.kind == kind) {
                return Err("order is the sections by name (pinned, tags, library), each once".into());
            }
            order.push(
                current
                    .iter()
                    .find(|s| s.kind == kind)
                    .copied()
                    .unwrap_or(suhina_library::Section { kind, open: true }),
            );
        }
    }
    // Sections left out keep the order and the open state they had.
    for was in &current {
        if !order.iter().any(|s| s.kind == was.kind) {
            order.push(*was);
        }
    }
    projects.file_library_mut().set_sections(order).map_err(CommandError)?;
    tags(projects, &Value::Null)
}

/// `library.setSectionOpen` ([#1254] › Sidebar sections): whether a section shows open.
pub(crate) fn set_section_open(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let kind = optional_text(params, "section")?
        .and_then(suhina_library::SectionKind::parse)
        .ok_or("section is pinned, tags or library")?;
    let open = Args::new(params).bool("open")?;
    projects.file_library_mut().set_section_open(kind, open).map_err(CommandError)?;
    Ok(json!({ "section": kind.name(), "open": open }))
}
