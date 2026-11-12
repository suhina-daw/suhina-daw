// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll's toolbar: its tool buttons, the Rows group, and the grid menu; and the
//! workflow entries of the options menu above the keys.

use super::*;

/// What the toolbar's buttons draw from: the clip they act on, the keys they show, how many notes
/// a paste would add, and the Toolbar size they are drawn at.
#[derive(Clone, Copy)]
pub(super) struct ToolbarCx<'a> {
    pub(super) clip: &'a PianoClip,
    pub(super) keymap: &'a crate::settings::keymap::Keymap,
    pub(super) copied: Option<usize>,
    pub(super) scale: f32,
    /// The Toolbar size setting `scale` comes from: `piano-roll-toolbar[size]`.
    pub(super) size: crate::settings::ToolbarSize,
}

/// The toolbar along the top of the piano roll (› Layout): the tools, the note clipboard, quantise,
/// the MIDI tools and the Rows group (Fold, Scale, Highlight) at the left; the workflow and ghost
/// menus, the grid value and the × at the right. Its items lie in one CSS row (#1372). With less
/// room it gives way in steps, never hiding a part outright: first the Grid caption, then the Rows
/// words (their icons stay), then the clipboard folds into an Edit menu, then the ghost menu loses
/// its word. A button that runs something adds to `actions`.
pub(super) fn toolbar(
    ui: &mut Ui,
    rect: Rect,
    cx: ToolbarCx<'_>,
    state: &mut PianoRollState,
    actions: &mut Vec<Action>,
) {
    use crate::kit::native_css::row::{self, Bar};
    let ToolbarCx { clip, keymap, copied, scale, .. } = cx;
    if rect.height() < 4.0 {
        return;
    }
    let size_word = cx.size.word();
    let bar_el = crate::kit::element::PIANO_ROLL_TOOLBAR.attribute("size", size_word).enter();
    let painter = ui.painter_at(rect);
    let fill = crate::kit::style_class::surface_or(theme::panel_background());
    bar_el.paint_with(&painter, rect, crate::kit::element::Native { radius: 0.0, fill });
    painter.hline(rect.x_range(), rect.bottom() - 0.5, theme::hairline(theme::border()));
    let [top, _, bottom, _] = bar_el.insets();
    let inner = (rect.height() - top - bottom).max(1.0);
    // The icon box, the gap and the strokes scale with Toolbar size; the box never outgrows the
    // band, so a small detail area shows the icons and not a clipped toolbar.
    let side = (TOOL_ICON * scale).min(inner);
    // `piano-tool { width; height }` sizes the buttons, so stretched artwork has room. Unstyled,
    // a button is wider than its icon by a little room at each side, so the icons do not crowd.
    let size = {
        let el = crate::kit::element::PIANO_TOOL.enter();
        let own = el.style();
        Vec2::new(
            own.as_ref().and_then(|c| c.width).map_or(side + BUTTON_ROOM * scale, |w| w.clamp(8.0, 96.0)),
            own.as_ref().and_then(|c| c.height).map_or(side, |h| h.clamp(8.0, inner.max(8.0))),
        )
    };
    // The menus as wide as their words, so they sit close by.
    let widths = Widths {
        size,
        side,
        inner,
        workflow: (menu_width(ui, (None, keymap.preset().name())), menu_width(ui, (None, ""))),
        ghosts: (menu_width(ui, (None, "Ghosts")), menu_width(ui, (None, ""))),
        grid: {
            let value = clip.grid_menu.label();
            (menu_width(ui, (Some("Grid"), &value)), menu_width(ui, (None, &value)))
        },
        rows: ROWS.map(|(_, word)| (row_toggle_width(ui, side, Some(word)), row_toggle_width(ui, side, None))),
    };
    let candidates: Vec<_> = Step::ALL.iter().map(|step| items(*step, &widths)).collect();
    let bar = Bar::of(&bar_el, rect.size());
    let Some((chosen, laid)) = row::fitting(&bar, &candidates) else { return };
    let (Some(step), Some(items)) = (Step::ALL.get(chosen).copied(), candidates.get(chosen)) else { return };
    let selected = !state.acting().is_empty();
    let mut rows = [None; 3];
    let mut rows_group = None;
    for (index, item) in items.iter().enumerate() {
        let Some(area) = laid.at(index, rect.min) else { continue };
        let id = item.id.as_deref().unwrap_or("");
        match item.name {
            "separator" => {
                let line = crate::kit::element::SEPARATOR.attribute("kind", "bar").enter();
                let ink = line.color_or(theme::border_strong());
                painter.vline(area.center().x, area.top() + 2.0..=area.bottom() - 2.0, theme::hairline(ink));
            }
            "group" => rows_group = Some(area),
            _ if item.parent.is_some() => {
                let slot = ROWS.iter().position(|(button, _)| button.id() == id).and_then(|at| rows.get_mut(at));
                if let Some(slot) = slot {
                    *slot = Some(area);
                }
            }
            "bar-item" => {
                let _item = crate::kit::element::BAR_ITEM.id(id).attribute("kind", id).enter();
                match id {
                    "piano-roll-workflow" => workflow_menu(ui, area, keymap, state, step < Step::Arrows),
                    "piano-roll-ghosts" => {
                        ghost_view::menu(ui, area, &clip.ghosts, &mut state.ghost_source, step < Step::Arrows);
                    }
                    _ => {
                        if let Some(picked) = grid_menu(ui, area, clip, keymap, step == Step::Full) {
                            state.grid_picked = Some(picked);
                        }
                    }
                }
            }
            _ => match id {
                "piano-edit" => {
                    if let Some(button) =
                        edit_menu(&ButtonPaint { ui, painter: &painter, keymap, scale }, area, selected)
                    {
                        actions.extend(state.clipboard_action(button, clip, copied));
                    }
                }
                _ => {
                    let Some(button) = IconButton::ALL.iter().copied().find(|button| button.id() == id) else {
                        continue;
                    };
                    if let Some(tool) = TOOLS.iter().find(|(_, b)| *b == button).map(|(tool, _)| *tool) {
                        if place(ui, &painter, (keymap, scale), area, button, (tool == state.tool, true)) {
                            state.tool = tool;
                        }
                        continue;
                    }
                    match button {
                        IconButton::Quantise => {
                            if place(ui, &painter, (keymap, scale), area, button, (state.quantize_open(), true)) {
                                let open = !state.quantize_open();
                                state.set_quantize_open(open);
                            }
                        }
                        IconButton::MidiTools => {
                            if place(ui, &painter, (keymap, scale), area, button, (state.tools_open, true)) {
                                state.tools_open = !state.tools_open;
                            }
                        }
                        IconButton::Hide => {
                            if place(ui, &painter, (keymap, scale), area, button, (false, true)) {
                                state.hide_requested = true;
                            }
                        }
                        _ => {
                            let active = matches!(button, IconButton::Paste) || selected;
                            if place(ui, &painter, (keymap, scale), area, button, (false, active)) {
                                actions.extend(state.clipboard_action(button, clip, copied));
                            }
                        }
                    }
                }
            },
        }
    }
    if let Some(group) = rows_group {
        rows_toggles(ui, &painter, group, &rows, step < Step::Icons, cx, state, actions);
    }
}

