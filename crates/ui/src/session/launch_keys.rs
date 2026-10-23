// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Computer keys that launch slots, scenes and stop buttons, and the mapping mode that sets
//! them (`docs/spec/ui-session-view.md` › Launch keys). A key down is the press and a key up the
//! release, and both run the command the mouse would ([`suhina_session::mapped_command`]). A
//! mapped key wins over a shortcut while the Session view is shown and no text field has the
//! keyboard; the window decides that and calls [`LaunchKeys::handle`] only then.

use egui::{Align2, Event, Key, Painter, Rect, StrokeKind};
use serde_json::json;
use suhina_engine::model::{MappingSource, MappingTarget, Project};

use crate::kit::theme::{self, PAD};
use crate::kit::widgets::{self, Action};

/// The mapping mode: on or off, and the control picked to map a key to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MapMode {
    pub on: bool,
    pub selected: Option<MappingTarget>,
}

/// What the window keeps for launch keys between frames: the mapping mode and the keys held
/// down, so each gets its release even when the window loses the keyboard.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchKeys {
    pub mode: MapMode,
    held: Vec<MappingSource>,
}

/// One key event, as [`LaunchKeys::handle`] needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    pub pressed: bool,
    pub repeat: bool,
    /// Ctrl, Alt, Shift or Cmd was down: a shortcut, never a launch key.
    pub modifiers: bool,
}

/// What [`LaunchKeys::handle`] did with an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Not a launch key: the shortcuts see it.
    Pass,
    /// Taken: the shortcuts must not see it.
    Taken,
}

impl LaunchKeys {
    /// Turns mapping on or off; off forgets the picked control.
    pub fn toggle_mode(&mut self) {
        self.mode.on = !self.mode.on;
        self.mode.selected = None;
    }

    /// Picks `target` in the mapping mode, or drops it when it was picked already.
    pub fn pick(&mut self, target: MappingTarget) {
        self.mode.selected = if self.mode.selected == Some(target) { None } else { Some(target) };
    }

    /// Releases every held key: the window lost the keyboard or left the Session view, and no
    /// key up will come. A gate or repeat clip stops as if its key had come up.
    pub fn release_all(&mut self, project: &Project, actions: &mut Vec<Action>) {
        for source in std::mem::take(&mut self.held) {
            if let Some((id, params)) = suhina_session::mapped_command(project, &source, false) {
                actions.push(Action::new(id, params));
            }
        }
    }

    /// Handles one key event while the Session view has the keyboard. In the mapping mode a key
    /// down maps the key to the picked control (Delete and Backspace remove its mappings, Escape
    /// leaves the mode); otherwise a mapped key down runs its press and its key up its release.
    /// Repeats of a held key are taken and do nothing.
    pub fn handle(&mut self, project: &Project, event: &KeyEvent, actions: &mut Vec<Action>) -> Outcome {
        let source = MappingSource::key(event.key.name());
        if !event.pressed {
            let Some(source) = source else { return Outcome::Pass };
            let Some(at) = self.held.iter().position(|s| *s == source) else { return Outcome::Pass };
            self.held.remove(at);
            if let Some((id, params)) = suhina_session::mapped_command(project, &source, false) {
                actions.push(Action::new(id, params));
            }
            return Outcome::Taken;
        }
        if event.modifiers {
            return Outcome::Pass;
        }
        if self.mode.on {
            return self.learn(event, source, actions);
        }
        let Some(source) = source else { return Outcome::Pass };
        if self.held.contains(&source) {
            return Outcome::Taken;
        }
        match suhina_session::mapped_command(project, &source, true) {
            Some((id, params)) => {
                actions.push(Action::new(id, params));
                self.held.push(source);
                Outcome::Taken
            }
            None => Outcome::Pass,
        }
    }

