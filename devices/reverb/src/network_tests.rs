// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// Each family's name, in table order ([`families`]), for the assertions' messages.
const FAMILY_NAMES: [char; FAMILY_COUNT] = ['A', 'B', 'C', 'D', 'F', 'R', 'E', 'H', 'J', 'K'];

/// Low's line `k`: its delay and allpass ([`length`]).
fn low(k: usize, room: f64, percent: f64, settled: bool, fast: bool) -> (f64, f64) {
    let (line, allpass, _) = length(&LOW_LINES, k, k, false, room, percent, settled, fast, REFERENCE_RATE);
    (line, allpass)
}

/// A Sparse, Mid or High line's lengths ([`length`]).
fn shared(delay: usize, family: usize, second: bool, room: f64, percent: f64, fast: bool) -> (f64, f64, f64) {
    length(&SHARED_LINES, delay, family, second, room, percent, true, fast, REFERENCE_RATE)
}

#[test]
fn every_densitys_a_and_b_lines_are_its_families_a_and_b() {
    // They run a hundredth of a frame long under Smooth None; Sparse's C line has A's delay but
    // is whole (`reverb-network-density-size`).
    for (design, lines) in [(&LOW, &[0, 1][..]), (&SPARSE, &[1]), (&MID, &[0, 7]), (&HIGH, &[0, 8])] {
        let ab: Vec<usize> = (0..design.lines).filter(|&k| reads_between_frames(design.families[k])).collect();
        assert_eq!(ab, lines, "{} lines", design.lines);
    }
}

#[test]
fn the_entry_is_its_measured_length_at_every_shape() {
    // The reference's entries (frames) at Shape 0, 0.25, 0.5, 0.75 and 1, Smooth None then Fast.
    for (fast, frames) in [(false, [-1.0, 391.0, 775.0, 1159.0, 1543.0]), (true, [-1.0, 384.0, 770.0, 1156.0, 1542.0])]
    {
        for (shape, want) in [0.0, 0.25, 0.5, 0.75, 1.0].into_iter().zip(frames) {
            assert_eq!(entry(shape, true, fast, REFERENCE_RATE), want, "Shape {shape}, fast {fast}");
        }
    }
}

#[test]
fn the_entry_is_rounded_at_the_sessions_rate() {
    // The reference's entries at Shape 0.25, 0.5 and 1 against Shape 0's (samples), Smooth None then Fast
    // (`reverb-rate-48k`, `reverb-rate-96k`).
    for (rate, none, fast) in [
        (48_000.0, [424.0, 840.0, 1680.0], [420.0, 840.0, 1680.0]),
        (96_000.0, [840.0, 1680.0, 3360.0], [840.0, 1680.0, 3360.0]),
    ] {
        for (fast, wants) in [(false, none), (true, fast)] {
            for (shape, want) in [0.25, 0.5, 1.0].into_iter().zip(wants) {
                let later = entry(shape, true, fast, rate) - entry(0.0, true, fast, rate);
                assert_eq!(later, want, "{rate} Hz, Shape {shape}, fast {fast}");
            }
        }
    }
}

#[test]
fn a_lines_first_pass_is_rounded_at_the_sessions_rate() {
    // The reference's first passes through Low's A line at Scale 40 % and Shape 0.5 after the first
    // reflection (samples), at Size 1.53, 10.54 and 100 (`reverb-rate-48k`, `reverb-rate-96k`).
    // Rounded in 44.1 kHz frames and converted after, None at 96 kHz came out up to 26 samples
    // long.
    let reads = [
        (48_000.0, false, [1521.010, 2129.010, 3569.010]),
        (96_000.0, false, [3041.006, 4265.006, 7129.006]),
        (48_000.0, true, [1515.615, 2125.879, 3561.916]),
        (96_000.0, true, [3031.230, 4250.758, 7122.832]),
    ];
    for (rate, fast, wants) in reads {
        for (size, want) in [1.53, 10.54, 100.0].into_iter().zip(wants) {
            let room = (size / 100.0f64).cbrt();
            let (line, allpass, _) = length(&LOW_LINES, A, A, false, room, 40.0, true, fast, rate);
            let fraction = if fast { 0.0 } else { NONE_FRACTION };
            let pass = line + fraction + allpass + entry(0.5, true, fast, rate);
            assert!((pass - want).abs() < 0.01, "{rate} Hz, Size {size}, fast {fast}: {pass} against {want}");
        }
    }
}

