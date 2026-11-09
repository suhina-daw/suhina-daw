// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the browser keeps between runs, in `browser.json` (`docs/spec/browser.md` › Where it is
//! kept): the folders, the pins, the recent items by group, the colour tags — every one with
//! an id of its own, grouped into folders of tags — and which items carry them. A file that
//! cannot be read gives an empty store and a warning; parts that do not make sense are
//! skipped with a warning.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

/// Largest `browser.json` read.
pub const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// Folders that can be added.
pub const MAX_FOLDERS: usize = 256;
/// Pinned items kept; the oldest go first.
pub const MAX_PINS: usize = 1000;
/// Recent items kept per group.
pub const MAX_RECENT: usize = 50;
/// Audio files outside projects' folders kept as trusted; the oldest go first.
pub const MAX_TRUSTED_AUDIO: usize = 20_000;
/// Projects whose source folders are kept, and folders kept for each.
pub const MAX_PROJECTS_WITH_FOLDERS: usize = 2000;
pub const MAX_PROJECT_FOLDERS: usize = 64;
/// Tags there may be by default, and at most.
pub const DEFAULT_TAG_LIMIT: usize = 64;
pub const MAX_TAG_LIMIT: usize = 1024;
/// Items that may carry tags.
const MAX_TAGGED: usize = 100_000;
/// Longest item key and tag name kept.
const MAX_KEY: usize = 4096;
const MAX_TAG_NAME: usize = 64;
/// Folders of tags there may be, and the deepest one nests: a drop that would go deeper is
/// refused ([#1254](https://github.com/suhina-daw/suhina-daw/issues/1254)).
pub const MAX_TAG_FOLDERS: usize = 256;
pub const MAX_TAG_DEPTH: usize = 4;
/// Longest tag folder name kept.
const MAX_TAG_FOLDER_NAME: usize = 64;
/// The ids of tags and tag folders: from 1, given out in order, never reused.
pub const FIRST_TAG_ID: u32 = 1;

/// The groups of recent items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Group {
    Projects,
    Samples,
    Midi,
    Instruments,
    Effects,
    Plugins,
    Presets,
}

impl Group {
    pub const ALL: [Group; 7] =
        [Self::Projects, Self::Samples, Self::Midi, Self::Instruments, Self::Effects, Self::Plugins, Self::Presets];

    pub fn name(self) -> &'static str {
        match self {
            Self::Projects => "projects",
            Self::Samples => "samples",
            Self::Midi => "midi",
            Self::Instruments => "instruments",
            Self::Effects => "effects",
            Self::Plugins => "plugins",
            Self::Presets => "presets",
        }
    }

    /// The name shown in the sidebar.
    pub fn title(self) -> &'static str {
        match self {
            Self::Projects => "Projects",
            Self::Samples => "Samples",
            Self::Midi => "MIDI",
            Self::Instruments => "Instruments",
            Self::Effects => "Effects",
            Self::Plugins => "Plug-ins",
            Self::Presets => "Presets",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|g| g.name() == text)
    }
}

/// A colour tag. Items, folders and the order refer to it by its `id`; the `name` is for
/// people and is unique in the library ([#1254](https://github.com/suhina-daw/suhina-daw/issues/1254)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub id: u32,
    pub name: String,
    /// `#RRGGBB`.
    pub color: String,
}

/// A folder of tags. It holds tags and other folders by id; nothing is keyed by a name, so
/// a rename changes only the tag it names ([#1254](https://github.com/suhina-daw/suhina-daw/issues/1254)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagFolder {
    pub id: u32,
    pub name: String,
    /// Its rows show under the name; a click opens and closes it, and it is remembered.
    pub open: bool,
}

/// A row of the Tags section, in the order the section shows them: a tag, or a folder whose
/// tags and folders follow under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagEntry {
    Tag(Tag),
    Folder { folder: TagFolder, tags: Vec<TagEntry> },
}

impl TagEntry {
    /// The entry's own id, a tag's or a folder's.
    pub fn id(&self) -> u32 {
        match self {
            Self::Tag(tag) => tag.id,
            Self::Folder { folder, .. } => folder.id,
        }
    }

    /// The name it shows in the section.
    pub fn name(&self) -> &str {
        match self {
            Self::Tag(tag) => &tag.name,
            Self::Folder { folder, .. } => &folder.name,
        }
    }
}

/// A collapsible section of the sidebar, in the order its title shows. Folders is not here:
/// it is always last and cannot move ([#1254](https://github.com/suhina-daw/suhina-daw/issues/1254)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    Pinned,
    Tags,
    Library,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    pub kind: SectionKind,
    pub open: bool,
}

impl SectionKind {
    /// The name over the section in the sidebar.
    pub fn title(self) -> &'static str {
        match self {
            Self::Pinned => "Pinned",
            Self::Tags => "Tags",
            Self::Library => "Library",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "Pinned" | "pinned" => Self::Pinned,
            "Tags" | "tags" => Self::Tags,
            "Library" | "library" => Self::Library,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Pinned => "pinned",
            Self::Tags => "tags",
            Self::Library => "library",
        }
    }

    /// The sections whose order the sidebar keeps ([#1254]).
    pub const ALL: [SectionKind; 3] = [Self::Pinned, Self::Tags, Self::Library];
}

impl Section {
    /// The sidebar's sections as a new library has them: Pinned at the top, then Tags, then
    /// Library, all open ([#1254](https://github.com/suhina-daw/suhina-daw/issues/1254)).
    pub fn default_sections() -> Vec<Section> {
        SectionKind::ALL.into_iter().map(|kind| Section { kind, open: true }).collect()
    }
}

/// The named colours a new tag takes its name and colour from: the seven a library starts
/// with first, then the CSS named colours (W3C Color Module Level 4) trimmed — no colour
/// carries two names (gray/grey, aqua/cyan, magenta/fuchsia) and consecutive entries sit
/// apart ([#1254](https://github.com/suhina-daw/suhina-daw/issues/1254)).
pub fn named_colours() -> &'static [(&'static str, &'static str)] {
    &[
        ("Red", "#C8645A"),
        ("Orange", "#D08F6E"),
        ("Yellow", "#C6AC5E"),
        ("Green", "#7FA673"),
        ("Blue", "#73AAC0"),
        ("Purple", "#A497CC"),
        ("Grey", "#8C8A85"),
        ("Maroon", "#800000"),
        ("Brown", "#A52A2A"),
        ("Salmon", "#FA8072"),
        ("Tomato", "#FF6347"),
        ("Chocolate", "#D2691E"),
        ("Gold", "#FFD700"),
        ("Olive", "#808000"),
        ("Lime", "#00FF00"),
        ("ForestGreen", "#228B22"),
        ("Teal", "#008080"),
        ("Turquoise", "#40E0D0"),
        ("SteelBlue", "#4682B4"),
        ("Navy", "#000080"),
        ("Indigo", "#4B0082"),
        ("Magenta", "#FF00FF"),
        ("Orchid", "#DA70D6"),
        ("Plum", "#DDA0DD"),
        ("HotPink", "#FF69B4"),
        ("Sienna", "#A0522D"),
        ("Peru", "#CD853F"),
        ("Khaki", "#F0E68C"),
        ("CadetBlue", "#5F9EA0"),
        ("RebeccaPurple", "#663399"),
    ]
}

/// Which plug-ins the browser lists when the Plug-ins category is chosen
/// ([the plug-in browsing spec](../spec/plugin-browsing.md) › Instruments and Effects).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PluginsView {
    /// The plug-in instruments, as one list sortable by column.
    #[default]
    Instruments,
    /// The plug-in effects, likewise.
    Effects,
}

impl PluginsView {
    pub fn name(self) -> &'static str {
        match self {
            Self::Instruments => "instruments",
            Self::Effects => "effects",
        }
    }

    /// The name on its button.
    pub fn title(self) -> &'static str {
        match self {
            Self::Instruments => "Instruments",
            Self::Effects => "Effects",
        }
    }

    /// The view [`PluginsView::name`] gives `text`; anything else is not one.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|v| v.name() == text)
    }

    pub const ALL: [PluginsView; 2] = [Self::Instruments, Self::Effects];

    /// The views kept by earlier versions (by vendor, by type, one list): read as the default.
    const EARLIER: [&'static str; 3] = ["grouped", "type", "flat"];
}

