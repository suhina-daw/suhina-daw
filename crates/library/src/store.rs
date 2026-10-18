// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the browser keeps between runs, in `browser.json` (`docs/spec/browser.md` › Where it is
//! kept): the folders, the pins, the recent items by group, the colour tags and which items
//! carry them. A file that cannot be read gives an empty store and a warning; parts that do not
//! make sense are skipped with a warning.

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
/// Tags there may be by default, and at most.
pub const DEFAULT_TAG_LIMIT: usize = 64;
pub const MAX_TAG_LIMIT: usize = 1024;
/// Items that may carry tags.
const MAX_TAGGED: usize = 100_000;
/// Longest item key and tag name kept.
const MAX_KEY: usize = 4096;
const MAX_TAG_NAME: usize = 64;

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

/// A colour tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub name: String,
    /// `#RRGGBB`.
    pub color: String,
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

/// The seven tags a new library starts with.
pub fn default_tags() -> Vec<Tag> {
    [
        ("Red", "#C8645A"),
        ("Orange", "#D08F6E"),
        ("Yellow", "#C6AC5E"),
        ("Green", "#7FA673"),
        ("Blue", "#73AAC0"),
        ("Purple", "#A497CC"),
        ("Grey", "#8C8A85"),
    ]
    .into_iter()
    .map(|(name, color)| Tag { name: name.into(), color: color.into() })
    .collect()
}

/// Whether `text` is a colour as `#RRGGBB`.
pub fn is_color(text: &str) -> bool {
    text.len() == 7 && text.starts_with('#') && text[1..].chars().all(|c| c.is_ascii_hexdigit())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    pub folders: Vec<PathBuf>,
    /// Newest last.
    pub pins: Vec<String>,
    /// Newest first.
    pub recent: BTreeMap<Group, Vec<String>>,
    pub tags: Vec<Tag>,
    /// Item key to the names of its tags.
    pub tagged: BTreeMap<String, Vec<String>>,
    pub tag_limit: usize,
    /// How the browser shows plug-ins.
    pub plugins_view: PluginsView,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            folders: Vec::new(),
            pins: Vec::new(),
            recent: BTreeMap::new(),
            tags: default_tags(),
            tagged: BTreeMap::new(),
            tag_limit: DEFAULT_TAG_LIMIT,
            plugins_view: PluginsView::Instruments,
        }
    }
}

