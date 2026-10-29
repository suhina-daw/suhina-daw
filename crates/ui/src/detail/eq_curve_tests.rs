// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use suhina_device_sdk::builtin;

/// The panel's parameters at the device's defaults, as `show` reads them.
fn panel_parameters() -> Vec<GraphParameter> {
    builtin::parameters(&suhina_device_eq::INFO).iter().map(|p| GraphParameter::new(p, p.default)).collect()
}

/// The display's rectangle, detached from the screen.
fn display() -> Rect {
    Rect::from_min_size(Pos2::ZERO, Vec2::new(DISPLAY[2], DISPLAY[3]))
}

/// The level in dB the curve's polyline shows at a frequency, between its neighbours.
fn level_at(points: &[Pos2], display: Rect, frequency: f64) -> f64 {
    let x = frequency_x(frequency, display);
    let index = points.partition_point(|point| point.x < x).max(1).min(points.len() - 1);
    let (left, right) = (points[index - 1], points[index]);
    let t = ((x - left.x) / (right.x - left.x).max(1.0)).clamp(0.0, 1.0);
    y_db(left.y + (right.y - left.y) * t, display)
}

#[test]
fn the_axes_map_the_parameters_ranges_onto_the_display() {
    let display = display();
    for frequency in [10.0, 100.0, 1000.0, 65.3, 22_000.0] {
        let back = x_frequency(frequency_x(frequency, display), display);
        assert!((back - frequency).abs() < frequency * 1.0e-4, "{frequency} Hz mapped to {back} Hz");
    }
    for db in [-15.0, -6.0, 0.0, 6.0, 15.0] {
        let back = y_db(db_y(db, display), display);
        assert!((back - db).abs() < 1.0e-3, "{db} dB mapped to {back} dB");
    }
    assert_eq!(db_y(-DB, display), display.bottom());
    assert_eq!(db_y(DB, display), display.top());
    assert_eq!(db_y(0.0, display), display.center().y);
}

#[test]
fn a_bell_bends_the_curve_to_its_gain_at_its_frequency() {
    let display = display();
    let mut parameters = panel_parameters();
    // Band 3 is the defaults' Bell at 1 kHz: raising its gain bends the curve to it.
    let gain = parameters.iter_mut().find(|p| p.id == "a3Gain").unwrap();
    gain.value = 12.0;
    let curve = bands(&parameters, 0);
    assert_eq!(curve[2].frequency, 1000.0);
    assert!((level_at(&curve_points(display, &curve), display, 1000.0) - 12.0).abs() < 0.1);
    // Far from the bell the curve is the other bands', all flat at their defaults.
    assert!(level_at(&curve_points(display, &curve), display, 100.0).abs() < 0.5);
}

#[test]
fn a_bands_dot_sits_at_its_gain_even_when_the_sum_does_not() {
    let display = display();
    let mut parameters = panel_parameters();
    // Two bells at one frequency, one boosted to the ceiling and one cut to the floor:
    // their sum near 1 kHz is back near 0, and each dot still marks its own band's gain.
    let frequency = parameters.iter_mut().find(|p| p.id == "a2Frequency").unwrap();
    frequency.value = 1000.0;
    let boost = parameters.iter_mut().find(|p| p.id == "a2Gain").unwrap();
    boost.value = 15.0;
    let cut = parameters.iter_mut().find(|p| p.id == "a3Gain").unwrap();
    cut.value = -15.0;
    let curve = bands(&parameters, 0);
    let dot = db_y(curve[2].gain, display);
    assert!(dot > display.center().y, "a negative gain draws below the 0 dB line");
    assert!(level_at(&curve_points(display, &curve), display, 1000.0) > -6.0);
}

#[test]
fn the_bands_read_their_curve() {
    let parameters = panel_parameters();
    for (curve, prefix) in [(0, 'a'), (1, 'b')] {
        let bands = bands(&parameters, curve);
        assert_eq!(bands.len(), BANDS);
        for (index, band) in bands.iter().enumerate() {
            assert_eq!(band.frequency, value(&parameters, &format!("{prefix}{}Frequency", index + 1)));
            assert_eq!(band.on, value(&parameters, &format!("{prefix}{}On", index + 1)) >= 0.5);
        }
    }
    // The defaults: curve A's first four bands on, curve B's off bands are off.
    let curve_a = bands(&parameters, 0);
    assert_eq!(curve_a.iter().filter(|band| band.on).count(), 4);
    let curve_b = bands(&parameters, 1);
    assert_eq!(curve_b.iter().filter(|band| band.on).count(), 4);
}

