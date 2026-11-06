// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn noise(state: &mut u32) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    *state as f32 / u32::MAX as f32 - 0.5
}

/// `per` rows around each of three centres far apart.
fn three(per: usize) -> Vec<Features> {
    let mut state = 5u32;
    let mut rows = Vec::new();
    for c in 0..3 {
        for _ in 0..per {
            let mut row = [0.0f32; FEATURE_COUNT];
            for (i, v) in row.iter_mut().enumerate() {
                *v = if i % 3 == c { 3.0 } else { 0.0 } + noise(&mut state) * 0.5;
            }
            rows.push(row);
        }
    }
    rows
}

const KINDS: [Kind; 3] = [Kind::Kick, Kind::Tom, Kind::Percussion];

#[test]
fn names_say_where_they_come_from() {
    let root = Path::new("/lib");
    assert_eq!(named(Path::new("/lib/Kicks/01.wav"), root), Some((Kind::Kick, Evidence::Folder)));
    assert_eq!(named(Path::new("/lib/Kicks/Snare 1.wav"), root), Some((Kind::Snare, Evidence::FileName)));
    assert_eq!(named(Path::new("/lib/Misc/01.wav"), root), None);
}

#[test]
fn unnamed_items_take_the_kind_they_sound_like() {
    let rows = three(20);
    let evidence: Vec<_> = (0..60).map(|i| (i % 20 != 0).then_some((KINDS[i / 20], Evidence::FileName))).collect();
    let kinds = classify(&rows, &evidence);
    for c in 0..3 {
        assert_eq!(kinds[c * 20], (KINDS[c], From::Sound));
        assert_eq!(kinds[c * 20 + 1], (KINDS[c], From::Name));
    }
}

#[test]
fn a_folder_gives_way_to_a_clear_sound_a_file_name_and_the_hand_do_not() {
    let rows = three(20);
    let mut evidence: Vec<_> = (0..60).map(|i| Some((KINDS[i / 20], Evidence::FileName))).collect();
    // Percussion sounds in a folder of kicks, named as kicks, set to kick by hand.
    evidence[40] = Some((Kind::Kick, Evidence::Folder));
    evidence[41] = Some((Kind::Kick, Evidence::FileName));
    evidence[42] = Some((Kind::Kick, Evidence::Hand));
    let kinds = classify(&rows, &evidence);
    assert_eq!(kinds[40], (Kind::Percussion, From::Sound));
    assert_eq!(kinds[41], (Kind::Kick, From::Name));
    assert_eq!(kinds[42], (Kind::Kick, From::Hand));
}

#[test]
fn a_sound_like_nothing_known_is_not_guessed() {
    let mut rows = three(20);
    let mut evidence: Vec<_> = (0..60).map(|i| Some((KINDS[i / 20], Evidence::FileName))).collect();
    // Far from all three, and halfway between two.
    rows.push([20.0; FEATURE_COUNT]);
    let mut between = [0.0f32; FEATURE_COUNT];
    for (i, v) in between.iter_mut().enumerate() {
        *v = if i % 3 < 2 { 1.5 } else { 0.0 };
    }
    rows.push(between);
    evidence.extend([None, None]);
    let kinds = classify(&rows, &evidence);
    assert_eq!(kinds[60], (Kind::Other, From::Nothing));
    assert_eq!(kinds[61], (Kind::Other, From::Nothing));
    // With nothing named, nothing is learned or guessed.
    let none = classify(&rows, &vec![None; rows.len()]);
    assert!(none.iter().all(|k| *k == (Kind::Other, From::Nothing)));
    assert!(Classifier::learn(&rows, &vec![None; rows.len()]).kinds().is_empty());
}

#[test]
fn too_few_file_names_let_the_folders_teach() {
    let rows = three(20);
    let evidence: Vec<_> = (0..60)
        .map(|i| match i / 20 {
            0 => Some((Kind::Kick, Evidence::Folder)),
            1 if i % 2 == 0 => Some((Kind::Tom, Evidence::FileName)),
            _ => None,
        })
        .collect();
    assert_eq!(Classifier::learn(&rows, &evidence).kinds(), vec![Kind::Kick, Kind::Tom]);
}