#[test]
fn every_familys_line_is_its_measured_length_at_48_and_96_khz() {
    // Each family's line's whole first pass, delay and both allpasses (frames), at Size 100 and
    // Scale 40 %, read line by line from the reference's Mid and High at 48 kHz None and Fast, then 96 kHz
    // None and Fast (`reverb-rate-48k`, `reverb-rate-96k`). E's under Fast is not met: the reference's is 1
    // and 2 frames shorter.
    let reads: [[f64; 4]; FAMILY_COUNT] = [
        [15011.0, 14996.931, 30003.0, 29992.862],
        [5539.0, 5533.546, 11083.0, 11065.092],
        [6363.0, 6353.0, 12723.0, 12705.0],
        [10755.0, 10748.0, 21499.0, 21493.0],
        [5963.0, 5947.0, 11907.0, 11894.0],
        [8059.0, 8045.0, 16099.0, 16088.0],
        [7763.0, 7751.0, 15507.0, 15501.0],
        [18483.0, 18474.0, 36955.0, 36945.0],
        [5571.0, 5565.0, 11131.0, 11129.0],
        [6115.0, 6104.0, 12211.0, 12206.0],
    ];
    let conditions = [(48_000.0, false), (48_000.0, true), (96_000.0, false), (96_000.0, true)];
    for (family, wants) in reads.into_iter().enumerate() {
        for ((rate, fast), want) in conditions.into_iter().zip(wants) {
            if family == E && fast {
                continue;
            }
            let (line, first, second) = length(&SHARED_LINES, family, family, true, 1.0, 40.0, true, fast, rate);
            let pass = line + first + second;
            let name = FAMILY_NAMES[family];
            assert!((pass - want).abs() < 1e-3, "{name} at {rate} Hz, fast {fast}: {pass} against {want}");
        }
    }
    // F's filtered reflections under Fast, a frame later than its 44.1 kHz reads alone allowed.
    assert_eq!(front(F, 1.0, true, true, 48_000.0), 3246.0);
    assert_eq!(front(F, 1.0, true, true, 96_000.0), 6492.0);
}

#[test]
fn the_loop_mix_is_orthogonal_and_close_to_the_measured_one() {
    for design in [&SPARSE, &LOW, &MID, &HIGH] {
        let mix = orthonormal(design);
        let n = design.lines;
        for i in 0..n {
            for j in 0..n {
                let dot: f64 = (0..n).map(|k| mix[k][i] * mix[k][j]).sum();
                let want = if i == j { 1.0 } else { 0.0 };
                assert!((dot - want).abs() < 1e-12, "{n} lines, columns {i} and {j}: {dot}");
                let read = design.mix[i][j];
                assert!((mix[i][j] - read).abs() < 2e-3, "{n} lines, {i}, {j}: {} against {read}", mix[i][j]);
            }
        }
    }
}

#[test]
fn the_loop_keeps_its_energy_with_each_line_at_its_level() {
    for design in [&SPARSE, &LOW, &MID, &HIGH] {
        let (mix, levels, n) = (loop_mix(design), &design.levels, design.lines);
        // Taken at their levels, the lines mix orthogonally.
        let heard = |k: usize, i: usize| mix[k][i] * levels[i] / levels[k];
        for i in 0..n {
            for j in 0..n {
                let dot: f64 = (0..n).map(|k| heard(k, i) * heard(k, j)).sum();
                let want = if i == j { 1.0 } else { 0.0 };
                assert!((dot - want).abs() < 1e-12, "{n} lines, columns {i} and {j}: {dot}");
            }
        }
        // Within 1 % of each other, about one.
        let mean = (levels[..n].iter().map(|l| l.ln()).sum::<f64>() / n as f64).exp();
        assert!((mean - 1.0).abs() < 1e-4, "{n} lines: {mean}");
        assert!(levels[..n].iter().all(|l| (l - 1.0).abs() < 0.01), "{n} lines: {levels:?}");
    }
    assert_eq!(loop_mix(&LOW), orthonormal(&LOW));
}

