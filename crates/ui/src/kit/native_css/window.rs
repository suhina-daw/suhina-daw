// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The window's layout: one grid on `window`, whose children are the bars, the lines over and
//! under the views, the notice strip and the three panels (#1370). The user-agent sheet states
//! the default grid, so this is the only path that places the window's regions.
use egui::{Rect, Vec2};

/// A region of the window's grid, in the order its elements are given.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Region {
    MenuBar,
    Tabs,
    Toolbar,
    /// The 1 px line over the views.
    TopLine,
    /// The strip that asks about an opened project or names missing files.
    Notice,
    Browser,
    Main,
    Detail,
    /// The 1 px line under the views.
    BottomLine,
    StatusBar,
}

impl Region {
    pub const ALL: [Region; 10] = [
        Region::MenuBar,
        Region::Tabs,
        Region::Toolbar,
        Region::TopLine,
        Region::Notice,
        Region::Browser,
        Region::Main,
        Region::Detail,
        Region::BottomLine,
        Region::StatusBar,
    ];

    /// Its element's name, id and `kind` attribute, as a theme selects it.
    pub fn element(self) -> (&'static str, &'static str, Option<&'static str>) {
        match self {
            Region::MenuBar => ("menu-bar", "menu-bar", None),
            Region::Tabs => ("project-tabs", "project-tabs", None),
            Region::Toolbar => ("toolbar", "toolbar", None),
            Region::TopLine => ("separator", "views-top", Some("views-top")),
            Region::Notice => ("notice-bar", "notice-bar", None),
            Region::Browser => ("workspace-panel", "panel-browser", Some("browser")),
            Region::Main => ("workspace-panel", "panel-main", Some("main")),
            Region::Detail => ("workspace-panel", "panel-detail", Some("detail")),
            Region::BottomLine => ("separator", "views-bottom", Some("views-bottom")),
            Region::StatusBar => ("status-bar", "status-bar", None),
        }
    }

    fn index(self) -> usize {
        Region::ALL.iter().position(|region| *region == self).unwrap_or(0)
    }
}

/// Where the detail area lies: `window[detail]`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DetailPlace {
    /// Along the bottom of the views, beside the browser.
    #[default]
    Below,
    /// Along the bottom across the full width, the browser above it.
    FullWidth,
    /// Stacked beside the view.
    Beside,
}

impl DetailPlace {
    /// Its `window[detail]` value.
    pub fn attribute(self) -> &'static str {
        match self {
            DetailPlace::Below => "below",
            DetailPlace::FullWidth => "full-width",
            DetailPlace::Beside => "beside",
        }
    }
}

/// The side the browser shows on: `window[browser]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BrowserSide {
    Left,
    Right,
}

impl BrowserSide {
    /// Its `window[browser]` value.
    pub fn attribute(self) -> &'static str {
        match self {
            BrowserSide::Left => "left",
            BrowserSide::Right => "right",
        }
    }
}

/// What the app knows about the window this frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Input {
    /// The room the grid lays out in.
    pub size: Vec2,
    pub detail: DetailPlace,
    /// The browser's side while it shows (`window[browser="left"]`); `None` while it does not.
    pub browser: Option<BrowserSide>,
    /// Whether the window has its own menu bar (`window[menu-bar]`).
    pub menu_bar: bool,
    /// Whether the detail area is closed (`#panel-detail[closed]`): it keeps its place, with no
    /// size, for its splitter.
    pub detail_closed: bool,
    /// Each region's content size, by [`Region::ALL`]'s order; `None` where the region is not
    /// there this frame (the menu bar turned off, one project's tabs, a hidden browser). A size
    /// left `None` on an axis is the grid's to decide.
    pub regions: [Option<[Option<f32>; 2]>; 10],
}

impl Input {
    pub fn set(&mut self, region: Region, size: [Option<f32>; 2]) {
        if let Some(slot) = self.regions.get_mut(region.index()) {
            *slot = Some(size);
        }
    }
}

/// The rectangles the grid gives the regions.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Window {
    rects: [Option<Rect>; 10],
    /// Whether the browser's width follows its splitter: the sheet leaves its size to its content
    /// size, the saved width, so the splitter stays (#1240).
    pub browser_follows: bool,
    /// The same for the detail area's height, or its width beside the view.
    pub detail_follows: bool,
}

impl Window {
    /// `region`'s rectangle: none when it is not there or the sheet hides it (`display: none`, an
    /// area its template lacks). It may be empty: the closed detail area keeps its place.
    pub fn rect(&self, region: Region) -> Option<Rect> {
        self.rects.get(region.index()).copied().flatten()
    }

