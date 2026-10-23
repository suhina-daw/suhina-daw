// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Keyboard shortcuts and launch keys: what a key does in the window.

use super::*;

impl App {
    /// Keys mapped to slots, scenes and stop buttons, and the keys of the mapping mode, before
    /// the shortcuts see them: only while the Session view is shown and nothing else has the
    /// keyboard (`docs/spec/ui-session-view.md` › Launch keys). A key held when that ends, or
    /// when the window loses focus, is released.
    pub(super) fn launch_keys(&mut self, ui: &Ui, actions: &mut Vec<Action>) {
        let dialog = self.project_dialog.is_some()
            || self.save_prompt.is_some()
            || self.export_dialog.is_some()
            || self.unsaved_prompt.is_some()
            || self.ara_prompt.is_some()
            || self.missing.open
            || !self.agent_asks.is_empty();
        let session_view = self.shown == View::Session && !self.sample_map.view.large;
        // A clicked slot or button keeps the focus; only a text field takes the keys away.
        let focused = session_view && !dialog && !self.settings_open && !ui.ctx().text_edit_focused();
        let lost = ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::WindowFocused(false))));
        let Some(project) = self.projects.active().and_then(|id| self.projects.get(id)).map(Session::project) else {
            return;
        };
        let keys = &mut self.view.launch_keys;
        if !focused || lost {
            keys.release_all(project, actions);
        }
        if !session_view && keys.mode.on {
            keys.toggle_mode();
        }
        if !focused {
            return;
        }
        let events = ui.input(|i| crate::session::launch_keys::key_events(&i.events));
        let mut taken = Vec::new();
        for (index, event) in events {
            if keys.handle(project, &event, actions) == crate::session::launch_keys::Outcome::Taken {
                taken.push(index);
            }
        }
        if !taken.is_empty() {
            ui.input_mut(|i| {
                let mut index = 0;
                i.events.retain(|_| {
                    let keep = !taken.contains(&index);
                    index += 1;
                    keep
                });
            });
        }
    }

    /// Turns the mapping mode on (showing the Session view) or off.
    pub(super) fn toggle_mapping_mode(&mut self) {
        let keys = &mut self.view.launch_keys;
        keys.toggle_mode();
        if keys.mode.on {
            self.shown = View::Session;
            self.close_large_map();
            self.status = Status::Hint(
                "Mapping keys: click a slot, a scene or a stop button, then press a key. Delete removes, Escape leaves"
                    .into(),
            );
        } else {
            self.status = Status::Hint("Mapping keys off".into());
        }
    }

    /// Whether a dialog is open that the keymap's actions wait for: their keys, in either
    /// window, and the menu bar's items do nothing meanwhile.
    pub(super) fn dialog_holds_actions(&self) -> bool {
        self.project_dialog.is_some()
            || self.save_prompt.is_some()
            || self.export_dialog.is_some()
            || self.unsaved_prompt.is_some()
            || self.ara_prompt.is_some()
            || self.missing.open
            || self.about_open
            || !self.agent_asks.is_empty()
            || self.updates.open
    }

    /// Runs the actions whose keys were pressed (the keymap), unless a text field has the
    /// keyboard or Settings is waiting for a key to bind.
    pub(super) fn shortcuts(&mut self, ui: &Ui, actions: &mut Vec<Action>) {
        // A prompt waiting on a stored edit (the ARA one) included: a key that changed the
        // project meanwhile would leave that edit naming what is gone.
        if ui.ctx().egui_wants_keyboard_input()
            || self.dialog_holds_actions()
            || (self.settings_open && self.page.capture.is_some())
            || widgets::value_active(ui.ctx())
        {
            return;
        }
        // The piano roll's own keys only while it has the keyboard; the selected slot's keys
        // only while it does not, as the two share keys.
        let notes = self.piano_roll.focused;
        let pressed = ui.input_mut(|i| {
            self.keymap.pressed(i, |id| {
                if notes {
                    !crate::settings::keymap::not_in_piano_roll(id)
                } else {
                    !crate::settings::keymap::piano_roll_only(id)
                }
            })
        });
        for action in pressed {
            self.do_action(&action, actions);
        }
    }
}

/// Keys are the shortcuts' (Tab switches the view, the arrows move the selection): egui's own
/// keyboard focus, which Tab and the arrows move from widget to widget, is kept to text fields.
/// A button that held it would also make every shortcut wait, as if the user were typing.
pub(super) fn keys_for_shortcuts(ctx: &egui::Context) {
    let focused = ctx.memory(|m| m.focused());
    let typing = focused.is_some_and(|id| egui::text_edit::TextEditState::load(ctx, id).is_some());
    if !typing {
        ctx.memory_mut(|m| {
            m.move_focus(egui::FocusDirection::None);
            if let Some(id) = focused {
                m.surrender_focus(id);
            }
        });
    }
}