#[test]
fn sparses_lines_are_their_measured_lengths() {
    // The reference's first passes at Size 100 less the entry (frames), as below.
    let measured: [[f64; 6]; 4] = [
        [2954.0, 3250.0, 3642.0, 4034.0, 4426.0, 4826.0],
        [3594.0, 3722.0, 3906.0, 4082.0, 4258.0, 4434.0],
        [4522.0, 4610.0, 4730.0, 4850.0, 4970.0, 5082.0],
        [9538.0, 9706.0, 9930.0, 10146.0, 10370.0, 10586.0],
    ];
    for (k, lengths) in measured.into_iter().enumerate() {
        for (percent, want) in [5.0, 20.0, 40.0, 60.0, 80.0, 100.0].into_iter().zip(lengths) {
            let (line, allpass, second) = shared(SPARSE_DELAYS[k], SPARSE.families[k], false, 1.0, percent, false);
            assert_eq!(line + allpass + second, want, "line {k} at Scale {percent} %");
        }
    }
}

#[test]
fn mids_and_highs_lines_are_their_measured_lengths() {
    // The reference's first passes at Size 100 less the entry (frames), Scale 5, 20, 40, 60, 80 and
    // 100 % (`reverb-network-density`).
    let measured: [[f64; 6]; 10] = [
        [3747.0, 4323.0, 5099.0, 5867.0, 6635.0, 7411.0],
        [3307.0, 4083.0, 5123.0, 6163.0, 7195.0, 8235.0],
        [3187.0, 4171.0, 5475.0, 6779.0, 8083.0, 9395.0],
        [5251.0, 6059.0, 7139.0, 8211.0, 9283.0, 10363.0],
        [8435.0, 9051.0, 9883.0, 10715.0, 11539.0, 12371.0],
        [4219.0, 4827.0, 5627.0, 6419.0, 7219.0, 8019.0],
        [3795.0, 4675.0, 5851.0, 7019.0, 8195.0, 9371.0],
        [4859.0, 5947.0, 7403.0, 8859.0, 10307.0, 11755.0],
        [10027.0, 11635.0, 13787.0, 15947.0, 18091.0, 20243.0],
        [16019.0, 16435.0, 16987.0, 17539.0, 18091.0, 18643.0],
    ];
    for (k, lengths) in measured.into_iter().enumerate() {
        let family = HIGH.families[k];
        for (percent, want) in [5.0, 20.0, 40.0, 60.0, 80.0, 100.0].into_iter().zip(lengths) {
            let (line, first, second) = shared(family, family, true, 1.0, percent, false);
            assert_eq!(line + first + second, want, "line {k} at Scale {percent} %");
        }
    }
    // Mid's are High's but the second and the last.
    let mid: Vec<_> = HIGH.families[..HIGH.lines].iter().filter(|f| ![J, H].contains(*f)).copied().collect();
    assert_eq!(mid, MID.families[..MID.lines]);
}

#[test]
fn d_and_gs_allpasses_split_as_measured_at_size_10_54() {
    // Each allpass (frames) where Mid's and High's tails match the reference's at Diffusion 30 and
    // 60 % (`reverb-network-density-split`). With the allpasses split as first read, D's
    // second came a block short at Scale 40 %, and at 100 % D's first and H's came a block long
    // and H's second a block short; at Diffusion 0.1 % only
    // their sums show, which hold at every Size and Scale read.
    let room = (10.54f64 / 100.0).cbrt();
    for (family, percent, want) in [(D, 40.0, (265.0, 529.0)), (D, 100.0, (657.0, 1305.0)), (H, 100.0, (289.0, 1025.0))]
    {
        let (_, first, second) = shared(family, family, true, room, percent, false);
        assert_eq!((first, second), want, "family {family} at Scale {percent} %");
    }
}

