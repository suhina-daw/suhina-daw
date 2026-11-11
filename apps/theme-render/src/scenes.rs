// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The scenes a theme is drawn in: a table of the project each opens and how it is driven, through
//! labels as the window tests drive the window. A scene whose control a theme hid says so instead
//! of panicking.
//!
//! The fixed scenes are [`FIXED`]; [`scenes`] adds the devices: every effect and MIDI effect in a
//! row, each instrument, and each device with an expanded panel, expanded.

use std::sync::Arc;

use egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use suhina_device_api::Role;
use suhina_device_registry::DEVICES;
use suhina_ui::App;

type Window = Harness<'static, App>;
/// What is done in a scene: `Err` names a control it could not find.
type Step = fn(&mut Window) -> Result<(), String>;
type Drive = Arc<dyn Fn(&mut Window) -> Result<(), String> + Send + Sync>;
type Open = Arc<dyn Fn() -> Result<App, String> + Send + Sync>;

/// A scene: its name (the PNGs' names), the window's width in points, the app it opens (`Err` if
/// a device it adds is refused), and what is done in it.
#[derive(Clone)]
pub struct Scene {
    pub name: String,
    pub width: f32,
    pub app: Open,
    pub drive: Drive,
}

/// The scenes that are the same whatever devices there are: `(name, what is done)`, each on the
/// demo project.
pub const FIXED: &[(&str, Step)] = &[
    ("launcher", settle),
    ("launcher-hover", launcher_hover),
    ("launcher-selected", |h| click(h, "Track Drums")),
    ("arrangement", arrangement),
    ("piano-roll", piano_roll),
    ("menu", |h| click(h, "File")),
    ("settings", settings),
    ("tooltip", tooltip),
];

/// The scenes with the views scrolled down and along, on a project with more tracks and scenes
/// than fit: what scrolls must never show through the headers, rulers and fixed rows that stay
/// where they are (#1233).
pub const SCROLLED: &[(&str, Step)] =
    &[("launcher-scrolled", launcher_scrolled), ("arrangement-scrolled", arrangement_scrolled)];

/// Wide enough for every effect and MIDI effect in one row.
const DEVICE_ROW: f32 = 5400.0;

/// Every scene, in the order they are rendered.
pub fn scenes() -> Vec<Scene> {
    let mut scenes: Vec<Scene> = FIXED
        .iter()
        .map(|&(name, drive)| Scene {
            name: name.to_owned(),
            width: crate::SIZE.0,
            app: Arc::new(|| Ok(crate::demo_app())),
            drive: Arc::new(drive),
        })
        .collect();
    scenes.extend(SCROLLED.iter().map(|&(name, drive)| Scene {
        name: name.to_owned(),
        width: crate::SIZE.0,
        app: Arc::new(crowded),
        drive: Arc::new(drive),
    }));
    let row: Vec<&'static str> = DEVICES.iter().filter(|d| d.role != Role::Instrument).map(|d| &*d.id).collect();
    scenes.push(Scene {
        name: "devices".into(),
        width: DEVICE_ROW,
        app: Arc::new(move || on_keys(&row)),
        drive: Arc::new(|h| click(h, "Track Keys")),
    });
    for device in DEVICES {
        let id: &'static str = &device.id;
        let name: &'static str = &device.name;
        let expands = suhina_device_registry::panel(id).is_some_and(|p| p.expanded.is_some());
        if device.role != Role::Instrument && !expands {
            continue;
        }
        scenes.push(Scene {
            name: format!("{}-{id}", if expands { "expanded" } else { "instrument" }),
            width: crate::SIZE.0,
            app: Arc::new(move || on_keys(&[id])),
            drive: Arc::new(move |h| {
                click(h, "Track Keys")?;
                if expands { click(h, &format!("Expand {name}")) } else { Ok(()) }
            }),
        });
    }
    scenes
}

/// The scenes to draw: all of them, or only the one called `only`.
pub fn select(only: Option<&str>) -> Vec<Scene> {
    scenes().into_iter().filter(|s| only.is_none_or(|name| s.name == name)).collect()
}

/// The scene called `name`.
pub fn find(name: &str) -> Option<Scene> {
    scenes().into_iter().find(|s| s.name == name)
}

