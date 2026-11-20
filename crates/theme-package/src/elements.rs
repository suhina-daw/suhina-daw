// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The elements a theme can style: one entry per element, with its name, kind, parts, ids,
//! attributes, settings and artwork (ADR 0031 › Amendment: the element registry). The style
//! tree, the artwork checks and the element reference all read this table, so an element that is
//! not here cannot be styled, and one that is here is documented.

/// What an element is, for how it paints and what its background means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A window, view, bar, panel or dialog. Its background is the surface behind its controls,
    /// never their fill.
    Region,
    /// A part that holds controls (a track header, a device). Its background, like a region's,
    /// stays behind its controls.
    Container,
    /// Something a person uses: a button, a knob, a clip.
    Control,
    /// A piece of a control or a region that is styled on its own: a fader's cap, a clip's name.
    Part,
    /// Something drawn many times without input of its own: a note, a ruler tick, a grid row.
    Drawn,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Region => "region",
            Self::Container => "container",
            Self::Control => "control",
            Self::Part => "part",
            Self::Drawn => "drawn item",
        }
    }
}

/// The ids an element of one name may have, as a theme writes them after `#`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ids {
    /// None the theme can rely on: the element gets one of its own (`node-…`) that changes.
    None,
    /// These names only, written `#{prefix}{name}`: a toolbar button is `#control-play`.
    Listed { prefix: &'static str, names: &'static [&'static str] },
    /// Any name the app gives that starts with this, written as it is: `#device-compressor`.
    Prefixed(&'static str),
    /// Any name the app gives: a device panel's control by its parameter's id (`#cutoff`).
    Any,
}

/// One artwork role of an element (`knob.body`), and Suhina's own artwork for it when there is
/// some. A role without it is drawn natively until a theme gives artwork of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Artwork {
    pub role: &'static str,
    /// What a sheet writes to give the role its image: `knob-body { image: url(…); }`. A role
    /// with artwork of Suhina's own has the element (and attribute) that draws it; the others
    /// are written as their role (`piano.draw { image: url(…); }`).
    pub selector: &'static str,
    pub default: Option<&'static str>,
}

/// One element of the style tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Element {
    /// What a theme writes: `knob { … }`.
    pub name: &'static str,
    pub kind: Kind,
    /// One sentence on what it is and where it is.
    pub description: &'static str,
    /// Elements drawn inside it as its own pieces.
    pub parts: &'static [&'static str],
    pub ids: Ids,
    /// The attributes the app gives it, from [`ATTRIBUTES`].
    pub attributes: &'static [&'static str],
    /// The registered properties the app reads from it (`suhina_style::property::Property`).
    pub settings: &'static [&'static str],
    pub artwork: &'static [Artwork],
}

/// The app's states of an element, written as attributes without a value (`button[checked]`).
pub const ATTRIBUTES: &[(&str, &str)] = &[
    ("checked", "a toggle that is on, the chosen tool, tab or row"),
    ("selected", "the selected track, clip, note, device or browser row"),
    ("playing", "play while the transport runs, a clip or key that plays"),
    ("recording", "record while it records, a clip being recorded"),
    ("armed", "a track armed for recording, and its arm button"),
    ("muted", "a muted track, a clip or note that is off, a device that is off"),
    ("queued", "a clip or a track's stop button waiting for its launch"),
    ("color", "a track's or clip's own colour, `#rrggbb` in lowercase: `attr(color type(<color>))`"),
    ("kind", "what an element is among those of its name: an icon's symbol, a panel or line of the window"),
    ("closed", "the detail area while it is closed, its place kept for its splitter"),
    ("detail", "where the window has its detail area: below, full-width or beside"),
    ("browser", "the side the window shows its browser on, left or right, while it shows"),
    ("menu-bar", "a window with its own menu bar"),
    (
        "size",
        "the piano roll's toolbar at its Toolbar size: normal, large or larger; a track header by its height: tiny, small, normal or spacious",
    ),
    ("layout", "which line of a track header holds what, by its width and height: short, narrow, side or stacked"),
    ("name-first", "a track header whose name's line comes first"),
    ("header-side", "the Arrangement header column's side of the timeline: left or right"),
    ("disclosure", "a container's reserved full-height disclosure gutter: left or right"),
    ("narrow", "a mixer strip 96 points wide or less"),
    ("name", "which dialog a dialog is: export, color-picker, unsaved and the others its element lists"),
];

/// The toolbar's areas, by id (`#tempo`).
pub const TOOLBAR_MODULES: &[&str] = &[
    "browser",
    "tempo",
    "signature",
    "keys",
    "grid",
    "count-in",
    "metronome",
    "transport",
    "position",
    "mapping",
    "performance",
    "views",
    "agents",
];

/// The groups of the bars' items, by id (`#toolbar-start`).
pub const GROUP_IDS: &[&str] = &[
    "toolbar-start",
    "toolbar-end",
    "app-bar-start",
    "app-bar-centre",
    "app-bar-end",
    "mixer-body",
    "mixer-output",
    "mixer-input",
    "mixer-monitor",
    "mixer-level",
    "mixer-buttons",
    "mixer-sends",
    "header-indent",
    "header-disclosure",
    "header-body",
    "header-first",
    "header-second",
];

/// The items of the status bar, the application bar and the piano roll's toolbar, by id.
pub const BAR_ITEM_IDS: &[&str] = &[
    "status-message",
    "status-counts",
    "app-name",
    "project-name",
    "project-timing",
    "piano-roll-workflow",
    "piano-roll-ghosts",
    "piano-roll-grid",
];

/// Buttons a theme selects by id, as `#control-NAME`: the toolbar's and a track's.
const CONTROL_IDS: &[&str] = &[
    "play",
    "stop",
    "record",
    "browser-toggle",
    "metronome-toggle",
    "solo",
    "cue",
    "arm",
    "main-metronome",
    "mixer-monitor-in",
    "mixer-monitor-auto",
    "mixer-monitor-off",
    "back-to-arrangement",
    "returns-toggle",
    "take-delete",
    "take-solo",
    "take-promote",
    "clip-envelope-edit",
    "clip-envelope-free",
    "focus-first",
    "device-on",
    "plugin-button",
    "ara-button",
    "track-on",
    "automation",
    "monitor",
];

/// The level fields of the track headers and the mixer, by their placement names.
const LEVEL_FIELDS: &[&str] =
    &["arrangement-volume", "arrangement-pan", "main-volume", "mixer-volume", "mixer-volume-readout", "mixer-pan"];

/// Settings of a value a person drags, inherited from its ancestors.
const VALUE_SETTINGS: &[&str] = &["appearance", "value-position", "value-decimals", "value-unit", "drag-pointer"];

macro_rules! default_svg {
    ($role:literal, $selector:literal) => {
        Artwork {
            role: $role,
            selector: $selector,
            default: Some(include_str!(concat!("../../../assets/themes/default/", $role, ".svg"))),
        }
    };
}

const fn own(role: &'static str) -> Artwork {
    Artwork { role, selector: role, default: None }
}

const NO: &[&str] = &[];
/// What a surface floating over the window reads: whether it blurs what is behind it.
const FLOATING: &[&str] = &["backdrop-filter"];
const NO_ART: &[Artwork] = &[];