#[test]
fn every_densitys_lines_are_their_measured_lengths_at_the_ends_of_the_size_range() {
    // The reference's first passes less the entry (frames), Scale 40 %, at Size 0.222 and 500, with
    // Smooth None and Fast (`reverb-network-density-size`); under Fast the A and B lines' are
    // between frames.
    let small = (0.222f64 / 100.0).cbrt();
    let large = (500.0f64 / 100.0).cbrt();
    let high: [[f64; 4]; 10] = [
        [675.0, 8699.0, 664.92, 8693.68],
        [683.0, 8747.0, 668.0, 8742.0],
        [723.0, 9355.0, 714.0, 9343.0],
        [939.0, 12187.0, 930.0, 12178.0],
        [1291.0, 16891.0, 1289.0, 16883.0],
        [747.0, 9595.0, 733.0, 9588.0],
        [771.0, 9987.0, 763.0, 9981.0],
        [979.0, 12651.0, 965.0, 12639.0],
        [1811.0, 23571.0, 1799.16, 23559.6],
        [2219.0, 29035.0, 2215.0, 29020.0],
    ];
    // Sparse's last line at Size 0.222 under Fast overlaps others and is not read.
    let sparse: [[f64; 4]; 4] = [
        [482.0, 6226.0, 475.0, 6220.0],
        [514.0, 6666.0, 509.98, 6661.68],
        [626.0, 8082.0, 617.0, 8077.0],
        [1306.0, 16970.0, f64::NAN, 16960.0],
    ];
    let check = |name: &str, k: usize, delay: usize, family: usize, second: bool, want: [f64; 4]| {
        for ((room, fast), want) in [(small, false), (large, false), (small, true), (large, true)].into_iter().zip(want)
        {
            if want.is_nan() {
                continue;
            }
            let (line, first, second) = shared(delay, family, second, room, 40.0, fast);
            let got = line + first + second;
            assert!((got - want).abs() < 0.06, "{name} line {k}, room {room}, fast {fast}: {got} against {want}");
        }
    };
    for (k, want) in high.into_iter().enumerate() {
        check("High", k, HIGH.families[k], HIGH.families[k], true, want);
    }
    for (k, want) in sparse.into_iter().enumerate() {
        check("Sparse", k, SPARSE_DELAYS[k], SPARSE.families[k], false, want);
    }
}

#[test]
fn each_line_of_mid_and_high_has_its_own_family() {
    // As at Low, each family's copy joins one line, and none shares its offset.
    for design in [&MID, &HIGH] {
        let mut peaks: Vec<_> =
            design.families[..design.lines].iter().map(|&family| peak(family, 1.0, true, false) as i64).collect();
        peaks.sort_unstable();
        peaks.dedup();
        assert_eq!(peaks.len(), design.lines);
    }
    assert_eq!((MID.families[2], HIGH.families[9]), (E, H));
    assert_eq!(peak(E, 1.0, true, false), 961.0);
    assert_eq!(peak(H, 1.0, true, false), 2073.0);
}

#[test]
fn every_direct_reflection_of_mid_and_high_has_a_group() {
    for (taps, design) in [(&crate::TAPS_MID[..], &MID), (&crate::TAPS_HIGH[..], &HIGH)] {
        assert_eq!(taps.len(), DIRECT_TAPS);
        assert!(design.tap_groups.iter().all(|&g| g < design.lines));
    }
}

/// The reference's whole lengths, delay and allpass (frames, C's after its family's offset), read at
/// each (Size, Scale %) the reference's renders were read for.
const MEASURED: [(f64, f64, [f64; LOOP]); 10] = [
    (100.0, 5.0, [2330.0, 2778.0, 5074.0, 3842.0, 6866.0, 9802.0]),
    (100.0, 20.0, [2402.0, 2922.0, 5178.0, 4322.0, 7130.0, 9898.0]),
    (100.0, 40.0, [2506.0, 3114.0, 5314.0, 4962.0, 7482.0, 10034.0]),
    (100.0, 100.0, [2810.0, 3706.0, 5730.0, 6866.0, 8530.0, 10418.0]),
    (50.0, 40.0, [1994.0, 2474.0, 4218.0, 3938.0, 5938.0, 7962.0]),
    (10.54, 5.0, [1106.0, 1314.0, 2394.0, 1826.0, 3250.0, 4634.0]),
    (10.54, 100.0, [1330.0, 1754.0, 2714.0, 3250.0, 4034.0, 4922.0]),
    (1.53, 5.0, [586.0, 698.0, 1258.0, 962.0, 1714.0, 2434.0]),
    (1.53, 40.0, [634.0, 786.0, 1322.0, 1242.0, 1866.0, 2490.0]),
    (1.53, 100.0, [706.0, 930.0, 1426.0, 1714.0, 2130.0, 2586.0]),
];