/// One icon button in `area` ([`ButtonPaint::place`]).
fn place(
    ui: &Ui,
    painter: &egui::Painter,
    (keymap, scale): (&crate::settings::keymap::Keymap, f32),
    area: Rect,
    button: IconButton,
    (chosen, active): (bool, bool),
) -> bool {
    ButtonPaint { ui, painter, keymap, scale }.place(area, button, chosen, active)
}

/// The tools, in the toolbar's order.
const TOOLS: [(Tool, IconButton); 6] = [
    (Tool::Select, IconButton::Select),
    (Tool::Draw, IconButton::Draw),
    (Tool::Paint, IconButton::Paint),
    (Tool::Erase, IconButton::Erase),
    (Tool::Slice, IconButton::Slice),
    (Tool::Mute, IconButton::Mute),
];

/// What the toolbar's items measure this frame: a button's box and its icon's side, the room
/// inside the bar, and each menu's and Rows toggle's width with its word and without.
struct Widths {
    size: Vec2,
    side: f32,
    inner: f32,
    workflow: (f32, f32),
    ghosts: (f32, f32),
    grid: (f32, f32),
    rows: [(f32, f32); 3],
}

/// The toolbar's items at `step`, in order.
fn items(step: Step, w: &Widths) -> Vec<crate::kit::native_css::row::Item> {
    use crate::kit::native_css::row::Item;
    let button = |button: IconButton| Item::named("piano-tool", button.id()).width(w.size.x).height(w.size.y);
    let separator = || Item::new("separator").attribute("kind", "bar").width(1.0).height(w.inner);
    let menu = |id: &str, width: f32| Item::named("bar-item", id).width(width).height(w.inner);
    let mut items: Vec<Item> = TOOLS.iter().map(|(_, b)| button(*b)).collect();
    items.push(separator());
    if step >= Step::Edit {
        items.push(Item::named("piano-tool", "piano-edit").width(w.size.x + 10.0).height(w.size.y));
    } else {
        items.extend([IconButton::Cut, IconButton::Copy, IconButton::Paste, IconButton::Duplicate].map(button));
    }
    items.push(separator());
    items.push(button(IconButton::Quantise));
    items.push(button(IconButton::MidiTools));
    if step < Step::Squeezed {
        items.push(separator());
        let group = items.len();
        items.push(Item::named("group", "piano-roll-rows").height(w.inner));
        for ((toggle, _), width) in ROWS.iter().zip(w.rows) {
            let width = if step < Step::Icons { width.0 } else { width.1 };
            items.push(Item::named("piano-tool", toggle.id()).width(width).in_group(group));
        }
    }
    items.push(Item::new("flexible-space"));
    if step < Step::Squeezed {
        let named = step < Step::Arrows;
        items.push(menu("piano-roll-workflow", if named { w.workflow.0 } else { w.workflow.1 }));
        items.push(menu("piano-roll-ghosts", if named { w.ghosts.0 } else { w.ghosts.1 }));
        items.push(separator());
    }
    items.push(menu("piano-roll-grid", if step == Step::Full { w.grid.0 } else { w.grid.1 }));
    items.push(Item::named("piano-tool", IconButton::Hide.id()).width(w.side).height(w.side));
    items
}