const fn element(name: &'static str, kind: Kind, description: &'static str) -> Element {
    Element { name, kind, description, parts: NO, ids: Ids::None, attributes: NO, settings: NO, artwork: NO_ART }
}

impl Element {
    const fn parts(mut self, parts: &'static [&'static str]) -> Self {
        self.parts = parts;
        self
    }
    const fn ids(mut self, ids: Ids) -> Self {
        self.ids = ids;
        self
    }
    const fn attributes(mut self, attributes: &'static [&'static str]) -> Self {
        self.attributes = attributes;
        self
    }
    const fn settings(mut self, settings: &'static [&'static str]) -> Self {
        self.settings = settings;
        self
    }
    const fn artwork(mut self, artwork: &'static [Artwork]) -> Self {
        self.artwork = artwork;
        self
    }
}

use Kind::{Container, Control, Drawn, Part, Region};

/// Every element, in the order the reference lists them.
pub const ELEMENTS: &[Element] = &[
    // The window and its bars.
    element("window", Region, "The whole window, under everything else: one grid of the bars and panels.")
        .attributes(&["detail", "browser", "menu-bar"])
        .settings(&[
            "artwork-fps",
            "liquid",
            "panel-gradient",
            "track-gradient",
            "clip-gradient",
            "backdrop-image",
            "backdrop-opacity",
            "backdrop-blur",
        ]),
    element("application-bar", Region, "The bar at the very top, with the application's menus.")
        .parts(&["group", "bar-item"]),
    element("menu-bar", Region, "The menu bar."),
    element("menu", Region, "An open menu of the menu bar.").settings(FLOATING),
    element("project-tabs", Region, "The bar of open projects' tabs.").parts(&["project-tab"]),
    element("project-tab", Control, "One open project's tab, its close button inside it.").attributes(&["selected"]),
    element("toolbar", Region, "The toolbar under the bars, with the transport.").parts(&["group", "toolbar-module"]),
    element("toolbar-module", Part, "One area of the toolbar, by its id; each is `[kind]` its id too: the time signature's words are `toolbar-module[kind=\"signature\"]`'s `color`, the launch quantisation's `[kind=\"grid\"]`'s.")
        .ids(Ids::Listed { prefix: "", names: TOOLBAR_MODULES })
        .attributes(&["kind"]),
    element("group", Container, "A group of a bar's items or a strip's parts, by id: the toolbar's start and end, the application bar's parts, the piano roll's Rows, a mixer strip's and a track header's rows; a strip's groups lay its parts out and paint nothing.")
        .ids(Ids::Listed { prefix: "", names: GROUP_IDS })
        .attributes(&["kind"]),
    element("flexible-space", Drawn, "A gap in a bar that takes the room left over."),
    element("bar-item", Part, "An item of the status bar, the application bar or the piano roll's toolbar, by id; the words of the piano roll toolbar's menus (Grid, Workflow, Ghosts) are `bar-item[kind=\"menu\"]`, `[muted]` while what it names is off and `[checked]` while the menu is hovered or open; a menu's title on the menu bar is `bar-item[kind=\"menu\"]` too (scope a rule with `menu-bar` or `piano-roll-toolbar` to reach one of them), and the status bar's counts `[kind=\"status-counts\"]`.")
        .ids(Ids::Listed { prefix: "", names: BAR_ITEM_IDS })
        .attributes(&["kind", "muted", "checked"]),
    element("status-bar", Region, "The bar at the bottom of the window.").parts(&["bar-item"]),
    element("notice-bar", Region, "The strip over the views that asks about an opened project or names missing files."),
    element("workspace-panel", Region, "A panel of the window: the browser, the main view, the detail area.")
        .ids(Ids::Listed { prefix: "", names: &["panel-browser", "panel-main", "panel-detail"] })
        .attributes(&["kind", "closed"]),
    element("divider", Control, "A strip's divider and the workspace's splitters; each is `[kind]` its id too; `#detail-splitter` (`divider[kind=\"detail-splitter\"]`, its strip's `background-color` and its grip's `color`) is `[closed]` while the detail area is closed.")
        .ids(Ids::Listed {
            prefix: "",
            names: &[
                "mixer-divider",
                "mixer-splitter",
                "detail-splitter",
                "browser-splitter",
                "arrangement-header-splitter",
            ],
        })
        .attributes(&["kind", "closed"]),
    // Views.
    element("browser", Region, "The browser panel.").parts(&[
        "browser-place",
        "browser-row",
        "browser-play",
        "browser-icon",
        "browser-tag-dot",
        "browser-preview-wave",
    ]),
    element("browser-place", Control, "A place or kind on the browser's left, `[kind=\"drop\"]` while what is dragged would land on it.").attributes(&["checked", "selected", "kind"]),
    element("browser-row", Control, "A row of the browser's list, `[kind=\"drop\"]` while what is dragged would land on it; a heading of the list's columns is `browser-row[kind=\"header\"]` and a tag in a row's tag menu `browser-row[kind=\"menu\"]`.").attributes(&["selected", "kind"]),
    element("browser-play", Control, "The button that previews a browser row, and the preview strip's play glyph, `[checked]` while it plays.").attributes(&["checked"]),
    element(
        "browser-icon",
        Drawn,
        "A browser row's icon, coloured by its `kind` attribute (`midi`, `sample`, `instrument`, `effect`).",
    )
    .attributes(&["kind"]),
    element("browser-tag-dot", Drawn, "The coloured dot of a tag in the browser."),
    element("browser-preview-wave", Drawn, "The waveform of the sound being previewed."),
    element("browser-fill", Drawn, "The fill the browser paints for itself where the region behind it gives none: its `background-color`, `browser-fill[kind=\"panel\"]` under the list, the sample map's cards and its similar strip, `browser-fill[kind=\"window\"]` behind the search field, the preview strip and a cluster's name on the sample map; the browser's splitter takes it too (`browser-fill[kind=\"splitter\"]`, stronger under the pointer) where `#browser-splitter` has none.")
        .attributes(&["kind"]),
    element("session", Region, "The Launcher view: tracks as columns of clip slots.")
        .parts(&["track-column", "scene-column"])
        .settings(&["track-width", "return-width", "scene-width", "slot-height", "header-height"]),
    element("track-column", Container, "A track's column in the Launcher view; its background is drawn under its slots. While `[selected]`, what that state changes in its background is drawn over the whole column, header, slots and mixer strip, so give `track-column[selected]` a translucent one.")
        .attributes(&["color", "selected", "muted"])
        .settings(&["track-artwork"]),
    element("scene-column", Part, "The column of scenes at the Launcher view's side."),
    element("scene", Control, "A scene's launch button; the Stop all button under the scenes is `scene[kind=\"stop-all\"]`.")
        .parts(&["label"])
        .attributes(&["playing", "kind"]),
    element("slot", Control, "A clip slot in the Launcher view; a track's stop button under the last scene is `slot[kind=\"stop\"]`, `[queued]` while its stop waits.")
        .parts(&["clip", "slot-mark", "slot-record"])
        .attributes(&["selected", "queued", "kind"])
        .settings(&["track-artwork"]),
    element("mapping-overlay", Drawn, "The tint over a control while keys are mapped to the Launcher, `[selected]` on the one picked; its `color` is the keys' text.")
        .attributes(&["selected"]),
    element("slot-record", Control, "The record button in an armed track's empty slot, `[recording]` while that slot records: its `color` for the dot and the word, its `background-color` over the slot while it records.")
        .attributes(&["recording"]),
    element("slot-mark", Drawn, "The mark in an empty slot that holds a stop button."),
    element("drop-target", Drawn, "Where a drop lands: the outline of the area (`kind=\"area\"`), the line a moved track goes to (`kind=\"insert\"`), or the slot with a + after a chain's last device (`kind=\"append\"`).")
        .attributes(&["kind"]),
    element("arrangement", Region, "The Arrangement view: tracks as rows along a timeline. Its `background-color` is the colour under the grid and the empty lanes; `arrangement[kind=\"panel\"]`'s is the one under its ruler, rows, headers and toolbar, and both are what the view draws over a region around it that has a background of its own.")
        .parts(&[
            "track-header",
            "track-lane",
            "ruler",
            "loop-region",
            "playhead",
            "add-track",
            "grid-line",
            "time-selection",
            "marquee",
            "start-marker",
        ])
        .attributes(&["kind"])
        .settings(&["track-headers", "track-height", "header-min-width", "header-tint"]),
    element("track-header", Container, "A track's header, in the Arrangement view and over a Launcher column; Main's over the Launcher's scenes is `track-header[kind=\"main\"]`.")
        .parts(&["track-name", "track-number", "track-colour", "fold", "group"])
        .attributes(&["color", "selected", "armed", "muted", "kind", "layout", "size", "name-first", "header-side", "disclosure"])
        .artwork(&[
            default_svg!("track.midi", "icon[kind=\"midi\"]"),
            default_svg!("track.audio", "icon[kind=\"audio\"]"),
            default_svg!("track.body", "track-header"),
            own("track.body.session"),
            own("track.body.arrangement"),
        ]),
    element("track-name", Part, "A track's name.").attributes(&["selected"]).artwork(&[
        default_svg!("track.name", "track-name"),
        own("track.name.session"),
        own("track.name.mixer"),
        own("track.name.arrangement"),
    ]),
    element("track-number", Part, "A track's number in its Arrangement header.").attributes(&["muted"]),
    element("track-colour", Part, "The band of a track's colour in its header or mixer strip; a click on it opens the colour picker."),
    element("fold", Control, "The triangle that folds a track or a group.")
        .attributes(&["checked"])
        .artwork(&[own("fold")]),
    element("add-track", Control, "The buttons that add a track, after the last one; a device group's `+ Branch` is `add-track[kind=\"branch\"]`.")
        .ids(Ids::Prefixed("add-"))
        .attributes(&["kind"])
        .artwork(&[own("add-track")]),
    element(
        "track-lane",
        Container,
        "A track's row of clips in the Arrangement view, its clips inside it, `[selected]` while its track is selected and `[checked]` while the piano roll edits it alone. Its background is painted over the grid lines, so give it a translucent one.",
    )
    .parts(&["clip"])
    .attributes(&["color", "selected", "muted", "checked"]),
    element("clip", Control, "A clip, in the Arrangement or in a Launcher slot.")
        .parts(&["clip-name", "clip-waveform", "clip-notes", "clip-gain", "clip-fade-handle", "clip-fade"])
        .attributes(&["color", "selected", "muted", "playing", "queued", "recording"])
        .settings(&["track-artwork"]),
    element("clip-name", Part, "An arrangement clip's name bar."),
    element("clip-waveform", Part, "An arrangement clip's waveform, and the audio editor's in the detail area: its `color`, where Settings › Waveform colour is on Theme; without a rule the editor draws it in the clip's own colour."),
    element("clip-notes", Part, "An arrangement clip's notes."),
    element("clip-envelope", Drawn, "An audio clip's gain envelope while it is edited: its points and line, in its `color`."),
    element("clip-gain", Control, "An arrangement clip's gain field.").settings(VALUE_SETTINGS),
    element("clip-fade-handle", Control, "An arrangement clip's fade handles; the handle that bends a fade's curve is `clip-fade-handle[kind=\"curve\"]`.")
        .attributes(&["kind"])
        .artwork(&[own("clip.fade-handle")]),
    element("clip-fade", Drawn, "A fade's line over an audio clip; a comp's crossfade between two takes is `clip-fade[kind=\"crossfade\"]`.")
        .attributes(&["kind"]),
    element("take-header", Container, "A take lane's header, with the take's `color`, `[selected]` while it is picked.")
        .attributes(&["color", "selected"]),
    element("ruler", Drawn, "The Arrangement view's ruler, and the piano roll's.")
        .parts(&["ruler-number", "ruler-tick"])
        .artwork(&[own("ruler")]),
    element("ruler-number", Drawn, "A bar number on the ruler."),
    element("ruler-tick", Drawn, "A line on the ruler."),
    element("tempo-mark", Drawn, "A tempo or time-signature change on the ruler: its line and what changes, in its `color`."),
    element("grid-line", Drawn, "A line of the Arrangement's grid, on a bar (`kind=\"bar\"`) or between bars (`kind=\"beat\"`), in its `color`.")
        .attributes(&["kind"]),
    element("time-selection", Drawn, "Selected time: its `background-color` over the lanes and its `color` at its edges; along the ruler (`kind=\"ruler\"`); the clip the Draw tool's drag makes (`kind=\"draw\"`); the piano roll's selection lit on its clip (`kind=\"editor\"`); what is picked or painted on take lanes (`kind=\"take\"`) and on automation lanes (`kind=\"automation\"`).")
        .attributes(&["kind"]),
    element("marquee", Drawn, "The rectangle a drag draws to pick whole clips; the piano roll's notes (`kind=\"notes\"`) and the sample map's points (`kind=\"points\"`).")
        .attributes(&["kind"]),
    element("start-marker", Drawn, "The play start marker under the ruler, and its line in the lane clicked (`kind=\"insert\"`); a clip's start and end markers in the piano roll (`kind=\"clip\"`) and its play start (`kind=\"piano\"`).")
        .attributes(&["kind"]),
    element("loop-region", Control, "The loop bar; the clip's loop bar in the piano roll's ruler is `loop-region[kind=\"clip\"]`, `[checked]` while the clip loops.").attributes(&["checked", "kind"]).artwork(&[own("loop-region")]),
    element("loop-line", Drawn, "The lines at the loop's ends."),
    element("automation-header", Container, "An automation lane's header, `[selected]` while it is picked; a group of lanes' is `automation-header[kind=\"group\"]`.")
        .attributes(&["selected", "kind"]),
    element("automation-lane", Drawn, "The time of a group of automation lanes, `automation-lane[kind=\"group\"]`: its `background-color`.")
        .attributes(&["kind"]),
    element("automation-line", Drawn, "An automation lane's envelope, the track's colour by default; `[kind=\"fixed\"]` while Settings colours every envelope alike, `[muted]` while it is off.")
        .attributes(&["muted", "kind"]),
    element("automation-point", Drawn, "A point of an automation envelope, the line's colour by default; `[kind=\"fixed\"]` while Settings colours every envelope alike, `[selected]` while it is picked.").attributes(&["selected", "kind"]),
    element("automation-pin", Drawn, "The pin of an automation lane pinned below its track's top lane."),
    element("marker", Control, "A marker's tag in the row under the loop: its background, its line's `border-color` and its name's `color`; `[checked]` while it is dragged.")
        .attributes(&["checked"]),
    element("arrangement-tool", Control, "A tool button of the Arrangement's own toolbar, `[checked]` on the chosen tool.")
        .attributes(&["checked"]),
    element("playhead", Drawn, "The play position line, in the arrangement, the piano roll and the browser's preview strip; where a jump waits for its beat is `playhead[kind=\"pending\"]`; the piano roll's is `playhead[kind=\"piano\"]` and the preview strip's `playhead[kind=\"preview\"]`.")
        .attributes(&["kind"]),
    element("mixer", Region, "The mixer, and a mixer strip in any view: `[narrow]` at 96 points or less, Main's `mixer[kind=\"main\"]`.")
        .parts(&["fader", "pan", "send", "meter", "device-list", "track-colour", "mixer-toggles", "group"])
        .attributes(&["color", "selected", "muted", "narrow", "kind"]),
    element("device-list", Container, "A mixer strip's list of its track's devices.").parts(&["device-row", "label"]),
    element("mixer-toggles", Container, "A mixer strip's solo, cue and arm, beside its track-on switch.").parts(&["button"]),
    element("device-row", Control, "A device in a mixer strip's list: `[selected]` while it is picked, `[muted]` while it is off; a device group's branch in its panel, `device-row[kind=\"branch\"]`, `[selected]` while its devices show, `[muted]` while it is muted.")
        .attributes(&["selected", "muted", "kind"]),
    element("detail", Region, "The detail area under the main view."),
    element("detail-fill", Drawn, "The fill the detail panels and the piano roll paint for themselves where the region behind them gives none: its `background-color`, `detail-fill[kind=\"panel\"]` for a panel, a column, a ruler row or a header, `detail-fill[kind=\"window\"]` for the rows behind the notes and the rows between a branch's names.")
        .attributes(&["kind"]),
    element("clip-editor", Region, "The clip editor in the detail area.").parts(&["editor-tab"]),
    element(
        "editor-tab",
        Control,
        "The clip editor's Clip and Devices tabs, the MIDI tools part's Notes and Transform tabs and the start page's Recent and Templates tabs (`#tab-recent`, `#tab-templates`). The Clip and Devices switch is `editor-tab[kind=\"switch\"]`; the Notes and Transform tabs are `editor-tab[kind=\"tab-notes\"]` and `[kind=\"tab-transform\"]`; the start page's are `editor-tab[kind=\"start\"]`, its `accent-color` the line under the one shown.",
    )
    .ids(Ids::Prefixed("tab-"))
    .attributes(&["checked", "kind"]),
    element("piano-roll", Region, "The piano roll, with the edited clip's `color`.").attributes(&["color"]).parts(&[
        "piano-grid",
        "piano-key",
        "piano-note",
        "velocity",
        "piano-tool",
        "midi-tools",
        "clip-info",
    ]),
    element("piano-roll-toolbar", Region, "The toolbar along the top of the piano roll.")
        .parts(&["piano-tool", "group", "bar-item", "separator", "flexible-space"])
        .attributes(&["size"]),
    element("piano-grid", Drawn, "The piano roll's grid: its `border-color` the lines between rows and beats, `piano-grid[kind=\"strong\"]`'s those on an octave and a bar.")
        .attributes(&["kind"]),
    element("piano-row", Drawn, "A row of the piano roll; the row under the pointer is `piano-row[kind=\"hover\"]`.")
        .attributes(&["kind"]),
    element("piano-row-black", Drawn, "A row of a black key."),
    element("piano-key", Drawn, "A white key; its `accent-color` lights a playing or selected key, and `piano-key[playing]`'s `color` is a playing key's name.").attributes(&["selected", "playing"]),
    element("piano-key-black", Drawn, "A black key.").attributes(&["selected", "playing"]),
    element("piano-key-c", Drawn, "A C key."),
    element("piano-key-hover", Drawn, "A white key under the pointer."),
    element("piano-key-black-hover", Drawn, "A black key under the pointer."),
    element("piano-key-line", Drawn, "The line between white keys."),
    element("piano-scale-mark", Drawn, "The mark on a key in the clip's scale."),
    element("piano-scale-strip", Drawn, "The strip left of the keys naming the octaves; the name of the row under the pointer is `piano-scale-strip[kind=\"pointer\"]`.")
        .attributes(&["kind"]),
    element("piano-range", Drawn, "The range a drag on the keys selects, in its `accent-color` where it gives no colours of its own."),
    element("piano-clip-strip", Drawn, "The strip over the ruler showing several picked clips: its `color` a clip's name, `[selected]`'s the edited clip's.")
        .attributes(&["selected"]),
    element("piano-time-selection", Drawn, "What is selected in time, in its `accent-color` where it gives no colours of its own."),
    element("piano-insert-mark", Drawn, "The mark a click leaves where a paste goes."),
    element("piano-note", Drawn, "A note; a note a draw or paint drag is making is `piano-note[kind=\"draft\"]`; a selected note with no background of its own is lit with `piano-note[selected]`'s `accent-color`. A note's own inks: `kind=\"outline\"` a selected note's edge, `kind=\"dashes\"` a muted note's, `kind=\"name\"` the name on a note (`[checked]` on a strong note).")
        .attributes(&["selected", "muted", "kind", "checked"]),
    element("velocity", Drawn, "A note's velocity stem, `[selected]` and `[muted]` as its note; the line a velocity drag draws is `velocity[kind=\"line\"]`; the figure shown while a velocity is dragged `velocity[kind=\"label\"]`.")
        .attributes(&["selected", "muted", "kind"]),
    element("piano-slice", Drawn, "The line a slice drag cuts along, `kind=\"cut\"` or `kind=\"delete\"` for the right button's; its `background-color` over what a delete takes away.")
        .attributes(&["kind"]),
    element("piano-tool", Control, "A tool button of the piano roll, `[checked]` while chosen, `:disabled` while it cannot be used; `kind` is its id without `#` (`piano-fold`).")
        .ids(Ids::Prefixed("piano-"))
        .attributes(&["checked", "kind"])
        .artwork(&[
            own("piano.select"),
            own("piano.draw"),
            own("piano.paint"),
            own("piano.erase"),
            own("piano.slice"),
            own("piano.mute"),
            own("piano.cut"),
            own("piano.copy"),
            own("piano.paste"),
            own("piano.duplicate"),
            own("piano.quantise"),
            own("piano.midi-tools"),
            own("piano.audition"),
            own("piano.hide"),
        ]),
    element("midi-tools", Container, "The MIDI tools part of a MIDI clip's panel.")
        .parts(&["tab-row"])
        .settings(&["gap"]),
    element("tab-row", Container, "A row of tabs: the MIDI tools part's Notes and Transform.").parts(&["editor-tab"]),
    element("clip-info", Container, "The clip's column beside the piano roll."),
    element("sample-map", Region, "The sample map: its `border-color` round its progress card, its `background-color` behind a cluster's name.").parts(&["sample-point"]),
    element("sample-point", Drawn, "A sound on the sample map, coloured by its `kind` attribute (`kick`, `snare`); `sample-point[selected]`'s `color` rings a selected one and its `accent-color` the previewed and similar ones.")
        .attributes(&["kind", "selected"]),
    // Devices.
    element("device-panel", Container, "The device chain's panel.").parts(&["device"]),
    element("device", Container, "A device's panel in the device chain, its `accent-color` the outline of a selected one; a device group's panel is `device[kind=\"group\"]`.")
        .parts(&["device-header", "device-control", "device-section"])
        .ids(Ids::Prefixed("device-"))
        .attributes(&["selected", "muted", "kind"])
        .settings(&["param-label"]),
    element("device-canvas", Drawn, "The colours a device panel's own drawings take from the theme, read once a frame in the device's scope: its `color` the text, its `background-color` the controls, its `border-color` the lines, its `accent-color` the accent; `device-canvas[kind=\"secondary\"]`, `[kind=\"value\"]` and `[kind=\"inverse\"]` (`color`) the other texts, `[kind=\"panel\"]`, `[kind=\"window\"]` and `[kind=\"active\"]` (`background-color`) the other fills; a control's own rule colours its drawing over these.")
        .attributes(&["kind"]),
    element("device-header", Part, "A device panel's header row: its `background-color`, and its `color` for the name."),
    element("device-section", Part, "A section of a device panel; a node of an expanded graph."),
    element("device-control", Control, "A control of a device panel, by its id; where the device gives no colours, a parameter's name is `device-control[kind=\"name\"]`, its value `[kind=\"value\"]` and a knob `[kind=\"knob\"]` (its disc the `background-color`, its ring the `color`, its pointer the `accent-color`).")
        .ids(Ids::Any)
        .attributes(&["kind"]),
    element("xy-control", Control, "A two-dimensional control of a device panel."),
    element("envelope", Drawn, "An envelope a device panel shows."),
    element("waveform", Drawn, "A waveform a panel shows."),
    element("slider", Control, "A parameter's slider (`slider[kind=\"parameter\"]`); the MIDI tools' quantise amount, as a knob (`slider[kind=\"quantise\"]`, its value's `color` and its pointer's `accent-color`); a swing amount, `[checked]` while it is the clip's own; the Arrangement's waveform zoom is `slider[kind=\"zoom\"]`, the metronome's level slider is in `popup[kind=\"level\"]`.")
        .parts(&["slider-rail", "slider-fill", "slider-handle"])
        .attributes(&["checked", "kind"])
        .ids(Ids::Listed { prefix: "", names: &["quantise-amount", "waveform-zoom"] })
        .settings(VALUE_SETTINGS)
        .artwork(&[own("slider.rail"), own("slider.fill"), own("slider.handle")]),
    element("slider-rail", Part, "A slider's rail."),
    element("slider-fill", Part, "A slider's fill up to its value."),
    element("slider-handle", Part, "A slider's handle."),
    // Controls everywhere.
    element("button", Control, "A button or switch; also egui's check boxes and drop-downs. A device's on switch is `#control-device-on` (`[checked]` while it runs), which a rule for every `button` reaches as it does any button; an expanded panel's × `button[kind=\"close\"]` and the browser's more tags `button[kind=\"more\"]` take only a rule for their kind. The toggles Suhina's own views draw (a track's on switch, cue, a view's switches) are `button[kind=\"toggle\"]`, solo `button[kind=\"solo\"]` and arm `button[kind=\"arm\"]`, lit with `[checked]`: their base look is dim words on the control fill, and the chosen fill and its text when lit; a rule for every `button` overrides it for the properties it sets, lit ones too, so state the lit look as well. The Launcher's, the mixer's and the Arrangement's own switches are `button[kind=\"record\"]` (the control bar's Record, `[checked]` while it records), `button[kind=\"metronome\"]` (the metronome's switches, `[checked]` while it clicks), `button[kind=\"monitor\"]` (a strip's In, Auto and Off, `[checked]` on the one chosen), `button[kind=\"envelope\"]` (a clip's Edit gain envelope and Free move), `button[kind=\"lanes\"]` (a track's automation switch, `[checked]` while its lanes show) and `button[kind=\"shape\"]` (an automation lane's shape pick, `[checked]` on the chosen one). The detail panels' and the browser's own: a panel's push button `button[kind=\"panel\"]` and switch `[kind=\"panel-toggle\"]`, an entry of a panel's list or of a right-click menu the kit draws (the metronome's) `[kind=\"entry\"]` (`[checked]` the chosen one), the browser's filter chips `[kind=\"chip\"]`, a tag's tick box `[kind=\"tag-check\"]`, the preview's play `[kind=\"preview\"]` and the piano roll's Focus switch `[kind=\"focus\"]`, each lit with `[checked]`. The app's own: the toolbar's Map `button[kind=\"map\"]` (`[checked]` while keys are being mapped), a connected agent's name `[kind=\"agent\"]`, Play `[kind=\"play\"]` (`#control-play`, `[playing]` while the transport runs), the browser's switch `[kind=\"browser\"]` (`#control-browser-toggle`, `[checked]` while it shows), a button that stands back `[kind=\"secondary\"]`, a tick box `[kind=\"check\"]`, a part of a row of choices in Settings `[kind=\"option\"]` or `[kind=\"preset\"]`, a yes/no switch `[kind=\"switch\"]`, each lit with `[checked]`.")
        .ids(Ids::Listed { prefix: "control-", names: CONTROL_IDS })
        .attributes(&["checked", "playing", "recording", "armed", "muted", "kind"])
        .artwork(&[
            default_svg!("button", "button"),
            default_svg!("play", "icon[kind=\"play\"]"),
            default_svg!("stop", "icon[kind=\"stop\"]"),
            default_svg!("record", "icon[kind=\"record\"]"),
            default_svg!("solo", "icon[kind=\"solo\"]"),
            default_svg!("arm", "icon[kind=\"arm\"]"),
            default_svg!("cue", "icon[kind=\"cue\"]"),
            own("track-on"),
            own("automation"),
            own("monitor"),
        ]),
    element("toggle", Drawn, "The colours of a switch its view draws itself, with no `button` of its own (a clip panel's On and Off, a device group's Mute): dim words, `[checked]` when it is on; the `button[kind=\"toggle\"]` of a switch drawn as a button is looked up the same way.")
        .attributes(&["checked"]),
    element("value", Control, "A value field, such as a volume bar or a clip panel's number field; drawn as a bar, its track is `value[kind=\"bar\"]`; a field of the clip, MIDI and follow panels and of a device group is `value[kind=\"field\"]` (tinted under the pointer, stronger while dragged), a device's parameter's value `value[kind=\"parameter\"]`.")
        .parts(&["value-fill", "value-handle"])
        .attributes(&["kind"])
        .ids(Ids::Listed { prefix: "", names: LEVEL_FIELDS })
        .settings(VALUE_SETTINGS)
        .artwork(&[own("volume.track"), own("volume.fill"), own("volume.handle")]),
    element("value-fill", Part, "The lit part of a level bar."),
    element("value-handle", Part, "The handle where a level bar's fill ends."),
    element("pan", Control, "A pan control; drawn as a field, its centre's ticks are `pan-centre` and the bar from the centre its `value-fill`; drawn as a bar, its track is `pan[kind=\"bar\"]`.")
        .parts(&["pan-centre", "value-fill"])
        .attributes(&["kind"])
        .ids(Ids::Listed { prefix: "", names: LEVEL_FIELDS })
        .settings(VALUE_SETTINGS)
        .artwork(&[
            own("pan.track"),
            own("pan.fill"),
            own("pan.handle"),
            own("pan.knob.body"),
            own("pan.knob.pointer"),
        ]),
    element("pan-centre", Drawn, "The ticks at a pan field's centre."),
    element("fader", Control, "A mixer strip's fader.")
        .parts(&["fader-cap", "fader-rail", "fader-scale"])
        .ids(Ids::Listed { prefix: "", names: LEVEL_FIELDS })
        .settings(VALUE_SETTINGS)
        .artwork(&[default_svg!("fader.cap", "fader-cap"), default_svg!("fader.rail", "fader-rail")]),
    element("fader-cap", Part, "A fader's cap; its artwork takes its `color`."),
    element("fader-rail", Part, "A fader's rail, the mixer's and the clip gain fader's (`fader-rail[kind=\"clip\"]`, its slot the `background-color`); its artwork takes its `color`.")
        .attributes(&["kind"]),
    element("fader-scale", Part, "The scale beside a fader, the mixer's and the clip gain fader's (`fader-scale[kind=\"clip\"]`).")
        .attributes(&["kind"]),
    element("send", Control, "A send, in a mixer strip and in a track header.")
        .ids(Ids::Prefixed("mixer-send-"))
        .settings(VALUE_SETTINGS)
        .artwork(&[own("send"), own("send.fill"), own("send.knob.body"), own("send.knob.pointer")]),
    element(
        "knob",
        Control,
        "A knob; the dial of a device canvas (the reverb's and the delay's) is `knob[kind=\"dial\"]`; a device panel's own knob face is never replaced.",
    )
    .parts(&["knob-body", "knob-pointer", "knob-ring"])
    .attributes(&["kind"])
        .settings(VALUE_SETTINGS)
        .artwork(&[
            default_svg!("knob.body", "knob-body"),
            default_svg!("knob.pointer", "knob-pointer"),
        ]),
    element("knob-body", Part, "A knob's body; its artwork takes its `color`."),
    element("knob-pointer", Part, "A knob's pointer, turned to its value; its artwork takes its `color`."),
    element("knob-ring", Part, "The disc between a dial's rim and its inner disc: its `background-color`, where the region the dial is in gives none."),
    element("icon", Part, "The symbol on a button or a track header, by its `kind` attribute (`play`, `midi`); the piano roll's tool pointer (`pointer`, its ring the `background-color`) and its magnifier (`magnifier`), a clip's colour swatch (`swatch`), a device's folder (`folder`).")
        .attributes(&["kind"]),
    element("meter", Control, "A level meter; the control bar's CPU meter is `meter[kind=\"cpu\"]`, its `background-color` the track under its level.")
        .attributes(&["kind"])
        .parts(&["meter-level", "meter-clip"])
        .settings(&["meter-style", "meter-segments", "meter-intensity"])
        .artwork(&[default_svg!("meter", "meter")]),
    element("meter-level", Part, "A meter's lit part; its `color` is the level's, `meter-level[kind=\"hot\"]` near the top.")
        .attributes(&["kind"]),
    element("meter-clip", Drawn, "A meter's clip light; its `background-color` while it is lit."),
    element("meter-vertical", Part, "A meter standing up, around its level."),
    element("meter-horizontal", Part, "A meter lying down, around its level."),
    element("dropdown", Control, "A drop-down menu's button; also a clip panel's lists. A routing field in a track header, which takes the colours of what it is on, is `dropdown[kind=\"subtle\"]`: only a rule for that kind reaches it. A clip panel's list is `dropdown[kind=\"panel\"]`.")
        .parts(&["dropdown-arrow"])
        .attributes(&["kind"])
        .artwork(&[default_svg!("folder", "icon[kind=\"folder\"]")]),
    element("dropdown-arrow", Part, "The arrow of a drop-down field, in its `color`; a rule for the field's `color` does not reach it. A clip panel's list's is `dropdown-arrow[kind=\"panel\"]`.")
        .attributes(&["kind"]),
    element("text-input", Control, "A text field; a number typed into a value control is `text-input[kind=\"typed\"]`; the browser's fields are `#browser-search` and `#browser-new-folder`.")
        .attributes(&["kind"])
        .ids(Ids::Listed { prefix: "", names: &["browser-search", "browser-new-folder"] }),
    element("scroll-bar", Drawn, "The mark of how far a row is scrolled, such as a mixer strip's sends or the device chain (`scroll-bar[kind=\"chain\"]`): its `background-color` the track, its `color` the thumb.")
        .attributes(&["kind"]),
    element("label", Drawn, "A label, its `color` inherited; a scene's number is `label[kind=\"scene-number\"]`. The browser's and the detail panels' words by how much they stand out: dim words `hint`, a control's or a figure's name `caption` (these two take only a colour a rule gives them, not one around them), a panel's title `title`, a value shown as words `value`, a fault `error`, a plug-in's state `plugin-state`, a device's parameter's name `parameter`, a check box's words `check`, words or a mark in the accent (a file found, a key being set, the toolbar editor's grid) `accent`, the light and the dark ink for text on a colour (a clip's, a track's), whichever reads, `on-light` and `on-dark`, a plug-in that is ready `ok`, a file that needs attention `warning`, words on a lit fill `inverse`, and the head of the MIDI tools' figures `head` (`[selected]` while a selection is what they act on, in its `accent-color`). Inside the browser, `hint`, `caption` and `value` take the browser's own colours.")
        .attributes(&["kind", "selected"]),
    element("separator", Drawn, "A rule between parts, by its `kind`: the window's lines over and under the views; a divider between a bar's items (`bar`); the Launcher's lines between its bands (`band`), its columns (`column`) and its slots (`slot`); the Arrangement's and the piano roll's under their rows (`row`), between their bands (`band`) and under each lane (`lane`); a line in a clip panel's list or a right-click menu the kit draws (`menu`); the lines over a device inside a device group (`group`); an expanded panel's lines between its columns (`column`); the browser's lines between its parts (`row`); the line under the place of a number a drag steps (`step`, `[selected]` while it is held).")
        .ids(Ids::Listed { prefix: "", names: &["views-top", "views-bottom"] })
        .attributes(&["kind", "selected"]),
    element("popup", Region, "A menu, right-click menu, dropdown list or control's popup; the metronome's level, with its slider, is `popup[kind=\"level\"]`.")
        .ids(Ids::Listed { prefix: "", names: &["metronome-level"] })
        .attributes(&["kind"])
        .settings(FLOATING),
    element("tooltip", Region, "A tooltip, and the pill that follows the pointer while something is dragged: its `background-color`, its `color`, its `border-color` and, when what is under the pointer takes the drop, its `accent-color` for its edge.").settings(FLOATING),
    element("progress", Drawn, "A progress bar: a dialog's, the sample map's busy card and the MIDI panel's range bar.").parts(&["progress-fill"]),
    element("progress-fill", Part, "A progress bar's done part, in each of those."),
    element("colour-field", Control, "The colour field or wheel, with its border and keyboard focus outline.").attributes(&["kind"]),
    // Pages and dialogs.
    element("settings", Region, "The Settings page; docked over the views, `backdrop-filter: blur(<length>)` blurs them behind it.").parts(&["settings-nav"]).settings(FLOATING),
    element("settings-nav", Control, "An entry of the Settings page's list, `[checked]` for the page shown.").attributes(&["checked"]),
    element("dialog", Region, "A dialog, named by its `name`: `about`, `agent`, `ara-edits` (Another track), `backup`, `closing`, `color-picker` (the track colour window), `export`, `folder-drop`, `group-removal`, `loading`, `missing-files`, `move-folder`, `name-clash`, `project` (New or open), `save`, `splash`, `theme-exists`, `unsaved` and `updates`. Its `padding` is the room between its edge and what it holds; its title row runs along its top inside its border, outside the padding.")
        .parts(&["dialog-title", "dialog-actions", "list-row"])
        .attributes(&["name"])
        .settings(FLOATING),
    element("dialog-title", Part, "A dialog's title row, with its Close button: its `background-color`, its words `label[kind=\"title\"]`."),
    element("dialog-actions", Part, "A dialog's row of buttons along the bottom of what it holds, laid out as a flex row: `justify-content` and `gap` place them, and a `flexible-space` in it, where a dialog puts one, takes the room left."),
    element("colour-swatch", Control, "A swatch of the colour picker, in a track's or a clip's menu: its `border-color` outlines the chosen one and the one under the pointer, and the square of the colour beside a picker's name, `colour-swatch[kind=\"square\"]`.").attributes(&["kind"]),
    element("list-row", Control, "A row of a list in a dialog or on the start page that a click picks (a recent project, a template): its `background-color` under the pointer, its `color` for its name. A dialog's row is `list-row[kind=\"dialog\"]`, a row of the Plug-in manager's table `list-row[kind=\"plugin\"]` (its fill while picked and under the pointer), a row of the missing files panel's table `list-row[kind=\"missing-file\"]` (the same) and the colour picker's row beside a name `list-row[kind=\"picker\"]`.").attributes(&["selected", "kind"]),
    element("start-page", Region, "The page shown with no project open."),
    element("plugin-manager", Region, "The Plug-in manager."),
    element("theme-card", Control, "A theme's card in the theme picker, `[selected]` for the theme in use: its `border-color` for its edge (and for a swatch's ring), its `background-color` for the tint under the theme in use and its `accent-color` for the word *Active*.")
        .parts(&["theme-swatch"])
        .attributes(&["selected"]),
    element("theme-swatch", Drawn, "A theme's swatch in the theme picker, with a `color` attribute.").attributes(&["color"]),
    element("app-fill", Drawn, "The fill the app's bars, Settings, dialogs and menus paint for themselves where the region behind them gives none: its `background-color`, `app-fill[kind=\"window\"]` behind a dialog and under a backdrop, `[kind=\"panel\"]` for a bar, a card or a dialog's panel, `[kind=\"panel-hover\"]` for a row under the pointer, `[kind=\"control\"]`, `[kind=\"control-hover\"]` and `[kind=\"control-active\"]` for a control the app draws itself, `[kind=\"knob\"]` the knob of a yes/no switch that is on where the window behind it is see-through.")
        .attributes(&["kind"]),
    element("widget", Drawn, "egui's own widgets (the fields, drop-downs, check boxes and entries of Settings, menus and dialogs, egui's windows) where no rule for `button`, `text-input`, `menu` or `scroll-bar` gives them a colour: its `background-color` and `color` at rest, `:hover` and `:active`, `widget[kind=\"static\"]` what egui draws that takes no input (its separators and frames in its `border-color`, its labels in its `color`; no `button` rule reaches it), `widget[checked]` the selection's fill and its `accent-color` the selection's outline, `widget[kind=\"window\"]` the fill and edge of egui's windows and menus and of the panels, `widget[kind=\"field\"]` the fill behind a text field. Its words at rest are `--text-value`, where a `button`'s are `--text-primary`.")
        .attributes(&["checked", "kind"]),
];