#[test]
fn a_line_is_its_measured_length() {
    for (size, percent, lengths) in MEASURED {
        let room = (size / 100.0f64).cbrt();
        for (k, want) in lengths.into_iter().enumerate() {
            let (line, allpass) = low(k, room, percent, true, false);
            assert_eq!(line + allpass, want, "line {k} at Size {size}, Scale {percent} %");
        }
    }
}

#[test]
fn the_smallest_rooms_c_and_r_are_their_measured_lengths() {
    // Read from the reference's tail at Size 0.222 (`reverb-network-small`): C's family offset and
    // delay, and R's A delay and G, are each one rounded length, not two.
    let room = (0.222 / 100.0f64).cbrt();
    assert_eq!(low(2, room, 40.0, true, false).0, 8.0 * 82.0 + 1.0);
    assert_eq!(low(5, room, 40.0, true, false).0, 8.0 * 160.0 + 1.0);
}

#[test]
fn the_largest_rooms_r_is_its_measured_length() {
    // The reference's tail at Size 500 (`reverb-size decay-3.46s-size-500`) has R a block shorter than
    // the reads at Size 0.222 to 100 put it: 2087 blocks and a frame.
    let room = 5.0f64.cbrt();
    assert_eq!(low(5, room, 40.0, true, false).0, 8.0 * 2087.0 + 1.0);
}

#[test]
fn r_at_size_350_is_its_measured_length() {
    // Low's tail at Size 350 (`reverb-network-size-level`) has R 1854 blocks and a frame, a
    // block longer than the reads at Size 0.222 to 100 and 500 alone put it.
    let room = 3.5f64.cbrt();
    assert_eq!(low(5, room, 40.0, true, false).0, 8.0 * 1854.0 + 1.0);
}

#[test]
fn mids_and_highs_lines_are_their_measured_lengths_at_size_400() {
    // The reference's first passes less the entry (frames) at Size 400, Scale 40 %, Smooth None
    // (`reverb-network-size-level`), where the 3193, 2337, 961 and 2497 families' lines are a
    // block shorter than the reads at the other Sizes alone put them.
    let room = 4.0f64.cbrt();
    for (family, want) in [(1, 8075.0), (5, 11739.0), (6, 11315.0), (9, 8907.0)] {
        let (line, first, second) = shared(family, family, true, room, 40.0, false);
        assert_eq!(line + first + second, want, "family {family}");
    }
}

#[test]
fn mids_and_highs_lines_are_their_measured_lengths_at_size_300_and_400() {
    // Every family's delay and allpasses (frames) at Scale 40 %, Smooth None, where Mid's and
    // High's tails match the reference's (`reverb-network-size-level`, −37 to −40 dB to 3 s).
    type Line = (f64, f64, f64);
    let measured: [(f64, [Line; FAMILY_COUNT]); 2] = [
        (
            300.0,
            [
                (13673.0, 321.0, 5889.0),
                (5113.0, 513.0, 1721.0),
                (5033.0, 641.0, 2753.0),
                (11849.0, 809.0, 1593.0),
                (4113.0, 1137.0, 2641.0),
                (6473.0, 345.0, 3849.0),
                (7177.0, 1369.0, 1737.0),
                (22889.0, 353.0, 1249.0),
                (4385.0, 377.0, 2625.0),
                (5785.0, 553.0, 1761.0),
            ],
        ),
        (
            400.0,
            [
                (15049.0, 353.0, 6481.0),
                (5625.0, 561.0, 1889.0),
                (5545.0, 705.0, 3025.0),
                (13041.0, 889.0, 1753.0),
                (4529.0, 1257.0, 2905.0),
                (7121.0, 377.0, 4241.0),
                (7897.0, 1505.0, 1913.0),
                (25193.0, 393.0, 1369.0),
                (4825.0, 409.0, 2889.0),
                (6369.0, 601.0, 1937.0),
            ],
        ),
    ];
    for (size, lines) in measured {
        let room = (size / 100.0f64).cbrt();
        for (family, want) in lines.into_iter().enumerate() {
            assert_eq!(shared(family, family, true, room, 40.0, false), want, "family {family} at Size {size}");
        }
    }
}

