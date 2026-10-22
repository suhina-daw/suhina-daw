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
    advanced: String,
    notices: Vec<String>,
    error: String,
}

pub fn show(ui: &mut egui::Ui, themes: &ThemesView, state: &mut State) -> Option<ThemeRequest> {
    if state.current != themes.current {
        state.current = themes.current.clone();
        state.role.clear();
        state.css.clear();
        state.advanced = themes.skin.css.as_deref().unwrap_or_default().into();
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
        ui.label("Missing artwork inherits themes/_defaults/assets/. This theme stores only its overrides.");
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
                ui.add(egui::TextEdit::multiline(&mut state.svg).code_editor().desired_rows(5).desired_width(f32::INFINITY));
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
            ui.label("Supported: :root colours, Session density, meter style/segments/intensity, toolbar order/hidden, .control-NAME placement, artwork frame rate and image URLs. Right-click the toolbar > Edit interface to move connected elements. Export a theme package for editable artwork and style.css.");
            if ui.button("Load compact Session geometry").clicked() { state.css = crate::kit::theme::COMPACT_SESSION_EXAMPLE.into(); }
            if ui.button("Load current style as CSS").clicked() {
                let theme = crate::kit::theme::Theme { name: themes.name.clone(), colors: themes.colors, skin: themes.skin.clone(), transparent_rgb: themes.transparent_rgb.clone(), ..Default::default() };
                state.css = theme_css::export(&theme);
                // The in-app editor keeps images in the model; relative paths are for package files.
                state.css = state.css.lines().filter(|line| !line.contains("image: url(")).collect::<Vec<_>>().join("\n");
            }
            ui.add(egui::TextEdit::multiline(&mut state.css).code_editor().desired_rows(8).desired_width(f32::INFINITY));
            if ui.button("Apply CSS").clicked() {
                let mut theme = crate::kit::theme::Theme { colors: themes.colors, skin: themes.skin.clone(), transparent_rgb: themes.transparent_rgb.clone(), ..Default::default() };
                let warnings = theme_css::apply(&mut theme, &state.css);
                // Filesystem paths need a package loader, not the GUI thread.
                if theme.skin.assets.values().any(|s| !s.trim_start().starts_with('<') && !skin::png_artwork(s)) { state.error = "Import CSS image URLs in a theme ZIP package, or paste SVG in the SVG editor.".into(); }
                else if warnings.is_empty() { request = Some(ThemeRequest::EditCss(state.css.clone())); state.error.clear(); }
                else { state.error = warnings.join("\n"); }
            }
        });
        ui.collapsing("Advanced CSS (Stylo and Taffy)", |ui| {
            ui.label("Modern cascade, variables, CSS colours, calc() and viewport media queries style native classes. Flex/Grid lays out toolbar modules (#tempo, #transport, #views) and workspace panels (#panel-browser, #panel-main, #panel-detail). Source is kept in JSON and advanced.css.");
            ui.label("Connected surfaces paint linear, radial and conic gradients, including repeating forms, and bounded inset/outset box shadows with approximate blur. Shared native text supports bounded text shadows with approximate blur; ordinary egui text still needs integration. Transitions and keyframes animate connected paint properties; turning off animations suppresses them. Transforms and filters are not painted. Registered scopes support siblings, named project objects and native response states; some raw controls still need integration. Toolbar height remains the host's height. Unsupported-effect notices appear when applying CSS.");
            if ui.button("Load current advanced CSS").clicked() { state.advanced = themes.skin.css.as_deref().unwrap_or_default().into(); }
            if ui.button("Load dark gloss example").clicked() { state.advanced = crate::kit::theme::GLOSS_EXAMPLE.into(); }
            if ui.button("Load Flex/Grid example").clicked() {
                state.advanced = ":root { --ink: rgb(180 220 240); }\n.browser { color: var(--ink); font-size: clamp(10px, 1vw, 14px); }\n.toolbar { display: flex; gap: 8px; }\n#transport { order: -1; }\n@media (max-width: 1000px) { #performance { display: none; } }\n".into();
            }
            if ui.button("Load workspace Grid example").clicked() {
                state.advanced = crate::kit::theme::WORKSPACE_GRID_EXAMPLE.into();
            }
            ui.add(egui::TextEdit::multiline(&mut state.advanced).code_editor().desired_rows(10).desired_width(f32::INFINITY));
            if ui.button("Apply advanced CSS").clicked() {
                if let Err(error) = crate::kit::native_css::inspect(&state.advanced) { state.error=error; }
                else {
                    let mut skin = themes.skin.clone();
                    skin.css = (!state.advanced.is_empty()).then(|| std::sync::Arc::from(state.advanced.as_str()));
                    request = Some(ThemeRequest::EditSkin(skin)); state.error.clear();
                    state.notices=crate::kit::native_css::inspect(&state.advanced).unwrap_or_default().into_iter().map(|notice| format!("Line {}: {} is preserved, but this native painter does not render it yet.",notice.line,notice.property)).collect();
                }
            }
            for notice in &state.notices { ui.label(notice); }
        });
        if !state.error.is_empty() { ui.colored_label(crate::kit::theme::recording(), &state.error); }
    });
    request
}