    pub fn translated(mut self, offset: Vec2) -> Self {
        for rect in self.rects.iter_mut().flatten() {
            *rect = rect.translate(offset);
        }
        self
    }
}

#[cfg(test)]
thread_local! {
    /// How many times `window` laid the grid out, for the tests.
    pub(super) static LAYS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Lays the window's regions out in `input.size`. `None` only for a size with no room (zero or
/// not finite): every sheet lays the window out, the user-agent sheet when the theme's cannot.
pub fn window(input: &Input) -> Option<Window> {
    if !input.size.is_finite() || input.size.x <= 0.0 || input.size.y <= 0.0 {
        return None;
    }
    super::ENGINE.with(|engine| {
        let mut engine = engine.borrow_mut();
        let key = format!("{input:?}");
        if let Some((previous, laid)) = &engine.window
            && previous == &key
        {
            return Some(laid.clone());
        }
        // A sheet the engine holds for another theme (the theme just changed) is not used: the
        // user-agent sheet lays the window out until the engine has the new one.
        let themed = engine.source.is_some() && super::same_source(engine.source.as_ref());
        let lay = |input: &Input| -> Option<[Option<Rect>; 10]> {
            let mut elements = Vec::new();
            let mut sizes = Vec::new();
            let mut present = Vec::new();
            for (region, size) in Region::ALL.iter().zip(&input.regions) {
                let Some(size) = size else { continue };
                let (name, id, kind) = region.element();
                let mut element = super::native::element(name, Some(id), None, Some(1));
                if let Some(kind) = kind {
                    element.attributes.push(("kind".into(), kind.into()));
                }
                if *region == Region::Detail && input.detail_closed {
                    element.attributes.push(("closed".into(), String::new()));
                }
                elements.push(element);
                sizes.push(Some(*size));
                present.push(*region);
            }
            let mut attributes = vec![("detail", input.detail.attribute())];
            if let Some(side) = input.browser {
                attributes.push(("browser", side.attribute()));
            }
            if input.menu_bar {
                attributes.push(("menu-bar", ""));
            }
            #[cfg(test)]
            LAYS.with(|lays| lays.set(lays.get() + 1));
            let laid = super::native::window_layout(
                themed.then_some(&*engine),
                &attributes,
                elements,
                &sizes,
                [input.size.x.min(16384.0), input.size.y.min(16384.0)],
            )?;
            let mut rects = [None; 10];
            for (region, (rect, style)) in present.iter().zip(laid) {
                if let Some(slot) = rects.get_mut(region.index()) {
                    *slot = rect.filter(|rect| style.visible && rect.width() >= 0.0 && rect.height() >= 0.0);
                }
            }
            Some(rects)
        };
        let rects = lay(input)?;
        // Laid out again with one panel's content size a point larger at a time: a panel the sheet
        // does not size grows with it, and keeps its splitter. No list of properties says which:
        // the layout does. One at a time, so a sheet that swaps the panels' tracks does not count
        // one panel's growth as the other's. Cached apart from the sizes, which a drag changes
        // every frame.
        let follows_key = {
            let mut shape = input.clone();
            for size in shape.regions.iter_mut().flatten() {
                *size = [size[0].map(|_| 0.0), size[1].map(|_| 0.0)];
            }
            format!("{shape:?}")
        };
        let known = engine.window_follows.as_ref().filter(|(previous, _)| previous == &follows_key).map(|(_, f)| *f);
        let (browser_follows, detail_follows) = match known {
            Some(follows) => follows,
            None => {
                let grown = |region: Region| -> bool {
                    let Some(Some(size)) = input.regions.get(region.index()) else { return false };
                    let axis = usize::from(size[0].is_none());
                    let mut larger = input.clone();
                    let mut bigger = *size;
                    if let Some(value) = bigger.get_mut(axis) {
                        *value = Some(value.unwrap_or(0.0) + 1.0);
                    }
                    larger.set(region, bigger);
                    let (Some(Some(at)), Some(then)) = (
                        rects.get(region.index()),
                        lay(&larger).and_then(|r| r.get(region.index()).copied().flatten()),
                    ) else {
                        return false;
                    };
                    if axis == 0 { then.width() > at.width() } else { then.height() > at.height() }
                };
                let follows = (grown(Region::Browser), grown(Region::Detail));
                engine.window_follows = Some((follows_key, follows));
                follows
            }
        };
        let laid = Window { rects, browser_follows, detail_follows };
        engine.window = Some((key, laid.clone()));
        Some(laid)
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
