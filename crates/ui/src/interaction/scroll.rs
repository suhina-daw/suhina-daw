// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! How far the timelines scroll: the Arrangement's and the piano roll's.

/// How far the timeline scrolls sideways: to the end of `content_beats`, or as far as a zoom
/// at the pointer reached (`reach` beats at the right edge), in a view `view_width` wide.
pub fn most_scroll_x(content_beats: f64, reach: f64, beat_width: f32, view_width: f32) -> f32 {
    ((content_beats.max(reach) as f32) * beat_width - view_width).max(0.0)
}