/// The element named `name`.
pub fn get(name: &str) -> Option<&'static Element> {
    ELEMENTS.iter().find(|element| element.name == name)
}

/// Whether the style tree has an element named `name`.
pub fn is_element(name: &str) -> bool {
    get(name).is_some()
}

/// Whether an element's background is the surface behind its controls rather than their fill:
/// regions and containers.
pub fn is_surface(name: &str) -> bool {
    get(name).is_some_and(|element| matches!(element.kind, Kind::Region | Kind::Container))
}

/// The names of the buttons a theme selects as `#control-NAME`.
pub fn control_ids() -> &'static [&'static str] {
    match get("button").map(|button| button.ids) {
        Some(Ids::Listed { names, .. }) => names,
        _ => &[],
    }
}

/// The id a theme selects an element named `name` by, when the app gave it `given` and the
/// registry says it keeps it (`#control-play`, `#device-compressor`); otherwise `None`, and the
/// element gets an id of its own.
pub fn element_id(name: &str, given: Option<&str>) -> Option<String> {
    let given = given?;
    match get(name)?.ids {
        Ids::None => None,
        Ids::Listed { prefix, names } => names.contains(&given).then(|| format!("{prefix}{given}")),
        Ids::Prefixed(prefix) => given.starts_with(prefix).then(|| given.to_owned()),
        Ids::Any => Some(given.to_owned()),
    }
}