/// How far the toolbar gives way this frame (› Layout): each step leaves out one more thing the
/// row has no room for, in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Step {
    /// Everything, with the grid value's caption.
    Full,
    /// Without the Grid caption.
    NoCaption,
    /// The Rows group as icons.
    Icons,
    /// The clipboard in one Edit menu.
    Edit,
    /// The workflow and ghost menus as their arrows.
    Arrows,
    /// Not even that fits: only the tools, the grid value and the ×.
    Squeezed,
}

impl Step {
    const ALL: [Step; 6] = [Step::Full, Step::NoCaption, Step::Icons, Step::Edit, Step::Arrows, Step::Squeezed];
}

/// The room a toolbar button has beside its icon, both sides together, at Normal size, in points.
const BUTTON_ROOM: f32 = 8.0;

/// The room a toolbar menu's button leaves before its first word and after its chevron, in points.
const MENU_PAD: f32 = 6.0;

/// The room between a toolbar menu's caption and its word, in points.
const MENU_SPACE: f32 = 4.0;

/// The room between a toolbar menu's word and its chevron, in points: enough that the chevron reads
/// as the button's sign, not as part of the value.
const CHEVRON_GAP: f32 = 6.0;

/// How wide a toolbar menu's chevron is, in points.
const CHEVRON: f32 = 7.0;