#[test]
fn under_smooth_fast_a_line_is_its_measured_loop() {
    // The reference's loops under Smooth Fast (frames: each line's first pass and 5), read at each
    // (Size, Scale %) of `reverb-network-fast`.
    let measured = [
        (100.0, 40.0, [2501.795, 3113.620, 5309.0, 4954.0, 7474.0, 10023.0]),
        (10.54, 40.0, [1181.926, 1472.137, 2508.0, 2341.0, 3531.0, 4735.0]),
        (1.53, 5.0, [578.192, 689.411, 1258.0, 954.0, 1704.0, 2433.0]),
        (1.53, 40.0, [622.191, 774.411, 1319.0, 1231.0, 1856.0, 2489.0]),
        (1.53, 100.0, [698.192, 920.409, 1422.0, 1705.0, 2116.0, 2585.0]),
    ];
    // C's, D's, F's and R's at the ends of the range (`reverb-network-density-size`).
    for (size, loops) in [(0.222, [694.0, 647.0, 976.0, 1308.0]), (500.0, [9078.0, 8470.0, 12778.0, 17138.0])] {
        let room = (size / 100.0f64).cbrt();
        for (k, want) in (2..LOOP).zip(loops) {
            let (line, allpass) = low(k, room, 40.0, true, true);
            assert_eq!(line + allpass, want, "line {k} at Size {size}");
        }
    }
    for (size, percent, loops) in measured {
        let room = (size / 100.0f64).cbrt();
        for (k, want) in loops.into_iter().enumerate() {
            let (line, allpass) = low(k, room, percent, true, true);
            let most = if k < 2 { 0.01 } else { 0.0 };
            assert!(
                (line + allpass - want).abs() <= most,
                "line {k} at Size {size}, Scale {percent} %: {}",
                line + allpass
            );
        }
    }
}

#[test]
fn under_smooth_fast_an_allpass_is_its_measured_length() {
    // Read at Diffusion 48 and 60 %, where a frame between a line's delay and its allpass
    // shows (`reverb-impulse` reverb-decay, reverb-decay-none, reverb-texture): A to R.
    let measured = [
        (100.0, 40.0, [204.0, 392.0, 279.0, 1274.0, 700.0, 258.0]),
        (10.54, 40.0, [96.0, 186.0, 132.0, 602.0, 331.0, 122.0]),
        (1.53, 5.0, [7.0, 13.0, 9.0, 40.0, 22.0, 8.0]),
        (1.53, 40.0, [51.0, 98.0, 70.0, 317.0, 174.0, 64.0]),
        (1.53, 100.0, [127.0, 244.0, 173.0, 791.0, 434.0, 160.0]),
        (500.0, 40.0, [348.0, 670.0, 477.0, 2178.0, 1196.0, 441.0]),
    ];
    for (size, percent, allpasses) in measured {
        let room = (size / 100.0f64).cbrt();
        for (k, want) in allpasses.into_iter().enumerate() {
            assert_eq!(low(k, room, percent, true, true).1, want, "line {k} at Size {size}, Scale {percent} %");
        }
    }
}

#[test]
fn under_smooth_fast_each_copy_starts_where_measured() {
    let measured = [
        (100.0, [396.0, 3191.0, 1640.0, 1336.0, 2983.0, 2337.0]),
        (10.54, [187.0, 1508.0, 775.0, 631.0, 1409.0, 1104.0]),
        (1.53, [99.0, 793.0, 408.0, 332.0, 741.0, 581.0]),
    ];
    // A's and D's filtered reflections at Size 500 (`reverb-structure size-500-er`).
    let room = 5.0f64.cbrt();
    assert_eq!(peak(A, room, true, true), 676.0);
    assert_eq!(peak(D, room, true, true), 2285.0);
    for (size, peaks) in measured {
        let room = (size / 100.0f64).cbrt();
        for (family, want) in peaks.into_iter().enumerate() {
            assert_eq!(peak(family, room, true, true), want, "{} at Size {size}", FAMILY_NAMES[family]);
        }
    }
}

