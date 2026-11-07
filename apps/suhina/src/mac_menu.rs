// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The macOS menu bar (#650): the system's own bar carries the same menus as the window's bar
//! (`suhina_ui::app::menus`), after the application menu macOS expects (About, Services, Hide,
//! Quit). It is kept in step every frame: an item's label and whether it can be picked change in
//! place, and the bar is built again when the items themselves change (a project added to Open
//! recent). A pick runs the item as a click in the window's bar does. Items carry no keys of their
//! own, so a key still reaches the window's keymap once.

use std::sync::{Arc, Mutex};

use muda::{IsMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSAppearance, NSApplication};
use objc2_foundation::NSString;
use suhina_ui::app::menus::{self, Entry};

/// The installed menu bar, and which system item runs which of the window's items.
pub struct MacMenu {
    /// Kept alive: the bar is AppKit's only while this is.
    _menu: Menu,
    /// The menus it was built from, to tell a change of label from a change of items.
    built: Vec<menus::Menu>,
    /// Every item and submenu, in the order of `built`'s entries, to update in place.
    lines: Vec<Line>,
    items: Vec<(MenuId, String)>,
    /// The items picked and not taken yet: the menu's handler fills it from AppKit's thread.
    picked: Arc<Mutex<Vec<MenuId>>>,
    /// The appearance last given to the application: whether it is light.
    light: Option<bool>,
}

enum Line {
    Item(MenuItem),
    Submenu(Submenu),
}

/// Builds the bar from `menus` and hands it to the application. Called on the main thread once the
/// window is up; `None` when AppKit would not take it. A pick wakes the window (`ctx`), which does
/// not draw while idle, so the item runs at once.
pub fn install(menus: &[menus::Menu], ctx: eframe::egui::Context) -> Option<MacMenu> {
    let picked = Arc::new(Mutex::new(Vec::new()));
    let queue = Arc::clone(&picked);
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if let Ok(mut queue) = queue.lock() {
            queue.push(event.id);
        }
        ctx.request_repaint();
    }));
    build(menus, picked)
}