/// How large a toolbar button's icon is drawn, against its own box: a little room above and below
/// it inside the button.
const ICON_IN_BUTTON: f32 = 0.82;

/// How wide a toolbar menu's button is with a dim `caption` and `word` on it (either may be left
/// out; with neither, the chevron alone): the words, the chevron after them and the padding.
pub(crate) fn menu_width(ui: &Ui, (caption, word): (Option<&str>, &str)) -> f32 {
    let font = theme::ui_font(theme::TEXT_SMALL);
    let caption = caption.map_or(0.0, |c| widgets::text_width(ui, c, font.clone()) + MENU_SPACE);
    let word = if word.is_empty() { 0.0 } else { widgets::text_width(ui, word, font) + CHEVRON_GAP };
    MENU_PAD + caption + word + CHEVRON + MENU_PAD
}

/// A toolbar menu's button in `rect`, drawn as the Rows group's toggles are, so the right side of
/// the toolbar has one look (› Layout): no fill until hovered or while its menu is open, rounded,
/// a dim `caption` when given, its `word` in `ink` and a chevron right after. Its accessible label
/// is `label`. Returns its response, for the menu to open from.
pub(crate) fn menu_face(
    ui: &Ui,
    rect: Rect,
    id: Id,
    (caption, word): (Option<&str>, &str),
    ink: Color32,
    label: &str,
) -> egui::Response {
    let rect =
        Rect::from_min_max(Pos2::new(rect.left(), rect.top() + 1.0), Pos2::new(rect.right(), rect.bottom() - 1.0));
    let response = widgets::button_area(ui, rect, id, label);
    let lit = response.hovered() || egui::Popup::menu(&response).is_open();
    let (fill, ink) = if lit {
        (ui.visuals().widgets.hovered.weak_bg_fill, theme::text_primary())
    } else {
        (Color32::TRANSPARENT, ink)
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, fill);
    let font = theme::ui_font(theme::TEXT_SMALL);
    let mut x = rect.left() + MENU_PAD;
    for (text, color, after) in
        [(caption, theme::text_hint(), MENU_SPACE), ((!word.is_empty()).then_some(word), ink, CHEVRON_GAP)]
    {
        let Some(text) = text else { continue };
        let area = Rect::from_min_max(Pos2::new(x, rect.top()), rect.max);
        widgets::text_in(painter, area, Align2::LEFT_CENTER, text, font.clone(), color);
        x += widgets::text_width(ui, text, font.clone()) + after;
    }
    chevron(painter, Pos2::new(x + CHEVRON / 2.0, rect.center().y), ink);
    response
}

/// A small chevron pointing down, centred on `at`: a toolbar menu's sign that it opens a list.
fn chevron(painter: &egui::Painter, at: Pos2, color: Color32) {
    let half = CHEVRON / 2.0;
    painter.add(egui::Shape::line(
        vec![at + Vec2::new(-half, -1.5), at + Vec2::new(0.0, 2.0), at + Vec2::new(half, -1.5)],
        Stroke::new(1.2, color),
    ));
}

/// The words of the Rows group: Fold, Scale (the clip's scale switch) and Highlight.
const ROWS: [(IconButton, &str); 3] =
    [(IconButton::Fold, "Fold"), (IconButton::Scale, "Scale"), (IconButton::Highlight, "Highlight")];

/// The width of one of the Rows group's toggles: its icon, and its word when `word`.
fn row_toggle_width(ui: &Ui, side: f32, word: Option<&str>) -> f32 {
    match word {
        Some(word) => side + 10.0 + widgets::text_width(ui, word, theme::ui_font(theme::TEXT_SMALL)),
        None => side + 6.0,
    }
}

