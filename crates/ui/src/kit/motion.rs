// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Animations, and the switch that turns them off. Every animated value in the views goes
//! through here, so with animations off each one jumps straight to where it is going.

use egui::{Color32, Context, Id};

/// How long the short transitions take, in seconds.
pub const HOVER: f32 = 0.08;
pub const VALUE: f32 = 0.12;
pub const SLIDE: f32 = 0.16;
/// One pulse of a queued clip, in seconds.
const PULSE_PERIOD: f64 = 0.8;
/// egui's own animations (combo boxes, panels) when ours are on.
const EGUI_ANIMATION_TIME: f32 = 0.083;

fn key() -> Id {
    Id::unique("suhina-animations")
}

/// Turns animations on or off, ours and egui's own.
pub fn set_enabled(ctx: &Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(key(), on));
    ctx.all_styles_mut(|style| style.animation_time = if on { EGUI_ANIMATION_TIME } else { 0.0 });
}

/// Animations are on unless turned off.
pub fn enabled(ctx: &Context) -> bool {
    ctx.data(|d| d.get_temp(key())).unwrap_or(true)
}

/// 0 … 1, moving toward 1 while `on` and toward 0 otherwise.
pub fn toward(ctx: &Context, id: Id, on: bool, seconds: f32) -> f32 {
    if enabled(ctx) { ctx.animate_bool_with_time(id, on, seconds) } else { f32::from(u8::from(on)) }
}

/// A value easing toward `target`.
pub fn value(ctx: &Context, id: Id, target: f32, seconds: f32) -> f32 {
    if enabled(ctx) && target.is_finite() { ctx.animate_value_with_time(id, target, seconds) } else { target }
}

/// Sets an animated value without easing, so the next change eases from here.
pub fn jump(ctx: &Context, id: Id, target: f32) -> f32 {
    if target.is_finite() { ctx.animate_value_with_time(id, target, 0.0) } else { target }
}

/// A slow breath between 0 and 1 for things that wait (a queued clip); 1 with animations off.
pub fn pulse(ctx: &Context) -> f32 {
    if !enabled(ctx) {
        return 1.0;
    }
    ctx.request_repaint();
    let time = ctx.input(|i| i.time);
    (0.5 + 0.5 * (time * std::f64::consts::TAU / PULSE_PERIOD).cos()) as f32
}

/// `from` blended toward `to` by `t`.
pub fn mix(from: Color32, to: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let channel = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        channel(from.r(), to.r()),
        channel(from.g(), to.g()),
        channel(from.b(), to.b()),
        channel(from.a(), to.a()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translucent_colours_are_not_premultiplied_twice() {
        let a = Color32::from_rgba_unmultiplied(80, 140, 200, 128);
        assert_eq!(mix(a, a, 0.5), a);
        assert_eq!(mix(a, Color32::TRANSPARENT, 0.0), a);
        assert_eq!(mix(a, Color32::TRANSPARENT, 1.0), Color32::TRANSPARENT);
    }

    #[test]
    fn colours_blend_from_one_end_to_the_other() {
        let (a, b) = (Color32::from_rgb(0, 100, 200), Color32::from_rgb(100, 100, 0));
        assert_eq!(mix(a, b, 0.0), a);
        assert_eq!(mix(a, b, 1.0), b);
        assert_eq!(mix(a, b, 0.5), Color32::from_rgb(50, 100, 100));
    }

    #[test]
    fn with_animations_off_values_jump() {
        let ctx = Context::default();
        set_enabled(&ctx, false);
        assert!(!enabled(&ctx));
        assert_eq!(toward(&ctx, Id::unique("a"), true, 1.0), 1.0);
        assert_eq!(value(&ctx, Id::unique("b"), 0.3, 1.0), 0.3);
        assert_eq!(pulse(&ctx), 1.0);
    }
}
