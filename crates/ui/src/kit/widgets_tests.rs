// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::kit::element::test_el;

#[test]
fn a_pan_and_a_send_take_their_own_knob_artwork_and_other_knobs_do_not() {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10"/></svg>"#;
    let mut skin = super::super::skin::Skin::default();
    skin.assets.insert("pan.knob.body".into(), svg.into());
    skin.assets.insert("send.knob.body".into(), svg.into());
    super::super::skin::configure(&skin);
    assert_eq!(knob_roles().0, "knob.body", "a device's knob");
    {
        let _pan = test_el("pan").enter();
        assert_eq!(knob_roles(), ("pan.knob.body", "pan.knob.pointer"));
    }
    {
        let _send = test_el("send").enter();
        assert_eq!(knob_roles(), ("send.knob.body", "send.knob.pointer"));
    }
    super::super::skin::configure(&super::super::skin::Skin::default());
    let _pan = test_el("pan").enter();
    assert_eq!(knob_roles().0, "knob.body", "without the theme's own artwork");
}

#[test]
fn active_toggle_text_stays_visible_over_transparent_or_gradient_panels() {
    let original = theme::palette();
    let mut palette = original;
    palette.window_background = Color32::TRANSPARENT;
    palette.text_value = Color32::from_rgb(220, 230, 220);
    let ctx = egui::Context::default();
    theme::set_palette(&ctx, palette);
    motion::set_enabled(&ctx, false);
    let mut output = ctx.run_ui(Default::default(), |ui| {
        let rect = Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::splat(24.0));
        let response = button_area(ui, rect, Id::unique("transparent-toggle"), "Track on");
        let (fill, text) = toggle_colors(ui, &response, true);
        assert_eq!(text.a(), 255);
        assert!(theme::contrast(fill, text) >= 4.5);
    });
    output.textures_delta.clear();
    theme::set_palette(&ctx, original);
}

#[test]
fn active_toggle_text_stays_visible_when_the_inverse_ink_is_translucent() {
    let original = theme::palette();
    let mut palette = original;
    // Nearly transparent white ink on a black lit fill: opaque white would read, this does not.
    palette.control_selected = Color32::BLACK;
    palette.text_inverse = Color32::from_rgba_unmultiplied(255, 255, 255, 20);
    let ctx = egui::Context::default();
    theme::set_palette(&ctx, palette);
    motion::set_enabled(&ctx, false);
    let mut output = ctx.run_ui(Default::default(), |ui| {
        let rect = Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::splat(24.0));
        let response = button_area(ui, rect, Id::unique("faint-ink-toggle"), "Track on");
        let (fill, text) = toggle_colors(ui, &response, true);
        assert_ne!(text, palette.text_inverse, "the faint ink is replaced");
        assert!(theme::text_contrast(text, fill) >= 4.5, "{text:?} on {fill:?}");
    });
    output.textures_delta.clear();
    theme::set_palette(&ctx, original);
}

#[test]
fn shared_form_controls_keep_surfaces_transparent_and_do_not_restyle_buttons() {
    let ctx = egui::Context::default();
    let mut text = String::from("Project name");
    let mut number = 12;
    let mut fields = Vec::new();
    let mut output = ctx.run_ui(Default::default(), |ui| {
        let original = ui.visuals().widgets.inactive.bg_fill;
        fields.push(text_input(ui, egui::TextEdit::singleline(&mut text)).rect);
        fields.push(number_input(ui, None, egui::DragValue::new(&mut number)).rect);
        fields.push(
            combo_box(ui, egui::ComboBox::from_id_salt("shared-choice").selected_text("One"), |ui| {
                ui.label("Two");
            })
            .response
            .rect,
        );
        assert_eq!(ui.visuals().widgets.inactive.bg_fill, original);
    });
    for rect in fields {
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::LineSegment { points, stroke } if points[0].y == rect.bottom() - 0.5 && points[0].y == points[1].y && stroke.width > 0.0)));
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Rect(shape) if rect.contains_rect(shape.rect) && (shape.fill.a() > 0 || shape.stroke.width > 0.0))));
    }
    output.textures_delta.clear();
}