/// The Rows group in `group`, framed: Fold, Scale and Highlight in `toggles`, each lit while on,
/// as an icon and a word or, when `words` is off, the icon alone (its tooltip names it). Fold shows
/// the rows with notes, or the scale's while the clip has one; Scale is the clip's scale switch,
/// the same as the clip panel's; Highlight tints the scale's rows (› Scale and key).
#[allow(clippy::too_many_arguments)]
fn rows_toggles(
    ui: &Ui,
    painter: &egui::Painter,
    group: Rect,
    toggles: &[Option<Rect>; 3],
    words: bool,
    cx: ToolbarCx<'_>,
    state: &mut PianoRollState,
    actions: &mut Vec<Action>,
) {
    let clip = cx.clip;
    let element = crate::kit::element::GROUP.id("piano-roll-rows").attribute("kind", "piano-roll-rows").enter();
    element.paint(painter, group);
    painter.rect_stroke(group, 4.0, theme::hairline(theme::border_strong()), StrokeKind::Inside);
    // Fold folds to the notes, Scale to the clip's scale: each lit while the rows are folded its way.
    let folded = state.fold == Fold::Notes;
    let scaled = state.fold == Fold::Scale && clip.scale.is_some();
    let hints = [
        "Fold: shows only the rows that have notes",
        "Scale: shows only the rows of the clip's scale and those with notes, turning the clip's scale on when it has none; again, every row",
        "Highlight scale: tints the rows of the clip's scale and marks its keys",
    ];
    let ons = [folded, scaled, state.highlight_scale];
    for (index, ((button, word), rect)) in ROWS.iter().zip(toggles).enumerate() {
        // A toggle the sheet hides is gone; the others keep their own places.
        let Some(rect) = rect else { continue };
        let on = ons.get(index).copied().unwrap_or(false);
        if !row_toggle(
            ui,
            painter,
            *rect,
            (*button, words.then_some(*word)),
            on,
            hints.get(index).copied().unwrap_or(""),
            cx.scale,
        ) {
            continue;
        }
        match button {
            IconButton::Fold => state.fold = if folded { Fold::Off } else { Fold::Notes },
            IconButton::Scale => {
                state.fold = if scaled { Fold::Off } else { Fold::Scale };
                // A clip with no scale gets one to fold to: the switch's (› Scale and key). Unfolding
                // keeps the clip's scale, so its rows stay highlighted on the whole keyboard.
                if !scaled && clip.scale.is_none() {
                    let mut params = crate::detail::piano_roll::scale_switch_params(ui.ctx(), &clip.address, None);
                    if let (Some(params), Some(address)) = (params.as_object_mut(), clip.address.as_object()) {
                        params.extend(address.clone());
                    }
                    actions.push(Action::new("clip.setScale", params));
                }
            }
            _ => {
                state.highlight_scale = !on;
                state.view_picked = Some(super::ViewPick::HighlightScale(!on));
            }
        }
    }
}

/// One of the Rows group's toggles: lit while on, its icon and, given one, its word; its accessible
/// label is the button's name, its tooltip `hint`. Returns whether it was clicked.
fn row_toggle(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    (button, word): (IconButton, Option<&str>),
    on: bool,
    hint: &str,
    scale: f32,
) -> bool {
    let response = widgets::button_area(ui, rect, Id::unique(("piano-roll-rows", button.name())), button.name());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, button.name()));
    // Lit as a selected word is (the selection's fill and ink), as the toolbar's words were.
    let visuals = ui.visuals();
    let (fill, ink) = if on {
        (visuals.selection.bg_fill, visuals.selection.stroke.color)
    } else if response.hovered() {
        (visuals.widgets.hovered.weak_bg_fill, theme::text_primary())
    } else {
        (Color32::TRANSPARENT, theme::text_secondary())
    };
    painter.rect_filled(rect, 3.0, fill);
    let icon = Rect::from_min_size(rect.min + Vec2::new(3.0, 0.0), Vec2::new(rect.height(), rect.height()));
    button.draw(painter, icon, ink, scale * ICON_IN_BUTTON);
    if let Some(word) = word {
        let text = Rect::from_min_max(Pos2::new(icon.right() + 2.0, rect.top()), rect.max);
        widgets::text_in(painter, text, Align2::LEFT_CENTER, word, theme::ui_font(theme::TEXT_SMALL), ink);
    }
    let clicked = response.clicked();
    response.on_hover_text(hint);
    clicked
}