#[test]
fn a_frequency_reads_as_the_left_column_reads_it() {
    assert_eq!(frequency_text(30.0), "30.0 Hz");
    assert_eq!(frequency_text(65.3), "65.3 Hz");
    assert_eq!(frequency_text(1000.0), "1.0 kHz");
    assert_eq!(frequency_text(18_000.0), "18.0 kHz");
}

#[test]
fn the_analyser_folds_a_tone_onto_its_band() {
    // A sine a quarter of full scale at 1 kHz, windowed and folded the way the panel does
    // (`hann`, `fold`): its band reads about −12 dB and its neighbours are far below.
    let band_of_1k = ((1000.0f64 / FREQUENCY.0).ln() / (FREQUENCY.1 / FREQUENCY.0).ln() * ANALYSER_BANDS as f64)
        .floor()
        .clamp(0.0, (ANALYSER_BANDS - 1) as f64) as usize;
    let mut samples = [0.0f32; RING];
    for (n, sample) in samples.iter_mut().enumerate() {
        *sample = 0.25 * (2.0 * std::f64::consts::PI * 1000.0 * n as f64 / 48_000.0).sin() as f32;
    }
    let window = hann();
    let sum = window.iter().sum::<f32>();
    let mut bins: Vec<rustfft::num_complex::Complex<f32>> = samples
        .iter()
        .zip(window.iter())
        .map(|(&sample, &weight)| rustfft::num_complex::Complex::new(sample * weight, 0.0))
        .collect();
    rustfft::FftPlanner::<f32>::new().plan_fft_forward(RING).process(&mut bins);
    let bands = fold(&bins, sum, 48_000.0);
    assert!((bands[band_of_1k] + 12.0).abs() < 1.0, "the 1 kHz band reads the tone's level, got {}", bands[band_of_1k]);
    for (band, level) in bands.iter().enumerate() {
        if band.abs_diff(band_of_1k) > 2 {
            assert!(*level < -30.0, "band {band} is {level} dB, far from a tone at 1 kHz");
        }
    }
}

#[test]
fn the_analyser_draws_on_its_own_scale_across_the_display() {
    let display = display();
    assert_eq!(analyser_y(0.0, display), display.top(), "a full-scale band at the top edge");
    assert_eq!(analyser_y(ANALYSER_DB.1, display), display.bottom(), "the floor at the bottom edge");
    assert!(
        (analyser_y(ANALYSER_DB.1 / 2.0, display) - display.center().y).abs() < 1.0e-3,
        "half the floor is the middle of the display"
    );
    assert_eq!(analyser_y(6.0, display), display.top(), "above full scale clamps to the top edge");
    assert_eq!(analyser_y(-90.0, display), display.bottom(), "below the floor clamps to the bottom edge");
}

#[test]
fn a_band_rises_quickly_and_falls_slowly_across_frames() {
    // The follower as the panel drives it: one step a frame, 30 ms apart. A band ten frames
    // into a louder reading is nearly there; ten frames into a quieter one it has fallen
    // some but is far from down — the fall is what keeps the area flowing.
    let follower = AttackRelease::new(ANALYSER_ATTACK, ANALYSER_RELEASE, 1.0 / 0.030);
    let mut shown = -60.0;
    for _ in 0..10 {
        shown = follower.rise(shown, -10.0);
    }
    assert!(shown > -16.0, "300 ms of rising brings the band near its louder reading, {shown}");
    let top = shown;
    for _ in 0..10 {
        shown = follower.rise(shown, -60.0);
    }
    assert!(shown < top - 20.0, "the band did fall, {shown} from {top}");
    assert!(shown > -45.0, "but 300 ms on it is still well above its quieter reading, {shown}");
}

#[test]
fn only_a_top_level_device_is_watched_by_the_analyser() {
    use crate::detail::detail_view::DeviceAt;
    assert_eq!(watch_key(&DeviceAt(vec![3])), Some(3), "a top-level device by its index");
    assert_eq!(watch_key(&DeviceAt(vec![])), None, "no path, no watch");
    assert_eq!(watch_key(&DeviceAt(vec![1, 0, 0])), None, "a grouped device is not tapped");
    assert_eq!(watch_key(&DeviceAt(vec![2, 1, 3, 0, 1])), None, "nested groups too");
}
