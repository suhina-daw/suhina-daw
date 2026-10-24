// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded artwork variants: raw track colour, interaction state, then optional image frames.
use super::skin::{Skin, State};
use egui::Color32;
use std::cell::Cell;

thread_local! { static TRACK_COLOR: Cell<Option<Color32>> = const { Cell::new(None) }; }
pub struct TrackColor(Option<Color32>);
pub fn track_color(color: Color32) -> TrackColor {
    TrackColor(TRACK_COLOR.with(|current| current.replace(Some(color))))
}
pub fn current_color() -> Option<Color32> {
    TRACK_COLOR.with(Cell::get)
}
impl Drop for TrackColor {
    fn drop(&mut self) {
        TRACK_COLOR.with(|current| current.set(self.0));
    }
}

pub fn variant(
    skin: &Skin,
    role: &str,
    state: State,
    color: Option<Color32>,
    time: f64,
    motion: bool,
) -> Option<(String, bool)> {
    choose(skin, role, state, color, time, motion, None)
}

/// Signal-driven frames do not animate with time or replace native clip handling.
pub fn level_variant(skin: &Skin, state: State, color: Option<Color32>, level: f32) -> Option<(String, bool)> {
    choose(skin, "meter", state, color, 0.0, false, Some(level))
}

#[allow(clippy::too_many_arguments)]
fn choose(
    skin: &Skin,
    role: &str,
    state: State,
    color: Option<Color32>,
    time: f64,
    motion: bool,
    level: Option<f32>,
) -> Option<(String, bool)> {
    let suffix = match state {
        State::Normal => "",
        State::Hover => ".hover",
        State::Active => ".active",
        State::Disabled => ".disabled",
        State::On => ".on",
    };
    // A toggle that is on falls back to the pressed artwork, as it looked before `.on` existed.
    let fallback = matches!(state, State::On).then_some(".active");
    let colored = color.map(|color| format!("{role}.color-{:02x}{:02x}{:02x}", color.r(), color.g(), color.b()));
    let candidates = [
        colored.as_ref().map(|key| format!("{key}{suffix}")),
        colored.as_ref().zip(fallback).map(|(key, fallback)| format!("{key}{fallback}")),
        colored,
        Some(format!("{role}{suffix}")),
        fallback.map(|fallback| format!("{role}{fallback}")),
        Some(role.to_owned()),
    ];
    for key in candidates.into_iter().flatten() {
        let frames = (0..32).take_while(|frame| skin.assets.contains_key(&format!("{key}.frame-{frame}"))).count();
        if frames > 0 {
            if let Some(level) = level {
                let level = if level.is_finite() { level.clamp(0.0, 1.0) } else { 0.0 };
                let index = ((level * frames as f32).floor() as usize).min(frames - 1);
                return Some((format!("{key}.frame-{index}"), false));
            }
            let phase = if motion && time.is_finite() {
                let fps = f64::from(skin.artwork_fps.clamp(1, 30));
                time.max(0.0).rem_euclid(frames as f64 / fps) * fps
            } else {
                0.0
            };
            return Some((format!("{key}.frame-{}", (phase.floor() as usize).min(frames - 1)), motion && frames > 1));
        }
        if level.is_none() && skin.assets.contains_key(&key) {
            return Some((key, false));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn measured_frames_clamp_levels_and_fall_back_across_missing_sequences() {
        let mut skin = Skin::default();
        for key in ["meter.frame-0", "meter.frame-1", "meter.color-00ff00.hover.frame-0"] {
            skin.assets.insert(key.into(), "test".into());
        }
        let pick = |state, color, value| level_variant(&skin, state, color, value).unwrap();
        assert_eq!(pick(State::Normal, None, 0.0), ("meter.frame-0".into(), false));
        assert_eq!(pick(State::Normal, None, 0.5).0, "meter.frame-1");
        assert_eq!(pick(State::Normal, None, 5.0).0, "meter.frame-1");
        assert_eq!(pick(State::Normal, None, f32::NAN).0, "meter.frame-0");
        assert_eq!(pick(State::Hover, Some(Color32::GREEN), 1.0).0, "meter.color-00ff00.hover.frame-0");
        assert_eq!(pick(State::Active, Some(Color32::GREEN), 1.0).0, "meter.frame-1");
        skin.assets.clear();
        skin.assets.insert("meter".into(), "test".into());
        assert_eq!(level_variant(&skin, State::Normal, None, 1.0), None);
    }
    #[test]
    fn nested_raw_track_colours_restore_their_parent_scope() {
        assert_eq!(current_color(), None);
        {
            let _outer = track_color(Color32::GREEN);
            assert_eq!(current_color(), Some(Color32::GREEN));
            {
                let _inner = track_color(Color32::RED);
                assert_eq!(current_color(), Some(Color32::RED));
            }
            assert_eq!(current_color(), Some(Color32::GREEN));
        }
        assert_eq!(current_color(), None);
    }
    #[test]
    fn colour_state_sequences_fall_back_and_freeze_with_motion_off() {
        let mut skin = Skin::default();
        for key in [
            "track.name",
            "track.name.hover",
            "track.name.color-00ff00",
            "track.name.color-00ff00.hover.frame-0",
            "track.name.color-00ff00.hover.frame-1",
        ] {
            skin.assets.insert(key.into(), "test".into());
        }
        let pick = |state, color, time, motion| variant(&skin, "track.name", state, color, time, motion).unwrap();
        assert_eq!(
            pick(State::Hover, Some(Color32::GREEN), 1.0 / 12.0, true),
            ("track.name.color-00ff00.hover.frame-1".into(), true)
        );
        assert_eq!(
            pick(State::Hover, Some(Color32::GREEN), 1.0 / 12.0, false),
            ("track.name.color-00ff00.hover.frame-0".into(), false)
        );
        assert_eq!(pick(State::Active, Some(Color32::GREEN), 0.0, true).0, "track.name.color-00ff00");
        assert_eq!(pick(State::Hover, Some(Color32::RED), f64::NAN, true).0, "track.name.hover");
        assert_eq!(pick(State::Normal, None, 0.0, true).0, "track.name");
    }
}