/// Every artwork role, with Suhina's own artwork where it has some.
pub fn artwork() -> impl Iterator<Item = &'static Artwork> {
    ELEMENTS.iter().flat_map(|element| element.artwork.iter())
}

/// The roles Suhina ships artwork for, and that artwork.
pub fn default_artwork() -> impl Iterator<Item = (&'static str, &'static str)> {
    artwork().filter_map(|art| art.default.map(|svg| (art.role, svg)))
}

/// How many roles Suhina ships artwork for.
pub const DEFAULT_ARTWORK: usize = {
    let mut count = 0;
    let mut at = 0;
    while at < ELEMENTS.len() {
        let mut role = 0;
        while role < ELEMENTS[at].artwork.len() {
            if ELEMENTS[at].artwork[role].default.is_some() {
                count += 1;
            }
            role += 1;
        }
        at += 1;
    }
    count
};

/// The role a sheet's `image` rule gives artwork to, by its selector: an element of a role's
/// (`knob-body`, `icon[kind="play"]`) or a role as written (`knob.body`, `piano.draw`), with a
/// state after it (`knob-body:hover` is `knob.body.hover`, `button[checked]` and `button:on`
/// are `button.on`). An attribute's value may be written with or without quotes and with spaces
/// (`icon[kind=play]`, `icon[ kind = 'play' ]`). A selector that names no role of Suhina's is
/// returned as it is, for the caller to refuse.
pub fn role_of_selector(selector: &str) -> String {
    let selector = canonical_attributes(selector.trim());
    let (base, state) = [
        (":hover", ".hover"),
        (":active", ".active"),
        (":disabled", ".disabled"),
        (":on", ".on"),
        ("[checked]", ".on"),
    ]
    .iter()
    .find_map(|(written, state)| selector.strip_suffix(written).map(|base| (base, *state)))
    .unwrap_or((selector.as_str(), ""));
    let role = artwork()
        .find(|art| art.default.is_some() && art.selector == base)
        .map_or_else(|| base.to_owned(), |art| art.role.to_owned());
    format!("{role}{state}")
}

