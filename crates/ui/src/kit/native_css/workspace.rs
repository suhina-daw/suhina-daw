// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Optional CSS geometry for the native browser, project view and detail editor.
use egui::{Rect, Vec2};

#[derive(Clone, Debug)]
pub struct Workspace {
    pub browser: Option<Rect>,
    pub main: Rect,
    pub detail: Option<Rect>,
}
impl Workspace {
    pub fn translated(mut self, offset: Vec2) -> Self {
        self.browser = self.browser.map(|rect| rect.translate(offset));
        self.main = self.main.translate(offset);
        self.detail = self.detail.map(|rect| rect.translate(offset));
        self
    }
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
        let laid = super::native::layout(&engine, elements, &defaults, [size.x, size.y])?;
        let layout = laid.requested.then(|| {
            let rectangle = |name: &str| -> Option<Rect> {
                let index = ids.iter().position(|id| *id == name)? + 1;
                let (rect, style) = laid.items.get(index)?;
                rect.filter(|rect| style.visible && rect.is_positive())
            };
            Workspace {
                browser: rectangle("panel-browser"),
                main: rectangle("panel-main").unwrap_or(Rect::from_min_size(egui::Pos2::ZERO, Vec2::ZERO)),
                detail: rectangle("panel-detail"),
            }
        });
        engine.workspace = Some((key, layout.clone()));
        layout
    })
}