#[test]
fn each_copy_peaks_where_measured() {
    let measured = [
        (100.0, [401.0, 3193.0, 1641.0, 1337.0, 2985.0, 2337.0]),
        (10.54, [193.0, 1513.0, 777.0, 633.0, 1409.0, 1105.0]),
        (0.222, [57.0, 417.0, 217.0, 177.0, 393.0, 305.0]),
        // `reverb-size decay-3.46s-size-500`: A and B read, the rest where a block either way
        // fits the reference's tail worse.
        (500.0, [681.0, 5457.0, 2809.0, 2289.0, 5105.0, 4001.0]),
        // Low's reflections alone at Size 250 to 400 (`reverb-network-size-level`), where the
        // A family is a block shorter at 250 and 350 and the B family a block longer at
        // 250 and 300 than the reads up to 100 and at 500 alone put them.
        (250.0, [537.0, 4337.0, 2233.0, 1817.0, 4049.0, 3177.0]),
        (300.0, [577.0, 4609.0, 2369.0, 1929.0, 4305.0, 3377.0]),
        (350.0, [601.0, 4849.0, 2497.0, 2033.0, 4529.0, 3553.0]),
        (400.0, [633.0, 5065.0, 2609.0, 2121.0, 4737.0, 3713.0]),
    ];
    for (size, peaks) in measured {
        let room = (size / 100.0f64).cbrt();
        for (family, want) in peaks.into_iter().enumerate() {
            assert_eq!(peak(family, room, true, false), want, "{} at Size {size}", FAMILY_NAMES[family]);
        }
    }
}

#[test]
fn mids_and_highs_own_families_peak_where_measured() {
    // Mid's and High's reflections alone at Size 0.222 and 500 (`reverb-network-density-size`):
    // families E, H, J and K, None then Fast.
    let measured = [
        (0.222, [129.0, 273.0, 313.0, 329.0], [126.0, 271.0, 313.0, 326.0]),
        (500.0, [1649.0, 3545.0, 4097.0, 4265.0], [1643.0, 3543.0, 4091.0, 4265.0]),
    ];
    // At Size 300 and 400 under None, where Mid's and High's tails match the reference's
    // (`reverb-network-size-level`).
    for (size, none) in [(300.0, [1393.0, 2993.0, 3457.0, 3601.0]), (400.0, [1529.0, 3297.0, 3801.0, 3961.0])] {
        let room = (size / 100.0f64).cbrt();
        for (family, want) in [E, H, J, K].into_iter().zip(none) {
            assert_eq!(peak(family, room, true, false), want, "{} at Size {size}", FAMILY_NAMES[family]);
        }
    }
    for (size, none, fast) in measured {
        let room = (size / 100.0f64).cbrt();
        for (family, (none, fast)) in [E, H, J, K].into_iter().zip(none.into_iter().zip(fast)) {
            let name = FAMILY_NAMES[family];
            assert_eq!(peak(family, room, true, false), none, "{name} at Size {size}");
            assert_eq!(peak(family, room, true, true), fast, "{name} at Size {size}, Fast");
        }
    }
}

#[test]
fn a_glide_runs_through_the_middle_of_the_steps() {
    // Moving, a delay or an allpass is within half a block of where it settles.
    for size in [0.5, 3.0, 27.0, 100.0, 333.0] {
        let room = (size / 100.0f64).cbrt();
        for k in 0..LOOP {
            let (a, b) = (low(k, room, 40.0, true, false), low(k, room, 40.0, false, false));
            let most = if k == 2 || k == 5 { BLOCK } else { BLOCK / 2.0 };
            assert!((a.0 - b.0).abs() <= most, "line {k} at Size {size}");
            assert!((a.1 - b.1).abs() <= BLOCK / 2.0, "allpass {k} at Size {size}");
        }
    }
}

#[test]
fn every_direct_reflection_of_low_has_a_group() {
    let direct = crate::TAPS_LOW.len();
    assert_eq!(direct, TAP_GROUPS.len());
}