    fn learn(&mut self, event: &KeyEvent, source: Option<MappingSource>, actions: &mut Vec<Action>) -> Outcome {
        if event.key == Key::Escape {
            self.toggle_mode();
            return Outcome::Taken;
        }
        let Some(target) = self.mode.selected else { return Outcome::Pass };
        if event.repeat {
            return Outcome::Taken;
        }
        let mut params = target_params(target);
        if matches!(event.key, Key::Delete | Key::Backspace) {
            actions.push(Action::new("launcher.removeMapping", params));
        } else if let Some(source) = source {
            params["source"] = json!(source.name());
            actions.push(Action::new("launcher.addMapping", params));
        } else {
            return Outcome::Pass;
        }
        Outcome::Taken
    }
}

/// The `target`, `track` and `scene` parameters of the mapping commands for `target`.
pub fn target_params(target: MappingTarget) -> serde_json::Value {
    let mut params = json!({ "target": target.kind() });
    if let Some(track) = target.track() {
        params["track"] = json!(track.0);
    }
    if let Some(scene) = target.scene() {
        params["scene"] = json!(scene + 1);
    }
    params
}

/// The key events in `events`, in order, with their place in the list.
pub fn key_events(events: &[Event]) -> Vec<(usize, KeyEvent)> {
    events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| match event {
            Event::Key { key, pressed, repeat, modifiers, .. } => {
                Some((index, KeyEvent { key: *key, pressed: *pressed, repeat: *repeat, modifiers: modifiers.any() }))
            }
            _ => None,
        })
        .collect()
}