fn texts(value: Option<&Value>, limit: usize, what: &str, warnings: &mut Vec<String>) -> Vec<String> {
    let Some(value) = value else { return Vec::new() };
    let Some(list) = value.as_array() else {
        warnings.push(format!("browser.json: {what} is not a list; ignored"));
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for item in list {
        match item.as_str() {
            Some(s) if !s.is_empty() && s.len() <= MAX_KEY && !out.iter().any(|o| o == s) => out.push(s.to_string()),
            Some(_) => {}
            None => warnings.push(format!("browser.json: an entry of {what} is not a text; skipped")),
        }
    }
    if out.len() > limit {
        warnings.push(format!("browser.json: {what} had more than {limit} entries; the rest were left out"));
        out.truncate(limit);
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
        let mut store = Self { folders, pins, ..Self::default() };
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
        if let Some(tags) = map.get("tags") {
            match tags.as_array() {
                Some(list) => {
                    store.tags.clear();
                    for tag in list {
                        let name = tag.get("name").and_then(Value::as_str).map(str::trim).unwrap_or("");
                        let color = tag.get("color").and_then(Value::as_str).unwrap_or("");
                        if name.is_empty() || name.len() > MAX_TAG_NAME {
                            warnings.push("browser.json: a tag without a usable name; skipped".into());
                        } else if !is_color(color) {
                            warnings.push(format!("browser.json: tag {name:?} has no #RRGGBB colour; skipped"));
                        } else if store.tags.iter().any(|t| t.name == name) {
                            warnings.push(format!("browser.json: tag {name:?} twice; the second skipped"));
                        } else if store.tags.len() >= store.tag_limit {
                            warnings
                                .push(format!("browser.json: more than {} tags; the rest skipped", store.tag_limit));
                            break;
                        } else {
                            store.tags.push(Tag { name: name.to_string(), color: color.to_string() });
                        }
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
            for (item, names) in tagged.iter().take(MAX_TAGGED) {
                if item.len() > MAX_KEY {
                    continue;
                }
                let names: Vec<String> = names
                    .as_array()
                    .map(|l| l.iter().filter_map(Value::as_str).filter(|n| store.tags.iter().any(|t| t.name == *n)))
                    .into_iter()
                    .flatten()
                    .map(str::to_string)
                    .collect();
                if !names.is_empty() {
                    store.tagged.insert(item.clone(), names);
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
            "tags": self.tags.iter().map(|t| json!({ "name": t.name, "color": t.color })).collect::<Vec<_>>(),
            "tagged": self.tagged,
            "tag_limit": self.tag_limit,
            "plugins_view": self.plugins_view.name(),
        })
    }

    /// Writes the store to `file`, through a file beside it that is moved over it.
    pub fn save(&self, file: &Path) -> std::io::Result<()> {
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp = file.with_extension("json.part");
        let text = serde_json::to_string_pretty(&self.to_json()).map_err(std::io::Error::other)?;
        std::fs::write(&temp, text)?;
        std::fs::rename(&temp, file)
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

    pub fn unpin(&mut self, item: &str) -> bool {
        let before = self.pins.len();
        self.pins.retain(|p| p != item);
        before != self.pins.len()
    }

    /// Adds tag `name` with `color`, or changes the colour (and name, to `rename`) of one.
    pub fn set_tag(&mut self, name: &str, color: Option<&str>, rename: Option<&str>) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() || name.len() > MAX_TAG_NAME {
            return Err(format!("a tag's name is 1 to {MAX_TAG_NAME} bytes"));
        }
        if let Some(c) = color
            && !is_color(c)
        {
            return Err("color must be #RRGGBB".into());
        }
        let renamed = rename.map(str::trim).filter(|r| *r != name);
        if let Some(r) = renamed {
            if r.is_empty() || r.len() > MAX_TAG_NAME {
                return Err(format!("a tag's name is 1 to {MAX_TAG_NAME} bytes"));
            }
            if self.tags.iter().any(|t| t.name == r) {
                return Err(format!("there is a tag {r:?} already"));
            }
        }
        match self.tags.iter().position(|t| t.name == name) {
            Some(i) => {
                if let (Some(tag), Some(c)) = (self.tags.get_mut(i), color) {
                    tag.color = c.to_string();
                }
                if let Some(r) = renamed {
                    if let Some(tag) = self.tags.get_mut(i) {
                        tag.name = r.to_string();
                    }
                    for names in self.tagged.values_mut() {
                        for n in names.iter_mut().filter(|n| *n == name) {
                            *n = r.to_string();
                        }
                    }
                }
            }
            None => {
                if self.tags.len() >= self.tag_limit {
                    return Err(format!("there are {} tags already, the limit", self.tag_limit));
                }
                let color = color.ok_or("a new tag needs a color, #RRGGBB")?;
                self.tags.push(Tag { name: renamed.unwrap_or(name).to_string(), color: color.to_string() });
            }
        }
        Ok(())
    }

    pub fn remove_tag(&mut self, name: &str) -> Result<(), String> {
        let before = self.tags.len();
        self.tags.retain(|t| t.name != name);
        if before == self.tags.len() {
            return Err(format!("there is no tag {name:?}"));
        }
        for names in self.tagged.values_mut() {
            names.retain(|n| n != name);
        }
        self.tagged.retain(|_, names| !names.is_empty());
        Ok(())
    }

    /// Sets or clears tag `tag` on `item`.
    pub fn tag(&mut self, item: &str, tag: &str, on: bool) -> Result<(), String> {
        if !self.tags.iter().any(|t| t.name == tag) {
            return Err(format!("there is no tag {tag:?}; library.tags lists them"));
        }
        if item.is_empty() || item.len() > MAX_KEY {
            return Err("item must be a path or an item key".into());
        }
        if on {
            if !self.tagged.contains_key(item) && self.tagged.len() >= MAX_TAGGED {
                return Err(format!("at most {MAX_TAGGED} items can carry tags"));
            }
            let names = self.tagged.entry(item.to_string()).or_default();
            if !names.iter().any(|n| n == tag) {
                names.push(tag.to_string());
            }
        } else if let Some(names) = self.tagged.get_mut(item) {
            names.retain(|n| n != tag);
            if names.is_empty() {
                self.tagged.remove(item);
            }
        }
        Ok(())
    }

    pub fn set_tag_limit(&mut self, limit: usize) -> Result<(), String> {
        if !(1..=MAX_TAG_LIMIT).contains(&limit) {
            return Err(format!("the tag limit is from 1 to {MAX_TAG_LIMIT}"));
        }
        if limit < self.tags.len() {
            return Err(format!("there are {} tags; remove some before lowering the limit", self.tags.len()));
        }
        self.tag_limit = limit;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("suhina-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_store_round_trips_and_a_missing_file_is_empty() {
        let dir = temp("round");
        let file = dir.join("browser.json");
        let (empty, warnings) = Store::load(&file);
        assert_eq!((empty.clone(), warnings.len()), (Store::default(), 0));
        assert_eq!(empty.tags.len(), 7);
        let mut store = Store::default();
        store.folders.push(PathBuf::from("/samples"));
        store.pin("/samples/kick.wav");
        store.record(Group::Samples, "/samples/a.wav");
        store.record(Group::Samples, "/samples/b.wav");
        store.record(Group::Samples, "/samples/a.wav");
        store.set_tag("Drums", Some("#112233"), None).unwrap();
        store.tag("/samples/kick.wav", "Drums", true).unwrap();
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
        assert_eq!(store.tags, [Tag { name: "Fine".into(), color: "#AABBCC".into() }]);
        assert_eq!(store.tagged["/a"], ["Fine"]);
        assert_eq!(store.tag_limit, DEFAULT_TAG_LIMIT);
        assert_eq!(warnings.len(), 6, "{warnings:#?}");
        std::fs::write(&file, vec![b' '; (MAX_FILE_BYTES + 1) as usize]).unwrap();
        assert_eq!(Store::load(&file).1.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tags_are_renamed_removed_and_limited() {
        let mut store = Store::default();
        store.tag("x", "Red", true).unwrap();
        store.set_tag("Red", None, Some("Hot")).unwrap();
        assert_eq!(store.tagged["x"], ["Hot"]);
        assert!(store.set_tag("Hot", Some("red"), None).is_err());
        assert!(store.set_tag("New", None, None).is_err(), "a new tag needs a colour");
        assert!(store.tag("x", "Nothing", true).is_err());
        store.remove_tag("Hot").unwrap();
        assert!(store.tagged.is_empty());
        assert!(store.set_tag_limit(3).is_err(), "six tags are left");
        store.set_tag_limit(6).unwrap();
        assert!(store.set_tag("Seventh", Some("#000000"), None).is_err());
        store.set_tag_limit(10).unwrap();
        store.set_tag("Seventh", Some("#000000"), None).unwrap();
        assert!(store.set_tag_limit(MAX_TAG_LIMIT + 1).is_err());
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
}
