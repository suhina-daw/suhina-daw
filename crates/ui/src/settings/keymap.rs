// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Keyboard shortcuts: which keys run which action, kept in a file the user can edit
//! (`keymap.json`: an action id to a list of keys such as `"Ctrl+Shift+Z"`, the four mouse
//! switches the Mouse-first preset sets, and the name of the `preset` they came from). An action
//! is a session command that needs no parameters, an action on the selection (which the window
//! turns into a command), or something the window itself does.

use std::collections::BTreeMap;

use egui::{Key, Modifiers};
use serde_json::{Value, json};

use crate::settings::workflow::{Setting, Workflow};

/// One key with its modifiers. `Ctrl` means Cmd on macOS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub modifiers: Modifiers,
    pub key: Key,
}

impl Shortcut {
    pub const fn new(modifiers: Modifiers, key: Key) -> Self {
        Self { modifiers, key }
    }

    /// Reads `Ctrl+Shift+Z`, `Space`, `Delete`, `Ctrl+,` and the like.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        // The key is after the last "+", unless the key is "+" itself.
        let (mods, key) = match text.rsplit_once('+') {
            Some((mods, "")) => (mods.strip_suffix('+').unwrap_or(mods), "+"),
            Some((mods, key)) => (mods, key),
            None => ("", text),
        };
        let mut modifiers = Modifiers::NONE;
        for part in mods.split('+').map(str::trim).filter(|p| !p.is_empty()) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "cmd" | "command" => modifiers = modifiers.plus(Modifiers::COMMAND),
                "shift" => modifiers = modifiers.plus(Modifiers::SHIFT),
                "alt" | "option" => modifiers = modifiers.plus(Modifiers::ALT),
                _ => return None,
            }
        }
        let key = Key::from_name(key.trim()).or_else(|| Key::from_name(&capitalised(key.trim())))?;
        Some(Self { modifiers, key })
    }

    /// The shortcut as the keymap file and the menus write it.
    pub fn text(&self) -> String {
        let mut parts = Vec::new();
        if self.modifiers.command || self.modifiers.ctrl || self.modifiers.mac_cmd {
            parts.push("Ctrl".to_string());
        }
        if self.modifiers.shift {
            parts.push("Shift".to_string());
        }
        if self.modifiers.alt {
            parts.push("Alt".to_string());
        }
        parts.push(self.key.name().to_string());
        parts.join("+")
    }

    /// The key as menus and Settings show it: as [`Shortcut::text`] writes it, but on macOS with
    /// the system's signs (⌘C, ⇧⌘D, ⌥⌘M).
    pub fn display(&self) -> String {
        if !cfg!(target_os = "macos") {
            return self.text();
        }
        let mut text = String::new();
        if self.modifiers.ctrl && !self.modifiers.mac_cmd && !self.modifiers.command {
            text.push('⌃');
        }
        if self.modifiers.alt {
            text.push('⌥');
        }
        if self.modifiers.shift {
            text.push('⇧');
        }
        if self.modifiers.command || self.modifiers.mac_cmd {
            text.push('⌘');
        }
        // ⌘, and ⌘+ rather than ⌘Comma and ⌘Plus.
        text.push_str(self.key.symbol_or_name());
        text
    }

    fn modifier_count(&self) -> usize {
        usize::from(self.modifiers.command || self.modifiers.ctrl)
            + usize::from(self.modifiers.shift)
            + usize::from(self.modifiers.alt)
    }
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |c| c.to_ascii_uppercase().to_string() + &chars.as_str().to_ascii_lowercase())
}

/// Something a key can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub id: String,
    pub label: String,
    /// Where it shows in the Shortcuts list.
    pub group: &'static str,
}