/// Draws the mapping mode over a control: a tint, an outline when it is picked, and the keys
/// mapped to it in small text at its right edge, `inset` points in from it (a scene keeps its
/// number visible).
pub fn overlay(painter: &Painter, rect: Rect, label: Option<&str>, picked: bool, inset: f32) {
    painter.rect_filled(rect, 0.0, theme::accent().gamma_multiply(if picked { 0.30 } else { 0.12 }));
    if picked {
        painter.rect_stroke(rect.shrink(0.5), 0.0, egui::Stroke::new(1.5, theme::accent()), StrokeKind::Inside);
    }
    if let Some(label) = label {
        let font = theme::mono_font(theme::TEXT_SMALL);
        let width = (rect.width() * 0.6).max(0.0);
        let right = rect.right() - PAD - inset;
        let area = Rect::from_min_max(egui::pos2(right - width, rect.top()), egui::pos2(right, rect.bottom()));
        widgets::text_in(painter, area, Align2::RIGHT_CENTER, label, font, theme::text_primary());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use suhina_engine::model::{
        Clip, ContentId, LaunchMapping, LaunchMode, LaunchSettings, LauncherColumn, Scene, Slot, TrackId,
    };

    /// One column of one slot, its clip in `mode`, and Q mapped to it.
    fn project(mode: LaunchMode) -> Project {
        let mut project = Project::default();
        let clip =
            Clip { launch: LaunchSettings { mode, ..LaunchSettings::default() }, ..Clip::new(ContentId(1), 0.0, 1.0) };
        project.launcher.scenes.push(Scene::default());
        project
            .launcher
            .columns
            .push(LauncherColumn { track: TrackId(1), slots: vec![Slot { clip: Some(clip), has_stop: true }] });
        project.launcher.mappings.push(LaunchMapping {
            source: MappingSource::key("Q").unwrap(),
            target: MappingTarget::Slot { track: TrackId(1), scene: 0 },
        });
        project
    }

    fn key(key: Key, pressed: bool) -> KeyEvent {
        KeyEvent { key, pressed, repeat: false, modifiers: false }
    }

    fn ids(actions: &[Action]) -> Vec<&str> {
        actions.iter().map(|a| a.id).collect()
    }

    #[test]
    fn a_mapped_key_presses_and_releases_and_ignores_repeats() {
        let project = project(LaunchMode::Gate);
        let mut keys = LaunchKeys::default();
        let mut actions = Vec::new();
        assert_eq!(keys.handle(&project, &key(Key::Q, true), &mut actions), Outcome::Taken);
        let repeat = KeyEvent { repeat: true, ..key(Key::Q, true) };
        assert_eq!(keys.handle(&project, &repeat, &mut actions), Outcome::Taken);
        assert_eq!(keys.handle(&project, &key(Key::Q, false), &mut actions), Outcome::Taken);
        assert_eq!(ids(&actions), ["clip.launch", "clip.release"]);
        assert_eq!(actions[0].params, json!({ "track": 1, "scene": 1 }));
        // Unmapped keys and keys with a modifier are the shortcuts'.
        assert_eq!(keys.handle(&project, &key(Key::W, true), &mut actions), Outcome::Pass);
        let ctrl = KeyEvent { modifiers: true, ..key(Key::Q, true) };
        assert_eq!(keys.handle(&project, &ctrl, &mut actions), Outcome::Pass);
        assert_eq!(keys.handle(&project, &key(Key::W, false), &mut actions), Outcome::Pass);
        assert_eq!(actions.len(), 2);
    }

    #[test]
    fn a_held_key_is_released_when_the_window_loses_the_keyboard() {
        let project = project(LaunchMode::Gate);
        let mut keys = LaunchKeys::default();
        let mut actions = Vec::new();
        keys.handle(&project, &key(Key::Q, true), &mut actions);
        keys.release_all(&project, &mut actions);
        assert_eq!(ids(&actions), ["clip.launch", "clip.release"]);
        // The key up that comes later finds nothing held.
        assert_eq!(keys.handle(&project, &key(Key::Q, false), &mut actions), Outcome::Pass);
        assert_eq!(actions.len(), 2);
        // A trigger clip has no release to send.
        let project = super::tests::project(LaunchMode::Trigger);
        keys.handle(&project, &key(Key::Q, true), &mut actions);
        keys.release_all(&project, &mut actions);
        assert_eq!(ids(&actions), ["clip.launch", "clip.release", "clip.launch"]);
    }

    #[test]
    fn the_mapping_mode_maps_the_next_key_to_the_picked_control() {
        let project = project(LaunchMode::Trigger);
        let mut keys = LaunchKeys::default();
        let mut actions = Vec::new();
        keys.toggle_mode();
        // Nothing picked: keys go on to the shortcuts, and a mapped key does not launch.
        assert_eq!(keys.handle(&project, &key(Key::W, true), &mut actions), Outcome::Pass);
        assert_eq!(keys.handle(&project, &key(Key::Q, true), &mut actions), Outcome::Pass);
        assert!(actions.is_empty());
        keys.pick(MappingTarget::Scene { scene: 0 });
        assert_eq!(keys.handle(&project, &key(Key::W, true), &mut actions), Outcome::Taken);
        assert_eq!(keys.handle(&project, &key(Key::Delete, true), &mut actions), Outcome::Taken);
        assert_eq!(ids(&actions), ["launcher.addMapping", "launcher.removeMapping"]);
        assert_eq!(actions[0].params, json!({ "target": "scene", "scene": 1, "source": "key:W" }));
        assert_eq!(actions[1].params, json!({ "target": "scene", "scene": 1 }));
        // Picking it again drops it; Escape leaves the mode.
        keys.pick(MappingTarget::Scene { scene: 0 });
        assert_eq!(keys.mode.selected, None);
        keys.pick(MappingTarget::Stop { track: TrackId(1) });
        assert_eq!(keys.handle(&project, &key(Key::Escape, true), &mut actions), Outcome::Taken);
        assert_eq!(keys.mode, MapMode::default());
    }

    #[test]
    fn targets_give_the_commands_their_parameters() {
        assert_eq!(
            target_params(MappingTarget::Slot { track: TrackId(3), scene: 1 }),
            json!({ "target": "slot", "track": 3, "scene": 2 })
        );
        assert_eq!(target_params(MappingTarget::Stop { track: TrackId(3) }), json!({ "target": "stop", "track": 3 }));
    }
}