/// Whether `text` is one id's selector: `#` over one or more ASCII digits and nothing else.
/// `#`, `#+1` and `#1a` are not ([#1254] review: a sign or an empty number is a name's,
/// never an id's).
fn is_id_selector(text: &str) -> bool {
    text.strip_prefix('#').is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

/// The id one selector spells (`#<digits>`), when it spells one that fits an id; `None`
/// for anything else, a name among them ([#1254] review).
pub fn id_of_selector(text: &str) -> Option<u32> {
    text.strip_prefix('#').filter(|_| is_id_selector(text)).and_then(|digits| digits.parse().ok())
}

/// Whether a name spells a selector of ids: one id's (`#<digits>`) or several joined with
/// `+` (`#1+#2`, a place of several). Such names are reserved, because a tag or folder
/// named so would answer the ids' selectors instead of its name ([#1254] review). Other
/// leading-# forms (`#`, `#notes`, `#+1`) are names like any of the rest.
pub fn name_is_id_form(name: &str) -> bool {
    name.split('+').all(is_id_selector)
}

/// What the stores' name checks say of a reserved name ([`name_is_id_form`]).
const ID_FORM_REFUSED: &str = "spells an id's selector (#<digits>, or several joined with +)";

/// The folder `id`'s children, wherever it sits in the forest (`None` back when there is
/// no such folder) — for counts and menus ([#1254]).
pub fn folder_children(entries: &[TagEntry], id: u32) -> Option<&[TagEntry]> {
    for entry in entries {
        if let TagEntry::Folder { folder, tags } = entry {
            if folder.id == id {
                return Some(tags);
            }
            if let Some(under) = folder_children(tags, id) {
                return Some(under);
            }
        }
    }
    None
}

/// Whether a name is taken at the level `parent` holds, whatever kind the taker is
/// ([#1254] review: a level keeps its names one word apart, tags and folders in the one
/// set — the shape the loader skips cannot be made by the moves either). `skip` is the
/// id the check reads past, the moved one's own.
fn level_name_taken(entries: &[TagEntry], parent: Option<u32>, name: &str, skip: u32) -> bool {
    let level = match parent {
        None => entries,
        Some(id) => folder_children(entries, id).unwrap_or(&[]),
    };
    level.iter().any(|e| e.id() != skip && e.name() == name)
}

/// Whether `text` is a colour as `#RRGGBB`.
pub fn is_color(text: &str) -> bool {
    text.len() == 7 && text.starts_with('#') && text[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Every tag in the forest, in the order shown: a level's folders carry their tags ahead of
/// the level's own tags, as the sidebar's rows go ([#1254] › Tag folders).
pub fn tags_iter(entries: &[TagEntry]) -> impl Iterator<Item = &Tag> {
    let mut all = Vec::new();
    collect_tags_in_order(entries, &mut all);
    all.into_iter()
}

fn collect_tags_in_order<'a>(entries: &'a [TagEntry], out: &mut Vec<&'a Tag>) {
    for entry in entries {
        if let TagEntry::Folder { tags, .. } = entry {
            collect_tags_in_order(tags, out);
        }
    }
    for entry in entries {
        if let TagEntry::Tag(tag) = entry {
            out.push(tag);
        }
    }
}

/// Every tag's id in the forest, in the order shown.
pub fn tag_ids(entries: &[TagEntry]) -> Vec<u32> {
    tags_iter(entries).map(|tag| tag.id).collect()
}

/// How many tags the forest holds, folders aside.
pub fn tag_count(entries: &[TagEntry]) -> usize {
    tags_iter(entries).count()
}

/// How many rows the forest holds, tags and folders together: what removing a folder moves
/// up ([#1254](https://github.com/suhina-daw/suhina-daw/issues/1254)).
pub fn entry_count(entries: &[TagEntry]) -> usize {
    entries.len()
        + entries.iter().fold(0, |sum, entry| match entry {
            TagEntry::Folder { tags, .. } => sum + entry_count(tags),
            TagEntry::Tag(_) => sum,
        })
}

/// What removing the folder `id` moves up: every row under it, tags and folders together —
/// the count [`Store::remove_tag_folder`] answers with, for the menus to ask first with
/// ([#1254] review: a folder holding only an empty folder carries one). 0 when there is no
/// such folder.
pub fn folder_carried(entries: &[TagEntry], id: u32) -> usize {
    folder_children(entries, id).map_or(0, entry_count)
}

/// The next free id, over every id the forest has — a tag's and a tag folder's both: ids
/// are from [`FIRST_TAG_ID`] and are never reused, so the highest plus one is the next
/// ([#1254](https://github.com/suhina-daw/suhina-daw/issues/1254)).
pub fn next_free_id(entries: &[TagEntry]) -> u32 {
    let mut ids = tag_ids(entries);
    ids.extend(folders_iter(entries).map(|f| f.id));
    ids.iter().copied().max().map_or(FIRST_TAG_ID, |high| high.saturating_add(1))
}

/// The id a new tag or folder takes: the next free one, while it stays within the ids a file
/// keeps ([`MAX_FILE_TAG_ID`]). Past it the make is refused: an id the store made past it
/// would be handed out anew on the next load, in file order, and the items carrying it would
/// come back carrying another tag ([#1254] review).
fn new_id(entries: &[TagEntry]) -> Result<u32, String> {
    let id = next_free_id(entries);
    if id > MAX_FILE_TAG_ID {
        return Err("the library has handed out every tag id it keeps; no tag or folder can be made".into());
    }
    Ok(id)
}

/// The tag with `id`, wherever it is in the forest.
pub fn tag_by_id(entries: &[TagEntry], id: u32) -> Option<&Tag> {
    tags_iter(entries).find(|tag| tag.id == id)
}

/// Whether a name is a tag's already: names are unique in the library ([#1254]).
pub fn tag_name_taken(entries: &[TagEntry], name: &str) -> bool {
    tags_iter(entries).any(|tag| tag.name == name)
}

/// The folder with `id`, wherever it is in the forest.
pub fn folder_by_id(entries: &[TagEntry], id: u32) -> Option<&TagFolder> {
    folders_iter(entries).find(|folder| folder.id == id)
}

/// Every folder in the forest, in the order shown.
pub fn folders_iter(entries: &[TagEntry]) -> impl Iterator<Item = &TagFolder> {
    let mut out = Vec::new();
    collect_folders(entries, &mut out);
    out.into_iter()
}

fn collect_folders<'a>(entries: &'a [TagEntry], out: &mut Vec<&'a TagFolder>) {
    for entry in entries {
        if let TagEntry::Folder { folder, tags } = entry {
            out.push(folder);
            collect_folders(tags, out);
        }
    }
}

/// How deep the entry with `id` sits: a top-level entry is 1; 0 when there is none.
pub fn entry_depth(entries: &[TagEntry], id: u32) -> usize {
    for entry in entries {
        match entry {
            TagEntry::Tag(tag) if tag.id == id => return 1,
            TagEntry::Folder { folder, tags } if folder.id == id => return 1,
            TagEntry::Folder { tags, .. } => {
                let under = entry_depth(tags, id);
                if under > 0 {
                    return under + 1;
                }
            }
            TagEntry::Tag(_) => {}
        }
    }
    0
}

/// How many folder levels the entry with `id` reaches down from its own row ([#1254]).
/// Tags are leaves and reach none: a tag's own folders are only the ones it sits within.
/// 0 when there is none.
pub fn entry_height(entries: &[TagEntry], id: u32) -> usize {
    for entry in entries {
        match entry {
            TagEntry::Folder { folder, .. } if folder.id == id => return height_of_entry(entry),
            TagEntry::Folder { tags, .. } => {
                let under = entry_height(tags, id);
                if under > 0 {
                    return under;
                }
            }
            TagEntry::Tag(_) => {}
        }
    }
    0
}

/// The folder levels one entry of the forest reaches: a tag is a leaf (0), a folder is 1
/// over its deepest child folder's ([#1254](https://github.com/suhina-daw/suhina-daw/issues/1254)).
/// The depth cap counts folder levels, so a tag carried into the deepest folder adds none.
fn height_of_entry(entry: &TagEntry) -> usize {
    match entry {
        TagEntry::Tag(_) => 0,
        TagEntry::Folder { tags, .. } => {
            1 + tags
                .iter()
                .filter_map(|child| match child {
                    TagEntry::Folder { .. } => Some(height_of_entry(child)),
                    TagEntry::Tag(_) => None,
                })
                .max()
                .unwrap_or(0)
        }
    }
}

/// The mutable tag with `id`, wherever it is.
fn tag_mut_in(entries: &mut Vec<TagEntry>, id: u32) -> Option<&mut Tag> {
    for entry in entries {
        match entry {
            TagEntry::Tag(tag) if tag.id == id => return Some(tag),
            TagEntry::Folder { tags, .. } => {
                if let Some(tag) = tag_mut_in(tags, id) {
                    return Some(tag);
                }
            }
            TagEntry::Tag(_) => {}
        }
    }
    None
}

/// The mutable folder with `id`, wherever it is.
fn folder_mut_in(entries: &mut Vec<TagEntry>, id: u32) -> Option<&mut TagFolder> {
    for entry in entries {
        if let TagEntry::Folder { folder, tags } = entry {
            if folder.id == id {
                return Some(folder);
            } else if let Some(folder) = folder_mut_in(tags, id) {
                return Some(folder);
            }
        }
    }
    None
}

/// The highest id a file's tag forest keeps: far past what any library makes (ids are never
/// reused, one per tag or folder made), and far enough under `u32::MAX` that the ids handed
/// out after it never overflow ([#1254] review: a file's ids are hostile input).
const MAX_FILE_TAG_ID: u32 = 1 << 30;

/// Whether a file's entry would be read in, as far as its own fields say: a name of the
/// right length, and a tag's colour. Only such an entry reserves its id, so a broken one
/// takes no room from the ids handed out ([#1254] review).
fn plausible(node: &Value, tag: bool) -> bool {
    let name = node.get("name").and_then(Value::as_str).map(str::trim).unwrap_or("");
    let max = if tag { MAX_TAG_NAME } else { MAX_TAG_FOLDER_NAME };
    !name.is_empty() && name.len() <= max && (!tag || node.get("color").and_then(Value::as_str).is_some_and(is_color))
}

/// The file's ids and names, before any entry is read: the ids its readable entries keep
/// (tags' and folders' together, which no handed-out id takes), every name it has (which no
/// renamed entry takes) and how many entries name each raw id (a raw id named once can be
/// followed when the reader replaces it) ([#1254] review).
fn survey_file(list: &[Value], depth: usize, ids: &mut FileIds) {
    for entry in list {
        let (node, tag) = match (entry.get("tag"), entry.get("folder")) {
            (Some(node), _) => (node, true),
            (None, Some(node)) => (node, false),
            (None, None) => (entry, true),
        };
        if let Some(name) = node.get("name").and_then(Value::as_str) {
            ids.names.insert(name.trim().to_string());
        }
        // A folder past the depth cap is left out with its cortège, so it keeps nothing.
        let read = tag || depth <= MAX_TAG_DEPTH;
        if let Some(raw) = node.get("id").and_then(Value::as_u64) {
            *ids.raw_count.entry(raw).or_insert(0) += 1;
            if read
                && plausible(node, tag)
                && let Ok(id) = u32::try_from(raw)
                && (FIRST_TAG_ID..=MAX_FILE_TAG_ID).contains(&id)
            {
                ids.kept.insert(id);
            }
        }
        if !tag
            && depth <= MAX_TAG_DEPTH
            && let Some(children) = entry.get("tags").and_then(Value::as_array)
        {
            survey_file(children, depth + 1, ids);
        }
    }
}

/// What reading a file's tag forest keeps besides the entries: the ids it hands out, the
/// names taken and the names and ids it had to change ([#1254] review). The checks are sets,
/// so a file of many entries reads in linear time ([#1254] review: each fallback id scanned
/// the forest, and a large file stalled the start).
#[derive(Default)]
struct FileIds {
    /// The next id handed out: past every id the file keeps.
    next: u32,
    /// Where the search for a free id goes on once `next` is past the cap: every id below it
    /// is kept or given.
    fallback: u32,
    /// Every id the file's readable entries keep, which no handed-out id takes.
    kept: std::collections::HashSet<u32>,
    /// The ids the entries read so far have.
    given: std::collections::HashSet<u32>,
    /// How many of the file's entries name each raw id.
    raw_count: std::collections::HashMap<u64, usize>,
    /// A raw id the file named once whose entry was given another id, and that id: the
    /// assignments by number follow it ([#1254] review: an id past the cap was replaced and
    /// the items carrying it lost the tag).
    remapped: std::collections::HashMap<u64, u32>,
    /// A name the file had that spells an id's selector, and the name its tag got instead,
    /// so the legacy assignments that named it still find it.
    renamed: Vec<(String, String)>,
    /// The selector-spelling folder names renamed, with their level: a second of them in the
    /// level is the duplicate it would have been ([#1254] review).
    renamed_folders: std::collections::HashSet<(Option<u32>, String)>,
    /// Every name the file's tags and folders have: a renamed one takes none of them, so it
    /// never pushes out an entry read after it ([#1254] review).
    names: std::collections::HashSet<String>,
    /// The names of the tags read so far (unique in the library).
    tag_names: std::collections::HashSet<String>,
    /// The names each level read so far shows, tags' and folders' (unique per level).
    level_names: std::collections::HashSet<(Option<u32>, String)>,
}

impl FileIds {
    fn of(list: &[Value]) -> Self {
        let mut ids = Self::default();
        survey_file(list, 1, &mut ids);
        ids.next = ids.kept.iter().copied().max().unwrap_or(0).saturating_add(1).max(FIRST_TAG_ID);
        ids.fallback = FIRST_TAG_ID;
        ids
    }

    /// The id an entry of the file keeps: its own, when that is one (from [`FIRST_TAG_ID`]
    /// to [`MAX_FILE_TAG_ID`]) no entry read before it has — tags and folders share the one
    /// id space ([#1254] review). `None` sends the entry to the ids handed out.
    fn own(&self, node: &Value) -> Option<u32> {
        node.get("id")
            .and_then(Value::as_u64)
            .and_then(|id| u32::try_from(id).ok())
            .filter(|id| (FIRST_TAG_ID..=MAX_FILE_TAG_ID).contains(id) && !self.given.contains(id))
    }

    /// An id for an entry without a usable one: the next past the file's, while that stays
    /// within [`MAX_FILE_TAG_ID`]; past it the lowest one neither the file keeps nor an entry
    /// read has, so every id read is one a reload reads back ([#1254] review: a file keeping
    /// the highest id handed out one past it). `None` when there is none left.
    fn hand_out(&mut self) -> Option<u32> {
        while self.next <= MAX_FILE_TAG_ID {
            let id = self.next;
            self.next += 1;
            if !self.kept.contains(&id) && !self.given.contains(&id) {
                return Some(id);
            }
        }
        while self.fallback <= MAX_FILE_TAG_ID {
            let id = self.fallback;
            self.fallback += 1;
            if !self.kept.contains(&id) && !self.given.contains(&id) {
                return Some(id);
            }
        }
        None
    }

    /// The id the entry `node` is read in under, with `name` at the level `parent`: its own,
    /// or one handed out; a raw id the file named for it alone then maps to it.
    fn give(&mut self, node: &Value, parent: Option<u32>, name: &str, tag: bool) -> Option<u32> {
        let id = self.own(node).or_else(|| self.hand_out())?;
        self.given.insert(id);
        if let Some(raw) = node.get("id").and_then(Value::as_u64)
            && raw != u64::from(id)
            && self.raw_count.get(&raw) == Some(&1)
        {
            self.remapped.insert(raw, id);
        }
        if tag {
            self.tag_names.insert(name.to_string());
        }
        self.level_names.insert((parent, name.to_string()));
        Some(id)
    }

    /// A handed-out id for #1250's flat tag, which has no id of its own, recorded as given.
    fn hand_out_named(&mut self, parent: Option<u32>, name: &str) -> Option<u32> {
        let id = self.hand_out()?;
        self.given.insert(id);
        self.tag_names.insert(name.to_string());
        self.level_names.insert((parent, name.to_string()));
        Some(id)
    }

    /// Whether `name` is taken for a renamed entry at the level `parent`: a name of the file,
    /// a tag's read so far, or one the level shows.
    fn name_taken(&self, parent: Option<u32>, name: &str) -> bool {
        self.names.contains(name)
            || self.tag_names.contains(name)
            || self.level_names.contains(&(parent, name.to_string()))
    }
}

/// A name for an entry whose own spells an id's selector (`#1`, a file from before the
/// selectors were reserved): `Tag #1`, numbered when that is taken as well, within `max`
/// bytes. An overlong name keeps as much of it as that number's room leaves, cut at a
/// character's edge ([#1254] review: `Tag ` over 63 digits had nothing to keep, and the
/// entry went with its assignments). The numbers go on until one is free: the names taken
/// are finite, so one is ([#1254] review: the search stopped at 99). `None` only when the
/// number itself no longer fits.
fn unreserved_name(kind: &str, name: &str, max: usize, taken: impl Fn(&str) -> bool) -> Option<String> {
    let room = max.saturating_sub(kind.len() + 1);
    if room == 0 {
        return None;
    }
    let mut n: u64 = 1;
    loop {
        let candidate = if n == 1 {
            format!("{kind} {}", head_at_boundary(name, room))
        } else {
            let number = n.to_string();
            let kept = number.len().saturating_add(1);
            if kept >= room {
                return None;
            }
            format!("{kind} {} {n}", head_at_boundary(name, room - kept))
        };
        if !taken(&candidate) {
            return Some(candidate);
        }
        n = n.checked_add(1)?;
    }
}

/// The first `keep` bytes of `s`, cut at a character's edge: the selectors' names read here
/// are ASCII digits, but a file's need not be, and a cut that splits a character panics.
/// Fewer bytes than asked when the edge asks for it ([#1254] review).
fn head_at_boundary(s: &str, keep: usize) -> &str {
    let end = keep.min(s.len());
    let end = (0..=end).rev().find(|k| s.is_char_boundary(*k)).unwrap_or(0);
    s.get(..end).unwrap_or("")
}

/// Reads one level of the file's tag forest into `entries` under `parent` (None for the
/// top); `depth` is how deep the level is, against [`MAX_TAG_DEPTH`] — the cap counts
/// folder levels, so tags sit in the deepest folder fine and a folder past it is left out
/// — and `ids` gives each entry its own id or one handed out, first served in file order,
/// from past every id the file keeps ([#1254]).
fn read_tag_entries(
    list: &[Value],
    parent: Option<u32>,
    entries: &mut Vec<TagEntry>,
    ids: &mut FileIds,
    depth: usize,
    warnings: &mut Vec<String>,
) {
    for entry in list {
        // A tag of the new shape, {"tag": {"id", "name", "color"}}, or #1250's flat one,
        // {"name", "color"} with no id of its own.
        let flat = entry.get("tag").is_none() && (entry.get("name").is_some() || entry.get("color").is_some());
        let node = if flat { Some(entry) } else { entry.get("tag") };
        if let Some(tag) = node {
            let name = tag.get("name").and_then(Value::as_str).map(str::trim).unwrap_or("");
            let color = tag.get("color").and_then(Value::as_str).unwrap_or("");
            if name.is_empty() || name.len() > MAX_TAG_NAME {
                warnings.push("browser.json: a tag without a usable name; skipped".into());
                continue;
            }
            if !is_color(color) {
                warnings.push(format!("browser.json: tag {name:?} has no #RRGGBB colour; skipped"));
                continue;
            }
            let name = if name_is_id_form(name) {
                // A second tag of the same selector name is the duplicate a second of any
                // name is: skipped, not renamed beside the first ([#1254] review).
                if ids.renamed.iter().any(|(from, _)| from == name) {
                    warnings.push(format!("browser.json: tag {name:?} twice; the second skipped"));
                    continue;
                }
                match unreserved_name("Tag", name, MAX_TAG_NAME, |n| ids.name_taken(parent, n)) {
                    Some(renamed) => {
                        warnings
                            .push(format!("browser.json: tag {name:?} spells an id's selector; renamed {renamed:?}"));
                        ids.renamed.push((name.to_string(), renamed.clone()));
                        renamed
                    }
                    None => {
                        warnings.push(format!("browser.json: tag {name:?} spells an id's selector; skipped"));
                        continue;
                    }
                }
            } else {
                name.to_string()
            };
            if ids.tag_names.contains(&name) {
                warnings.push(format!("browser.json: tag {name:?} twice; the second skipped"));
            } else if ids.level_names.contains(&(parent, name.clone())) {
                // A name the level's own folder has: the shape the moves refuse, skipped on
                // load ([#1254] review), so a reload keeps the same turns.
                warnings.push(format!("browser.json: tag {name:?} conflicts with a sibling folder; skipped"));
            } else {
                let given = if flat { ids.hand_out_named(parent, &name) } else { ids.give(tag, parent, &name, true) };
                let Some(id) = given else {
                    warnings.push(format!("browser.json: no tag id is left for {name:?}; skipped"));
                    continue;
                };
                if let Some(level) = level_mut(entries, parent) {
                    level.push(TagEntry::Tag(Tag { id, name, color: color.to_string() }));
                }
            }
        } else if let Some(folder) = entry.get("folder") {
            // {"folder": {"id", "name", "open"}, "tags": [...]}.
            // A folder nests at most four deep: one past the cap is left out, its
            // cortège with it ([#1254]).
            if depth > MAX_TAG_DEPTH {
                warnings.push(format!(
                    "browser.json: tag folders nest more than {MAX_TAG_DEPTH} deep; the rest are left out"
                ));
                break;
            }
            let name = folder.get("name").and_then(Value::as_str).map(str::trim).unwrap_or("");
            let open = folder.get("open").and_then(Value::as_bool).unwrap_or(true);
            if name.is_empty() || name.len() > MAX_TAG_FOLDER_NAME {
                warnings.push("browser.json: a tag folder without a usable name; skipped".into());
                continue;
            }
            // A folder whose name spells a selector keeps its place and its cortège under a
            // name of its own ([#1254] review); a second of that name in the level is the
            // duplicate it would have been.
            let name = if name_is_id_form(name) {
                if !ids.renamed_folders.insert((parent, name.to_string())) {
                    warnings.push(format!("browser.json: tag folder {name:?} twice in a level; the second skipped"));
                    continue;
                }
                match unreserved_name("Folder", name, MAX_TAG_FOLDER_NAME, |n| ids.name_taken(parent, n)) {
                    Some(renamed) => {
                        warnings.push(format!(
                            "browser.json: tag folder {name:?} spells an id's selector; renamed {renamed:?}"
                        ));
                        renamed
                    }
                    None => {
                        warnings.push(format!("browser.json: tag folder {name:?} spells an id's selector; skipped"));
                        continue;
                    }
                }
            } else {
                name.to_string()
            };
            if ids.level_names.contains(&(parent, name.clone())) {
                warnings.push(format!("browser.json: tag folder {name:?} twice in a level; the second skipped"));
            } else if folders_iter(entries).count() >= MAX_TAG_FOLDERS {
                warnings.push(format!("browser.json: more than {MAX_TAG_FOLDERS} tag folders; the rest skipped"));
                break;
            } else {
                let Some(id) = ids.give(folder, parent, &name, false) else {
                    warnings.push(format!("browser.json: no tag id is left for folder {name:?}; skipped"));
                    continue;
                };
                if let Some(level) = level_mut(entries, parent) {
                    level.push(TagEntry::Folder { folder: TagFolder { id, name, open }, tags: Vec::new() });
                }
                if let Some(children) = entry.get("tags").and_then(Value::as_array) {
                    read_tag_entries(children, Some(id), entries, ids, depth + 1, warnings);
                }
            }
        } else {
            warnings.push("browser.json: a tag entry that is neither a tag nor a folder; skipped".into());
        }
    }
}

/// The list one level of the forest is held in, for writing: `None` for the top level,
/// else the folder's children (`None` back when there is no such folder).
fn level_mut(entries: &mut Vec<TagEntry>, parent: Option<u32>) -> Option<&mut Vec<TagEntry>> {
    match parent {
        None => Some(entries),
        Some(id) => {
            // This level's own folder matches first; nested folders are searched after it,
            // and a folder at any depth carries its own children ([#1254]).
            for entry in entries.iter_mut() {
                match entry {
                    TagEntry::Folder { folder, .. } if folder.id == id => {
                        if let TagEntry::Folder { tags, .. } = entry {
                            return Some(tags);
                        }
                    }
                    TagEntry::Folder { tags, .. } => {
                        if let Some(level) = level_mut(tags, parent) {
                            return Some(level);
                        }
                    }
                    TagEntry::Tag(_) => {}
                }
            }
            None
        }
    }
}

/// The names of the entries one level shows ([#1254]: folders are unique per level).
fn level_names(entries: &[TagEntry], parent: Option<u32>) -> Vec<String> {
    match parent {
        None => entries.iter().map(TagEntry::name).map(str::to_string).collect(),
        Some(folder_id) => match folder_by_id(entries, folder_id) {
            Some(_) => match folder_children(entries, folder_id) {
                Some(level) => level.iter().map(TagEntry::name).map(str::to_string).collect(),
                None => Vec::new(),
            },
            None => Vec::new(),
        },
    }
}

/// Removes the entry with `id` from the forest, keeping it (a folder takes its entries
/// with it) ([#1254](https://github.com/suhina-daw/suhina-daw/issues/1254)).
/// The entry with `id`, from wherever it sits in the forest — a nested tag's and a nested
/// folder's too ([#1254]: any entry moves).
fn take_entry(entries: &mut Vec<TagEntry>, id: u32) -> Option<TagEntry> {
    if let Some(at) = entries.iter().position(|e| e.id() == id) {
        return Some(entries.remove(at));
    }
    for entry in entries.iter_mut() {
        if let TagEntry::Folder { tags, .. } = entry
            && let Some(taken) = take_entry(tags, id)
        {
            return Some(taken);
        }
    }
    None
}

/// Where the entry with `id` is held: its parent (`None` for the top) and its position in
/// that level's list.
pub fn find_entry_parent(entries: &[TagEntry], id: u32) -> Option<(Option<u32>, usize)> {
    let at = entries.iter().position(|e| e.id() == id);
    if let Some(at) = at {
        return Some((None, at));
    }
    for entry in entries {
        if let TagEntry::Folder { folder, tags } = entry
            && let Some((parent, at)) = find_entry_parent(tags, id)
        {
            return Some((parent.or(Some(folder.id)), at));
        }
    }
    None
}

/// Inserts `entry` at `position` of the level `parent` holds, clamped to its end
/// ([#1254](https://github.com/suhina-daw/suhina-daw/issues/1254)).
fn put_entry(entries: &mut Vec<TagEntry>, parent: Option<u32>, entry: &TagEntry, position: usize) -> bool {
    match level_mut(entries, parent) {
        Some(level) => {
            level.insert(position.min(level.len()), entry.clone());
            true
        }
        None => false,
    }
}

/// Drops the tag `id` from the forest, wherever it is.
fn drop_tag_entry(entries: &mut Vec<TagEntry>, id: u32) {
    for entry in entries.iter_mut() {
        if let TagEntry::Folder { tags, .. } = entry {
            drop_tag_entry(tags, id);
        }
    }
    entries.retain(|e| e.id() != id);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    pub folders: Vec<PathBuf>,
    /// Newest last.
    pub pins: Vec<String>,
    /// Newest first.
    pub recent: BTreeMap<Group, Vec<String>>,
    /// The Tags section: every tag and folder, in the order it shows. Items, folders and
    /// the order refer to tags by their ids, so a rename changes only the tag ([#1254]).
    pub tags: Vec<TagEntry>,
    /// Item key to the ids of the tags it carries.
    pub tagged: BTreeMap<String, Vec<u32>>,
    /// The most tags there may be.
    pub tag_limit: usize,
    /// The sidebar's collapsible sections in the order their titles show; Folders is not
    /// here, it is always last ([#1254](https://github.com/suhina-daw/suhina-daw/issues/1254)).
    pub sections: Vec<Section>,
    /// How the browser shows plug-ins.
    pub plugins_view: PluginsView,
    /// Audio files outside a project's folder that the person loaded into a project and saved
    /// it with, which opening a project may read (`docs/spec/missing-files.md` › Project file
    /// boundaries). Newest first.
    pub trusted_audio: Vec<PathBuf>,
    /// For each project file (its absolute path), the folders outside it that its audio was
    /// loaded from: opening it reads them and the missing files panel searches them. Kept on
    /// this machine, never in the project file, so a project cannot name folders to read.
    pub project_folders: BTreeMap<String, Vec<PathBuf>>,
}

impl Default for Store {
    fn default() -> Self {
        let mut next = FIRST_TAG_ID;
        let tags = named_colours()
            .iter()
            .take(7)
            .map(|(name, color)| {
                let id = next;
                next += 1;
                TagEntry::Tag(Tag { id, name: name.to_string(), color: color.to_string() })
            })
            .collect();
        Self {
            folders: Vec::new(),
            pins: Vec::new(),
            recent: BTreeMap::new(),
            tags,
            tagged: BTreeMap::new(),
            tag_limit: DEFAULT_TAG_LIMIT,
            sections: Section::default_sections(),
            plugins_view: PluginsView::Instruments,
            trusted_audio: Vec::new(),
            project_folders: BTreeMap::new(),
        }
    }
}

fn texts(value: Option<&Value>, limit: usize, what: &str, warnings: &mut Vec<String>) -> Vec<String> {
    let Some(value) = value else { return Vec::new() };
    let Some(list) = value.as_array() else {
        warnings.push(format!("browser.json: {what} is not a list; ignored"));
        return Vec::new();
    };
    // Read up to the limit only: a file can hold far more entries than are kept, and each one
    // kept is compared with those before it.
    let mut out: Vec<String> = Vec::new();
    for item in list {
        match item.as_str() {
            Some(s) if !s.is_empty() && s.len() <= MAX_KEY && !out.iter().any(|o| o == s) => {
                if out.len() == limit {
                    warnings
                        .push(format!("browser.json: {what} had more than {limit} entries; the rest were left out"));
                    break;
                }
                out.push(s.to_string());
            }
            Some(_) => {}
            None => warnings.push(format!("browser.json: an entry of {what} is not a text; skipped")),
        }
    }
    out
}

impl Store {
    /// Reads `file`; a missing file is an empty store. Returns what was skipped.
    pub fn load(file: &Path) -> (Self, Vec<String>) {
        let mut warnings = Vec::new();
        let Ok(meta) = std::fs::metadata(file) else { return (Self::default(), warnings) };
        if meta.len() > MAX_FILE_BYTES {
            warnings.push(format!("browser.json is larger than {} MiB; not read", MAX_FILE_BYTES / 1024 / 1024));
            return (Self::default(), warnings);
        }
        let parsed = std::fs::read_to_string(file)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string()));
        let value = match parsed {
            Ok(Value::Object(map)) => map,
            Ok(_) => {
                warnings.push("browser.json is not an object; not read".into());
                return (Self::default(), warnings);
            }
            Err(e) => {
                warnings.push(format!("browser.json could not be read: {e}"));
                return (Self::default(), warnings);
            }
        };
        (Self::from_map(&value, &mut warnings), warnings)
    }

    fn from_map(map: &Map<String, Value>, warnings: &mut Vec<String>) -> Self {
        let folders =
            texts(map.get("folders"), MAX_FOLDERS, "folders", warnings).into_iter().map(PathBuf::from).collect();
        let pins = texts(map.get("pinned"), MAX_PINS, "pinned", warnings);
        let trusted_audio = texts(map.get("trusted_audio"), MAX_TRUSTED_AUDIO, "trusted_audio", warnings)
            .into_iter()
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .collect();
        let mut store = Self { folders, pins, trusted_audio, ..Self::default() };
        if let Some(projects) = map.get("project_folders") {
            match projects.as_object() {
                Some(projects) => {
                    for (project, folders) in projects.iter().take(MAX_PROJECTS_WITH_FOLDERS) {
                        let what = format!("project_folders of {project}");
                        let folders: Vec<PathBuf> = texts(Some(folders), MAX_PROJECT_FOLDERS, &what, warnings)
                            .into_iter()
                            .map(PathBuf::from)
                            .filter(|p| p.is_absolute())
                            .collect();
                        if !folders.is_empty() && project.len() <= MAX_KEY {
                            store.project_folders.insert(project.clone(), folders);
                        }
                    }
                }
                None => warnings.push("browser.json: project_folders is not an object; ignored".into()),
            }
        }
        if let Some(recent) = map.get("recent") {
            match recent.as_object() {
                Some(groups) => {
                    for (name, items) in groups {
                        match Group::parse(name) {
                            Some(group) => {
                                let list = texts(Some(items), MAX_RECENT, &format!("recent {name}"), warnings);
                                store.recent.insert(group, list);
                            }
                            None => warnings.push(format!("browser.json: unknown recent group {name:?}; skipped")),
                        }
                    }
                }
                None => warnings.push("browser.json: recent is not an object; ignored".into()),
            }
        }
        if let Some(limit) = map.get("tag_limit") {
            match limit.as_u64().and_then(|n| usize::try_from(n).ok()) {
                Some(n) if (1..=MAX_TAG_LIMIT).contains(&n) => store.tag_limit = n,
                _ => warnings
                    .push(format!("browser.json: tag_limit must be from 1 to {MAX_TAG_LIMIT}; the default is used")),
            }
        }
        if let Some(list) = map.get("sections").and_then(Value::as_array) {
            // The order of the three movable sections, each with whether it is open;
            // Folders is not in the list, it stays last.
            let mut sections: Vec<Section> = Vec::new();
            for entry in list {
                let Some(kind) = entry.get("kind").and_then(Value::as_str).and_then(SectionKind::parse) else {
                    warnings.push("browser.json: a section without a known kind; skipped".into());
                    continue;
                };
                let open = entry.get("open").and_then(Value::as_bool).unwrap_or(true);
                if sections.iter().any(|s: &Section| s.kind == kind) {
                    warnings.push(format!("browser.json: section {} twice; the second skipped", kind.name()));
                } else {
                    sections.push(Section { kind, open });
                }
            }
            for kind in SectionKind::ALL {
                if !sections.iter().any(|s| s.kind == kind) {
                    sections.push(Section { kind, open: true });
                }
            }
            store.sections = sections;
        }
        // The names the reader changed (a name that spelled a selector), for the legacy
        // assignments below that named the tag ([#1254] review).
        let mut renamed: Vec<(String, String)> = Vec::new();
        // The raw ids the reader replaced, for the assignments by number ([#1254] review).
        let mut remapped: std::collections::HashMap<u64, u32> = std::collections::HashMap::new();
        if let Some(tags) = map.get("tags") {
            // The tags are the forest a new file keeps: a tag is {"tag": {"id", "name",
            // "color"}}, a folder is {"folder": {"id", "name", "open"}, "tags": [...]}, up
            // to the nesting depth. #1250's file kept a flat list {"name", "color"}
            // without ids: those are read in as top-level entries and given the next free
            // ids, first served in file order ([#1254](https://github.com/suhina-daw/suhina-daw/issues/1254)).
            match tags.as_array() {
                Some(list) => {
                    store.tags.clear();
                    // The ids handed out start past every id the file keeps ([#1254] review).
                    let mut ids = FileIds::of(list);
                    read_tag_entries(list, None, &mut store.tags, &mut ids, 1, warnings);
                    renamed = ids.renamed;
                    remapped = ids.remapped;
                    if tag_count(&store.tags) > store.tag_limit {
                        // The limit is over the tags a file read in; the last ones drop.
                        for id in tag_ids(&store.tags).split_off(store.tag_limit) {
                            drop_tag_entry(&mut store.tags, id);
                        }
                        warnings.push(format!("browser.json: more than {} tags; the rest skipped", store.tag_limit));
                    }
                }
                None => warnings.push("browser.json: tags is not a list; the defaults are used".into()),
            }
        }
        if let Some(view) = map.get("plugins_view") {
            match view.as_str().and_then(PluginsView::parse) {
                Some(view) => store.plugins_view = view,
                None if view.as_str().is_some_and(|v| PluginsView::EARLIER.contains(&v)) => {}
                None => warnings
                    .push("browser.json: plugins_view is not instruments or effects; the default is used".into()),
            }
        }
        if let Some(tagged) = map.get("tagged").and_then(Value::as_object) {
            for (item, ids) in tagged.iter().take(MAX_TAGGED) {
                if item.len() > MAX_KEY {
                    continue;
                }
                // #1250's file named the tags; theirs are the ids of the tags those names
                // name ([#1254](https://github.com/suhina-daw/suhina-daw/issues/1254)). An
                // unknown tag is skipped with a warning, as the store does elsewhere.
                let mut carries: Vec<u32> = Vec::new();
                let mut unknown = 0;
                for value in ids.as_array().into_iter().flatten() {
                    match value {
                        // The new format: ids.
                        Value::Number(n) => {
                            // A raw id the reader gave its tag another for follows it.
                            let id = n
                                .as_u64()
                                .and_then(|raw| remapped.get(&raw).copied().or_else(|| u32::try_from(raw).ok()));
                            if let Some(id) = id
                                && tag_by_id(&store.tags, id).is_some()
                                && !carries.contains(&id)
                            {
                                carries.push(id);
                            } else if n.as_u64().is_some() {
                                unknown += 1;
                            }
                        }
                        // The old format: names.
                        Value::String(name) => {
                            let name = renamed
                                .iter()
                                .find(|(from, _)| from == name)
                                .map_or(name.as_str(), |(_, to)| to.as_str());
                            if let Some(id) = tags_iter(&store.tags).find(|t| t.name == name).map(|t| t.id)
                                && !carries.contains(&id)
                            {
                                carries.push(id);
                            } else {
                                unknown += 1;
                            }
                        }
                        _ => unknown += 1,
                    }
                }
                if unknown > 0 {
                    warnings.push(format!(
                        "browser.json: {unknown} of {item}'s tag names are not tags of the file; skipped"
                    ));
                }
                if !carries.is_empty() {
                    store.tagged.insert(item.clone(), carries);
                }
            }
        }
        store
    }

    pub fn to_json(&self) -> Value {
        let recent: Map<String, Value> =
            self.recent.iter().map(|(g, items)| (g.name().to_string(), json!(items))).collect();
        let folders: Vec<String> = self.folders.iter().map(|f| f.display().to_string()).collect();
        json!({
            "folders": folders,
            "pinned": self.pins,
            "recent": recent,
            "tags": tag_entries_json(&self.tags),
            "tagged": self.tagged,
            "tag_limit": self.tag_limit,
            "sections": self.sections.iter().map(|s| json!({ "kind": s.kind.name(), "open": s.open }))
                .collect::<Vec<_>>(),
            "plugins_view": self.plugins_view.name(),
            "trusted_audio": self.trusted_audio.iter().map(|f| f.display().to_string()).collect::<Vec<_>>(),
            "project_folders": self
                .project_folders
                .iter()
                .map(|(p, folders)| (p.clone(), json!(folders.iter().map(|f| f.display().to_string()).collect::<Vec<_>>())))
                .collect::<Map<String, Value>>(),
        })
    }

    /// Writes the store to `file`, through a file beside it that is moved over it.
    pub fn save(&self, file: &Path) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(&self.to_json()).map_err(std::io::Error::other)?;
        suhina_fs::write_atomic(file, text)
    }

    /// Puts `item` first in `group`.
    pub fn record(&mut self, group: Group, item: &str) {
        if item.is_empty() || item.len() > MAX_KEY {
            return;
        }
        let list = self.recent.entry(group).or_default();
        list.retain(|i| i != item);
        list.insert(0, item.to_string());
        list.truncate(MAX_RECENT);
    }

    pub fn pin(&mut self, item: &str) -> bool {
        if item.is_empty() || item.len() > MAX_KEY || self.pins.iter().any(|p| p == item) {
            return false;
        }
        if self.pins.len() >= MAX_PINS {
            self.pins.remove(0);
        }
        self.pins.push(item.to_string());
        true
    }

    /// What an item carries goes with it when its file moves to `to` (#943): its pin, its
    /// places among the recent items and its tags (by id, [#1254]).
    pub fn relocate(&mut self, from: &str, to: &str) -> bool {
        if from == to || to.is_empty() || to.len() > MAX_KEY {
            return false;
        }
        let mut changed = false;
        for pin in self.pins.iter_mut().filter(|p| *p == from) {
            *pin = to.to_string();
            changed = true;
        }
        for list in self.recent.values_mut() {
            if list.iter().any(|i| i == from) {
                list.retain(|i| i != to);
                for item in list.iter_mut().filter(|i| *i == from) {
                    *item = to.to_string();
                }
                changed = true;
            }
        }
        if let Some(tags) = self.tagged.remove(from) {
            self.tagged.insert(to.to_string(), tags);
            changed = true;
        }
        changed
    }

    pub fn unpin(&mut self, item: &str) -> bool {
        let before = self.pins.len();
        self.pins.retain(|p| p != item);
        before != self.pins.len()
    }

    /// The next name and colour a new tag takes: the first name of the named colours that
    /// no tag has yet, with that colour ([#1254](https://github.com/suhina-daw/suhina-daw/issues/1254)).
    /// A list exhausted numbers the first ("Red 2"), the way `track.paste` names a copy.
    pub fn next_tag_name_and_color(&self) -> (String, String) {
        let named = named_colours();
        for (name, color) in named {
            if !tag_name_taken(&self.tags, name) {
                return (name.to_string(), color.to_string());
            }
        }
        let (name, color) = named.first().map_or(("Red", "#C8645A"), |(n, c)| (*n, *c));
        for n in 2.. {
            let numbered = format!("{name} {n}");
            if !tag_name_taken(&self.tags, &numbered) {
                return (numbered, color.to_string());
            }
        }
        (format!("{name} 2"), color.to_string())
    }

    /// Adds tag `name` (or its id `id`'s) with `color`, or changes the colour and name of
    /// the one it names. A `name` that no tag has and no `id` names a new one, created with
    /// the named colours' next turn when `color` is None ([#1254]). The name it wants
    /// belongs to another tag already: refused.
    ///
    /// `folder` puts a made tag in that folder from its first byte ([#1254] review: the
    /// destination's own level is where the make asks its name), where `None` is the top.
    /// Its own change only: a make, the same colours as its top-level ride, the checks at
    /// the level it lands in; colours and renames of a tag another `id` names go on as
    /// they did, with `folder` refused.
    pub fn set_tag(
        &mut self,
        name: &str,
        color: Option<&str>,
        rename: Option<&str>,
        folder: Option<u32>,
    ) -> Result<(), String> {
        let wanted = name.trim();
        if wanted.is_empty() || wanted.len() > MAX_TAG_NAME {
            return Err(format!("a tag's name is 1 to {MAX_TAG_NAME} bytes"));
        }
        if name_is_id_form(wanted) {
            return Err(format!("a tag's name never {ID_FORM_REFUSED}"));
        }
        if let Some(c) = color
            && !is_color(c)
        {
            return Err("color must be #RRGGBB".into());
        }
        let found = tags_iter(&self.tags).find(|t| t.name == wanted).map(|t| t.id);
        match found {
            Some(id) => {
                if folder.is_some() {
                    return Err("a tag the library has cannot be made again; give its moves to library.moveTag".into());
                }
                self.set_tag_color(id, color)?;
                if let Some(renamed) = rename {
                    self.rename_tag(id, renamed)?;
                }
            }
            None => {
                if tag_count(&self.tags) >= self.tag_limit {
                    return Err(format!("there are {} tags already, the limit", self.tag_limit));
                }
                // The name it is made under: the rename's when one is asked, held to the
                // same checks whichever colour came with it ([#1254] review: the no-colour
                // make answers its `rename` too).
                let mut named = wanted.to_string();
                if let Some(r) = rename.map(str::trim).filter(|r| !r.is_empty()) {
                    if r.len() > MAX_TAG_NAME {
                        return Err(format!("a tag's name is 1 to {MAX_TAG_NAME} bytes"));
                    }
                    if name_is_id_form(r) {
                        return Err(format!("a tag's name never {ID_FORM_REFUSED}"));
                    }
                    if tag_name_taken(&self.tags, r) {
                        return Err(format!("there is a tag {r:?} already"));
                    }
                    named = r.to_string();
                }
                // The destination: `folder`'s level when one is given, else the top — the
                // make asks its name of the level it lands in, folder or not ([#1254]
                // review; the folder's own existence checked beside it).
                if folder.is_some_and(|folder_id| folder_by_id(&self.tags, folder_id).is_none()) {
                    return Err(format!("there is no tag folder #{:?}", folder.unwrap_or(0)));
                }
                if level_names(&self.tags, folder).contains(&named) {
                    // The conflicting entry is the folder that has the wanted name, wherever
                    // the make was headed ([#1254] review: the message said tag and named
                    // the destination's own name).
                    return Err(match folder {
                        Some(_) => format!("there is a tag folder {named:?} there already"),
                        None => format!("there is a tag folder {named:?} at the top already"),
                    });
                }
                let color = match color {
                    // The colour as it was asked; else the named colours' turn from the
                    // name it is made under.
                    Some(c) => c.to_string(),
                    None => self.new_tag_color_for(&named),
                };
                self.push_tag(named, color, folder)?;
            }
        }
        Ok(())
    }

    /// The colour a new tag named `name` takes when none was asked: the named colours'
    /// entry the name names (a hand-named "Tomato" gets its tomato), else the enum's next
    /// colour ([#1254](https://github.com/suhina-daw/suhina-daw/issues/1254)).
    fn new_tag_color_for(&self, name: &str) -> String {
        match named_colours().iter().find(|(named, _)| *named == name.trim()) {
            Some((_, color)) => color.to_string(),
            None => self.next_tag_name_and_color().1,
        }
    }

    fn push_tag(&mut self, name: String, color: String, folder: Option<u32>) -> Result<(), String> {
        let id = new_id(&self.tags)?;
        let tag = TagEntry::Tag(Tag { id, name, color });
        match level_mut(&mut self.tags, folder) {
            Some(level) => {
                level.push(tag);
                Ok(())
            }
            None => Err(format!("there is no tag folder {folder:?}")),
        }
    }

    /// Changes `id`'s colour; `None` changes nothing.
    pub fn set_tag_color(&mut self, id: u32, color: Option<&str>) -> Result<(), String> {
        let Some(color) = color else { return Ok(()) };
        if !is_color(color) {
            return Err("color must be #RRGGBB".into());
        }
        match self.tag_mut(id) {
            Some(tag) => {
                tag.color = color.to_string();
                Ok(())
            }
            None => Err(format!("there is no tag {id}")),
        }
    }

    fn tag_mut(&mut self, id: u32) -> Option<&mut Tag> {
        tag_mut_in(&mut self.tags, id)
    }

    /// Renames `id`'s tag; the name stays unique in the library, and nothing that referred
    /// to the tag by its id changes ([#1254](https://github.com/suhina-daw/suhina-daw/issues/1254)).
    /// A name a folder of the tag's own level has is refused too ([#1254] review): the two
    /// kinds keep their names apart, in the forest and over a reload both.
    pub fn rename_tag(&mut self, id: u32, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() || name.len() > MAX_TAG_NAME {
            return Err(format!("a tag's name is 1 to {MAX_TAG_NAME} bytes"));
        }
        if name_is_id_form(name) {
            return Err(format!("a tag's name never {ID_FORM_REFUSED}"));
        }
        if tag_name_taken(&self.tags, name) && tag_by_id(&self.tags, id).map(|t| t.name.as_str()) != Some(name) {
            return Err(format!("there is a tag {name:?} already"));
        }
        // A folder's name at the tag's own level: the tag takes none, so the shape the
        // loader refuses cannot be saved ([#1254] review).
        if let Some((parent, _)) = find_entry_parent(&self.tags, id)
            && level_names(&self.tags, parent).iter().any(|shown| shown == name)
            && tag_by_id(&self.tags, id).map(|t| t.name.as_str()) != Some(name)
        {
            return Err(format!("there is a tag folder {name:?} at its level already"));
        }
        match self.tag_mut(id) {
            Some(tag) => {
                tag.name = name.to_string();
                Ok(())
            }
            None => Err(format!("there is no tag {id}")),
        }
    }

    /// Removes the tag `id`, from the list and from every item that carries it.
    pub fn remove_tag(&mut self, id: u32) -> Result<(), String> {
        if tag_by_id(&self.tags, id).is_none() {
            return Err(format!("there is no tag {id}"));
        }
        drop_tag_entry(&mut self.tags, id);
        for ids in self.tagged.values_mut() {
            ids.retain(|carried| *carried != id);
        }
        self.tagged.retain(|_, ids| !ids.is_empty());
        Ok(())
    }

    /// Sets or clears the tag `id` on `item` ([#1254]: items carry ids, not names).
    pub fn tag(&mut self, item: &str, id: u32, on: bool) -> Result<(), String> {
        if tag_by_id(&self.tags, id).is_none() {
            return Err(format!("there is no tag {id}; library.tags lists them"));
        }
        if item.is_empty() || item.len() > MAX_KEY {
            return Err("item must be a path or an item key".into());
        }
        if on {
            if !self.tagged.contains_key(item) && self.tagged.len() >= MAX_TAGGED {
                return Err(format!("at most {MAX_TAGGED} items can carry tags"));
            }
            let ids = self.tagged.entry(item.to_string()).or_default();
            if !ids.contains(&id) {
                ids.push(id);
            }
        } else if let Some(ids) = self.tagged.get_mut(item) {
            ids.retain(|carried| *carried != id);
            if ids.is_empty() {
                self.tagged.remove(item);
            }
        }
        Ok(())
    }

    /// The tag folder's next free name: `Folder`, then `Folder 2` ([#1254]).
    fn next_folder_name(&self, parent: Option<u32>) -> String {
        let level = level_names(&self.tags, parent);
        for n in 1.. {
            let name = match n {
                1 => "Folder".to_string(),
                n => format!("Folder {n}"),
            };
            if !level.iter().any(|shown| shown == &name) {
                return name;
            }
        }
        "Folder".to_string()
    }

    /// Adds a tag folder named `name` (the next free "Folder" when None) under `parent`,
    /// giving its id ([#1254](https://github.com/suhina-daw/suhina-daw/issues/1254)).
    pub fn add_tag_folder(&mut self, name: Option<&str>, parent: Option<u32>) -> Result<u32, String> {
        if parent.is_some_and(|p| entry_depth(&self.tags, p) >= MAX_TAG_DEPTH) {
            return Err(format!("tag folders nest at most {MAX_TAG_DEPTH} deep"));
        }
        let name = match name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(name) => {
                if name.len() > MAX_TAG_FOLDER_NAME {
                    return Err(format!("a tag folder's name is 1 to {MAX_TAG_FOLDER_NAME} bytes"));
                }
                if name_is_id_form(name) {
                    return Err(format!("a tag folder's name never {ID_FORM_REFUSED}"));
                }
                if level_names(&self.tags, parent).iter().any(|shown| shown == name) {
                    return Err(format!("there is a tag folder {name:?} here already"));
                }
                name.to_string()
            }
            None => self.next_folder_name(parent),
        };
        if folders_iter(&self.tags).count() >= MAX_TAG_FOLDERS {
            return Err(format!("there are {MAX_TAG_FOLDERS} tag folders already, the limit"));
        }
        let id = new_id(&self.tags)?;
        match level_mut(&mut self.tags, parent) {
            Some(level) => {
                level.push(TagEntry::Folder { folder: TagFolder { id, name, open: true }, tags: Vec::new() });
                Ok(id)
            }
            None => Err(format!("there is no tag folder {parent:?}")),
        }
    }

    /// Renames the tag folder `id`; the name is unique among the folders of its own level
    /// ([#1254](https://github.com/suhina-daw/suhina-daw/issues/1254): a folder under another
    /// parent may carry the name).
    pub fn rename_tag_folder(&mut self, id: u32, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() || name.len() > MAX_TAG_FOLDER_NAME {
            return Err(format!("a tag folder's name is 1 to {MAX_TAG_FOLDER_NAME} bytes"));
        }
        if name_is_id_form(name) {
            return Err(format!("a tag folder's name never {ID_FORM_REFUSED}"));
        }
        // A folder's name is unique among the folders of its own level (tags keep their own
        // uniqueness, so the few characters that could be both names are refused wherever
        // they show).
        let Some((parent, _)) = find_entry_parent(&self.tags, id) else {
            return Err(format!("there is no tag folder {id}"));
        };
        if level_names(&self.tags, parent).iter().any(|shown| *shown == name)
            && folder_by_id(&self.tags, id).map(|f| f.name.as_str()) != Some(name)
        {
            return Err(format!("there is a tag folder {name:?} already"));
        }
        match folder_mut_in(&mut self.tags, id) {
            Some(folder) => {
                folder.name = name.to_string();
                Ok(())
            }
            None => Err(format!("there is no tag folder {id}")),
        }
    }

    /// Sets whether the tag folder `id` shows open ([#1254]: each folder remembers).
    pub fn set_tag_folder_open(&mut self, id: u32, open: bool) -> Result<(), String> {
        match folder_mut_in(&mut self.tags, id) {
            Some(folder) => {
                folder.open = open;
                Ok(())
            }
            None => Err(format!("there is no tag folder {id}")),
        }
    }

    /// Removes the tag folder `id`, moving its tags and folders up to its parent: removing
    /// a folder never removes tags ([#1254](https://github.com/suhina-daw/suhina-daw/issues/1254)).
    /// The count of what moved is returned, for the asking-first of the menus.
    pub fn remove_tag_folder(&mut self, id: u32) -> Result<usize, String> {
        let Some((parent, at)) = find_entry_parent(&self.tags, id) else {
            return Err(format!("there is no tag folder {id}"));
        };
        // The folder's entry, from the level it is held in, whichever depth it sits at
        // ([#1254] folders nest).
        let Some(entry) =
            level_mut(&mut self.tags, parent).and_then(|level| (at < level.len()).then(|| level.remove(at)))
        else {
            return Err(format!("there is no tag folder {id}"));
        };
        let TagEntry::Folder { tags: children, folder } = entry else {
            return Err(format!("there is no tag folder {id}"));
        };
        // The count the menus ask first with: every row that moves up — tags AND folders
        // ([#1254]: a folder's whole cortège moves up).
        let carried = entry_count(&children);
        // The children go where the folder was, in its order ([#1254]). A child's name a
        // sibling at that level has: the lift would sit two of a name together, and the
        // reload skips the second — the children stay put and the removal is refused
        // ([#1254] review: a rename is the way out, not a lift).
        let taken =
            children.iter().map(|c| c.name().to_string()).find(|name| level_name_taken(&self.tags, parent, name, id));
        if let Some(child) = taken {
            if let Some(level) = level_mut(&mut self.tags, parent) {
                let at = at.min(level.len());
                level.insert(at, TagEntry::Folder { folder, tags: children });
            }
            return Err(format!(
                "there is a tag or tag folder {child:?} at that level already; rename it or the lifted one first"
            ));
        }
        match level_mut(&mut self.tags, parent) {
            Some(level) => {
                level.splice(at..at, children);
                Ok(carried)
            }
            None => Err(format!("there is no tag folder {id}")),
        }
    }

    /// Moves the tag or tag folder `id` under `parent` (the top when None), at `position`
    /// of that level, clamped to its length. Nesting stays within the depth cap, a tag is
    /// in one folder at a time, and a folder does not move under itself ([#1254]).
    pub fn move_tag_node(&mut self, id: u32, parent: Option<u32>, position: usize) -> Result<(), String> {
        let origin = find_entry_parent(&self.tags, id);
        let Some(entry) = take_entry(&mut self.tags, id) else {
            return Err(format!("there is no tag or tag folder {id}"));
        };
        // The parent has to exist once the entry is out of the way (it did not sit inside
        // the moved folder), and the nesting stays within the cap: the parent's folder
        // level plus the entry's own reach ([#1254]). A tag reaches none — a tag lives in
        // the deepest folder fine ([#1254]): it is the folders that nest.
        let valid = match parent {
            None => true,
            Some(p) => {
                folder_by_id(&self.tags, p).is_some()
                    && entry_depth(&self.tags, p) + height_of_entry(&entry) <= MAX_TAG_DEPTH
            }
        };
        if !valid {
            let (old_parent, at) = origin.unwrap_or((None, 0));
            put_entry(&mut self.tags, old_parent, &entry, at);
            return Err(format!("tag folders nest at most {MAX_TAG_DEPTH} deep"));
        }
        // A name the destination's level holds — other than the moved one's own slot: a
        // same-level reorder is a move's own ([#1254] review) — is refused, so a move
        // cannot set up the two-of-a-name level the reload would skip. The entry goes
        // back where it was.
        if level_name_taken(&self.tags, parent, entry.name(), id) {
            let (old_parent, at) = origin.unwrap_or((None, 0));
            put_entry(&mut self.tags, old_parent, &entry, at);
            return Err(format!(
                "there is a {} {:?} at that level already; rename it first",
                match &entry {
                    TagEntry::Folder { .. } => "tag folder",
                    TagEntry::Tag(_) => "tag",
                },
                entry.name()
            ));
        }
        if !put_entry(&mut self.tags, parent, &entry, position) {
            // Does not happen in one synchronous call, but the entry is never lost.
            let (old_parent, at) = origin.unwrap_or((None, 0));
            put_entry(&mut self.tags, old_parent, &entry, at);
            return Err(format!("there is no tag folder {parent:?}"));
        }
        Ok(())
    }

    /// The sections in the order their titles show: the three movable ones in the order the
    /// caller gave, first slot of a kind when one is asked twice (the command refuses the
    /// repeats; this keeps the memory whole whatever a caller says), and whether each is
    /// open. Missing kinds are appended in their default order ([#1254]).
    pub fn set_sections(&mut self, order: Option<&[Section]>) {
        if let Some(order) = order {
            let mut sections: Vec<Section> = Vec::new();
            for section in order {
                if !sections.iter().any(|s| s.kind == section.kind) {
                    sections.push(*section);
                }
            }
            for kind in SectionKind::ALL {
                if !sections.iter().any(|s| s.kind == kind) {
                    if let Some(known) = self.sections.iter().find(|s| s.kind == kind) {
                        sections.push(*known);
                    } else {
                        sections.push(Section { kind, open: true });
                    }
                }
            }
            self.sections = sections;
        }
    }

    /// Sets whether the section `kind` is open ([#1254](https://github.com/suhina-daw/suhina-daw/issues/1254)).
    pub fn set_section_open(&mut self, kind: SectionKind, open: bool) {
        if let Some(section) = self.sections.iter_mut().find(|s| s.kind == kind) {
            section.open = open;
        }
    }

    pub fn set_tag_limit(&mut self, limit: usize) -> Result<(), String> {
        if !(1..=MAX_TAG_LIMIT).contains(&limit) {
            return Err(format!("the tag limit is from 1 to {MAX_TAG_LIMIT}"));
        }
        let tags = tag_count(&self.tags);
        if limit < tags {
            return Err(format!("there are {tags} tags; remove some before lowering the limit"));
        }
        self.tag_limit = limit;
        Ok(())
    }
}

/// Tag nodes' json: a tag is {"tag": {"id", "name", "color"}}, a folder is
/// {"folder": {"id", "name", "open"}, "tags": [...]}, depth as the forest has it ([#1254]).
fn tag_entries_json(entries: &[TagEntry]) -> Vec<Value> {
    entries
        .iter()
        .map(|entry| match entry {
            TagEntry::Tag(tag) => json!({ "tag": { "id": tag.id, "name": tag.name, "color": tag.color } }),
            TagEntry::Folder { folder, tags } => {
                json!({ "folder": { "id": folder.id, "name": folder.name, "open": folder.open }, "tags": tag_entries_json(tags) })
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
