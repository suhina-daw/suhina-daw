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

fn set(parameters: &mut [GraphParameter], id: &str, value: f64) {
    parameters.iter_mut().find(|p| p.id == id).unwrap().value = value;
}

/// The level in dB the curve's polyline shows at a frequency, between its neighbours.
fn level_at(points: &[Pos2], display: Rect, range: f64, frequency: f64) -> f64 {
    let x = frequency_x(frequency, display);
    let index = points.partition_point(|point| point.x < x).max(1).min(points.len() - 1);
    let (left, right) = (points[index - 1], points[index]);
    let t = ((x - left.x) / (right.x - left.x).max(1.0)).clamp(0.0, 1.0);
    y_db(left.y + (right.y - left.y) * t, display, range)
}

#[test]
fn the_axes_map_the_parameters_ranges_onto_the_display() {
    let display = display();
    for frequency in [10.0, 100.0, 1000.0, 65.3, 22_000.0] {
        let back = x_frequency(frequency_x(frequency, display), display);
        assert!((back - frequency).abs() < frequency * 1.0e-4, "{frequency} Hz mapped to {back} Hz");
    }
    for range in DB_RANGES {
        for db in [-range, -range / 2.0, 0.0, range / 3.0, range] {
            let back = y_db(db_y(db, display, range), display, range);
            assert!((back - db).abs() < 1.0e-3, "{db} dB mapped to {back} dB at ±{range}");
        }
        assert_eq!(db_y(-range, display, range), display.bottom());
        assert_eq!(db_y(range, display, range), display.top());
        assert_eq!(db_y(0.0, display, range), display.center().y);
    }
}

#[test]
fn a_bell_bends_the_curve_to_its_gain_at_its_frequency() {
    let display = display();
    let mut parameters = panel_parameters();
    // Band 3 is the defaults' Bell at 1 kHz: raising its gain bends the curve to it.
    set(&mut parameters, "a3Gain", 12.0);
    let bands = bands(&parameters);
    assert_eq!(bands[2].frequency, 1000.0);
    let stereo = curve_points(display, 15.0, &bands, |band| band.place == 0);
    assert!((level_at(&stereo, display, 15.0, 1000.0) - 12.0).abs() < 0.1);
    // Far from the bell the curve is the other bands', all flat at their defaults.
    assert!(level_at(&stereo, display, 15.0, 100.0).abs() < 0.5);
}

#[test]
fn a_band_on_one_side_bends_only_that_sides_curve() {
    let display = display();
    let mut parameters = panel_parameters();
    set(&mut parameters, "a3Gain", 9.0);
    set(&mut parameters, "a3Place", 4.0);
    let bands = bands(&parameters);
    let stereo = curve_points(display, 15.0, &bands, |band| band.place == 0);
    let side = curve_points(display, 15.0, &bands, |band| band.place == 0 || band.place == 4);
    assert!(level_at(&stereo, display, 15.0, 1000.0).abs() < 0.5, "the stereo curve stays flat");
    assert!((level_at(&side, display, 15.0, 1000.0) - 9.0).abs() < 0.1, "the side's curve has the bell");
}

#[test]
fn a_slot_not_in_use_shapes_no_curve() {
    let display = display();
    let mut parameters = panel_parameters();
    set(&mut parameters, "a3Gain", 12.0);
    set(&mut parameters, "a3InUse", 0.0);
    let bands = bands(&parameters);
    assert!(!bands[2].in_use);
    let stereo = curve_points(display, 15.0, &bands, |band| band.place == 0);
    assert!(level_at(&stereo, display, 15.0, 1000.0).abs() < 0.5);
}

#[test]
fn the_bands_read_their_slots() {
    let parameters = panel_parameters();
    let bands = bands(&parameters);
    assert_eq!(bands.len(), BANDS);
    for (index, band) in bands.iter().enumerate() {
        assert_eq!(band.frequency, value(&parameters, &format!("a{}Frequency", index + 1)));
        assert_eq!(band.on, value(&parameters, &format!("a{}On", index + 1)) >= 0.5);
        assert_eq!(band.in_use, index < suhina_device_eq::FIRST_BANDS, "band {}", index + 1);
    }
    // The defaults: the first eight in use, four of them on.
    assert_eq!(bands.iter().filter(|band| band.heard()).count(), 4);
}

#[test]
fn a_frequency_reads_as_the_panel_reads_it() {
    assert_eq!(frequency_text(30.0), "30.0 Hz");
    assert_eq!(frequency_text(65.3), "65.3 Hz");
    assert_eq!(frequency_text(1000.0), "1.0 kHz");
    assert_eq!(frequency_text(18_000.0), "18.0 kHz");
    assert_eq!(gain_text(4.2), "+4.2 dB");
    assert_eq!(gain_text(-3.5), "-3.5 dB");
    assert_eq!(gain_text(0.0), "0.0 dB");
}

#[test]
fn a_frequency_reads_as_its_nearest_note_and_cents() {
    assert_eq!(note_text(440.0), "A4 +0");
    assert_eq!(note_text(261.63), "C4 +0");
    assert_eq!(note_text(27.5), "A0 +0");
    assert_eq!(note_text(440.0 * 2f64.powf(13.0 / 1200.0)), "A4 +13");
    assert_eq!(note_text(440.0 * 2f64.powf(-20.0 / 1200.0)), "A4 -20");
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
    let follower = AttackRelease::new(ANALYSER_ATTACK, ANALYSER_RELEASE, 1.0 / 0.030);
    let mut shown = -60.0;
    for _ in 0..10 {
        shown = follower.rise(shown, -10.0);
    }
    assert!(((shown + 10.0).abs()) < 4.0, "300 ms of rising sits on the louder reading, {shown}");
    let top = shown;
    for _ in 0..10 {
        shown = follower.rise(shown, -60.0);
    }
    assert!(shown < top - 3.0, "the band did fall, {shown} from {top}");
    assert!(shown > top - 18.0, "but 300 ms on it is still most of the way up, {shown} against {top}");
}