fn build(menus: &[menus::Menu], picked: Arc<Mutex<Vec<MenuId>>>) -> Option<MacMenu> {
    let menu = Menu::new();
    let app = Submenu::new("Suhina", true);
    app.append_items(&[
        &PredefinedMenuItem::about(None, None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::services(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::hide(None),
        &PredefinedMenuItem::hide_others(None),
        &PredefinedMenuItem::show_all(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::quit(None),
    ])
    .ok()?;
    menu.append(&app).ok()?;
    let (mut lines, mut items) = (Vec::new(), Vec::new());
    for bar_menu in menus {
        let submenu = Submenu::new(bar_menu.title, true);
        append(&submenu, &bar_menu.entries, &mut lines, &mut items)?;
        menu.append(&submenu).ok()?;
    }
    menu.init_for_nsapp();
    Some(MacMenu { _menu: menu, built: menus.to_vec(), lines, items, picked, light: None })
}

fn append(parent: &Submenu, entries: &[Entry], lines: &mut Vec<Line>, items: &mut Vec<(MenuId, String)>) -> Option<()> {
    for entry in entries {
        match entry {
            Entry::Separator => parent.append(&PredefinedMenuItem::separator()).ok()?,
            Entry::Item(item) => {
                let line = MenuItem::new(&item.label, item.enabled, None);
                parent.append(&line as &dyn IsMenuItem).ok()?;
                items.push((line.id().clone(), item.id.clone()));
                lines.push(Line::Item(line));
            }
            Entry::Submenu { label, enabled, entries } => {
                let submenu = Submenu::new(label, *enabled);
                lines.push(Line::Submenu(submenu.clone()));
                append(&submenu, entries, lines, items)?;
                parent.append(&submenu).ok()?;
            }
        }
    }
    Some(())
}

/// The ids and kinds of every line, which a change of label or enabled state leaves alone.
fn shape(menus: &[menus::Menu]) -> Vec<String> {
    fn lines(entries: &[Entry], out: &mut Vec<String>) {
        for entry in entries {
            match entry {
                Entry::Separator => out.push("-".into()),
                Entry::Item(item) => out.push(item.id.clone()),
                Entry::Submenu { entries, .. } => {
                    out.push(">".into());
                    lines(entries, out);
                    out.push("<".into());
                }
            }
        }
    }
    let mut out = Vec::new();
    for menu in menus {
        out.push(menu.title.to_string());
        lines(&menu.entries, &mut out);
    }
    out
}

/// Every item and submenu's label and enabled state, in the order `append` keeps them.
fn states(menus: &[menus::Menu]) -> Vec<(String, bool)> {
    fn lines(entries: &[Entry], out: &mut Vec<(String, bool)>) {
        for entry in entries {
            match entry {
                Entry::Separator => {}
                Entry::Item(item) => out.push((item.label.clone(), item.enabled)),
                Entry::Submenu { label, enabled, entries } => {
                    out.push((label.clone(), *enabled));
                    lines(entries, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    for menu in menus {
        lines(&menu.entries, &mut out);
    }
    out
}

impl MacMenu {
    /// Gives the application the light or dark appearance of the theme in use. The system's menus
    /// follow it, not the window's own appearance. The bar across the top of the screen is the
    /// system's and follows the desktop.
    pub fn follow_theme(&mut self, light: bool) {
        if self.light == Some(light) {
            return;
        }
        let Some(thread) = MainThreadMarker::new() else { return };
        let name = NSString::from_str(if light { "NSAppearanceNameAqua" } else { "NSAppearanceNameDarkAqua" });
        NSApplication::sharedApplication(thread).setAppearance(NSAppearance::appearanceNamed(&name).as_deref());
        self.light = Some(light);
    }

    /// Brings the bar in step with `menus`, the window's menus this frame.
    pub fn sync(&mut self, menus: &[menus::Menu]) {
        if self.built == menus {
            return;
        }
        if shape(&self.built) != shape(menus) {
            if let Some(rebuilt) = build(menus, Arc::clone(&self.picked)) {
                *self = rebuilt;
            }
            return;
        }
        let (old, new) = (states(&self.built), states(menus));
        for ((line, before), after) in self.lines.iter().zip(&old).zip(&new) {
            if before == after {
                continue;
            }
            match line {
                Line::Item(item) => {
                    item.set_text(&after.0);
                    item.set_enabled(after.1);
                }
                Line::Submenu(submenu) => {
                    submenu.set_text(&after.0);
                    submenu.set_enabled(after.1);
                }
            }
        }
        self.built = menus.to_vec();
    }

    /// The window's items picked from the bar since the last call, in order.
    pub fn take(&self) -> Vec<String> {
        let picked = self.picked.lock().map(|mut p| std::mem::take(&mut *p)).unwrap_or_default();
        picked
            .iter()
            .filter_map(|picked| self.items.iter().find(|(id, _)| id == picked).map(|(_, item)| item.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use suhina_ui::app::menus::Item;

    fn bar(label: &str, recent: usize) -> Vec<menus::Menu> {
        vec![menus::Menu {
            title: "File",
            entries: vec![
                Item::new("file.open", label).into(),
                Entry::Separator,
                Entry::Submenu {
                    label: "Open recent".into(),
                    enabled: recent > 0,
                    entries: (0..recent).map(|i| Item::new(format!("menu.recent.{i}"), "A").into()).collect(),
                },
            ],
        }]
    }

    #[test]
    fn a_new_label_keeps_the_items_and_a_new_item_does_not() {
        assert_eq!(shape(&bar("Open…", 1)), shape(&bar("Open", 1)));
        assert_ne!(shape(&bar("Open…", 1)), shape(&bar("Open…", 2)));
        assert_eq!(states(&bar("Open", 0)), vec![("Open".to_string(), true), ("Open recent".to_string(), false)]);
    }
}