/// `selector` with each attribute selector written as Suhina writes it: `[kind="play"]` for
/// `[kind=play]`, `[kind='play']` or `[ kind = "play" ]`, and `[checked]` for `[ checked ]`.
fn canonical_attributes(selector: &str) -> String {
    let mut canonical = String::with_capacity(selector.len() + 4);
    let mut rest = selector;
    while let Some(open) = rest.find('[') {
        canonical.push_str(&rest[..open]);
        let Some(close) = rest[open..].find(']').map(|close| open + close) else {
            canonical.push_str(&rest[open..]);
            return canonical;
        };
        let inner = rest[open + 1..close].trim();
        match inner.split_once('=') {
            Some((name, value)) => {
                let value = value.trim();
                let unquoted = ['"', '\'']
                    .iter()
                    .find_map(|quote| value.strip_prefix(*quote).and_then(|v| v.strip_suffix(*quote)))
                    .unwrap_or(value);
                canonical.push_str(&format!("[{}=\"{unquoted}\"]", name.trim()));
            }
            None => canonical.push_str(&format!("[{inner}]")),
        }
        rest = &rest[close + 1..];
    }
    canonical.push_str(rest);
    canonical
}

/// The selector a sheet writes for `role`'s artwork, with its state (`knob.body.hover` is
/// `knob-body:hover`, `button.on` is `button[checked]`); a role without artwork of Suhina's own,
/// or with a colour or frame in it, is written as it is, states as pseudo-classes.
pub fn selector_of_role(role: &str) -> String {
    for (suffix, written) in
        [(".hover", ":hover"), (".active", ":active"), (".disabled", ":disabled"), (".on", "[checked]")]
    {
        if let Some(base) = role.strip_suffix(suffix) {
            return match artwork().find(|art| art.default.is_some() && art.role == base) {
                Some(art) => format!("{}{written}", art.selector),
                None => format!("{base}{}", if suffix == ".on" { ":on" } else { written }),
            };
        }
    }
    artwork()
        .find(|art| art.default.is_some() && art.role == role)
        .map_or_else(|| role.to_owned(), |art| art.selector.to_owned())
}

