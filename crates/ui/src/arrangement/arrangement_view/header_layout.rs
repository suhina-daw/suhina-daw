// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A track header's box tree (#1453): the app says which parts the header has room for, how big
//! each is and which of its two lines holds it (by the header's width and height,
//! `docs/design/components.md` › Track header levels); the style sheet places them, the user-agent
//! sheet as Suhina's own layout did. Painting and the pointer read the rectangles it gives.

use super::*;
use crate::kit::native_css::row::{self, Bar, Item};

type Tree = row::Tree<Slot>;

/// What a header's layout depends on besides its size.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Header {
    /// Groups and folders reserve a full-height disclosure gutter facing the timeline.
    pub(super) container: bool,
    pub(super) headers_left: bool,
    /// How deep in groups the track is: the band of its own color gets narrower.
    pub(super) depth: usize,
    /// How many groups it is in, whose bands run down its indent.
    pub(super) ancestors: usize,
    pub(super) armable: bool,
    /// Whether it plays notes: it has a MIDI monitor button where there is room.
    pub(super) notes: bool,
    /// Whether it has take lanes: a button shows and hides them.
    pub(super) takes: bool,
    /// How many sends it has.
    pub(super) sends: usize,
}

/// Where a header's parts go; a part the header has no room for, or the sheet hides, is `None`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Parts {
    /// The groups' bands down the indent (`group#header-indent`), and the scale they are drawn at.
    pub(super) indent: Option<Rect>,
    pub(super) indent_scale: f32,
    /// The band of the track's own color (`track-color`).
    pub(super) band: Option<Rect>,
    pub(super) meter: Option<Rect>,
    pub(super) fold: Option<Rect>,
    pub(super) disclosure: Option<Rect>,
    pub(super) automation: Option<Rect>,
    pub(super) takes: Option<Rect>,
    pub(super) track_on: Option<Rect>,
    pub(super) solo: Option<Rect>,
    pub(super) cue: Option<Rect>,
    pub(super) arm: Option<Rect>,
    pub(super) monitor: Option<Rect>,
    pub(super) name: Option<Rect>,
    pub(super) volume: Option<Rect>,
    pub(super) pan: Option<Rect>,
    /// The sends shown, from the first shown on.
    pub(super) sends: Vec<Option<Rect>>,
    pub(super) previous: Option<Rect>,
    pub(super) next: Option<Rect>,
    /// Where the audio goes, and the input under it.
    pub(super) output: Option<Rect>,
    pub(super) input: Option<Rect>,
    /// How many sends the header has room for, and whether it scrolls them with ‹ and ›.
    pub(super) shown_sends: usize,
    pub(super) navigation: bool,
    /// The header's look by its size: its rows' height and its name's font size.
    pub(super) row: f32,
    pub(super) name_size: f32,
    /// The attributes the header element takes for its look: `layout`, `size` and `name-first`.
    /// They change only where the header crosses one of its sizes, so a drag of the header column
    /// changes the style tree only there (#1489).
    pub(super) words: (&'static str, &'static str, bool),
}

impl Parts {
    /// A container owns a full-height layout column. A theme that omits that column keeps
    /// the old free-gutter fallback, stopping before any neighbouring control.
    pub(super) fn fold_target(&self, bounds: Rect) -> Option<Rect> {
        if let Some(disclosure) = self.disclosure {
            return Some(disclosure.intersect(bounds));
        }
        let fold = self.fold?;
        let mut target =
            Rect::from_min_max(Pos2::new(fold.left(), bounds.top()), Pos2::new(fold.right(), bounds.bottom()));
        for control in [
            self.indent,
            self.band,
            self.meter,
            self.automation,
            self.takes,
            self.track_on,
            self.solo,
            self.cue,
            self.arm,
            self.monitor,
            self.name,
            self.volume,
            self.pan,
            self.previous,
            self.next,
            self.output,
            self.input,
        ]
        .into_iter()
        .chain(self.sends.iter().copied())
        .flatten()
        {
            if control.left() < target.right() && control.right() > target.left() {
                if control.bottom() <= fold.top() {
                    target.min.y = target.top().max(control.bottom() + 1.0);
                } else if control.top() >= fold.bottom() {
                    target.max.y = target.bottom().min(control.top() - 1.0);
                } else {
                    return Some(fold.intersect(bounds));
                }
            }
        }
        Some(target.intersect(bounds))
    }
}