/// Actions the window does itself or on the selection, with their labels.
const WINDOW_ACTIONS: [(&str, &str, &str); 70] = [
    ("file.new", "New project or open one", "File"),
    ("file.open", "Open a project", "File"),
    ("file.save", "Save the project", "File"),
    ("file.saveAs", "Save the project under another name", "File"),
    ("file.saveTemplate", "Save the project as a template", "File"),
    ("transport.toggle", "Play or stop", "Transport"),
    ("transport.playFromSelection", "Play from the start of the selected clip", "Transport"),
    ("view.toggle", "Switch between Session and Arrangement", "View"),
    ("view.session", "Show the Session view", "View"),
    ("view.arrangement", "Show the Arrangement view", "View"),
    ("settings.open", "Open or close Settings", "View"),
    ("view.detail", "Show or hide the detail area", "View"),
    ("view.editor", "Hide the clip editor, or show it again", "View"),
    ("view.mappingMode", "Map keys to clip slots, scenes and stop buttons, or stop mapping", "View"),
    ("view.browser", "Show or hide the browser", "View"),
    ("browser.search", "Search in the browser", "View"),
    ("view.sampleMap", "Show or hide the sample map in the main area", "View"),
    ("view.sampleMapWindow", "Open the sample map in a window of its own, or put it back", "View"),
    ("view.zoomIn", "Make the whole window larger", "View"),
    ("view.zoomOut", "Make the whole window smaller", "View"),
    ("view.zoomReset", "Make the whole window its normal size", "View"),
    ("view.waveformZoomIn", "Draw audio clips' waveforms larger, the sound unchanged", "View"),
    ("view.waveformZoomOut", "Draw audio clips' waveforms smaller, down to their own size", "View"),
    ("selection.copy", "Copy the selected clips, or the selected notes in the piano roll", "Edit"),
    ("selection.cut", "Cut the selected clips, or the selected notes in the piano roll", "Edit"),
    (
        "selection.paste",
        "Paste after the selected clip, or at the playhead on the selected track; notes in the piano roll",
        "Edit",
    ),
    ("selection.duplicate", "Duplicate the selected clips, or the selected notes in the piano roll", "Edit"),
    ("selection.delete", "Delete the selected clips or device, or the selected notes in the piano roll", "Edit"),
    ("selection.mute", "Mute or unmute the selected clips, or turn the device clicked last off or on", "Edit"),
    ("selection.rename", "Rename the selected clip", "Edit"),
    ("selection.selectAll", "Select every clip in the arrangement", "Edit"),
    ("selection.split", "Split the selected clip at the playhead", "Edit"),
    ("selection.consolidate", "Consolidate the selected clip into a new one", "Edit"),
    ("selection.group", "Group the selected tracks, or the picked devices into a device group", "Tracks"),
    ("selection.ungroup", "Take the picked device group of one branch apart", "Edit"),
    ("track.addAudio", "Add an audio track", "Tracks"),
    ("track.addMidi", "Add a MIDI track", "Tracks"),
    ("clip.insertMidi", "Insert an empty MIDI clip over the time selection", "Edit"),
    ("view.automation", "Show or hide every track's automation lanes", "View"),
    ("selection.launch", "Launch the selected clip slot", "Session"),
    ("selection.up", "Select the clip slot above", "Session"),
    ("selection.down", "Select the clip slot below", "Session"),
    ("selection.left", "Select the clip slot in the track to the left", "Session"),
    ("selection.right", "Select the clip slot in the track to the right", "Session"),
    ("pianoRoll.quantize", "Quantise the piano roll's selected notes, or all its notes", PIANO_ROLL),
    ("pianoRoll.selectAll", "Select every note in the piano roll", PIANO_ROLL),
    ("pianoRoll.deselect", "Clear the piano roll's note selection", PIANO_ROLL),
    ("pianoRoll.earlier", "Move the selected notes one grid step earlier", PIANO_ROLL),
    ("pianoRoll.later", "Move the selected notes one grid step later", PIANO_ROLL),
    ("pianoRoll.up", "Move the selected notes a semitone up", PIANO_ROLL),
    ("pianoRoll.down", "Move the selected notes a semitone down", PIANO_ROLL),
    ("pianoRoll.octaveUp", "Move the selected notes an octave up", PIANO_ROLL),
    ("pianoRoll.octaveDown", "Move the selected notes an octave down", PIANO_ROLL),
    ("pianoRoll.shorter", "Make the selected notes one grid step shorter", PIANO_ROLL),
    ("pianoRoll.longer", "Make the selected notes one grid step longer", PIANO_ROLL),
    ("pianoRoll.draw", "Switch the piano roll between the Select and Draw tools", PIANO_ROLL),
    ("pianoRoll.tool1", "Choose the Select tool", PIANO_ROLL),
    ("pianoRoll.tool2", "Choose the Draw tool", PIANO_ROLL),
    ("pianoRoll.tool3", "Choose the Paint tool", PIANO_ROLL),
    ("pianoRoll.tool4", "Choose the Erase tool", PIANO_ROLL),
    ("pianoRoll.tool5", "Choose the Slice tool", PIANO_ROLL),
    ("marker.addAtPlayhead", "Add an arrangement marker at the playhead", "Edit"),
    ("pianoRoll.tool6", "Choose the Mute tool", PIANO_ROLL),
    ("pianoRoll.gridNarrow", "Make the piano roll's grid finer", PIANO_ROLL),
    ("pianoRoll.gridWiden", "Make the piano roll's grid coarser", PIANO_ROLL),
    ("pianoRoll.gridTriplet", "Turn the piano roll's triplet grid on or off", PIANO_ROLL),
    ("pianoRoll.gridSnap", "Turn snapping to the piano roll's grid on or off", PIANO_ROLL),
    ("pianoRoll.zoomSelection", "Zoom the piano roll to the selected notes, or the whole clip", PIANO_ROLL),
    ("pianoRoll.zoomClip", "Zoom the piano roll to the whole clip", PIANO_ROLL),
    ("help.about", "Show which version of Suhina this is", "Help"),
];