/// The note clipboard as one menu, when the toolbar is short of room: the Copy icon and an arrow,
/// its list Cut, Copy, Paste and Duplicate (greyed out with nothing to act on, Paste aside).
/// Returns the button picked.
fn edit_menu(paint: &ButtonPaint<'_>, area: Rect, selected: bool) -> Option<IconButton> {
    let ui = paint.ui;
    let response = widgets::button_area(ui, area, Id::unique("piano-roll-edit-menu"), "Edit");
    let fill = crate::kit::motion::mix(Color32::TRANSPARENT, theme::control_hover(), widgets::hover(ui, &response));
    let ink = theme::text_secondary();
    paint.painter.rect_filled(area, 3.0, fill);
    let icon = Rect::from_min_size(area.min, Vec2::splat(area.height()));
    IconButton::Copy.draw(paint.painter, icon, ink, paint.scale * ICON_IN_BUTTON);
    chevron(paint.painter, Pos2::new(area.right() - MENU_PAD, area.center().y), ink);
    let mut picked = None;
    egui::Popup::menu(&response).show(|ui| {
        for button in [IconButton::Cut, IconButton::Copy, IconButton::Paste, IconButton::Duplicate] {
            let enabled = matches!(button, IconButton::Paste) || selected;
            let text = egui::RichText::new(icon_hint(button, paint.keymap)).font(theme::ui_font(theme::TEXT_SMALL));
            if ui.add_enabled(enabled, egui::Button::new(text)).clicked() {
                picked = Some(button);
                ui.close();
            }
        }
    });
    response.on_hover_text("Edit: cut, copy, paste, duplicate");
    picked
}

/// The toolbar's grid menu (› Grid and snapping): its button shows the grid value, after a dim
/// "Grid" when `caption`; its menu holds Snap to grid, the adaptive widths, the fixed values,
/// Narrow grid, Widen grid and Triplet grid, each with its key, the chosen ones ticked. Returns
/// the grid picked this frame.
pub(super) fn grid_menu(
    ui: &mut Ui,
    rect: Rect,
    clip: &PianoClip,
    keymap: &crate::settings::keymap::Keymap,
    caption: bool,
) -> Option<PianoGrid> {
    let grid = clip.grid_menu;
    let shown = grid.label();
    let label = format!("Grid value {shown}");
    let id = Id::unique("piano-roll-grid-menu");
    let words = (caption.then_some("Grid"), shown.as_str());
    let response = menu_face(ui, rect, id, words, theme::text_secondary(), &label);
    let mut picked = None;
    egui::Popup::menu(&response).show(|ui| {
        let keys = grid_key_labels(keymap, &PIANO_GRID_KEYS);
        picked = grid_entries(ui, grid, &keys, &|_| true);
    });
    response.on_hover_text("The grid: snapping, adaptive or fixed lines, triplets");
    picked.filter(|p| *p != grid)
}

/// The piano roll's grid keys: snap, narrower, wider, triplets.
pub(crate) const PIANO_GRID_KEYS: [&str; 4] =
    ["pianoRoll.gridSnap", "pianoRoll.gridNarrow", "pianoRoll.gridWiden", "pianoRoll.gridTriplet"];

