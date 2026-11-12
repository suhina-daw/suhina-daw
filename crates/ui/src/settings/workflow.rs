// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll's workflow settings: every behaviour that differs between the keymap presets
//! (Select-first, Draw-first, Mouse-first) as a setting of its own, so a Custom mode can mix them
//! one by one (› Workflow modes in `docs/spec/ui-clip-editor.md`). The keymap keeps them, beside
//! the keys, in `keymap.json`.

use serde_json::{Map, Value, json};

use crate::settings::keymap::{Keymap, MouseSettings, MouseSwitch, Preset};

/// One behaviour of the piano roll that the workflow modes set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Setting {
    /// What a click on an empty spot of the grid does in the Select tool.
    EmptyClick,
    /// What a right-click on a note does.
    RightClick,
    /// A right-button drag with Shift held slices the notes it crosses.
    ShiftRight,
    /// The right button held with Alt plays the clip from the spot under the pointer.
    AltRight,
    /// The key that duplicates the selected notes (and clips).
    Duplicate,
    /// What a double-click on an empty spot of the grid does in the Select tool.
    DoubleClickEmpty,
    /// What a double-click on a note does in the Select tool.
    DoubleClickNote,
}

impl Setting {
    pub const ALL: [Self; 7] = [
        Self::EmptyClick,
        Self::RightClick,
        Self::ShiftRight,
        Self::AltRight,
        Self::Duplicate,
        Self::DoubleClickEmpty,
        Self::DoubleClickNote,
    ];

    /// The name a Settings row and the toolbar's workflow menu show.
    pub fn name(self) -> &'static str {
        match self {
            Self::EmptyClick => "Click on empty grid",
            Self::RightClick => "Right click on a note",
            Self::ShiftRight => "Shift+right drag",
            Self::AltRight => "Alt+right button",
            Self::Duplicate => "Duplicate key",
            Self::DoubleClickEmpty => "Double-click on empty grid",
            Self::DoubleClickNote => "Double-click on a note",
        }
    }

    /// The gesture in a word or two, for the toolbar's workflow menu, where the setting's value
    /// stands beside it.
    pub fn short_name(self) -> &'static str {
        match self {
            Self::EmptyClick => "Click empty",
            Self::RightClick => "Right-click note",
            Self::ShiftRight => "Shift right-drag",
            Self::AltRight => "Alt right-hold",
            Self::Duplicate => "Duplicate",
            Self::DoubleClickEmpty => "Double-click empty",
            Self::DoubleClickNote => "Double-click note",
        }
    }

    /// The choices, in the order the setting's index counts them.
    pub fn choices(self) -> &'static [&'static str] {
        match self {
            Self::EmptyClick => &["Clears the selection", "Draws a note"],
            Self::RightClick => &["Does nothing", "Deletes in Draw and Paint", "Deletes in every tool"],
            Self::ShiftRight => &["Does nothing", "Splits notes"],
            Self::AltRight => &["Does nothing", "Plays from the point"],
            Self::Duplicate => &["Ctrl+D", "Ctrl+B"],
            Self::DoubleClickEmpty => &["Adds a note", "Does nothing"],
            Self::DoubleClickNote => &["Deletes it", "Does nothing"],
        }
    }

    /// The choices in a word or two, for the Settings row's segmented control.
    pub fn short_choices(self) -> &'static [&'static str] {
        match self {
            Self::EmptyClick => &["Selects", "Draws"],
            Self::RightClick => &["Nothing", "Draw, Paint", "Every tool"],
            Self::ShiftRight => &["Off", "Splits"],
            Self::AltRight => &["Off", "Plays"],
            Self::Duplicate => &["Ctrl+D", "Ctrl+B"],
            Self::DoubleClickEmpty => &["Adds", "Nothing"],
            Self::DoubleClickNote => &["Deletes", "Nothing"],
        }
    }

    /// What the setting is about, for the note under its Settings row.
    pub fn note(self) -> &'static str {
        match self {
            Self::EmptyClick => "In the Select tool: clear the selection, or draw a note there",
            Self::RightClick => "Delete the note under the pointer, in Draw and Paint or in every tool",
            Self::ShiftRight => "A right-button drag with Shift slices the notes it crosses",
            Self::AltRight => "The right button held with Alt plays the clip from the pointer",
            Self::Duplicate => "The key that duplicates; with Ctrl+B the browser takes Ctrl+Shift+B",
            Self::DoubleClickEmpty => "In the Select tool, on an empty spot of the grid",
            Self::DoubleClickNote => "In the Select tool, on a note or its edge",
        }
    }

    /// The key `keymap.json` keeps the setting under, and the words it writes for each choice.
    fn json_key(self) -> &'static str {
        match self {
            Self::EmptyClick => "workflowEmptyClick",
            Self::RightClick => "workflowRightClick",
            Self::ShiftRight => "workflowShiftRight",
            Self::AltRight => "workflowAltRight",
            Self::Duplicate => "workflowDuplicate",
            Self::DoubleClickEmpty => "workflowDoubleClickEmpty",
            Self::DoubleClickNote => "workflowDoubleClickNote",
        }
    }

    fn json_words(self) -> &'static [&'static str] {
        match self {
            Self::EmptyClick => &["selects", "draws"],
            Self::RightClick => &["nothing", "drawAndPaint", "everyTool"],
            Self::ShiftRight | Self::AltRight => &["nothing", "on"],
            Self::Duplicate => &["Ctrl+D", "Ctrl+B"],
            Self::DoubleClickEmpty => &["adds", "nothing"],
            Self::DoubleClickNote => &["deletes", "nothing"],
        }
    }

    /// Whether `key` is one the workflow settings keep in `keymap.json`.
    pub fn is_json_key(key: &str) -> bool {
        Self::ALL.iter().any(|s| s.json_key() == key)
    }
}

