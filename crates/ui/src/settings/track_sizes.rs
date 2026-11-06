// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Named Arrangement row sizes, their customization and settings-file compatibility.

/// A named row height; units are only exposed in Advanced settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackSize {
    Tiny,
    Compact,
    Small,
    Medium,
    Large,
}

impl TrackSize {
    pub const ALL: [Self; 5] = [Self::Tiny, Self::Compact, Self::Small, Self::Medium, Self::Large];
    pub fn index(self) -> usize {
        match self {
            Self::Tiny => 0,
            Self::Compact => 1,
            Self::Small => 2,
            Self::Medium => 3,
            Self::Large => 4,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Tiny => "Tiny",
            Self::Compact => "Compact",
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
        }
    }
    fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.name().eq_ignore_ascii_case(value))
    }
}

/// Ordered, bounded sizes. Private heights prevent invalid order entering the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackSizes {
    heights: [u16; 5],
    pub tracks: TrackSize,
    pub returns: TrackSize,
    pub advanced: bool,
}

impl Default for TrackSizes {
    fn default() -> Self {
        Self { heights: [14, 20, 40, 64, 104], tracks: TrackSize::Small, returns: TrackSize::Compact, advanced: false }
    }
}

impl TrackSizes {
    pub fn height(self, size: TrackSize) -> u16 {
        self.heights[size.index()]
    }
    /// Neighbours impose strict order; all sizes stay within the view's supported range.
    pub fn bounds(self, size: TrackSize) -> (u16, u16) {
        let i = size.index();
        let lower = if i == 0 { 14 } else { self.heights[i - 1] + 1 };
        let upper = if i == 4 { 400 } else { self.heights[i + 1] - 1 };
        (lower, upper)
    }
    pub fn set_height(&mut self, size: TrackSize, height: u16) {
        let (lower, upper) = self.bounds(size);
        self.heights[size.index()] = height.clamp(lower, upper);
    }
    pub fn to_text(self) -> String {
        let [a, b, c, d, e] = self.heights;
        format!(
            "arrangement_track_size = {}\narrangement_return_size = {}\narrangement_size_heights = {a},{b},{c},{d},{e}\narrangement_sizes_advanced = {}\n",
            self.tracks.name().to_lowercase(),
            self.returns.name().to_lowercase(),
            if self.advanced { "on" } else { "off" }
        )
    }
    /// Read one complete setting, accepting old files through defaults.
    pub fn parse(&mut self, key: &str, value: &str) -> bool {
        match key {
            "arrangement_track_size" | "arrangement_return_size" => {
                let Some(size) = TrackSize::parse(value) else { return false };
                if key == "arrangement_track_size" {
                    self.tracks = size;
                } else {
                    self.returns = size;
                }
            }
            "arrangement_sizes_advanced" if matches!(value, "on" | "off") => self.advanced = value == "on",
            "arrangement_size_heights" => {
                let mut parsed = [0_u16; 5];
                let mut values = value.split(',');
                let mut count = 0;
                for entry in &mut parsed {
                    let Some(value) = values.next() else { break };
                    let Ok(n) = value.trim().parse::<u16>() else { return false };
                    *entry = n;
                    count += 1;
                }
                if values.next().is_some() || count < 4 {
                    return false;
                }
                // Older files store Compact through Large, with no Tiny entry.
                if count == 4 {
                    parsed.rotate_right(1);
                    parsed[0] = 14;
                }
                // Normalize the entire tuple together, independent of settings-line ordering.
                let mut previous = 13;
                for (i, entry) in parsed.iter_mut().enumerate() {
                    *entry = (*entry).clamp(previous + 1, 400 - (4 - i) as u16);
                    previous = *entry;
                }
                self.heights = parsed;
            }
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    #[test]
    fn presets_and_custom_sizes_round_trip_and_old_files_keep_defaults() {
        let mut settings = Settings::default();
        let sizes = &mut settings.view.track_sizes;
        sizes.tracks = TrackSize::Medium;
        sizes.returns = TrackSize::Small;
        sizes.set_height(TrackSize::Medium, 78);
        sizes.advanced = true;
        let (read, warnings) = Settings::from_text(&settings.to_text());
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(read.view.track_sizes, settings.view.track_sizes);
        assert_eq!(Settings::from_text("").0.view.track_sizes, TrackSizes::default());
    }

    #[test]
    fn edits_and_hostile_settings_never_invert_neighbour_sizes() {
        let mut sizes = TrackSizes::default();
        for size in TrackSize::ALL {
            for value in [0, 1, 20, 400, u16::MAX] {
                sizes.set_height(size, value);
                assert!(sizes.heights.windows(2).all(|w| w[0] < w[1]));
                assert!(sizes.heights.iter().all(|h| (14..=400).contains(h)));
            }
        }
        assert!(sizes.parse("arrangement_size_heights", "65535,0,0,0"));
        assert_eq!(sizes.heights, [14, 397, 398, 399, 400]);
        let before = sizes;
        assert!(!sizes.parse("arrangement_size_heights", "20,NaN,64,100"));
        assert_eq!(sizes, before);
        assert!(!sizes.parse("arrangement_size_heights", "14,20,40,64,100,200"));
        assert_eq!(sizes, before);
    }

    #[test]
    fn legacy_custom_heights_keep_their_named_sizes_and_gain_tiny() {
        let mut sizes = TrackSizes::default();
        assert!(sizes.parse("arrangement_size_heights", "24,44,78,120"));
        assert_eq!(sizes.heights, [14, 24, 44, 78, 120]);
        assert!(sizes.parse("arrangement_track_size", "tiny"));
        let (read, warnings) = Settings::from_text(&sizes.to_text());
        assert!(warnings.is_empty());
        assert_eq!(read.view.track_sizes, sizes);
        assert!(sizes.parse("arrangement_size_heights", "65535,0,0,0,0"));
        assert_eq!(sizes.heights, [396, 397, 398, 399, 400]);
    }
}