/// The toolbar's workflow menu (› Workflow modes): its button shows the mode's name (its arrow
/// alone when the toolbar is short of room), its menu the workflow entries.
fn workflow_menu(
    ui: &mut Ui,
    rect: Rect,
    keymap: &crate::settings::keymap::Keymap,
    state: &mut PianoRollState,
    named: bool,
) {
    let mode = keymap.preset();
    let label = format!("Workflow {}", mode.name());
    let word = if named { mode.name() } else { "" };
    let id = Id::unique("piano-roll-workflow-menu");
    let response = menu_face(ui, rect, id, (None, word), theme::text_secondary(), &label);
    egui::Popup::menu(&response).show(|ui| workflow_entries(ui, keymap, state));
    response.on_hover_text("How clicks, right-clicks and double-clicks work: a preset, or Custom");
}

/// The workflow entries (› Workflow modes), the toolbar's workflow menu: the four modes and, under
/// a divider, a submenu per workflow setting with its choices, each in a word or two with its value
/// dim at the right, then Allow stacked notes. A pick is left in `state` for the window, which
/// keeps it in the keymap.
pub(super) fn workflow_entries(ui: &mut Ui, keymap: &crate::settings::keymap::Keymap, state: &mut PianoRollState) {
    use crate::settings::keymap::Preset;
    use crate::settings::workflow::{Pick, Setting};
    let mode = keymap.preset();
    let current = keymap.workflow();
    let font = theme::ui_font(theme::TEXT_SMALL);
    let mut picked = None;
    for choice in Preset::MODES {
        let response = ui.selectable_label(choice == mode, egui::RichText::new(choice.name()).font(font.clone()));
        if response.clicked() {
            if choice != mode {
                picked = Some(Pick::Mode(choice));
            }
            ui.close();
        }
    }
    ui.separator();
    // Each setting in a word or two, its value dim at the right and its choices one level down:
    // short enough to read at a glance, and changed and tried at once.
    for setting in Setting::ALL {
        let chosen = current.get(setting);
        let shown = setting.choices().get(chosen).copied().unwrap_or("");
        let value = setting.short_choices().get(chosen).copied().unwrap_or(shown);
        let button = egui::Button::new(egui::RichText::new(setting.short_name()).font(font.clone())).right_text((
            egui::RichText::new(value).font(font.clone()).color(theme::text_hint()),
            egui::containers::menu::SubMenuButton::arrow_atom(None),
        ));
        let label = format!("{}: {shown}", setting.name());
        let (response, _) = egui::containers::menu::SubMenuButton::from_button(button).ui(ui, |ui| {
            for (index, choice) in setting.choices().iter().enumerate() {
                let response = ui.selectable_label(index == chosen, egui::RichText::new(*choice).font(font.clone()));
                let name = format!("{}: {choice}", setting.name());
                response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, index == chosen, &name));
                if response.clicked() {
                    if index != chosen {
                        picked = Some(Pick::Setting(setting, index));
                    }
                    ui.close();
                }
            }
        });
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
        response.on_hover_text(setting.note());
    }
    // How notes that land on others of their key behave belongs with how the mouse edits them
    // (› Allow stacked notes).
    ui.separator();
    let stacked = !state.notes_replace;
    let response = ui.selectable_label(stacked, egui::RichText::new("Allow stacked notes").font(font.clone()));
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, stacked, "Workflow: Allow stacked notes"));
    if response.clicked() {
        state.view_picked = Some(super::ViewPick::AllowStacked(!stacked));
        ui.close();
    }
    // With stacked notes off, what a note covered from its start does: it goes whole, or keeps what
    // runs on past the note placed over it (› Resizing, Covered notes).
    let whole = state.covered_whole;
    let mut covered_pick = None;
    ui.add_enabled_ui(!stacked, |ui| {
        let value = if whole { "Delete" } else { "Keep rest" };
        let button = egui::Button::new(egui::RichText::new("Covered notes").font(font.clone())).right_text((
            egui::RichText::new(value).font(font.clone()).color(theme::text_hint()),
            egui::containers::menu::SubMenuButton::arrow_atom(None),
        ));
        let (response, _) = egui::containers::menu::SubMenuButton::from_button(button).ui(ui, |ui| {
            for (on, name) in [(true, "Delete them"), (false, "Keep the rest")] {
                let response = ui.selectable_label(whole == on, egui::RichText::new(name).font(font.clone()));
                let label = format!("Covered notes: {name}");
                response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, whole == on, &label));
                if response.clicked() {
                    if whole != on {
                        covered_pick = Some(on);
                    }
                    ui.close();
                }
            }
        });
        let label = format!("Covered notes: {}", if whole { "Delete them" } else { "Keep the rest" });
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, !stacked, &label));
        response.on_hover_text(
            "With stacked notes off: a note that one placed over it covers from its start goes whole, or keeps what runs on past",
        );
    });
    if let Some(on) = covered_pick {
        state.view_picked = Some(super::ViewPick::CoveredWhole(on));
    }
    if picked.is_some() {
        state.workflow_picked = picked;
    }
}