/// A workflow mode or one setting's choice, picked in the piano roll's toolbar menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Mode(Preset),
    Setting(Setting, usize),
}

impl Pick {
    /// Makes the pick in `keymap`: a preset fills in its settings, a setting makes the mode Custom.
    pub fn apply(self, keymap: &mut Keymap) {
        match self {
            Self::Mode(preset) => keymap.set_preset(preset),
            Self::Setting(setting, index) => keymap.set_workflow(setting, index),
        }
    }
}

/// What a right-click on a note does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RightClick {
    #[default]
    Nothing,
    /// Deletes the note in the Draw and Paint tools only.
    DeletesInDrawAndPaint,
    /// Deletes the note in every tool, and a right-button drag deletes the notes it passes.
    Deletes,
}

/// The piano roll's behaviours, one value per [`Setting`]. The default is Select-first's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Workflow {
    /// In the Select tool, a click on an empty spot draws a note (on) or clears the selection.
    pub empty_click_draws: bool,
    pub right_click: RightClick,
    pub shift_right_splits: bool,
    pub alt_right_plays: bool,
    /// The duplicate key is Ctrl+B (on) or Ctrl+D.
    pub duplicate_ctrl_b: bool,
    /// In the Select tool, a double-click on an empty spot adds a note.
    pub double_click_adds: bool,
    /// In the Select tool, a double-click on a note deletes it.
    pub double_click_deletes: bool,
}

impl Default for Workflow {
    fn default() -> Self {
        Self::SELECT_FIRST
    }
}

impl Workflow {
    /// Select-first: a click selects, the right button does nothing, Ctrl+D duplicates, and a
    /// double-click adds a note on an empty spot and deletes the note under it.
    pub const SELECT_FIRST: Self = Self {
        empty_click_draws: false,
        right_click: RightClick::Nothing,
        shift_right_splits: false,
        alt_right_plays: false,
        duplicate_ctrl_b: false,
        double_click_adds: true,
        double_click_deletes: true,
    };

    /// Draw-first: Select-first with Ctrl+B to duplicate and a right-click deleting in Draw and
    /// Paint.
    pub const DRAW_FIRST: Self =
        Self { right_click: RightClick::DeletesInDrawAndPaint, duplicate_ctrl_b: true, ..Self::SELECT_FIRST };

    /// Mouse-first: the left button draws in Select and the right button deletes, splits with
    /// Shift and plays with Alt.
    pub const MOUSE_FIRST: Self = Self {
        empty_click_draws: true,
        right_click: RightClick::Deletes,
        shift_right_splits: true,
        alt_right_plays: true,
        ..Self::SELECT_FIRST
    };

    /// The chosen choice of `setting`, as an index into [`Setting::choices`].
    pub fn get(&self, setting: Setting) -> usize {
        match setting {
            Setting::EmptyClick => usize::from(self.empty_click_draws),
            Setting::RightClick => match self.right_click {
                RightClick::Nothing => 0,
                RightClick::DeletesInDrawAndPaint => 1,
                RightClick::Deletes => 2,
            },
            Setting::ShiftRight => usize::from(self.shift_right_splits),
            Setting::AltRight => usize::from(self.alt_right_plays),
            Setting::Duplicate => usize::from(self.duplicate_ctrl_b),
            Setting::DoubleClickEmpty => usize::from(!self.double_click_adds),
            Setting::DoubleClickNote => usize::from(!self.double_click_deletes),
        }
    }

