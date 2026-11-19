// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Persistent, protocol-only configuration of a physical MIDI input.
use serde_json::{Value, json};

/// How a physical MIDI input selects its expression zones.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MpeMode {
    /// Ordinary channel MIDI; RPN 6 does not enable expression zones.
    #[default]
    Off,
    /// Zone configuration follows the controller's RPN 6 messages.
    Automatic,
    /// The user's zone sizes and sensitivity take precedence over controller configuration.
    Manual,
}

/// A port's two MPE zones. Counts exclude their manager channel; bends are semitones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MpeConfig {
    pub mode: MpeMode,
    pub lower_members: u8,
    pub upper_members: u8,
    pub lower_member_bend: u8,
    pub lower_manager_bend: u8,
    pub upper_member_bend: u8,
    pub upper_manager_bend: u8,
}

impl Default for MpeConfig {
    fn default() -> Self {
        Self {
            mode: MpeMode::Off,
            lower_members: 15,
            upper_members: 0,
            lower_member_bend: 48,
            lower_manager_bend: 2,
            upper_member_bend: 48,
            upper_manager_bend: 2,
        }
    }
}

impl MpeConfig {
    /// Portable settings/command representation with stable field names.
    pub fn to_json(self) -> Value {
        json!({
            "mode": match self.mode { MpeMode::Off => "off", MpeMode::Automatic => "automatic", MpeMode::Manual => "manual" },
            "lower_members": self.lower_members, "upper_members": self.upper_members,
            "lower_member_bend": self.lower_member_bend, "lower_manager_bend": self.lower_manager_bend,
            "upper_member_bend": self.upper_member_bend, "upper_manager_bend": self.upper_manager_bend,
        })
    }

    /// Reads bounded settings, rejecting malformed/unknown fields and overlapping zones.
    pub fn from_json(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let mut config = Self::default();
        for (key, value) in object {
            if key == "mode" {
                config.mode = match value.as_str()? {
                    "off" => MpeMode::Off,
                    "automatic" => MpeMode::Automatic,
                    "manual" => MpeMode::Manual,
                    _ => return None,
                };
                continue;
            }
            let field = match key.as_str() {
                "lower_members" => &mut config.lower_members,
                "upper_members" => &mut config.upper_members,
                "lower_member_bend" => &mut config.lower_member_bend,
                "lower_manager_bend" => &mut config.lower_manager_bend,
                "upper_member_bend" => &mut config.upper_member_bend,
                "upper_manager_bend" => &mut config.upper_manager_bend,
                _ => return None,
            };
            *field = u8::try_from(value.as_u64()?).ok()?;
        }
        config.is_valid().then_some(config)
    }
    /// Rejects overlapping manual zones and unsupported sensitivity instead of clamping them.
    pub fn is_valid(self) -> bool {
        let lower = self.lower_members;
        let upper = self.upper_members;
        lower <= 15
            && upper <= 15
            && (lower == 0 || upper == 0 || u16::from(lower) + u16::from(upper) <= 14)
            && [self.lower_member_bend, self.lower_manager_bend, self.upper_member_bend, self.upper_manager_bend]
                .into_iter()
                .all(|range| range <= 96)
    }
}