/// The Shortcuts list's group for the piano roll's own actions.
const PIANO_ROLL: &str = "Piano roll";

/// Whether an action works only while the piano roll has the keyboard (after a click in it). Its
/// keys are left for the rest of the window otherwise: Escape and the arrows mean other things
/// elsewhere. Quantise (Ctrl+U) means nothing elsewhere, so it works whenever the piano roll
/// shows a clip.
pub fn piano_roll_only(action: &str) -> bool {
    action.starts_with("pianoRoll.") && action != "pianoRoll.quantize"
}

/// Whether `action` works only on a selected clip slot in the Session view: the piano roll keys
/// never do (it edits arrangement clips), so the two may share keys.
pub fn slot_only(action: &str) -> bool {
    matches!(action, "selection.launch" | "selection.up" | "selection.down" | "selection.left" | "selection.right")
}

/// Whether `action` is left alone while the piano roll has the keyboard: the selected slot's keys,
/// and selecting every arrangement clip, whose key selects every note there instead.
pub fn not_in_piano_roll(action: &str) -> bool {
    slot_only(action) || action == "selection.selectAll"
}

/// Whether `a` and `b` can both run on the same key press, so one key for both is a conflict.
fn overlap(a: &str, b: &str) -> bool {
    !((piano_roll_only(a) && not_in_piano_roll(b)) || (not_in_piano_roll(a) && piano_roll_only(b)))
}

/// The detail area's own actions, listed with the window's, and their default keys.
const DETAIL_ACTIONS: [(&str, &str, &str); 3] = [
    ("view.devices", "Show the selected clip's track devices, or the clip again", "View"),
    (
        "view.araEditor",
        "Open the selected audio clip in its track's plug-in that edits clips (such as a pitch editor)",
        "View",
    ),
    ("view.plugin", "Open or close the selected track's plug-in editor", "View"),
];
const DETAIL_DEFAULTS: [(&str, &[&str]); 3] =
    [("view.devices", &["Ctrl+Shift+D"]), ("view.araEditor", &["Alt+M"]), ("view.plugin", &["P"])];

/// Every action a key can be bound to: the window's, then each command that needs no
/// parameters.
pub fn actions() -> Vec<Action> {
    let mut all: Vec<Action> = WINDOW_ACTIONS
        .iter()
        .chain(&DETAIL_ACTIONS)
        .map(|(id, label, group)| Action { id: (*id).into(), label: (*label).into(), group })
        .collect();
    let session = suhina_session::commands().iter().map(|c| (c.id, c.label, c.params.iter().all(|p| !p.required)));
    let projects =
        suhina_session::project_commands().iter().map(|c| (c.id, c.label, c.params.iter().all(|p| !p.required)));
    for (id, label, no_params) in session.chain(projects) {
        // Saving goes through the window's own actions, which ask for a name the first time.
        if no_params && id != "project.save" && !all.iter().any(|a| a.id == id) {
            all.push(Action { id: id.into(), label: label.into(), group: "Commands" });
        }
    }
    all
}

/// One of the four jobs the Mouse-first preset gives the mouse buttons, each a switch of its own
/// (Settings › Clip editor › Workflow, › Keymap presets in `docs/spec/ui-clip-editor.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseSwitch {
    /// In the Select tool, a press on an empty spot of the grid draws a note there.
    LeftDraws,
    /// A right-click on a note deletes it, and a right-button drag deletes the notes it passes.
    RightDeletes,
    /// A right-click with Shift held slices the notes under a short drag.
    ShiftRightSplits,
    /// The right button held with Alt plays the clip from the spot under the pointer.
    AltRightPlays,
}

