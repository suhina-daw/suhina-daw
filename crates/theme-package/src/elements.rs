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
];

/// The toolbar's areas, by id (`#tempo`).
pub const TOOLBAR_MODULES: &[&str] = &[
    "browser",
    "tempo",
    "signature",
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
    "track-on",
    "automation",
    "monitor",
];

/// The level fields of the track headers and the mixer, by their placement names.
const LEVEL_FIELDS: &[&str] = &["arrangement-volume", "arrangement-pan", "main-volume", "mixer-volume", "mixer-pan"];

/// Settings of a value a person drags, inherited from its ancestors.
const VALUE_SETTINGS: &[&str] = &["appearance", "value-position", "value-decimals", "value-unit"];

macro_rules! default_svg {
    ($role:literal) => {
        Artwork { role: $role, default: Some(include_str!(concat!("../../../assets/themes/default/", $role, ".svg"))) }
    };
}

const fn own(role: &'static str) -> Artwork {
    Artwork { role, default: None }
}

const NO: &[&str] = &[];
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
    element("window", Region, "The whole window, under everything else.").settings(&[
        "artwork-fps",
        "liquid",
        "panel-gradient",
        "track-gradient",
        "clip-gradient",
        "backdrop-image",
        "backdrop-opacity",
        "backdrop-blur",
    ]),
    element("application-bar", Region, "The bar at the very top, with the application's menus."),
    element("menu-bar", Region, "The menu bar."),
    element("menu", Region, "An open menu of the menu bar."),
    element("project-tabs", Region, "The bar of open projects' tabs."),
    element("toolbar", Region, "The toolbar under the bars, with the transport.").parts(&["toolbar-module"]),
    element("toolbar-module", Part, "One area of the toolbar, by its id.")
        .ids(Ids::Listed { prefix: "", names: TOOLBAR_MODULES }),
    element("status-bar", Region, "The bar at the bottom of the window."),
    element("workspace", Region, "The layout under the bars, holding the panels.")
        .ids(Ids::Listed { prefix: "", names: &["workspace"] }),
    element("workspace-panel", Region, "A panel of the workspace: the browser, the main view, the detail area.")
        .ids(Ids::Listed { prefix: "", names: &["panel-browser", "panel-main", "panel-detail"] }),
    element("divider", Control, "A strip's divider and the workspace's splitters.")
        .ids(Ids::Listed { prefix: "", names: &["mixer-divider", "detail-splitter", "browser-splitter"] }),
    // Views.
    element("browser", Region, "The browser panel.").parts(&[
        "browser-place",
        "browser-row",
        "browser-play",
        "browser-tag-dot",
        "browser-preview-wave",
    ]),
    element("browser-place", Control, "A place or kind on the browser's left.").attributes(&["checked", "selected"]),
    element("browser-row", Control, "A row of the browser's list.").attributes(&["selected"]),
    element("browser-play", Control, "The button that previews a browser row."),
    element("browser-tag-dot", Drawn, "The coloured dot of a tag in the browser."),
    element("browser-preview-wave", Drawn, "The waveform of the sound being previewed."),
    element("session", Region, "The Launcher view: tracks as columns of clip slots.")
        .parts(&["track-column", "scene-column"])
        .settings(&["track-width", "return-width", "scene-width", "slot-height", "header-height"]),
    element("track-column", Container, "A track's column in the Launcher view.")
        .attributes(&["selected", "muted"])
        .settings(&["track-artwork"]),
    element("scene-column", Part, "The column of scenes at the Launcher view's side."),
    element("scene", Control, "A scene's launch button."),
    element("slot", Control, "A clip slot in the Launcher view.").settings(&["track-artwork"]),
    element("arrangement", Region, "The Arrangement view: tracks as rows along a timeline.")
        .parts(&["track-header", "ruler", "loop-region", "playhead", "clip", "add-track"])
        .settings(&["track-headers", "track-height", "header-min-width", "header-tint", "compact-below"]),
    element("track-header", Container, "A track's header, in the Arrangement view and over a Launcher column.")
        .parts(&["track-name", "track-number", "fold"])
        .attributes(&["selected", "armed", "muted"])
        .artwork(&[
            default_svg!("track.midi"),
            default_svg!("track.audio"),
            default_svg!("track.body"),
            own("track.body.session"),
            own("track.body.arrangement"),
        ]),
    element("track-name", Part, "A track's name.").attributes(&["selected"]).artwork(&[
        default_svg!("track.name"),
        own("track.name.session"),
        own("track.name.mixer"),
        own("track.name.arrangement"),
    ]),
    element("track-number", Part, "A track's number in its Arrangement header.").attributes(&["muted"]),
    element("fold", Control, "The triangle that folds a track or a group.")
        .attributes(&["checked"])
        .artwork(&[own("fold")]),
    element("add-track", Control, "The buttons that add a track, after the last one.")
        .ids(Ids::Prefixed("add-"))
        .artwork(&[own("add-track")]),
    element("clip", Control, "An arrangement clip.")
        .parts(&["clip-name", "clip-waveform", "clip-notes", "clip-gain", "clip-fade-handle"])
        .attributes(&["selected", "muted", "playing", "recording"])
        .settings(&["track-artwork"]),
    element("clip-name", Part, "An arrangement clip's name bar."),
    element("clip-waveform", Part, "An arrangement clip's waveform."),
    element("clip-notes", Part, "An arrangement clip's notes."),
    element("clip-gain", Control, "An arrangement clip's gain field.").settings(VALUE_SETTINGS),
    element("clip-fade-handle", Control, "An arrangement clip's fade handles.").artwork(&[own("clip.fade-handle")]),
    element("ruler", Drawn, "The Arrangement view's ruler.")
        .parts(&["ruler-number", "ruler-tick"])
        .artwork(&[own("ruler")]),
    element("ruler-number", Drawn, "A bar number on the ruler."),
    element("ruler-tick", Drawn, "A line on the ruler."),
    element("loop-region", Control, "The loop bar.").attributes(&["checked"]).artwork(&[own("loop-region")]),
    element("loop-line", Drawn, "The lines at the loop's ends."),
    element("playhead", Drawn, "The play position line."),
    element("mixer", Region, "The mixer, and a mixer strip in any view.")
        .parts(&["fader", "pan", "send", "meter"])
        .attributes(&["selected", "muted"]),
    element("detail", Region, "The detail area under the main view."),
    element("clip-editor", Region, "The clip editor in the detail area.").parts(&["editor-tab"]),
    element(
        "editor-tab",
        Control,
        "The clip editor's Clip and Devices tabs, and the MIDI tools part's Notes and Transform tabs.",
    )
    .ids(Ids::Prefixed("tab-"))
    .attributes(&["checked"]),
    element("piano-roll", Region, "The piano roll.").parts(&[
        "piano-grid",
        "piano-key",
        "piano-note",
        "velocity",
        "piano-tool",
        "midi-tools",
        "clip-info",
    ]),
    element("piano-grid", Drawn, "The piano roll's grid."),
    element("piano-row", Drawn, "A row of the piano roll."),
    element("piano-row-black", Drawn, "A row of a black key."),
    element("piano-key", Drawn, "A white key.").attributes(&["selected", "playing"]),
    element("piano-key-black", Drawn, "A black key.").attributes(&["selected", "playing"]),
    element("piano-key-c", Drawn, "A C key."),
    element("piano-key-hover", Drawn, "A white key under the pointer."),
    element("piano-key-black-hover", Drawn, "A black key under the pointer."),
    element("piano-key-line", Drawn, "The line between white keys."),
    element("piano-scale-mark", Drawn, "The mark on a key in the clip's scale."),
    element("piano-scale-strip", Drawn, "The strip left of the keys naming the octaves."),
    element("piano-range", Drawn, "The range a drag on the keys selects."),
    element("piano-clip-strip", Drawn, "The strip over the ruler showing several picked clips."),
    element("piano-time-selection", Drawn, "What is selected in time."),
    element("piano-insert-mark", Drawn, "The mark a click leaves where a paste goes."),
    element("piano-note", Drawn, "A note.").attributes(&["selected", "muted"]),
    element("velocity", Drawn, "A note's velocity stem."),
    element("piano-tool", Control, "A tool button of the piano roll.")
        .ids(Ids::Prefixed("piano-"))
        .attributes(&["checked"])
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
    element("midi-tools", Container, "The MIDI tools part of a MIDI clip's panel.").settings(&["gap"]),
    element("clip-info", Container, "The clip's column beside the piano roll."),
    element("sample-map", Region, "The sampler's sample map."),
    // Devices.
    element("device-panel", Container, "The device chain's panel.").parts(&["device"]),
    element("device", Container, "A device's panel in the device chain.")
        .parts(&["device-header", "device-control", "device-section"])
        .ids(Ids::Prefixed("device-"))
        .attributes(&["selected", "muted"])
        .settings(&["param-label"]),
    element("device-header", Part, "A device panel's header row."),
    element("device-section", Part, "A section of a device panel."),
    element("device-control", Control, "A control of a device panel, by its id.").ids(Ids::Any),
    element("xy-control", Control, "A two-dimensional control of a device panel."),
    element("envelope", Drawn, "An envelope a device panel shows."),
    element("waveform", Drawn, "A waveform a panel shows."),
    element("slider", Control, "A parameter's slider.")
        .parts(&["slider-rail", "slider-fill", "slider-handle"])
        .settings(VALUE_SETTINGS)
        .artwork(&[own("slider.rail"), own("slider.fill"), own("slider.handle")]),
    element("slider-rail", Part, "A slider's rail."),
    element("slider-fill", Part, "A slider's fill up to its value."),
    element("slider-handle", Part, "A slider's handle."),
    // Controls everywhere.
    element("button", Control, "A button; also egui's check boxes and drop-downs.")
        .ids(Ids::Listed { prefix: "control-", names: CONTROL_IDS })
        .attributes(&["checked", "playing", "recording", "armed", "muted"])
        .artwork(&[
            default_svg!("button"),
            default_svg!("play"),
            default_svg!("stop"),
            default_svg!("record"),
            default_svg!("solo"),
            default_svg!("arm"),
            default_svg!("cue"),
            own("track-on"),
            own("automation"),
            own("monitor"),
        ]),
    element("value", Control, "A value field, such as a volume bar.")
        .parts(&["value-fill", "value-handle"])
        .ids(Ids::Listed { prefix: "", names: LEVEL_FIELDS })
        .settings(VALUE_SETTINGS)
        .artwork(&[own("volume.track"), own("volume.fill"), own("volume.handle")]),
    element("value-fill", Part, "The lit part of a level bar."),
    element("value-handle", Part, "The handle where a level bar's fill ends."),
    element("pan", Control, "A pan control.")
        .ids(Ids::Listed { prefix: "", names: LEVEL_FIELDS })
        .settings(VALUE_SETTINGS)
        .artwork(&[
            own("pan.track"),
            own("pan.fill"),
            own("pan.handle"),
            own("pan.knob.body"),
            own("pan.knob.pointer"),
        ]),
    element("fader", Control, "A mixer strip's fader.")
        .parts(&["fader-cap", "fader-rail", "fader-scale"])
        .ids(Ids::Listed { prefix: "", names: LEVEL_FIELDS })
        .settings(VALUE_SETTINGS)
        .artwork(&[default_svg!("fader.cap"), default_svg!("fader.rail")]),
    element("fader-cap", Part, "A fader's cap."),
    element("fader-rail", Part, "A fader's rail."),
    element("fader-scale", Part, "The scale beside a fader."),
    element("send", Control, "A send, in a mixer strip and in a track header.")
        .ids(Ids::Prefixed("mixer-send-"))
        .settings(VALUE_SETTINGS)
        .artwork(&[own("send"), own("send.fill"), own("send.knob.body"), own("send.knob.pointer")]),
    element("knob", Control, "A knob.")
        .settings(VALUE_SETTINGS)
        .artwork(&[default_svg!("knob.body"), default_svg!("knob.pointer")]),
    element("meter", Control, "A level meter.")
        .parts(&["meter-level"])
        .settings(&["meter-style", "meter-segments", "meter-intensity"])
        .artwork(&[default_svg!("meter")]),
    element("meter-level", Part, "A meter's lit part."),
    element("meter-vertical", Part, "A meter standing up, around its level."),
    element("meter-horizontal", Part, "A meter lying down, around its level."),
    element("dropdown", Control, "A drop-down menu's button."),
    element("text-input", Control, "A text field."),
    element("label", Drawn, "A label."),
    element("separator", Drawn, "A rule between parts."),
    element("popup", Region, "A control's popup.").ids(Ids::Listed { prefix: "", names: &["metronome-level"] }),
    element("progress", Drawn, "A progress bar.").parts(&["progress-fill"]),
    element("progress-fill", Part, "A progress bar's done part."),
    // Pages and dialogs.
    element("settings", Region, "The Settings page.").parts(&["settings-nav"]),
    element("settings-nav", Control, "An entry of the Settings page's list."),
    element("dialog", Region, "A dialog."),
    element("start-page", Region, "The page shown with no project open."),
    element("plugin-manager", Region, "The Plug-in manager."),
    element("export", Region, "The Export audio dialog."),
    element("theme-swatch", Drawn, "A theme's swatch in the theme picker, with a `color` attribute."),
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
        assert_eq!(DEFAULT_ARTWORK, 16);
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
}
