// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Optional CSS geometry for the native browser, project view and detail editor.
use egui::{Rect, Vec2};

#[derive(Clone, Debug)]
pub struct Workspace {
    pub browser: Option<Rect>,
    pub main: Rect,
    pub detail: Option<Rect>,
    /// Whether the browser's width follows its splitter: the sheet leaves its size to it (a
    /// layout with only gaps or margins does), so the splitter stays (#1240).
    pub browser_follows: bool,
    /// The same for the detail area's height, or its width when it is stacked beside the view.
    pub detail_follows: bool,
}
impl Workspace {
    pub fn translated(mut self, offset: Vec2) -> Self {
        self.browser = self.browser.map(|rect| rect.translate(offset));
        self.main = self.main.translate(offset);
        self.detail = self.detail.map(|rect| rect.translate(offset));
        self
    }
}

#[cfg(test)]
thread_local! {
    /// How many times `workspace` laid the grid out, for the tests.
    pub(super) static LAYS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// `detail` is (stacked beside the view, native preferred size). Native visibility
/// remains authoritative; CSS may further hide a panel but cannot open closed panels.
/// `None` when the theme writes no workspace layout: the native splitters stay in charge.
pub fn workspace(size: Vec2, browser: Option<f32>, detail: Option<(bool, f32)>) -> Option<Workspace> {
    if !size.is_finite() || size.x <= 0.0 || size.y <= 0.0 {
        return None;
    }
    super::ENGINE.with(|engine| {
        let mut engine = engine.borrow_mut();
        engine.source.as_ref()?;
        if !super::same_source(engine.source.as_ref()) {
            return None;
        }
        let key = format!("{size:?}:{browser:?}:{detail:?}");
        if let Some((previous, layout)) = &engine.workspace
            && previous == &key
        {
            return layout.clone();
        }
        // What the follows flags depend on: not the preferred sizes, which a drag changes every
        // frame. The sizes only move the panels' tracks; whether a panel's track is its own
        // preferred size is the sheet's, read once for the window's size and its open panels.
        let follows_key = format!("{size:?}:{}:{:?}", browser.is_some(), detail.map(|(stacked, _)| stacked));
        let known_follows = engine
            .workspace_follows
            .as_ref()
            .filter(|(previous, _)| previous == &follows_key)
            .map(|(_, follows)| *follows);
        let mut ids = Vec::new();
        if browser.is_some() {
            ids.push("panel-browser");
        }
        if detail.is_some() {
            ids.push("panel-detail");
        }
        ids.push("panel-main");
        let width = browser.unwrap_or(0.0).clamp(0.0, 4096.0);
        let stacked = detail.is_some_and(|(stacked, _)| stacked);
        let detail_size = detail.map_or(0.0, |(_, size)| size).clamp(0.0, 4096.0);
        // The native grid at the panels' preferred sizes, under the sheet's rules.
        let lay = |width: f32, detail_size: f32| {
            let (columns, rows, detail_position) = if stacked {
                (format!("{width}px minmax(0,1fr) {detail_size}px"), "minmax(0,1fr)".to_owned(), "grid-column:3;grid-row:1;")
            } else {
                (format!("{width}px minmax(0,1fr)"), format!("minmax(0,1fr) {detail_size}px"), "grid-column:2;grid-row:2;")
            };
            let defaults = format!(
                ".workspace {{display:grid;width:{}px;height:{}px;grid-template-columns:{columns};grid-template-rows:{rows};}} #panel-browser {{grid-column:1;grid-row:1 / -1;}} #panel-main {{grid-column:2;grid-row:1;}} #panel-detail {{{detail_position}}}",
                size.x.min(16384.0),
                size.y.min(16384.0)
            );
            let mut elements = vec![super::native::element("workspace", Some("workspace"), None, Some(1))];
            for id in &ids {
                elements.push(super::native::element("workspace-panel", Some(id), None, Some(2)));
            }
            #[cfg(test)]
            LAYS.with(|lays| lays.set(lays.get() + 1));
            let laid = super::native::layout(&engine, elements, &defaults, [size.x, size.y])?;
            let rectangle = |name: &str| -> Option<Rect> {
                let index = ids.iter().position(|id| *id == name)? + 1;
                let (rect, style) = laid.items.get(index)?;
                rect.filter(|rect| style.visible && rect.is_positive())
            };
            Some((laid.requested, rectangle("panel-browser"), rectangle("panel-main"), rectangle("panel-detail")))
        };
        let (requested, browser_rect, main, detail_rect) = lay(width, detail_size)?;
        let mut probed = None;
        let layout = if requested {
            let (browser_follows, detail_follows) = match known_follows {
                Some(follows) => follows,
                None => {
                    // Laid out again with one panel's preferred size a point larger at a time: a
                    // panel the sheet does not size grows with its own, and keeps its splitter.
                    // No list of properties says which: the layout does. One at a time, so a
                    // sheet that swaps the panels' tracks does not count one panel's growth as
                    // the other's.
                    let (_, wider, _, _) = lay(width + 1.0, detail_size)?;
                    let (_, _, _, larger) = lay(width, detail_size + 1.0)?;
                    let grew = |at: Option<Rect>, then: Option<Rect>, across: bool| {
                        at.zip(then)
                            .is_some_and(|(a, b)| if across { b.width() > a.width() } else { b.height() > a.height() })
                    };
                    let follows = (grew(browser_rect, wider, true), grew(detail_rect, larger, stacked));
                    probed = Some(follows);
                    follows
                }
            };
            Some(Workspace {
                browser: browser_rect,
                main: main.unwrap_or(Rect::from_min_size(egui::Pos2::ZERO, Vec2::ZERO)),
                detail: detail_rect,
                browser_follows,
                detail_follows,
            })
        } else {
            None
        };
        if let Some(follows) = probed {
            engine.workspace_follows = Some((follows_key, follows));
        }
        engine.workspace = Some((key, layout.clone()));
        layout
    })
}

/// The strip between a `panel` and the `main` view beside, above or under it, where its splitter
/// goes: their gap, at least `thickness` across, centred on it; nothing when they overlap.
pub fn splitter_between(panel: Rect, main: Rect, thickness: f32) -> Rect {
    let strip = |from: f32, to: f32| {
        let middle = (from + to) / 2.0;
        let half = (to - from).max(thickness) / 2.0;
        egui::Rangef::new(middle - half, middle + half)
    };
    if panel.right() <= main.left() {
        Rect::from_x_y_ranges(strip(panel.right(), main.left()), panel.y_range())
    } else if main.right() <= panel.left() {
        Rect::from_x_y_ranges(strip(main.right(), panel.left()), panel.y_range())
    } else if main.bottom() <= panel.top() {
        Rect::from_x_y_ranges(panel.x_range(), strip(main.bottom(), panel.top()))
    } else if panel.bottom() <= main.top() {
        Rect::from_x_y_ranges(panel.x_range(), strip(panel.bottom(), main.top()))
    } else {
        Rect::NOTHING
    }
}