/// The header's arrangement: which line holds what (`track-header[layout]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    /// One row: the buttons, the name and a volume field.
    Short,
    /// Two rows in a narrow header: the name, then the buttons and the volume.
    Narrow,
    /// Two rows: the buttons with the volume and pan beside them, the name and sends.
    Side,
    /// Two rows: the buttons and the name, then the volume, the sends and the pan.
    Stacked,
}

impl Shape {
    fn word(self) -> &'static str {
        match self {
            Self::Short => "short",
            Self::Narrow => "narrow",
            Self::Side => "side",
            Self::Stacked => "stacked",
        }
    }
}

/// The width of the band of a track `depth` groups deep, and of a group's band down the indent.
pub(super) fn nesting_band_width(depth: usize) -> f32 {
    match depth {
        0 => COLOR_BAND,
        1 => 4.0,
        2 => 3.0,
        _ => 2.0,
    }
}

/// What the header `el` (entered, or not where no style engine runs) lays out in `rect`, its whole
/// box: the parts of a track `header`.
pub(super) fn layout(el: Option<&crate::kit::element::Entered>, rect: Rect, header: Header) -> Parts {
    let mut bar = el.map_or_else(|| Bar::strip("track-header", rect.size()), |el| Bar::kind_of(el, rect.size()));
    // The header's border and padding, as its layout gives them: what it holds goes inside.
    let insets = row::insets(&bar).unwrap_or([0.0; 4]);
    let inside = row::inset(rect, insets);
    let spacious = inside.height() >= 60.0;
    let tiny = inside.height() < 20.0;
    let header_row = if tiny {
        (inside.height() - 2.0).max(1.0)
    } else if spacious {
        20.0
    } else {
        HEADER_ROW
    };
    let field = if spacious { 20.0 } else { FIELD };
    let (size, pad, gap) = if tiny {
        ("tiny", 1.0, 4.0)
    } else if inside.height() < 48.0 {
        ("small", 2.0, 4.0)
    } else if spacious {
        ("spacious", 6.0, 10.0)
    } else {
        ("normal", HEADER_PAD_Y, PAD)
    };
    // The groups' bands keep the controls their room in a narrow column.
    let indent: f32 = (0..header.ancestors).map(|level| nesting_band_width(level) + 2.0).sum();
    let indent_room = (inside.width() - COLOR_BAND - 3.0 * PAD - meter::WIDTH - 114.0).max(0.0);
    let indent_scale = if indent > 0.0 { (indent_room / indent).min(1.0) } else { 1.0 };
    let band = nesting_band_width(header.depth);
    let gutter = if header.container { FOLD + 2.0 } else { 0.0 };
    let fold_in_row = if header.container { 0.0 } else { FOLD };
    let x0 = inside.left() + indent * indent_scale + band + PAD + gutter;
    let x1 = inside.right() - PAD - meter::WIDTH - PAD;
    let width = x1 - x0;
    let tall = inside.height() >= SHOWS_MIXER;
    let narrow = tall && width < fold_in_row + 2.0 + SOLO + PAD + ON_TOGGLE + 3.0 * (SOLO + 2.0) + PAD + LEAST_NAME;
    let solo = if narrow || !tall { 18.0 } else { SOLO };
    let on = if narrow || !tall { 24.0 } else { ON_TOGGLE };
    let name_first =
        tall && width - fold_in_row - 2.0 - on - 3.0 * (solo + 2.0) - PAD - PAN_FIELD - 4.0 >= LEAST_VOLUME;
    // Where the buttons' row starts and ends, as the shapes below lay it out.
    let first = if narrow {
        x0
    } else if name_first {
        x0 + fold_in_row + 2.0
    } else {
        x0 + fold_in_row + 2.0 + solo + PAD
    };
    let monitor_width = if tall { 32.0 } else { 24.0 };
    let buttons_end = first + on + 2.0 * (solo + 2.0) + if header.armable { solo + 2.0 } else { 0.0 };
    let monitor_end = if header.takes && !narrow { x1 - solo - 4.0 } else { x1 };
    // A narrow header's volume follows the buttons: the monitor button leaves it some room.
    let monitor = header.notes
        && buttons_end + 2.0 + monitor_width <= monitor_end
        && !(narrow && buttons_end + 2.0 + monitor_width + PAD >= x1);
    let controls_end = buttons_end + if monitor { 2.0 + monitor_width } else { 0.0 };
    let side = tall && x1 - PAN_FIELD - 4.0 - (controls_end + PAD) >= LEAST_VOLUME;
    let shape = if !tall {
        Shape::Short
    } else if narrow {
        Shape::Narrow
    } else if side {
        Shape::Side
    } else {
        Shape::Stacked
    };
    // The sends: as many as leave the name, or the volume, its room.
    let step = SEND_WIDTH + SEND_GAP;
    let below_left = if shape == Shape::Stacked { x0 + fold_in_row + 2.0 } else { first };
    let lanes_right = below_left + (on - solo) / 2.0 + solo + if header.takes { SOLO + 2.0 } else { 0.0 };
    let send_room = match shape {
        Shape::Side => x1 - lanes_right - PAD - LEAST_NAME,
        Shape::Stacked => x1 - PAN_FIELD - 4.0 - below_left - LEAST_VOLUME,
        _ => 0.0,
    };
    let overflow = header.sends > (send_room / step).floor().max(0.0) as usize;
    let navigation = if overflow && send_room >= step + 36.0 { 36.0 } else { 0.0 };
    let shown_sends = header.sends.min(((send_room - navigation) / step).floor().max(0.0) as usize);
    let navigation = navigation > 0.0 && shown_sends > 0;
    // A one-row header keeps a volume field where its name leaves room for one.
    let name_end = if header.takes { x1 - solo - 4.0 } else { x1 };
    let short_name = name_end - (controls_end + PAD);
    let compact_volume =
        (shape == Shape::Short && short_name >= 28.0 + PAD + 48.0).then(|| (short_name * 0.45).clamp(48.0, 80.0));
    // The routing rows under the controls, taller still, where they end inside the header.
    let lines_bottom = inside.top() + pad + header_row + if tall { gap + header_row.max(field) } else { 0.0 };
    let routing_top = lines_bottom + PAD + 4.0;
    let output = inside.height() >= SHOWS_ROUTING && routing_top + DROPDOWN <= inside.bottom() - pad;
    let input =
        output && inside.height() >= SHOWS_MIDI_INPUT && routing_top + 2.0 * DROPDOWN + PAD <= inside.bottom() - pad;
    let meter_height =
        if tall { header_row + gap + field } else { header_row }.min(inside.height() - 2.0 * pad).max(0.0);

    bar = bar
        .attribute("layout", shape.word())
        .attribute("size", size)
        .flag("name-first", name_first)
        .attribute("header-side", if header.headers_left { "left" } else { "right" });
    if header.container {
        bar = bar.attribute("disclosure", if header.headers_left { "right" } else { "left" });
    }
    let mut tree = Tree::default();
    if header.ancestors > 0 {
        tree.add(Item::part("group", "header-indent").width(indent * indent_scale), Slot::Indent);
    }
    tree.add(Item::part("track-color", "arrangement-color").width(band), Slot::Band);
    let disclosure =
        header.container.then(|| tree.add(Item::part("group", "header-disclosure").width(FOLD), Slot::Disclosure));
    let body = tree.add(Item::part("group", "header-body"), Slot::None);
    tree.add(Item::part("meter", "arrangement-meter").width(meter::WIDTH).height(meter_height), Slot::Meter);
    let first_line = tree.add(Item::part("group", "header-first").in_group(body), Slot::None);
    let second_line = tall.then(|| tree.add(Item::part("group", "header-second").in_group(body), Slot::None));
    let button = [Some(solo), Some(header_row)];
    let buttons = |tree: &mut Tree, line: usize| {
        tree.put("button", "track-on", [Some(on), Some(header_row)], line, Slot::TrackOn);
        tree.put("button", "solo", button, line, Slot::Solo);
        tree.put("button", "cue", button, line, Slot::Cue);
        if header.armable {
            tree.put("button", "arm", button, line, Slot::Arm);
        }
        if monitor {
            tree.put("button", "monitor", [Some(monitor_width), Some(header_row)], line, Slot::Monitor);
        }
    };
    let sends = |tree: &mut Tree, line: usize| {
        for index in 0..shown_sends {
            tree.put("send", "arrangement-send", [Some(SEND_WIDTH), Some(field)], line, Slot::Send(index));
        }
        if navigation {
            tree.put("button", "sends-previous", [Some(16.0), Some(field)], line, Slot::Previous);
            tree.put("button", "sends-next", [Some(16.0), Some(field)], line, Slot::Next);
        }
    };
    let name = [None, Some(header_row)];
    tree.put("fold", "arrangement-fold", [Some(FOLD), Some(header_row)], disclosure.unwrap_or(first_line), Slot::Fold);
    match (shape, second_line) {
        (Shape::Short, _) | (_, None) => {
            tree.put("button", "automation", button, first_line, Slot::Automation);
            buttons(&mut tree, first_line);
            tree.put("track-name", "arrangement-name", name, first_line, Slot::Name);
            if let Some(width) = compact_volume {
                tree.put("value", "arrangement-volume", [Some(width), Some(header_row)], first_line, Slot::Volume);
            }
            if header.takes {
                tree.put("button", "takes", button, first_line, Slot::Takes);
            }
        }
        (Shape::Narrow, Some(second)) => {
            tree.put("track-name", "arrangement-name", name, first_line, Slot::Name);
            if header.takes {
                tree.put("button", "takes", button, first_line, Slot::Takes);
            }
            tree.put("button", "automation", button, first_line, Slot::Automation);
            buttons(&mut tree, second);
            tree.put("value", "arrangement-volume", [None, Some(header_row)], second, Slot::Volume);
        }
        (Shape::Side, Some(second)) => {
            let (top, below) = if name_first { (second, first_line) } else { (first_line, second) };
            buttons(&mut tree, top);
            tree.put("value", "arrangement-volume", [None, Some(header_row)], top, Slot::Volume);
            tree.put("value", "arrangement-pan", [Some(PAN_FIELD), Some(header_row)], top, Slot::Pan);
            tree.put("button", "automation", button, below, Slot::Automation);
            if header.takes {
                tree.put("button", "takes", button, below, Slot::Takes);
            }
            tree.put("track-name", "arrangement-name", name, below, Slot::Name);
            sends(&mut tree, below);
        }
        (Shape::Stacked, Some(second)) => {
            let (top, below) = if name_first { (second, first_line) } else { (first_line, second) };
            tree.put("button", "automation", button, top, Slot::Automation);
            buttons(&mut tree, top);
            tree.put("track-name", "arrangement-name", name, top, Slot::Name);
            if header.takes {
                tree.put("button", "takes", button, top, Slot::Takes);
            }
            tree.put("value", "arrangement-volume", [None, Some(field)], below, Slot::Volume);
            sends(&mut tree, below);
            tree.put("value", "arrangement-pan", [Some(PAN_FIELD), Some(field)], below, Slot::Pan);
        }
    }
    if output {
        tree.put("dropdown", "arrangement-output", [None, Some(DROPDOWN)], body, Slot::Output);
        if input {
            tree.put("dropdown", "arrangement-input", [None, Some(DROPDOWN)], body, Slot::Input);
        }
    }
    let mut parts = Parts {
        indent_scale,
        sends: vec![None; shown_sends],
        shown_sends,
        navigation,
        row: header_row,
        words: (shape.word(), size, name_first),
        name_size: if tiny {
            10.0
        } else if spacious {
            theme::TEXT + 2.0
        } else {
            theme::TEXT
        },
        ..Parts::default()
    };
    for (slot, at) in tree.lay_out(&bar, rect.min) {
        match slot {
            Slot::None => {}
            Slot::Indent => parts.indent = at,
            Slot::Band => parts.band = at,
            Slot::Meter => parts.meter = at,
            Slot::Fold => parts.fold = at,
            Slot::Disclosure => parts.disclosure = at,
            Slot::Automation => parts.automation = at,
            Slot::Takes => parts.takes = at,
            Slot::TrackOn => parts.track_on = at,
            Slot::Solo => parts.solo = at,
            Slot::Cue => parts.cue = at,
            Slot::Arm => parts.arm = at,
            Slot::Monitor => parts.monitor = at,
            Slot::Name => parts.name = at,
            Slot::Volume => parts.volume = at,
            Slot::Pan => parts.pan = at,
            Slot::Send(send) => {
                if let Some(place) = parts.sends.get_mut(send) {
                    *place = at;
                }
            }
            Slot::Previous => parts.previous = at,
            Slot::Next => parts.next = at,
            Slot::Output => parts.output = at,
            Slot::Input => parts.input = at,
        }
    }
    parts
}

impl Tree {
    /// A part `name` of `kind` with its content `size` on the line (or the body) at `line`.
    fn put(&mut self, name: &'static str, kind: &str, size: [Option<f32>; 2], line: usize, slot: Slot) {
        let mut item = Item::part(name, kind).in_group(line);
        item.size = size;
        self.add(item, slot);
    }
}

/// Which of [`Parts`] an item of the header's tree is.
#[derive(Clone, Copy)]
enum Slot {
    None,
    Indent,
    Band,
    Meter,
    Fold,
    Disclosure,
    Automation,
    Takes,
    TrackOn,
    Solo,
    Cue,
    Arm,
    Monitor,
    Name,
    Volume,
    Pan,
    Send(usize),
    Previous,
    Next,
    Output,
    Input,
}

#[cfg(test)]
#[path = "header_layout_tests.rs"]
mod tests;