#[test]
fn form_dropdown_has_an_underline_without_a_box_or_fill() {
    let ctx = egui::Context::default();
    let rect = Rect::from_min_size(Pos2::new(20.0, 20.0), Vec2::new(180.0, 18.0));
    let mut output = ctx.run_ui(Default::default(), |ui| {
        let response = button_area(ui, rect, Id::unique("form-dropdown-test"), "Choice");
        form_dropdown(ui, ui.painter(), rect, &response, "Every 5 minutes");
    });
    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::LineSegment { points, stroke } if points[0].y == rect.bottom() - 0.5
                && points[0].y == points[1].y && stroke.width > 0.0)));
    assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(shape) if shape.rect == rect)));
    output.textures_delta.clear();
}

#[test]
fn values_read_like_the_mixer() {
    assert_eq!(format_db(0.0), "0.0");
    assert_eq!(format_db(-6.02), "-6.0");
    assert_eq!(format_db(f64::NEG_INFINITY), "-inf");
    assert_eq!(format_db(-80.0), "-inf");
    assert_eq!(format_pan(0.0), "C");
    assert_eq!(format_pan(-0.3), "15L");
    assert_eq!(format_pan(1.0), "50R");
}

#[test]
fn a_value_drag_hides_the_pointer_until_the_button_is_up_or_the_window_loses_focus() {
    use egui::{Event, PointerButton, ViewportCommand, ViewportId};
    let ctx = egui::Context::default();
    let rect = Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::splat(20.0));
    let pointer = |pos: Pos2, pressed: Option<bool>| {
        let mut events = vec![Event::PointerMoved(pos)];
        if let Some(pressed) = pressed {
            events.push(Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            });
        }
        egui::RawInput { events, ..Default::default() }
    };
    let moved = std::cell::Cell::new(Vec2::ZERO);
    let frame = |input| {
        let mut output = ctx.run_ui(input, |ui| {
            release_value_drag(ui.ctx());
            let response = ui.interact(rect, Id::unique("knob"), Sense::drag());
            if response.dragged() {
                moved.set(moved.get() + value_drag_delta(ui, &response));
            }
        });
        output.textures_delta.clear();
        output.viewport_output.get(&ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default()
    };
    let held = |ctx: &egui::Context| ctx.data(|d| d.get_temp::<ValueDrag>(value_drag_id())).is_some();
    // egui hits the widgets of the frame before, so the pointer is over the knob first.
    frame(pointer(Pos2::new(20.0, 20.0), None));
    let hidden = [
        frame(pointer(Pos2::new(20.0, 20.0), Some(true))),
        frame(pointer(Pos2::new(20.0, 10.0), None)),
        frame(pointer(Pos2::new(20.0, 0.0), None)),
    ]
    .concat();
    assert!(held(&ctx), "the drag holds the pointer");
    assert!(hidden.contains(&ViewportCommand::CursorVisible(false)), "{hidden:?}");
    assert!(moved.get().y < 0.0, "dragging up moves up: {:?}", moved.get());
    let shown = frame(pointer(Pos2::new(20.0, 0.0), Some(false)));
    let shown = [shown, frame(pointer(Pos2::new(20.0, 0.0), None))].concat();
    assert!(!held(&ctx));
    // The icon a view sets next shows it: a command would come after it and overrule a hidden one.
    assert!(!shown.contains(&ViewportCommand::CursorVisible(true)), "{shown:?}");
    // The release went to another window: losing focus gives the pointer back all the same.
    frame(pointer(Pos2::new(20.0, 20.0), None));
    frame(pointer(Pos2::new(20.0, 20.0), Some(true)));
    frame(pointer(Pos2::new(20.0, 10.0), None));
    assert!(held(&ctx), "a second drag holds the pointer");
    let unfocused = || egui::RawInput { focused: false, ..pointer(Pos2::new(20.0, 10.0), None) };
    let shown = [frame(unfocused()), frame(unfocused())].concat();
    assert!(!held(&ctx), "an unfocused window never holds the pointer");
    assert!(!shown.contains(&ViewportCommand::CursorVisible(true)), "{shown:?}");
    // Focus back with the drag still going (the release never came): the pointer stays shown.
    let back = [frame(pointer(Pos2::new(20.0, 5.0), None)), frame(pointer(Pos2::new(20.0, 0.0), None))].concat();
    assert!(!held(&ctx), "only a new press takes the pointer again");
    assert!(!back.contains(&ViewportCommand::CursorVisible(false)), "{back:?}");
    let before = moved.get();
    frame(pointer(Pos2::new(20.0, -5.0), None));
    assert_eq!(moved.get(), before, "a drag that lost the pointer moves nothing");
}