/// The element a role belongs to: `knob` for `knob.body`.
pub fn role_element(role: &str) -> Option<&'static Element> {
    ELEMENTS.iter().find(|element| element.artwork.iter().any(|art| art.role == role))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_roles_and_attributes_are_unique_and_well_formed() {
        for (at, element) in ELEMENTS.iter().enumerate() {
            assert!(
                suhina_style::selector::valid_attribute_name(element.name),
                "{} is not an element name",
                element.name
            );
            assert!(!ELEMENTS[..at].iter().any(|other| other.name == element.name), "{} twice", element.name);
            for part in element.parts {
                assert!(is_element(part), "{} has an unknown part {part}", element.name);
            }
            for attribute in element.attributes {
                assert!(ATTRIBUTES.iter().any(|(name, _)| name == attribute), "{}[{attribute}]", element.name);
            }
            assert!(element.description.ends_with('.'), "{}", element.name);
        }
        let roles: Vec<_> = artwork().map(|art| art.role).collect();
        for (at, role) in roles.iter().enumerate() {
            assert!(!roles[..at].contains(role), "the role {role} twice");
        }
        assert_eq!(default_artwork().count(), DEFAULT_ARTWORK);
        assert_eq!(DEFAULT_ARTWORK, 17);
        // The element that draws a role's default artwork is in the registry.
        for art in artwork().filter(|art| art.default.is_some()) {
            let name = art.selector.split(['[', ':']).next().unwrap_or_default();
            assert!(is_element(name), "{} is drawn by {}, which is not an element", art.role, art.selector);
        }
    }

    #[test]
    fn artwork_selectors_and_roles_turn_into_each_other() {
        for (selector, role) in [
            ("knob-body", "knob.body"),
            ("knob-body:hover", "knob.body.hover"),
            ("icon[kind=\"play\"]", "play"),
            ("button[checked]", "button.on"),
            ("fader-cap:active", "fader.cap.active"),
            ("track-header", "track.body"),
        ] {
            assert_eq!(role_of_selector(selector), role, "{selector}");
            assert_eq!(selector_of_role(role), selector, "{role}");
        }
        // Roles of an earlier form and roles without default artwork stay as they are written.
        assert_eq!(role_of_selector("knob.body:hover"), "knob.body.hover");
        assert_eq!(role_of_selector("solo:on"), "solo.on");
        assert_eq!(role_of_selector("piano.draw"), "piano.draw");
        // An attribute's value without quotes, in single quotes or with spaces names the same role.
        for selector in ["icon[kind=play]", "icon[kind='play']", "icon[ kind = \"play\" ]"] {
            assert_eq!(role_of_selector(selector), "play", "{selector}");
        }
        assert_eq!(role_of_selector("icon[kind=solo][ checked ]"), "solo.on");
        assert_eq!(role_of_selector("icon[kind=stop]:hover"), "stop.hover");
        assert_eq!(selector_of_role("piano.draw:on"), "piano.draw:on");
        assert_eq!(selector_of_role("piano.draw.on"), "piano.draw:on");
    }

    #[test]
    fn ids_follow_each_elements_rule() {
        assert_eq!(element_id("button", Some("play")).as_deref(), Some("control-play"));
        assert_eq!(element_id("button", Some("ok")), None);
        assert_eq!(element_id("device", Some("device-compressor")).as_deref(), Some("device-compressor"));
        assert_eq!(element_id("device", Some("compressor")), None);
        assert_eq!(element_id("device-control", Some("cutoff")).as_deref(), Some("cutoff"));
        assert_eq!(element_id("value", Some("main-volume")).as_deref(), Some("main-volume"));
        assert_eq!(element_id("toolbar-module", Some("tempo")).as_deref(), Some("tempo"));
        assert_eq!(element_id("clip", Some("clip-3")), None);
        assert_eq!(element_id("nothing", Some("x")), None);
        assert!(control_ids().contains(&"metronome-toggle"));
    }

    #[test]
    fn regions_and_containers_are_surfaces() {
        for name in ["window", "toolbar", "arrangement", "track-header", "device", "popup"] {
            assert!(is_surface(name), "{name}");
        }
        for name in ["button", "knob", "clip", "toolbar-module", "piano-note"] {
            assert!(!is_surface(name), "{name}");
        }
        assert_eq!(role_element("knob.body").map(|e| e.name), Some("knob"));
        assert_eq!(role_element("piano.draw").map(|e| e.name), Some("piano-tool"));
    }

    /// The reference lists `backdrop-filter` for the elements the style engine reads it on.
    #[test]
    fn the_floating_elements_are_those_backdrop_filter_reads() {
        let listed: Vec<&str> =
            ELEMENTS.iter().filter(|e| e.settings.contains(&"backdrop-filter")).map(|e| e.name).collect();
        let mut read = suhina_style::property::BACKDROP_FILTERED.to_vec();
        let mut listed = listed;
        read.sort_unstable();
        listed.sort_unstable();
        assert_eq!(listed, read);
    }
}