impl MouseSwitch {
    pub const ALL: [Self; 4] = [Self::LeftDraws, Self::RightDeletes, Self::ShiftRightSplits, Self::AltRightPlays];

    /// The name the Settings row shows.
    pub fn name(self) -> &'static str {
        match self {
            Self::LeftDraws => "Left click draws",
            Self::RightDeletes => "Right click deletes a note",
            Self::ShiftRightSplits => "Shift+right click splits",
            Self::AltRightPlays => "Alt+right plays from the point",
        }
    }

    /// The key the keymap file keeps the switch under.
    pub fn json_key(self) -> &'static str {
        match self {
            Self::LeftDraws => "mouseLeftDraws",
            Self::RightDeletes => "mouseRightDeletes",
            Self::ShiftRightSplits => "mouseShiftRightSplits",
            Self::AltRightPlays => "mouseAltRightPlays",
        }
    }

    /// The switch `key` names; `None` when it is not one of the four (a hand-edited file).
    pub fn from_json_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|switch| switch.json_key() == key)
    }
}

/// The four mouse switches, kept with the keys in `keymap.json`. All off is what Select-first
/// and Draw-first leave; Mouse-first turns every one of them on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MouseSettings {
    pub left_draws: bool,
    pub right_deletes: bool,
    pub shift_right_splits: bool,
    pub alt_right_plays: bool,
}

impl MouseSettings {
    /// Every switch on, as the Mouse-first preset sets them.
    pub const ALL_ON: Self =
        Self { left_draws: true, right_deletes: true, shift_right_splits: true, alt_right_plays: true };

    /// Whether `switch` is on.
    pub fn get(self, switch: MouseSwitch) -> bool {
        match switch {
            MouseSwitch::LeftDraws => self.left_draws,
            MouseSwitch::RightDeletes => self.right_deletes,
            MouseSwitch::ShiftRightSplits => self.shift_right_splits,
            MouseSwitch::AltRightPlays => self.alt_right_plays,
        }
    }

    /// Turns `switch` on or off, leaving the other three as they are.
    pub fn set(&mut self, switch: MouseSwitch, on: bool) {
        match switch {
            MouseSwitch::LeftDraws => self.left_draws = on,
            MouseSwitch::RightDeletes => self.right_deletes = on,
            MouseSwitch::ShiftRightSplits => self.shift_right_splits = on,
            MouseSwitch::AltRightPlays => self.alt_right_plays = on,
        }
    }

    /// Whether every switch is on.
    pub fn all_on(self) -> bool {
        self == Self::ALL_ON
    }
}

/// The keys bound to each action, the piano roll's workflow settings (the four mouse switches
/// among them), and the mode the user chose: a preset, or Custom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    bindings: BTreeMap<String, Vec<Shortcut>>,
    preset: Preset,
    workflow: Workflow,
}

/// A set of keys for a way of working, chosen in Settings › Shortcuts (› Keymap presets in
/// `docs/spec/ui-clip-editor.md`). The names describe the habit, not another program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Preset {
    /// Suhina's own keys: duplicate is Ctrl+D, a right-click on a note does nothing.
    #[default]
    SelectFirst,
    /// Drawing notes first: duplicate is Ctrl+B (which the browser's Ctrl+B gives up) and a
    /// right-click on a note deletes it in the Draw and Paint tools.
    DrawFirst,
    /// Working with the mouse: Select-first's keys with every mouse switch on, so the left button
    /// draws in the Select tool and the right button deletes, splits and plays.
    MouseFirst,
    /// The workflow settings mixed one by one (› Workflow modes): what any setting changed by
    /// hand turns the mode into.
    Custom,
}

impl Preset {
    /// The three presets, each a fixed set of keys and workflow settings.
    pub const ALL: [Self; 3] = [Self::SelectFirst, Self::DrawFirst, Self::MouseFirst];
    /// The workflow modes the piano roll's toolbar offers: the three presets, then Custom.
    pub const MODES: [Self; 4] = [Self::SelectFirst, Self::DrawFirst, Self::MouseFirst, Self::Custom];

