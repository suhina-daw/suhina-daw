// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Native theme controls edit the same skin model as package CSS.
use super::settings_view::{ThemeRequest, ThemesView};
use crate::kit::{skin, theme_css};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    current: String,
    role: String,
    svg: String,
    css: String,
    /// The sheet the notices below were found in.
    advanced: String,
    notices: Vec<String>,
    error: String,
    /// Text typed into a declared setting's field, by property, until it is applied.
    drafts: std::collections::BTreeMap<String, String>,
}

/// The settings the theme declares itself (`@property` with a label), each with a control for its
/// type: a colour, a number in its range, a choice of keywords, or text checked against its type.
/// `held` is set while the pointer is held on one of them, so the window behind stays clear for
/// as long as the setting is being changed.
pub fn declared(ui: &mut egui::Ui, themes: &ThemesView, state: &mut State, held: &mut bool) -> Option<ThemeRequest> {
    use suhina_style::registered::{Component, Syntax};
    if themes.declared.is_empty() {
        return None;
    }
    let mut request = None;
    ui.label(egui::RichText::new("Theme settings").strong());
    ui.label(
        egui::RichText::new("Settings this theme offers. They are saved in the variant in use.")
            .small()
            .color(crate::kit::theme::text_secondary()),
    );
    let mut group: Option<&str> = None;
    for property in &themes.declared {
        if property.group.as_deref() != group {
            group = property.group.as_deref();
            if let Some(group) = group {
                ui.label(egui::RichText::new(group).small().strong());
            }
        }
        let label = property.label.as_deref().unwrap_or(&property.name);
        let value = themes.values.get(&property.name).cloned().or_else(|| property.initial.clone()).unwrap_or_default();
        let only = match &property.syntax {
            Syntax::Alternatives(parts) if parts.len() == 1 && !parts[0].1 => Some(&parts[0].0),
            _ => None,
        };
        let mut next = None;
        ui.horizontal(|ui| {
            ui.label(label);
            match (only, property.syntax.keywords(), property.range) {
                (Some(Component::Color), _, _) => {
                    let rgba = match suhina_style::value::Color::parse(&value) {
                        Ok(suhina_style::value::Color::Rgba(rgba)) => rgba,
                        _ => [0, 0, 0, 255],
                    };
                    let mut color = egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
                    let popup = ui.auto_id_with("popup");
                    let response = ui.color_edit_button_srgba(&mut color);
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ColorWell, true, label));
                    *held |= egui::Popup::is_id_open(ui.ctx(), popup) && ui.input(|input| input.pointer.primary_down());
                    if response.changed() {
                        next = Some(crate::kit::theme::hex(color));
                    }
                }
                (Some(Component::Number | Component::Integer), _, Some((min, max))) => {
                    let integer = matches!(only, Some(Component::Integer));
                    let mut number = value.trim().parse::<f32>().unwrap_or(min).clamp(min, max);
                    let mut slider = egui::Slider::new(&mut number, min..=max);
                    if let Some(step) = property.step {
                        slider = slider.step_by(f64::from(step));
                    } else if integer {
                        slider = slider.step_by(1.0);
                    }
                    let response = ui.add(slider);
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, label));
                    *held |= response.dragged() || response.is_pointer_button_down_on();
                    if response.changed() {
                        next = Some(if integer { format!("{}", number.round() as i64) } else { format!("{number}") });
                    }
                }
                (_, Some(keywords), _) => {
                    egui::ComboBox::from_id_salt(("theme-setting", &property.name))
                        .selected_text(value.as_str())
                        .show_ui(ui, |ui| {
                            for keyword in keywords {
                                if ui.selectable_label(value == keyword, keyword).clicked() {
                                    next = Some(keyword.to_string());
                                }
                            }
                        });
                }
                _ => {
                    let draft = state.drafts.entry(property.name.clone()).or_insert_with(|| value.clone());
                    let edit = ui.add(egui::TextEdit::singleline(draft).desired_width(200.0));
                    if edit.lost_focus() {
                        let typed = draft.trim().to_string();
                        if property.accepts(&typed) {
                            next = Some(typed);
                        } else {
                            *draft = value.clone();
                            state.error = format!("{label}: {typed} is not a value of this setting");
                        }
                    }
                }
            }
        });
        if let Some(next) = next.filter(|next| *next != value) {
            state.drafts.remove(&property.name);
            request = Some(ThemeRequest::EditValue { name: property.name.clone(), value: next });
        }
    }
    request
}