#[test]
fn only_a_top_level_device_is_watched_by_the_analyser() {
    use crate::detail::detail_view::DeviceAt;
    assert_eq!(watch_key(&DeviceAt(vec![3])), Some(3), "a top-level device by its index");
    assert_eq!(watch_key(&DeviceAt(vec![])), None, "no path, no watch");
    assert_eq!(watch_key(&DeviceAt(vec![1, 0, 0])), None, "a grouped device is not tapped");
    assert_eq!(watch_key(&DeviceAt(vec![2, 1, 3, 0, 1])), None, "nested groups too");
}

#[test]
fn a_typed_frequency_reads_numbers_k_notation_and_notes() {
    let near = |typed: &str, expected: f64| {
        let got = parse_frequency(typed).unwrap_or_else(|| panic!("{typed} did not read"));
        assert!((got - expected).abs() < expected * 1.0e-4, "{typed}: {got} against {expected}");
    };
    near("440", 440.0);
    near("1,5", 1.5);
    near("6k3", 6300.0);
    near("2k", 2000.0);
    near("1.2k", 1200.0);
    near("A4", 440.0);
    near("a", 440.0);
    near("A0", 27.5);
    near("C4", 261.6256);
    near("D#5", 622.254);
    near("Bb3", 233.082);
    near("A4 +13", 440.0 * 2f64.powf(13.0 / 1200.0));
    near("A4-20", 440.0 * 2f64.powf(-20.0 / 1200.0));
    near("A +13", 440.0 * 2f64.powf(13.0 / 1200.0));
    near("A -20", 440.0 * 2f64.powf(-20.0 / 1200.0));
    for nonsense in ["", "k", "H4", "A4 +x", "kk", "A999999999999999999", "A11", "A4 +5000"] {
        assert_eq!(parse_frequency(nonsense), None, "{nonsense}");
    }
    assert_eq!(parse_number("-3,5"), Some(-3.5));
    assert_eq!(parse_number("x"), None);
}

/// A drawn curve: 0 dB everywhere it is drawn, with `shape` added over the columns it names.
fn stroke(shape: impl Fn(usize) -> f64) -> Vec<Option<f64>> {
    (0..STROKE_COLUMNS).map(|c| Some(shape(c))).collect()
}

#[test]
fn a_drawn_hump_becomes_a_bell_at_its_peak() {
    let peak = 64;
    let columns = stroke(|c| 9.0 * (-((c as f64 - peak as f64) / 6.0).powi(2)).exp());
    let bands = stroke_bands(&columns);
    assert_eq!(bands.len(), 1, "{bands:?}");
    let (kind, frequency, gain, q) = bands[0];
    assert_eq!(kind, 3);
    assert!((frequency - column_frequency(peak)).abs() < 1.0);
    assert!((gain - 9.0).abs() < 0.01);
    assert!(q > 0.3 && q < 12.0, "{q}");
    // A narrower hump gives a higher Q.
    let narrow = stroke_bands(&stroke(|c| 9.0 * (-((c as f64 - peak as f64) / 2.0).powi(2)).exp()));
    assert!(narrow[0].3 > q, "{} against {q}", narrow[0].3);
}

#[test]
fn a_drawn_curve_makes_one_band_per_stretch_away_from_0_db() {
    let columns = stroke(|c| match c {
        30..=40 => 6.0,
        80..=90 => -8.0,
        _ => 0.0,
    });
    let bands = stroke_bands(&columns);
    assert_eq!(bands.len(), 2, "{bands:?}");
    assert!(bands[0].2 > 0.0 && bands[1].2 < 0.0, "a boost then a cut, low to high");
    assert!(bands[0].1 < bands[1].1);
}

#[test]
fn a_drawn_curve_from_an_edge_is_a_shelf_or_a_cut() {
    let shelf = stroke_bands(&stroke(|c| if c < 40 { 6.0 } else { 0.0 }));
    assert_eq!(shelf.iter().map(|b| b.0).collect::<Vec<_>>(), [2], "a low shelf");
    let cut = stroke_bands(&stroke(|c| if c > 110 { -24.0 } else { 0.0 }));
    assert_eq!(cut.iter().map(|b| b.0).collect::<Vec<_>>(), [7], "a high cut");
    assert!(stroke_bands(&vec![None; STROKE_COLUMNS]).is_empty(), "nothing drawn, nothing made");
    assert!(stroke_bands(&stroke(|_| 0.5)).is_empty(), "a flat curve makes nothing");
}

#[test]
fn the_analysers_range_and_tilt_move_its_line() {
    let display = display();
    assert_eq!(analyser_y_in(-120.0, display, 120.0), display.bottom(), "the floor at the bottom of a 120 dB range");
    assert!((analyser_y_in(-60.0, display, 120.0) - display.center().y).abs() < 1.0e-3, "-60 dB halfway down");
    assert_eq!(tilted(-30.0, 1000.0, 4.5), -30.0, "the tilt pivots at 1 kHz");
    assert!((tilted(-30.0, 2000.0, 4.5) + 25.5).abs() < 1.0e-9, "an octave up, 4.5 dB higher");
    assert!((tilted(-30.0, 500.0, 3.0) + 33.0).abs() < 1.0e-9, "an octave down, 3 dB lower");
}