    /// The name the menu and the keymap file use.
    pub fn name(self) -> &'static str {
        match self {
            Self::SelectFirst => "Select-first",
            Self::DrawFirst => "Draw-first",
            Self::MouseFirst => "Mouse-first",
            Self::Custom => "Custom",
        }
    }

    /// The mode `name` names; `None` when it is not one of them (an edited file, a rename).
    pub fn from_name(name: &str) -> Option<Self> {
        Self::MODES.into_iter().find(|p| p.name().eq_ignore_ascii_case(name.trim()))
    }

    /// What the preset says beyond the default keys, in its own words: the Settings row shows it.
    pub fn note(self) -> &'static str {
        match self {
            Self::SelectFirst => "Duplicate is Ctrl+D; a right-click on a note does nothing",
            Self::DrawFirst => "Duplicate is Ctrl+B; in Draw and Paint a right-click on a note deletes it",
            Self::MouseFirst => "Every mouse switch is on; the left button draws in Select",
            Self::Custom => "Each behaviour set on its own, in the toolbar's menu or Settings › General",
        }
    }

    /// Whether a right-click on a note deletes it in the Draw and Paint tools.
    pub fn right_click_deletes(self) -> bool {
        matches!(self, Self::DrawFirst | Self::MouseFirst)
    }

    /// The key the preset gives the duplicate action, as the Shortcuts list shows it.
    pub fn duplicate_key(self) -> &'static str {
        match self {
            Self::SelectFirst | Self::MouseFirst | Self::Custom => "Ctrl+D",
            Self::DrawFirst => "Ctrl+B",
        }
    }

    /// The mouse switches the preset sets: all four with Mouse-first, none with the others.
    pub fn mouse(self) -> MouseSettings {
        if self == Self::MouseFirst { MouseSettings::ALL_ON } else { MouseSettings::default() }
    }

    /// The workflow settings the preset fills in; `None` for Custom, which keeps what it has.
    pub fn workflow(self) -> Option<Workflow> {
        match self {
            Self::SelectFirst => Some(Workflow::SELECT_FIRST),
            Self::DrawFirst => Some(Workflow::DRAW_FIRST),
            Self::MouseFirst => Some(Workflow::MOUSE_FIRST),
            Self::Custom => None,
        }
    }
}

/// The keymap file's entry for the preset.
const PRESET_KEY: &str = "preset";

/// The duplicate key of Draw-first, and the browser's key otherwise.
const CTRL_B: Shortcut = Shortcut::new(Modifiers::COMMAND, Key::B);