impl State {
    /// Forgets what was typed into settings' fields, which then show their values again.
    pub fn forget_drafts(&mut self) {
        self.drafts.clear();
    }
}

pub fn show(ui: &mut egui::Ui, themes: &ThemesView, state: &mut State) -> Option<ThemeRequest> {
    if state.current != themes.current {
        state.current = themes.current.clone();
        state.role.clear();
        state.css.clear();
        state.drafts.clear();
        state.error.clear();
    }
    let mut request = None;
    ui.collapsing("Controls, meters and toolbar", |ui| {
        ui.label("To move the current toolbar without changing its layout, right-click it and choose Edit toolbar. Outlined controls snap to an 8 point grid; Done saves, Cancel discards.");
        let mut skin = themes.skin.clone();
        ui.horizontal(|ui| {
            ui.label("Meter style");
            for style in [skin::MeterStyle::Continuous, skin::MeterStyle::Segmented, skin::MeterStyle::Needle, skin::MeterStyle::Frames] {
                ui.selectable_value(&mut skin.meter, style, style.name());
            }
        });
        ui.add(egui::Slider::new(&mut skin.meter_intensity, 0..=100).text("Meter intensity").suffix("%"))
            .on_hover_text("Dims the measured level, artwork and glow. Audio levels and clip warnings stay unchanged.");
        if skin.meter == skin::MeterStyle::Segmented { ui.add(egui::Slider::new(&mut skin.segments, 4..=64).text("Meter segments")); }
        if skin.meter == skin::MeterStyle::Frames { ui.label("Supply meter.frame-0, meter.frame-1, … artwork in this theme. The measured peak chooses a frame; missing sequences use the continuous meter."); }
        if ui.button("Customize default SVG controls").clicked() { skin.assets = skin::inherited_assets(); }
        if ui.button("Inherit default SVG controls").clicked() { skin.assets.clear(); }
        ui.label("Missing artwork uses the artwork built into Suhina. This theme stores only its own.");
        ui.label("SVGs affect appearance; control gestures and shortcuts stay the same.");
        ui.separator();
        let mut custom = skin.toolbar.is_some();
        if ui.checkbox(&mut custom, "Custom toolbar layout").changed() {
            skin.toolbar = custom.then(Default::default);
        }
        if let Some(toolbar) = skin.toolbar.as_mut() {
            ui.label("Right-click the top control bar and choose Edit toolbar. Drag the outlined real controls on the snap grid; Done saves and Cancel discards. You can also reorder modules here.");
            let mut movement = None;
            for (index, module) in toolbar.order.clone().iter().enumerate() {
                ui.horizontal(|ui| {
                    let mut visible = !toolbar.hidden.contains(module);
                    if ui.checkbox(&mut visible, module).changed() {
                        toolbar.hidden.retain(|s| s != module);
                        if !visible { toolbar.hidden.push(module.clone()); }
                    }
                    if ui.add_enabled(index > 0, egui::Button::new("↑")).on_hover_text(format!("Move {module} left")).clicked() { movement = Some((index, index.saturating_sub(1))); }
                    if ui.add_enabled(index + 1 < toolbar.order.len(), egui::Button::new("↓")).on_hover_text(format!("Move {module} right")).clicked() { movement = Some((index,index + 1)); }
                });
            }
            if let Some((a,b)) = movement { toolbar.order.swap(a,b); }
            if ui.button("Reset toolbar layout").clicked() { *toolbar = Default::default(); }
        }
        if skin != themes.skin { request = Some(ThemeRequest::EditSkin(skin)); }
        ui.collapsing("Edit an SVG control", |ui| {
            if state.current != themes.current { state.current = themes.current.clone(); state.role.clear(); state.css.clear(); }
            let old = state.role.clone();
            egui::ComboBox::from_id_salt("skin-asset-role").selected_text(if state.role.is_empty() { "Choose control" } else { &state.role }).show_ui(ui, |ui| {
                for role in skin::ROLES { for suffix in ["", ".hover", ".active", ".disabled"] {
                    let role = format!("{role}{suffix}"); ui.selectable_value(&mut state.role, role.clone(), role);
                } }
            });
            if old != state.role {
                state.svg = themes.skin.assets.get(&state.role).filter(|source| !skin::png_artwork(source)).cloned().unwrap_or_default();
                state.error.clear();
            }
            if !state.role.is_empty() {
                if themes.skin.assets.get(&state.role).is_some_and(|source| skin::png_artwork(source)) {
                    ui.label("This role uses PNG artwork. Edit its package file externally, or paste SVG below to replace it.");
                }
                let editor = ui.add(egui::TextEdit::multiline(&mut state.svg).code_editor().desired_rows(5).desired_width(f32::INFINITY));
                editor.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, "Artwork SVG"));
                if ui.button("Apply SVG").clicked() {
                    match skin::parse_svg(&state.svg) {
                        Ok(_) => { let mut skin = themes.skin.clone(); skin.assets.insert(state.role.clone(),state.svg.clone()); request = Some(ThemeRequest::EditSkin(skin)); state.error.clear(); }
                        Err(error) => state.error = error,
                    }
                }
                if ui.button("Remove artwork override").clicked() {
                    let mut skin = themes.skin.clone(); skin.assets.remove(&state.role); state.svg.clear(); request = Some(ThemeRequest::EditSkin(skin));
                }
            }
        });
        ui.collapsing("Native CSS", |ui| {
            ui.label("Settings and layout in the form of earlier versions: :root colours, Launcher density, meter style/segments/intensity, toolbar order/hidden, .control-NAME placement, artwork frame rate. Applying it saves the settings in the variant in use and the layout in theme.css. Place connected elements by hand with .control-NAME rules for now.");
            if ui.button("Load compact Launcher geometry").clicked() { state.css = crate::kit::theme::COMPACT_SESSION_EXAMPLE.into(); }
            if ui.button("Load current style as CSS").clicked() {
                let theme = crate::kit::theme::Theme { name: themes.name.clone(), colors: themes.colors, skin: themes.skin.clone(), transparent_rgb: themes.transparent_rgb.clone(), ..Default::default() };
                state.css = theme_css::export(&theme);
                // The in-app editor keeps images in the model; relative paths are for package files.
                state.css = state.css.lines().filter(|line| !line.contains("image: url(")).collect::<Vec<_>>().join("\n");
            }
            let editor = ui.add(egui::TextEdit::multiline(&mut state.css).code_editor().desired_rows(8).desired_width(f32::INFINITY));
            editor.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, "Native CSS"));
            if ui.button("Apply CSS").clicked() {
                let mut theme = crate::kit::theme::Theme { colors: themes.colors, skin: themes.skin.clone(), transparent_rgb: themes.transparent_rgb.clone(), ..Default::default() };
                let warnings = theme_css::apply(&mut theme, &state.css);
                // Filesystem paths need a package loader, not the GUI thread.
                if theme.skin.assets.values().any(|s| !s.trim_start().starts_with('<') && !skin::png_artwork(s)) { state.error = "Import CSS image URLs in a theme ZIP package, or paste SVG in the SVG editor.".into(); }
                else if warnings.is_empty() { request = Some(ThemeRequest::EditCss(state.css.clone())); state.error.clear(); }
                else { state.error = warnings.join("\n"); }
            }
        });
        // The notices are worked out before the heading, so a closed section still says it has some.
        let sheet = themes.skin.css.as_deref().unwrap_or_default();
        if state.advanced != sheet {
            state.advanced = sheet.into();
            state.notices = crate::kit::native_css::inspect(sheet).unwrap_or_default().into_iter().map(|notice| format!("Line {}, column {}: {}. The rest of the sheet still applies.", notice.line, notice.column, notice.message)).collect();
        }
        let heading = match state.notices.len() { 0 => "Theme style sheet".to_string(), 1 => "Theme style sheet (1 notice)".to_string(), n => format!("Theme style sheet ({n} notices)") };
        egui::CollapsingHeader::new(heading).id_salt("theme-style-sheet").show(ui, |ui| {
            ui.label("The theme's look and layout are its style sheet, theme.css, in the theme's folder: variables, selectors, @media, gradients, shadows, transitions, Flex and Grid. Its name, swatch and description are in theme.json. Edit them in any text editor; Suhina reads them again within a second. Settings changed here are saved in a variant (variants/NAME.json); moved controls are written into theme.css, changing only their own rules. A built-in theme cannot be edited: Duplicate it to change its CSS.");
            ui.label("A theme offers its own settings with @property, for example: @property --glow { syntax: \"<number>\"; initial-value: 0.6; label: \"Glow\"; range: 0 1; }. They appear under Theme settings.");
            if ui.button("Open the themes folder").clicked() { request = Some(ThemeRequest::OpenFolder); }
            if state.notices.is_empty() { ui.label("The sheet reads without notices."); }
            for notice in &state.notices { ui.label(notice); }
        });
        if !state.error.is_empty() { ui.colored_label(crate::kit::theme::recording(), &state.error); }
    });
    request
}