/// One press and a drag up on a knob, with the setting `setting` and the knob's own answer
/// `own` ([`value_drag_hides`]): the commands it sent and the pointer it showed while dragged.
fn drag_with(setting: bool, own: Option<bool>) -> (Vec<egui::ViewportCommand>, egui::CursorIcon) {
    use egui::{Event, PointerButton, ViewportId};
    let ctx = egui::Context::default();
    set_drag_hides_pointer(&ctx, setting);
    let rect = Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::splat(20.0));
    let frame = |pos: Pos2, pressed: Option<bool>| {
        let mut events = vec![Event::PointerMoved(pos)];
        if let Some(pressed) = pressed {
            events.push(Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            });
        }
        let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
            release_value_drag(ui.ctx());
            let response =
                ui.interact(rect, Id::unique("knob"), Sense::drag()).on_hover_cursor(egui::CursorIcon::PointingHand);
            if let Some(hides) = own {
                value_drag_hides(ui.ctx(), &response, hides);
            }
            let mut preview = None;
            drag_value_axis(ui, &response, &mut preview, 0.0, 1.0, 1.0, (-100.0, 100.0), 0.0, DragAxis::Vertical);
            hide_pointer_during_value_drag(ui.ctx());
        });
        output.textures_delta.clear();
        let commands = output.viewport_output.get(&ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default();
        (commands, output.platform_output.cursor_icon)
    };
    frame(Pos2::new(20.0, 20.0), None);
    let (mut commands, _) = frame(Pos2::new(20.0, 20.0), Some(true));
    let (more, _) = frame(Pos2::new(20.0, 10.0), None);
    commands.extend(more);
    let (more, icon) = frame(Pos2::new(20.0, 0.0), None);
    commands.extend(more);
    (commands, icon)
}

#[test]
fn a_value_drag_shows_the_pointer_when_the_setting_or_its_control_says_so() {
    let hidden = egui::ViewportCommand::CursorVisible(false);
    // The setting, on by default: hidden.
    let (commands, icon) = drag_with(true, None);
    assert!(commands.contains(&hidden), "{commands:?}");
    assert_eq!(icon, egui::CursorIcon::None);
    // Off: the pointer stays, with the arrow of the drag's axis.
    let (commands, icon) = drag_with(false, None);
    assert!(!commands.contains(&hidden), "{commands:?}");
    assert_eq!(icon, egui::CursorIcon::ResizeVertical);
    // The control's own answer (its element's `drag-pointer`) wins over the setting, both ways.
    let (commands, icon) = drag_with(false, Some(true));
    assert!(commands.contains(&hidden), "{commands:?}");
    assert_eq!(icon, egui::CursorIcon::None);
    let (commands, icon) = drag_with(true, Some(false));
    assert!(!commands.contains(&hidden), "{commands:?}");
    assert_eq!(icon, egui::CursorIcon::ResizeVertical);
}

/// Wayland moves a pointer only while it is locked: the pointer goes back before the lock ends.
#[test]
fn a_value_drag_puts_the_pointer_back_before_it_unlocks_it() {
    use egui::ViewportCommand;
    let ctx = egui::Context::default();
    let back = Pos2::new(5.0, 6.0);
    let held = ValueDrag { raw: true, back: Some(back), released: false, hides: true, icon: egui::CursorIcon::None };
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.ctx().data_mut(|d| d.insert_temp(value_drag_id(), held));
        release_value_drag(ui.ctx());
    });
    output.textures_delta.clear();
    let commands = output.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default();
    let at = |command: &ViewportCommand| commands.iter().position(|c| c == command);
    let moved = at(&ViewportCommand::CursorPosition(back)).expect("moved back");
    let unlocked = at(&ViewportCommand::CursorGrab(egui::CursorGrab::None)).expect("unlocked");
    assert!(moved < unlocked, "{commands:?}");
}