const DEFAULTS: [(&str, &[&str]); 71] = [
    ("file.new", &["Ctrl+N"]),
    ("track.addAudio", &["Ctrl+T"]),
    ("track.addMidi", &["Ctrl+Shift+T"]),
    ("clip.insertMidi", &["Ctrl+Shift+M"]),
    ("view.automation", &["A"]),
    ("file.open", &["Ctrl+O"]),
    ("file.save", &["Ctrl+S"]),
    ("file.saveAs", &["Ctrl+Shift+S"]),
    ("transport.toggle", &["Space"]),
    ("transport.playFromSelection", &["Shift+Space"]),
    ("view.toggle", &["Tab"]),
    ("view.detail", &["Shift+Tab"]),
    ("view.editor", &["Ctrl+Shift+E"]),
    ("view.mappingMode", &["Ctrl+M"]),
    ("view.browser", &["Ctrl+B"]),
    ("browser.search", &["Ctrl+F"]),
    ("view.sampleMap", &["Ctrl+Alt+Shift+M"]),
    ("view.sampleMapWindow", &["Ctrl+Alt+M"]),
    ("settings.open", &["Ctrl+Comma"]),
    ("view.zoomIn", &["Ctrl+Plus", "Ctrl+Equals"]),
    ("view.zoomOut", &["Ctrl+Minus"]),
    ("view.zoomReset", &["Ctrl+0"]),
    ("view.waveformZoomIn", &["Ctrl+Alt+Up"]),
    ("view.waveformZoomOut", &["Ctrl+Alt+Down"]),
    ("edit.undo", &["Ctrl+Z"]),
    ("edit.redo", &["Ctrl+Shift+Z", "Ctrl+Y"]),
    ("selection.copy", &["Ctrl+C"]),
    ("selection.cut", &["Ctrl+X"]),
    ("selection.paste", &["Ctrl+V"]),
    ("selection.duplicate", &["Ctrl+D"]),
    ("selection.delete", &["Delete", "Backspace"]),
    ("selection.mute", &["0"]),
    ("selection.rename", &["Ctrl+R"]),
    ("selection.selectAll", &["Ctrl+A"]),
    ("selection.split", &["Ctrl+E"]),
    ("selection.consolidate", &["Ctrl+J"]),
    ("selection.group", &["Ctrl+G"]),
    ("selection.ungroup", &["Ctrl+Shift+G"]),
    ("selection.launch", &["Enter"]),
    ("selection.up", &["Up"]),
    ("selection.down", &["Down"]),
    ("selection.left", &["Left"]),
    ("selection.right", &["Right"]),
    ("pianoRoll.quantize", &["Ctrl+U"]),
    ("marker.addAtPlayhead", &["M"]),
    ("transport.setLoopEnabled", &["Ctrl+L"]),
    ("project.next", &["Ctrl+Tab"]),
    ("project.previous", &["Ctrl+Shift+Tab"]),
    ("pianoRoll.selectAll", &["Ctrl+A"]),
    ("pianoRoll.deselect", &["Escape"]),
    ("pianoRoll.earlier", &["Left"]),
    ("pianoRoll.later", &["Right"]),
    ("pianoRoll.up", &["Up"]),
    ("pianoRoll.down", &["Down"]),
    ("pianoRoll.octaveUp", &["Shift+Up"]),
    ("pianoRoll.octaveDown", &["Shift+Down"]),
    ("pianoRoll.shorter", &["Shift+Left"]),
    ("pianoRoll.longer", &["Shift+Right"]),
    ("pianoRoll.draw", &["B"]),
    ("pianoRoll.tool1", &["1"]),
    ("pianoRoll.tool2", &["2"]),
    ("pianoRoll.tool3", &["3"]),
    ("pianoRoll.tool4", &["4"]),
    ("pianoRoll.tool5", &["5"]),
    ("pianoRoll.tool6", &["6"]),
    ("pianoRoll.gridNarrow", &["Ctrl+1"]),
    ("pianoRoll.gridWiden", &["Ctrl+2"]),
    ("pianoRoll.gridTriplet", &["Ctrl+3"]),
    ("pianoRoll.gridSnap", &["Ctrl+4"]),
    ("pianoRoll.zoomSelection", &["Z"]),
    ("pianoRoll.zoomClip", &["Shift+Z"]),
];

impl Default for Keymap {
    fn default() -> Self {
        Self { bindings: bindings_for(&DEFAULTS), preset: Preset::default(), workflow: Workflow::default() }
    }
}

/// The keys of `defaults` and of the detail area's actions, read from their text.
fn bindings_for(defaults: &[(&str, &[&str])]) -> BTreeMap<String, Vec<Shortcut>> {
    defaults
        .iter()
        .chain(&DETAIL_DEFAULTS)
        .map(|(id, keys)| ((*id).to_string(), keys.iter().filter_map(|k| Shortcut::parse(k)).collect()))
        .collect()
}

