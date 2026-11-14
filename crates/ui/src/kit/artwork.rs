// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded artwork variants: interaction state, then optional image frames.
use super::skin::{Skin, State};

pub fn variant(skin: &Skin, role: &str, state: State, time: f64, motion: bool) -> Option<(String, bool)> {
    choose(skin, role, state, time, motion, None)
}

/// Signal-driven frames do not animate with time or replace native clip handling.
pub fn level_variant(skin: &Skin, state: State, level: f32) -> Option<(String, bool)> {
    choose(skin, "meter", state, 0.0, false, Some(level))
}

fn choose(
    skin: &Skin,
    role: &str,
    state: State,
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
    let candidates =
        [Some(format!("{role}{suffix}")), fallback.map(|fallback| format!("{role}{fallback}")), Some(role.to_owned())];
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
        for key in ["meter.frame-0", "meter.frame-1", "meter.hover.frame-0"] {
            skin.assets.insert(key.into(), "test".into());
        }
        let pick = |state, value| level_variant(&skin, state, value).unwrap();
        assert_eq!(pick(State::Normal, 0.0), ("meter.frame-0".into(), false));
        assert_eq!(pick(State::Normal, 0.5).0, "meter.frame-1");
        assert_eq!(pick(State::Normal, 5.0).0, "meter.frame-1");
        assert_eq!(pick(State::Normal, f32::NAN).0, "meter.frame-0");
        assert_eq!(pick(State::Hover, 1.0).0, "meter.hover.frame-0");
        assert_eq!(pick(State::Active, 1.0).0, "meter.frame-1");
        skin.assets.clear();
        skin.assets.insert("meter".into(), "test".into());
        assert_eq!(level_variant(&skin, State::Normal, 1.0), None);
    }
    #[test]
    fn state_sequences_fall_back_and_freeze_with_motion_off() {
        let mut skin = Skin::default();
        for key in ["track.name", "track.name.hover.frame-0", "track.name.hover.frame-1", "track.name.active"] {
            skin.assets.insert(key.into(), "test".into());
        }
        let pick = |state, time, motion| variant(&skin, "track.name", state, time, motion).unwrap();
        assert_eq!(pick(State::Hover, 1.0 / 12.0, true), ("track.name.hover.frame-1".into(), true));
        assert_eq!(pick(State::Hover, 1.0 / 12.0, false), ("track.name.hover.frame-0".into(), false));
        assert_eq!(pick(State::Hover, f64::NAN, true).0, "track.name.hover.frame-0");
        assert_eq!(pick(State::On, 0.0, true).0, "track.name.active", "on falls back to pressed");
        assert_eq!(pick(State::Normal, 0.0, true).0, "track.name");
        assert_eq!(pick(State::Disabled, 0.0, true).0, "track.name");
    }
}