#[test]
fn raw_motion_locks_the_pointer_and_is_scaled_to_points() {
    use egui::{Event, PointerButton, ViewportCommand, ViewportId};
    let ctx = egui::Context::default();
    ctx.set_zoom_factor(2.0);
    let rect = Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::splat(20.0));
    let moved = std::cell::Cell::new(Vec2::ZERO);
    let frame = |events: Vec<Event>| {
        let input = egui::RawInput { events, ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| {
            release_value_drag(ui.ctx());
            let response = ui.interact(rect, Id::unique("knob"), Sense::drag());
            if response.dragged() {
                moved.set(moved.get() + value_drag_delta(ui, &response));
            }
        });
        output.textures_delta.clear();
        output.viewport_output.get(&ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default()
    };
    let at = Pos2::new(20.0, 20.0);
    let locked = ViewportCommand::CursorGrab(egui::CursorGrab::Locked);
    frame(vec![Event::PointerMoved(at)]);
    frame(vec![Event::PointerMoved(at)]);
    frame(vec![
        Event::PointerMoved(at),
        Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Default::default() },
    ]);
    // No raw motion: a pen. The pointer's own movement moves the value and nothing locks it.
    let pen = frame(vec![Event::PointerMoved(Pos2::new(20.0, 10.0))]);
    assert!(!pen.contains(&locked), "{pen:?}");
    assert!(moved.get().y < 0.0, "{:?}", moved.get());
    // Raw motion: a mouse. It locks the pointer, in points, and the pointer's movement no longer counts.
    moved.set(Vec2::ZERO);
    let mouse = frame(vec![Event::MouseMoved(Vec2::new(0.0, -8.0))]);
    assert!(mouse.contains(&locked), "{mouse:?}");
    assert_eq!(moved.get(), Vec2::new(0.0, -4.0));
    frame(vec![Event::PointerMoved(Pos2::new(20.0, 0.0))]);
    assert_eq!(moved.get(), Vec2::new(0.0, -4.0), "only raw motion counts once it has come");
}

#[test]
fn a_view_drawn_after_a_value_drag_does_not_show_the_pointer_again() {
    let ctx = egui::Context::default();
    let run = |hold: bool, after: bool| {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            if hold {
                ui.ctx().data_mut(|d| {
                    d.insert_temp(
                        value_drag_id(),
                        ValueDrag { raw: true, back: None, released: false, hides: true, icon: egui::CursorIcon::None },
                    )
                });
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            if after {
                hide_pointer_during_value_drag(ui.ctx());
            }
        });
        output.textures_delta.clear();
        output.platform_output.cursor_icon
    };
    assert_eq!(run(true, true), egui::CursorIcon::None);
    ctx.data_mut(|d| d.remove::<ValueDrag>(value_drag_id()));
    assert_eq!(run(false, true), egui::CursorIcon::ResizeVertical);
}

/// The icon is the frame's own: the frame a value drag lets go, no view has to bring the pointer back.
#[test]
fn the_frame_a_value_drag_lets_go_the_icon_shows_the_pointer() {
    let ctx = egui::Context::default();
    let held = |released| ValueDrag { raw: true, back: None, released, hides: true, icon: egui::CursorIcon::None };
    let icon = |drag: Option<ValueDrag>| {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            if let Some(drag) = drag {
                ui.ctx().data_mut(|d| d.insert_temp(value_drag_id(), drag));
            }
            release_value_drag(ui.ctx());
            hide_pointer_during_value_drag(ui.ctx());
        });
        output.textures_delta.clear();
        output.platform_output.cursor_icon
    };
    assert_eq!(icon(Some(held(true))), egui::CursorIcon::Default);
    assert_eq!(icon(None), egui::CursorIcon::Default);
}

/// `text_in` cuts a name with an ellipsis while a letter still fits before it, and draws
/// nothing at all where only the ellipsis, or not even that, would fit (#1198).
#[test]
fn text_in_draws_nothing_where_only_an_ellipsis_would_fit() {
    let texts = |width: f32| {
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::new(width, 16.0));
        let mut output = ctx.run_ui(Default::default(), |ui| {
            text_in(ui.painter(), rect, Align2::LEFT_CENTER, "Kick 01", FontId::proportional(12.0), Color32::WHITE);
        });
        output.textures_delta.clear();
        output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(text) => Some((text.galley.size().x, text.galley.text().to_string())),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    for width in [-4.0, 0.0, 2.0, 5.0] {
        assert!(texts(width).is_empty(), "{width}: {:?}", texts(width));
    }
    let cut = texts(24.0);
    assert_eq!(cut.len(), 1);
    assert!(cut[0].0 <= 24.5, "{cut:?}");
    assert_eq!(texts(400.0).len(), 1, "the whole name");
}

#[test]
fn a_silent_value_steps_up_from_the_scale_end_and_stays_when_stepped_down() {
    let range = (-70.0, 6.0);
    assert_eq!(stepped(f64::NEG_INFINITY, range, 1.0), -69.0);
    assert_eq!(stepped(f64::NEG_INFINITY, range, -1.0), f64::NEG_INFINITY);
    assert_eq!(stepped(-10.0, range, -1.0), -11.0);
    assert_eq!(stepped(5.5, range, 3.0), 6.0);
}