impl Keymap {
    /// Reads a keymap file over the defaults: an action listed there takes the keys listed
    /// (an empty list unbinds it), `preset` names the mode, the four `mouse*` entries are the
    /// mouse switches of earlier files and the `workflow*` entries the workflow settings. Unknown
    /// actions, keys, presets and entries are skipped with a warning; an unknown preset leaves the
    /// default one, and a setting the file leaves out is the preset's. A file whose settings are
    /// not the ones its preset fills in is Custom.
    pub fn from_json(text: &str) -> (Self, Vec<String>) {
        let mut keymap = Self::default();
        let mut warnings = Vec::new();
        let Ok(Value::Object(map)) = serde_json::from_str::<Value>(text) else {
            return (keymap, vec!["the keymap file is not a JSON object; using the default keys".into()]);
        };
        if let Some(named) = map.get(PRESET_KEY) {
            match named.as_str().and_then(Preset::from_name) {
                Some(preset) => keymap.preset = preset,
                None => warnings.push(format!(
                    "keymap: {} is not a preset the program knows; using {}",
                    named,
                    Preset::default().name()
                )),
            }
        }
        keymap.workflow = keymap.preset.workflow().unwrap_or_default();
        let known = actions();
        for (id, keys) in &map {
            if id == PRESET_KEY || Setting::is_json_key(id) {
                continue;
            }
            if let Some(switch) = MouseSwitch::from_json_key(id) {
                match keys.as_bool() {
                    Some(on) => keymap.workflow.set_mouse(switch, on),
                    None => warnings.push(format!("keymap: {keys} for {id} is not true or false")),
                }
                continue;
            }
            if !known.iter().any(|a| a.id == *id) {
                if id.starts_with("mouse") {
                    warnings.push(format!("keymap: {id} is not a mouse switch"));
                } else {
                    warnings.push(format!("keymap: {id} is not an action"));
                }
                continue;
            }
            let texts: Vec<&str> = match keys {
                Value::String(one) => vec![one.as_str()],
                Value::Array(list) => list.iter().filter_map(Value::as_str).collect(),
                _ => {
                    warnings.push(format!("keymap: {id} needs a key or a list of keys"));
                    continue;
                }
            };
            let mut parsed = Vec::new();
            for text in texts {
                match Shortcut::parse(text) {
                    Some(shortcut) => parsed.push(shortcut),
                    None => warnings.push(format!("keymap: {text:?} for {id} is not a key")),
                }
            }
            keymap.bindings.insert(id.clone(), parsed);
        }
        keymap.workflow.read_json(&map, &mut warnings);
        // The duplicate key is the one bound, whatever the file's setting says; a file that binds
        // no key to it takes the setting's.
        if map.contains_key("selection.duplicate") {
            keymap.workflow.duplicate_ctrl_b = keymap.keys("selection.duplicate").contains(&CTRL_B);
        } else {
            keymap.bind_duplicate(keymap.workflow.duplicate_ctrl_b);
        }
        if keymap.preset.workflow().is_some_and(|w| w != keymap.workflow) {
            keymap.preset = Preset::Custom;
        }
        (keymap, warnings)
    }

    /// Every binding, the four mouse switches and the preset, as the keymap file keeps them.
    pub fn to_json(&self) -> String {
        let mut map: serde_json::Map<String, Value> = self
            .bindings
            .iter()
            .map(|(id, keys)| (id.clone(), json!(keys.iter().map(Shortcut::text).collect::<Vec<_>>())))
            .collect();
        map.insert(PRESET_KEY.to_string(), json!(self.preset.name()));
        let mouse = self.mouse();
        for switch in MouseSwitch::ALL {
            map.insert(switch.json_key().to_string(), json!(mouse.get(switch)));
        }
        self.workflow.write_json(&mut map);
        serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_default()
    }

    /// The preset the keys came from.
    pub fn preset(&self) -> Preset {
        self.preset
    }

    /// The four mouse switches, as the workflow settings set them now.
    pub fn mouse(&self) -> MouseSettings {
        self.workflow.mouse()
    }

    /// Turns one mouse switch on or off; like any workflow setting changed by hand, it makes the
    /// mode Custom.
    pub fn set_mouse(&mut self, switch: MouseSwitch, on: bool) {
        self.workflow.set_mouse(switch, on);
        self.preset = Preset::Custom;
    }

    /// The piano roll's workflow settings, as they are now.
    pub fn workflow(&self) -> Workflow {
        self.workflow
    }

    /// Chooses choice `index` of one workflow setting: the mode becomes Custom, and the
    /// duplicate key's setting binds the key (› Workflow modes).
    pub fn set_workflow(&mut self, setting: Setting, index: usize) {
        let mut workflow = self.workflow;
        workflow.set(setting, index);
        if workflow == self.workflow {
            return;
        }
        if setting == Setting::Duplicate {
            self.bind_duplicate(workflow.duplicate_ctrl_b);
        }
        self.workflow = workflow;
        self.preset = Preset::Custom;
    }

    /// Duplicate on Ctrl+B, the browser then on Ctrl+Shift+B; or both on their defaults.
    fn bind_duplicate(&mut self, ctrl_b: bool) {
        if ctrl_b {
            let ctrl_shift = Modifiers::COMMAND.plus(Modifiers::SHIFT);
            self.bindings.insert("view.browser".to_string(), vec![Shortcut::new(ctrl_shift, Key::B)]);
            self.bindings.insert("selection.duplicate".to_string(), vec![CTRL_B]);
        } else {
            self.bindings.insert("view.browser".to_string(), vec![CTRL_B]);
            self.bindings.insert("selection.duplicate".to_string(), vec![Shortcut::new(Modifiers::COMMAND, Key::D)]);
        }
    }