/// Runs until nothing repaints, or for the most frames a scene may take.
fn settle(h: &mut Window) -> Result<(), String> {
    // Something that repaints every frame (a meter) still leaves a drawn frame.
    let _steps = h.try_run();
    Ok(())
}

fn click(h: &mut Window, label: &str) -> Result<(), String> {
    settle(h)?;
    h.query_by_label(label).ok_or_else(|| format!("no control labelled {label:?}"))?.click();
    settle(h)
}

fn hover_first(h: &mut Window, contains: &str) -> Result<(), String> {
    let at = h
        .query_all_by_label_contains(contains)
        .next()
        .ok_or_else(|| format!("no control with {contains:?} in its label"))?
        .rect()
        .center();
    h.hover_at(at);
    settle(h)
}

fn launcher_hover(h: &mut Window) -> Result<(), String> {
    settle(h)?;
    hover_first(h, "Launch ")
}

fn arrangement(h: &mut Window) -> Result<(), String> {
    click(h, "Arrangement view")?;
    hover_first(h, "Clip Beat A on Drums")
}

fn piano_roll(h: &mut Window) -> Result<(), String> {
    click(h, "Arrangement view")?;
    click(h, "Clip Beat B on Drums at bar 17")?;
    h.remove_cursor();
    settle(h)
}

/// The demo project with `devices` added to Keys in order: an instrument takes the place of
/// Keys' own.
fn on_keys(devices: &[&str]) -> Result<App, String> {
    let mut session = suhina_session::Session::new(suhina_ui::demo::demo_project());
    for id in devices {
        session
            .execute("device.add", &json!({ "track": "Keys", "id": id }))
            .map_err(|e| format!("device.add {id}: {e}"))?;
    }
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    Ok(app)
}

/// The demo project with every track copied twice and every scene three times, so both views
/// scroll both ways.
fn crowded() -> Result<App, String> {
    let mut session = suhina_session::Session::new(suhina_ui::demo::demo_project());
    for _ in 0..2 {
        session
            .execute("track.duplicate", &json!({ "tracks": ["Drums", "Bass", "Keys", "Vox"] }))
            .map_err(|e| format!("track.duplicate: {e}"))?;
    }
    for _ in 0..3 {
        for scene in 1..=6 {
            session
                .execute("scene.duplicate", &json!({ "scene": scene }))
                .map_err(|e| format!("scene.duplicate: {e}"))?;
        }
    }
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    Ok(app)
}

/// The wheel turned over the control labelled `over` (its first match), by `delta` points.
fn scroll(h: &mut Window, over: &str, delta: egui::Vec2) -> Result<(), String> {
    let at = h
        .query_all_by_label_contains(over)
        .next()
        .ok_or_else(|| format!("no control with {over:?} in its label"))?
        .rect()
        .center();
    h.hover_at(at);
    h.step();
    h.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta,
        modifiers: Modifiers::NONE,
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Unknown,
    });
    h.step();
    h.remove_cursor();
    settle(h)?;
    // Under the wheel, the content moved; a view that did not scroll would hide what this tests.
    let moved = h.query_all_by_label_contains(over).next().is_none_or(|n| n.rect().center() != at);
    if moved { Ok(()) } else { Err(format!("the view under {over:?} did not scroll")) }
}

fn launcher_scrolled(h: &mut Window) -> Result<(), String> {
    settle(h)?;
    scroll(h, "Launch Beat A on Drums", egui::vec2(-150.0, -120.0))
}

fn arrangement_scrolled(h: &mut Window) -> Result<(), String> {
    click(h, "Arrangement view")?;
    scroll(h, "Clip Beat A on Drums", egui::vec2(-300.0, -200.0))
}

fn settings(h: &mut Window) -> Result<(), String> {
    settle(h)?;
    h.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    settle(h)
}

/// A tooltip: the pointer held over the toolbar's browser button until its tooltip shows.
fn tooltip(h: &mut Window) -> Result<(), String> {
    hover_first(h, "Show browser")?;
    // egui shows a tooltip once the pointer has rested; a second of fixed frames passes it.
    h.run_steps((1.0 / crate::STEP).ceil() as usize);
    Ok(())
}