    /// Chooses choice `index` of `setting`; an index past the choices changes nothing.
    pub fn set(&mut self, setting: Setting, index: usize) {
        if index >= setting.choices().len() {
            return;
        }
        let on = index == 1;
        match setting {
            Setting::EmptyClick => self.empty_click_draws = on,
            Setting::RightClick => {
                self.right_click = match index {
                    0 => RightClick::Nothing,
                    1 => RightClick::DeletesInDrawAndPaint,
                    _ => RightClick::Deletes,
                }
            }
            Setting::ShiftRight => self.shift_right_splits = on,
            Setting::AltRight => self.alt_right_plays = on,
            Setting::Duplicate => self.duplicate_ctrl_b = on,
            Setting::DoubleClickEmpty => self.double_click_adds = !on,
            Setting::DoubleClickNote => self.double_click_deletes = !on,
        }
    }

    /// Whether a right-click on a note deletes it with `in_draw_or_paint` telling whether the
    /// Draw or Paint tool is chosen.
    pub fn right_click_deletes(&self, in_draw_or_paint: bool) -> bool {
        match self.right_click {
            RightClick::Nothing => false,
            RightClick::DeletesInDrawAndPaint => in_draw_or_paint,
            RightClick::Deletes => true,
        }
    }

    /// The four mouse switches of Mouse-first, as these settings set them.
    pub fn mouse(&self) -> MouseSettings {
        MouseSettings {
            left_draws: self.empty_click_draws,
            right_deletes: self.right_click == RightClick::Deletes,
            shift_right_splits: self.shift_right_splits,
            alt_right_plays: self.alt_right_plays,
        }
    }

    /// Turns one mouse switch on or off. Off, "right click deletes a note" goes back to deleting
    /// nothing only when it deleted in every tool, so Draw-first's delete in Draw and Paint stays.
    pub fn set_mouse(&mut self, switch: MouseSwitch, on: bool) {
        match switch {
            MouseSwitch::LeftDraws => self.empty_click_draws = on,
            MouseSwitch::RightDeletes if on => self.right_click = RightClick::Deletes,
            MouseSwitch::RightDeletes => {
                if self.right_click == RightClick::Deletes {
                    self.right_click = RightClick::Nothing;
                }
            }
            MouseSwitch::ShiftRightSplits => self.shift_right_splits = on,
            MouseSwitch::AltRightPlays => self.alt_right_plays = on,
        }
    }

    /// Writes every setting into `map`, as `keymap.json` keeps them.
    pub fn write_json(&self, map: &mut Map<String, Value>) {
        for setting in Setting::ALL {
            let word = setting.json_words().get(self.get(setting)).copied().unwrap_or("");
            map.insert(setting.json_key().to_string(), json!(word));
        }
    }

    /// Reads the settings `map` has over `self`; a value that is not one of a setting's words is
    /// left out, with a warning.
    pub fn read_json(&mut self, map: &Map<String, Value>, warnings: &mut Vec<String>) {
        for setting in Setting::ALL {
            let Some(value) = map.get(setting.json_key()) else { continue };
            let found = value
                .as_str()
                .and_then(|word| setting.json_words().iter().position(|w| w.eq_ignore_ascii_case(word.trim())));
            match found {
                Some(index) => self.set(setting, index),
                None => warnings.push(format!("keymap: {value} for {} is not one of its choices", setting.json_key())),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_setting_reads_back_what_was_set() {
        for setting in Setting::ALL {
            for index in 0..setting.choices().len() {
                let mut workflow = Workflow::default();
                workflow.set(setting, index);
                assert_eq!(workflow.get(setting), index, "{setting:?}");
                assert_eq!(setting.json_words().len(), setting.choices().len(), "{setting:?}");
                assert_eq!(setting.short_choices().len(), setting.choices().len(), "{setting:?}");
            }
            // An index past the choices changes nothing.
            let mut workflow = Workflow::MOUSE_FIRST;
            workflow.set(setting, 9);
            assert_eq!(workflow, Workflow::MOUSE_FIRST);
        }
    }

    #[test]
    fn the_settings_go_through_the_file_and_nonsense_is_warned_about() {
        let mut map = Map::new();
        let mut custom = Workflow::DRAW_FIRST;
        custom.set(Setting::DoubleClickNote, 1);
        custom.write_json(&mut map);
        let mut again = Workflow::default();
        let mut warnings = Vec::new();
        again.read_json(&map, &mut warnings);
        assert_eq!((again, warnings.len()), (custom, 0));
        map.insert("workflowRightClick".into(), json!("sometimes"));
        map.insert("workflowAltRight".into(), json!(true));
        let mut read = Workflow::default();
        read.read_json(&map, &mut warnings);
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert_eq!(read.right_click, RightClick::Nothing, "a bad value leaves the setting as it was");
    }

    #[test]
    fn a_right_click_deletes_where_the_setting_says() {
        assert!(!Workflow::SELECT_FIRST.right_click_deletes(true));
        assert!(Workflow::DRAW_FIRST.right_click_deletes(true));
        assert!(!Workflow::DRAW_FIRST.right_click_deletes(false));
        assert!(Workflow::MOUSE_FIRST.right_click_deletes(false));
    }
}