    /// Puts the keys of `preset` back: every default, with the preset's own changes, and its
    /// workflow settings. Keys and settings changed by hand go back to the preset's; everything
    /// else keeps what it has. Custom changes nothing but the mode's name, so the settings stay
    /// as they are to be changed one by one.
    pub fn set_preset(&mut self, preset: Preset) {
        let Some(workflow) = preset.workflow() else {
            self.preset = Preset::Custom;
            return;
        };
        self.bindings = Self::default().bindings;
        // Draw-first: duplicate takes Ctrl+B; the browser keeps a key of its own, Ctrl+Shift+B.
        if workflow.duplicate_ctrl_b {
            self.bind_duplicate(true);
        }
        self.preset = preset;
        self.workflow = workflow;
    }

    pub fn keys(&self, action: &str) -> &[Shortcut] {
        self.bindings.get(action).map_or(&[], Vec::as_slice)
    }

    /// The first key of an action as menus show it, if it has one.
    pub fn label(&self, action: &str) -> Option<String> {
        self.keys(action).first().map(Shortcut::display)
    }

    /// The first key of an action as a keymap file writes it ("Ctrl+D"), on every system.
    pub fn binding_text(&self, action: &str) -> Option<String> {
        self.keys(action).first().map(Shortcut::text)
    }

    pub fn set(&mut self, action: &str, keys: Vec<Shortcut>) {
        self.bindings.insert(action.to_string(), keys);
    }

    /// Keys bound to more than one action, with those actions.
    pub fn conflicts(&self) -> Vec<(Shortcut, Vec<String>)> {
        let mut seen: Vec<(Shortcut, Vec<String>)> = Vec::new();
        for (id, keys) in &self.bindings {
            for key in keys {
                match seen.iter_mut().find(|(k, _)| k == key) {
                    Some((_, ids)) => ids.push(id.clone()),
                    None => seen.push((*key, vec![id.clone()])),
                }
            }
        }
        seen.retain(|(_, ids)| ids.iter().enumerate().any(|(i, a)| ids[i + 1..].iter().any(|b| overlap(a, b))));
        seen
    }

    /// The actions whose keys were pressed this frame, of those `wanted` lets through, taking
    /// their key presses so nothing else sees them; the keys of the other actions are left alone.
    /// Longer combinations are tried first, so Ctrl+Shift+Z is not read as Ctrl+Z.
    pub fn pressed(&self, input: &mut egui::InputState, wanted: impl Fn(&str) -> bool) -> Vec<String> {
        clipboard_keys(input);
        let mut all: Vec<(&String, &Shortcut)> = self
            .bindings
            .iter()
            .filter(|(id, _)| wanted(id))
            .flat_map(|(id, keys)| keys.iter().map(move |k| (id, k)))
            .collect();
        all.sort_by_key(|(_, k)| std::cmp::Reverse(k.modifier_count()));
        let mut fired = Vec::new();
        for (id, key) in all {
            let shortcut = egui::KeyboardShortcut::new(key.modifiers, key.key);
            let pressed = if key.modifiers.is_none() {
                input.consume_key(Modifiers::NONE, key.key)
            } else {
                input.consume_shortcut(&shortcut)
            };
            if pressed && !fired.contains(id) {
                fired.push(id.clone());
            }
        }
        fired
    }
}

/// A key written as the keymap writes it ("Ctrl+C"), as menus show it on this system.
pub fn shown(text: &str) -> String {
    Shortcut::parse(text).map_or_else(|| text.to_string(), |s| s.display())
}

/// The window reports Ctrl+X, Ctrl+C and Ctrl+V (Cmd on macOS) as cut, copy and paste events
/// rather than as keys (and Ctrl+V only when the system clipboard holds text). They become their
/// keys again here, so the keymap's actions for them fire (`selection.cut`, `.copy`, `.paste`).
fn clipboard_keys(input: &mut egui::InputState) {
    let modifiers = input.modifiers;
    for event in &mut input.events {
        let key = match event {
            egui::Event::Cut => Key::X,
            egui::Event::Copy => Key::C,
            egui::Event::Paste(_) => Key::V,
            _ => continue,
        };
        *event = egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: modifiers.plus(Modifiers::COMMAND),
        };
    }
}

#[cfg(test)]
#[path = "keymap_tests.rs"]
mod tests;