#[test]
fn related_kinds_do_not_make_a_guess_unsure() {
    let rows = three(20);
    // The first two groups are closed and open hats, so close relatives.
    let hats = [Kind::ClosedHat, Kind::OpenHat, Kind::Kick];
    let evidence: Vec<_> = (0..60).map(|i| (i % 20 != 0).then_some((hats[i / 20], Evidence::FileName))).collect();
    let kinds = classify(&rows, &evidence);
    assert_eq!(kinds[0], (Kind::ClosedHat, From::Sound));
    // A folder of hats keeps its open hat.
    let mut evidence: Vec<_> = (0..60).map(|i| Some((hats[i / 20], Evidence::FileName))).collect();
    evidence[0] = Some((Kind::Hat, Evidence::Folder));
    assert_eq!(classify(&rows, &evidence)[0], (Kind::Hat, From::Name));
}

#[test]
fn many_items_are_classified_on_several_threads_with_the_same_result() {
    let rows = three(3_000);
    let evidence: Vec<_> =
        (0..rows.len()).map(|i| (i % 7 != 0).then_some((KINDS[i / 3_000], Evidence::FileName))).collect();
    let parallel = classify(&rows, &evidence);
    let classifier = Classifier::learn(&rows, &evidence);
    let serial: Vec<_> = rows.iter().zip(&evidence).map(|(r, e)| classifier.kind_of(r, *e)).collect();
    assert_eq!(parallel, serial);
    assert!(parallel.iter().enumerate().all(|(i, (k, _))| *k == KINDS[i / 3_000]));
}

#[test]
fn the_layout_input_keeps_kinds_apart() {
    let rows = three(10);
    // Every item of the first group but one called a tom: they sit with the toms now.
    let mut kinds: Vec<Kind> = (0..30).map(|i| KINDS[i / 10]).collect();
    kinds[0] = Kind::Tom;
    kinds[29] = Kind::Other;
    let pulled = layout_rows(&rows, &kinds);
    assert_eq!(pulled.len(), 30);
    assert_eq!(pulled[29], rows[29]);
    let nearest_other = |i: usize| {
        (0..29)
            .filter(|&j| j != i)
            .min_by(|&a, &b| distance(&pulled[a], &pulled[i]).total_cmp(&distance(&pulled[b], &pulled[i])))
            .unwrap()
    };
    assert_eq!(kinds[nearest_other(0)], Kind::Tom);
    for i in 1..29 {
        assert_eq!(kinds[nearest_other(i)], kinds[i], "item {i}");
    }
    let d = directions();
    for (i, a) in d.iter().enumerate() {
        for (j, b) in d.iter().enumerate() {
            let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
            assert!((dot - if i == j { 1.0 } else { 0.0 }).abs() < 1e-4);
        }
    }
}

/// Run with `cargo test -p suhina-soundmap --release --lib found_quickly -- --ignored`.
#[test]
#[ignore = "a bound on elapsed time, for an idle machine"]
fn the_kinds_of_150_000_items_are_found_quickly() {
    let n = 150_000;
    let mut state = 11u32;
    let rows: Vec<Features> = (0..n)
        .map(|i| {
            let mut row = [0.0f32; FEATURE_COUNT];
            for (k, v) in row.iter_mut().enumerate() {
                *v = ((i % 12) * (k % 5)) as f32 * 0.1 + noise(&mut state);
            }
            row
        })
        .collect();
    let evidence: Vec<_> = (0..n)
        .map(|i| match i % 4 {
            0 => Some((Kind::ALL[i % 12], Evidence::FileName)),
            1 => Some((Kind::ALL[(i + 3) % 12], Evidence::Folder)),
            _ => None,
        })
        .collect();
    let started = std::time::Instant::now();
    let kinds = classify(&rows, &evidence);
    assert_eq!(kinds.len(), n);
    // Well under a second in a release build (ADR 0013 › Kinds); a debug build is many
    // times slower.
    assert!(started.elapsed() < std::time::Duration::from_secs(30), "{:?}", started.elapsed());
}
