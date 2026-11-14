// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the window's grid is given (#1370): the layout settings as attributes on `window`, and the
//! content sizes the app knows for the regions there this frame.

use super::*;
use crate::kit::native_css::{BrowserSide, DetailPlace, Region, WindowInput};

impl App {
    /// Where the detail area lies and the side the browser shows on, as the settings say:
    /// `window[detail]` and `window[browser]`. A stacked detail area lies beside the view, which
    /// wins over the full width, unless it shows a clip's editor.
    pub(super) fn window_place(&self, has_project: bool) -> (DetailPlace, Option<BrowserSide>) {
        // With the devices stacked, a clip's editor still lies along the bottom.
        let detail = if !self.editor_along_bottom() {
            DetailPlace::Beside
        } else if has_project
            && self.settings.device_layout != DeviceLayout::Stack
            && self.settings.view.detail_layout == crate::settings::view_settings::DetailLayout::FullWidth
        {
            DetailPlace::FullWidth
        } else {
            DetailPlace::Below
        };
        let side = match self.settings.browser_side {
            Side::Left => BrowserSide::Left,
            Side::Right => BrowserSide::Right,
        };
        (detail, (self.browser_part > 0.001).then_some(side))
    }

    /// The grid's input for a window of `size`: which regions are there and their content sizes.
    /// A bar's is its height less the line the user-agent sheet gives it as its border, which the
    /// layout adds back with the theme's padding and border (#1336).
    pub(super) fn window_input(&self, size: Vec2, has_project: bool, notice: bool) -> WindowInput {
        let (detail, browser) = self.window_place(has_project);
        let mut input =
            WindowInput { size, detail, browser, menu_bar: self.settings.view.menu_bar, ..Default::default() };
        let height = |height: f32| [None, Some(height)];
        if self.settings.view.menu_bar {
            input.set(Region::MenuBar, height(theme::MENU_BAR - 1.0));
        }
        if self.projects.len() >= 2 {
            input.set(Region::Tabs, height(TABS - 1.0));
        }
        input.set(Region::Toolbar, height(theme::CONTROL_BAR - 1.0));
        input.set(Region::TopLine, height(1.0));
        if notice {
            input.set(Region::Notice, height(theme::ROW + 12.0));
        }
        if browser.is_some() {
            input.set(Region::Browser, [Some(self.browser_width_in(size.x) * self.browser_part.clamp(0.0, 1.0)), None]);
        }
        input.set(Region::Main, [None, None]);
        if has_project {
            // Closed, the detail area keeps its place with no size, for its splitter in the gap.
            let open = self.detail_open;
            let size = if detail == DetailPlace::Beside {
                [Some(if open { self.detail_shown.x } else { 0.0 }), None]
            } else {
                [None, Some(if open { self.detail_shown.y } else { 0.0 })]
            };
            input.set(Region::Detail, size);
            input.detail_closed = !open;
        }
        input.set(Region::BottomLine, height(1.0));
        input.set(Region::StatusBar, height(theme::STATUS_BAR - 1.0));
        input
    }
}
