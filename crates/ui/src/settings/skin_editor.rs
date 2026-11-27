// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The variant editor's controls for the settings a theme declares itself.
use super::settings_view::{ThemeRequest, ThemesView};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    error: String,
    /// Text typed into a declared setting's field, by property, until it is applied.
    drafts: std::collections::BTreeMap<String, String>,
}

/// The settings the theme declares itself (`@property` with a label), each with a control for its
/// type: a color, a number in its range, a choice of keywords, or text checked against its type.
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
            .color(crate::kit::element::caption_ink()),
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
                    let mut hsva = hsva_of(rgba);
                    let popup = ui.auto_id_with("popup");
                    // Normal blending only: a CSS color has no additive form, and egui keeps an
                    // additive one with an alpha of 0, which would save it as transparent black.
                    let response =
                        egui::color_picker::color_edit_button_hsva(ui, &mut hsva, egui::color_picker::Alpha::OnlyBlend);
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ColorWell, true, label));
                    *held |= egui::Popup::is_id_open(ui.ctx(), popup) && ui.input(|input| input.pointer.primary_down());
                    if response.changed() {
                        next = Some(crate::kit::theme::hex_rgba(hsva.to_srgba_unmultiplied()));
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
                    crate::kit::widgets::combo_box(
                        ui,
                        egui::ComboBox::from_id_salt(("theme-setting", &property.name)).selected_text(value.as_str()),
                        |ui| {
                            for keyword in keywords {
                                if crate::kit::menu::item(ui, keyword, value == keyword).clicked() {
                                    next = Some(keyword.to_string());
                                    ui.close();
                                }
                            }
                        },
                    );
                }
                _ => {
                    let draft = state.drafts.entry(property.name.clone()).or_insert_with(|| value.clone());
                    let edit = ui.add(egui::TextEdit::singleline(draft).desired_width(200.0));
                    // A value that is acceptable is taken as it is typed, so a pending edit never
                    // waits for the field to lose focus (leaving the editor would not see it); one
                    // that is not is put back when the field loses focus.
                    let typed = draft.trim().to_string();
                    if edit.changed() && property.accepts(&typed) {
                        next = Some(typed.clone());
                    }
                    if edit.lost_focus() {
                        if property.accepts(&typed) {
                            state.drafts.remove(&property.name);
                        } else {
                            *draft = value.clone();
                            state.error = format!("{label}: {typed} is not a value of this setting");
                        }
                    }
                }
            }
        });
        if let Some(next) = next.filter(|next| *next != value) {
            request = Some(ThemeRequest::EditValue { name: property.name.clone(), value: next });
        }
    }
    request
}

/// A CSS color (unmultiplied sRGB) for egui's picker, its color kept apart from its alpha so a
/// fully transparent color keeps its hue: egui's other color types are premultiplied and lose it.
fn hsva_of([r, g, b, a]: [u8; 4]) -> egui::ecolor::Hsva {
    let mut hsva = egui::ecolor::Hsva::from_srgba_unmultiplied([r, g, b, 255]);
    hsva.a = f32::from(a) / 255.0;
    hsva
}

impl State {
    /// Forgets what was typed into settings' fields, which then show their values again.
    pub fn forget_drafts(&mut self) {
        self.drafts.clear();
    }
}

#[cfg(test)]
mod tests {
    /// A color keeps its hue through the picker at any alpha, 0 included, as CSS keeps it.
    #[test]
    fn a_color_keeps_its_hue_at_every_alpha() {
        for rgba in [[0xFF, 0x30, 0x00, 0x00], [0xFF, 0x30, 0x00, 0x80], [0x12, 0x34, 0x56, 0xFF]] {
            assert_eq!(super::hsva_of(rgba).to_srgba_unmultiplied(), rgba);
        }
    }
}