/// The toolbar's icon buttons as they are painted this frame: the surface they are drawn on, the
/// keys their tooltips show and the Toolbar size their shapes are drawn at.
pub(super) struct ButtonPaint<'a> {
    pub(super) ui: &'a Ui,
    pub(super) painter: &'a egui::Painter,
    pub(super) keymap: &'a crate::settings::keymap::Keymap,
    pub(super) scale: f32,
}

impl ButtonPaint<'_> {
    /// One icon button: the shape in the tool's colours, a tooltip with its name and the key it
    /// has, and the same name as its accessible label. Returns whether it was clicked.
    pub(super) fn place(&self, area: Rect, button: IconButton, chosen: bool, active: bool) -> bool {
        let ui = self.ui;
        let painter = self.painter;
        let response = widgets::button_area(ui, area, Id::unique(("piano-roll-icon", button.name())), button.name());
        let hint = icon_hint(button, self.keymap);
        let response = response.on_hover_text(hint.clone());
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, active, chosen, hint.clone()));
        // `piano-tool` styles them all, `#piano-draw` and the others one; `[checked]` is the
        // chosen tool or an open column. `piano.draw` (`.on`, `.hover`) replaces its shape.
        let el = crate::kit::element::PIANO_TOOL
            .id(button.id())
            .attribute("kind", button.id())
            .response(&response)
            .attr(crate::kit::element::Checked, chosen)
            .enter();
        let color = el.color_or(if !active {
            theme::text_hint()
        } else if chosen {
            theme::text_primary()
        } else {
            theme::text_secondary()
        });
        let state = if !active {
            crate::kit::skin::State::Disabled
        } else {
            crate::kit::skin::State::response(&response, chosen)
        };
        let role = format!("piano.{}", button.id().trim_start_matches("piano-"));
        if active {
            // Only the chosen tool or an open part has a fill; the others show one on hover.
            let fill = if chosen { theme::control_active() } else { Color32::TRANSPARENT };
            let fill = crate::kit::motion::mix(fill, theme::control_hover(), widgets::hover(ui, &response));
            el.paint_with(painter, area, crate::kit::element::Native { radius: 3.0, fill });
        }
        if !(crate::kit::skin::has_own(&role) && crate::kit::skin::paint_as(&el, painter, area, &role, state, color)) {
            button.draw(painter, area, color, self.scale * ICON_IN_BUTTON);
        }
        active && response.clicked()
    }
}

/// The name and key an icon button shows, such as "Draw (2)": the key is the one the keymap has
/// bound to the button's action now, so a rebound key (or one a preset moved) shows as it is.
pub(super) fn icon_hint(button: IconButton, keymap: &crate::settings::keymap::Keymap) -> String {
    match button.action().and_then(|id| keymap.label(id)) {
        Some(key) => format!("{} ({key})", button.name()),
        None => button.name().to_string(),
    }
}
