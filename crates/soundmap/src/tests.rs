// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// A folder of generated samples, removed when dropped.
struct Library {
    root: PathBuf,
}

impl Library {
    fn new(name: &str) -> Self {
        let root = unique_dir(name);
        std::fs::create_dir_all(&root).unwrap();
        Library { root }
    }

    fn write(&self, relative: &str, samples: &[f32]) -> SourceFile {
        let path = self.root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for s in samples {
            writer.write_sample(*s).unwrap();
        }
        writer.finalize().unwrap();
        self.file(relative)
    }

    fn file(&self, relative: &str) -> SourceFile {
        let path = self.root.join(relative);
        let meta = std::fs::metadata(&path).unwrap();
        let modified = meta.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        SourceFile { folder: self.root.clone(), path, size: meta.len(), modified }
    }
}

/// A new folder name for every call, so runs whose process ids repeat never meet a folder
/// another run is still deleting.
pub(crate) fn unique_dir(name: &str) -> PathBuf {
    static COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    std::env::temp_dir().join(format!("suhina-soundmap-{name}-{}-{nanos}-{n}", std::process::id()))
}

impl Drop for Library {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn noise(seed: u32, n: usize) -> Vec<f32> {
    let mut state = seed.max(1);
    (0..n)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as f32 / u32::MAX as f32 - 0.5
        })
        .collect()
}

/// A kick: a falling low sine with a fast decay.
fn kick(variant: u32) -> Vec<f32> {
    let rate = 44_100.0;
    let mut phase = 0.0f32;
    (0..22_050)
        .map(|i| {
            let t = i as f32 / rate;
            let hz = 50.0 + variant as f32 * 2.0 + 120.0 * (-t * 30.0).exp();
            phase += std::f32::consts::TAU * hz / rate;
            phase.sin() * (-t * 8.0).exp() * 0.8
        })
        .collect()
}

/// A hat: short high noise.
fn hat(variant: u32) -> Vec<f32> {
    let raw = noise(variant + 11, 8_000);
    let mut out: Vec<f32> = raw.windows(2).map(|w| (w[1] - w[0]) * (1.0 - 0.0)).collect();
    for (i, s) in out.iter_mut().enumerate() {
        *s *= (-(i as f32) / 900.0).exp();
    }
    out
}

fn analysed(library: &Library, files: &[SourceFile]) -> SoundMap {
    let mut map = SoundMap::new(Some(library.root.join(".cache")), 2);
    map.update(files);
    map.wait();
    map
}

#[test]
fn a_library_is_analysed_laid_out_and_kinds_are_found() {
    let library = Library::new("layout");
    let mut files = Vec::new();
    for v in 0..12 {
        files.push(library.write(&format!("Kicks/kick {v}.wav"), &kick(v)));
        files.push(library.write(&format!("Hats/hat {v}.wav"), &hat(v)));
    }
    // Unnamed sounds in a folder with no kind.
    files.push(library.write("Misc/thing a.wav", &kick(20)));
    files.push(library.write("Misc/thing b.wav", &hat(20)));
    let mut map = analysed(&library, &files);
    let status = map.status(&files);
    assert_eq!(status, vec![FolderStatus { folder: library.root.clone(), analysed: 26, queued: 0, failed: 0 }]);

    let laid = map.map(&files, DEFAULT_MAX_SECONDS);
    assert_eq!(laid.points.len(), 26);
    assert_eq!(laid.pending, 0);
    let kind = |name: &str| laid.points.iter().find(|p| p.path.ends_with(name)).map(|p| (p.kind, p.guessed)).unwrap();
    assert_eq!(kind("kick 3.wav"), (Kind::Kick, false));
    assert_eq!(kind("hat 3.wav"), (Kind::Hat, false));
    assert_eq!(kind("thing a.wav"), (Kind::Kick, true));
    assert_eq!(kind("thing b.wav"), (Kind::Hat, true));

    // Similar items of a kick are kicks.
    let similar = laid.similar(&library.root.join("Kicks/kick 0.wav"), 8);
    assert_eq!(similar.len(), 8);
    assert!(
        similar.iter().all(|(p, _)| p.to_string_lossy().contains("kick") || p.ends_with("thing a.wav")),
        "{similar:?}"
    );

    // The same scope gives the same map, also in a new sound map that reads the kept layout.
    assert_eq!(map.map(&files, DEFAULT_MAX_SECONDS).points, laid.points);
    drop(map);
    let mut reopened = SoundMap::new(Some(library.root.join(".cache")), 1);
    reopened.update(&files);
    assert!(reopened.queued.is_empty());
    assert_eq!(reopened.map(&files, DEFAULT_MAX_SECONDS).points, laid.points);
    let grid = laid.grid(1.0);
    assert!(layout_from_bytes(&layout_to_bytes(&[[0.0, 1.0]])).is_some());
    assert!(layout_from_bytes(&[1, 2, 3]).is_none());
    assert!(layout_from_bytes(&layout_to_bytes(&[[2.0, 1.0]])).is_none());
    assert_eq!(grid.cells.len(), 26);
}

#[test]
fn a_layout_counts_its_progress_from_0_to_1() {
    let rows: Vec<features::Features> = (0..64)
        .map(|i| {
            let mut row = [0.0; features::FEATURE_COUNT];
            for (slot, v) in row.iter_mut().zip(noise(i + 1, features::FEATURE_COUNT)) {
                *slot = v + (i % 4) as f32;
            }
            row
        })
        .collect();
    let s = Standardised::new(&rows);
    let progress = layout::Progress::new();
    assert_eq!(progress.fraction(), 0.0);
    let positions = layout::layout_with_progress(&s, 1, &progress);
    assert_eq!(positions.len(), 64);
    assert_eq!(progress.fraction(), 1.0);
    // A clone shares the count; a layout too small for t-SNE ends at 1 as well.
    let shared = layout::Progress::new();
    let few = Standardised::new(&rows[..3]);
    layout::layout_with_progress(&few, 1, &shared.clone());
    assert_eq!(shared.fraction(), 1.0);
}

#[test]
fn the_cache_is_kept_and_follows_changes() {
    let library = Library::new("cache");
    let a = library.write("kick a.wav", &kick(1));
    let b = library.write("kick b.wav", &kick(2));
    let broken = library.root.join("broken.wav");
    std::fs::write(&broken, b"not audio").unwrap();
    let broken = library.file("broken.wav");
    let files = vec![a.clone(), b.clone(), broken.clone()];
    {
        let mut map = analysed(&library, &files);
        assert!(matches!(map.analysis(&broken), Some(Analysis::Failed(_))));
        assert!(matches!(map.analysis(&a), Some(Analysis::Analysed { .. })), "{:?}", map.analysis(&a));
        assert!(matches!(map.analysis(&b), Some(Analysis::Analysed { .. })), "{:?}", map.analysis(&b));
        let laid = map.map(&files, DEFAULT_MAX_SECONDS);
        assert_eq!((laid.points.len(), laid.failed), (2, 1));
    }
    // A new sound map reads the cache and queues nothing.
    let mut map = SoundMap::new(Some(library.root.join(".cache")), 1);
    map.update(&files);
    assert!(map.queued.is_empty());
    assert!(matches!(map.analysis(&a), Some(Analysis::Analysed { .. })));

    // A changed file is analysed again; a removed one is forgotten.
    let mut changed = b.clone();
    changed.modified += 1;
    map.update(&[a.clone(), changed.clone()]);
    assert!(map.queued.contains(&changed.path));
    map.wait();
    assert!(map.analysis(&changed).is_some());
    assert!(map.caches.values().all(|c| !c.entries.contains_key(Path::new("broken.wav"))));

    // Reanalysing clears the folder's cache and queues its files.
    map.reanalyse(&library.root, std::slice::from_ref(&a));
    assert!(map.queued.contains(&a.path));
    map.wait();
    assert!(map.analysis(&a).is_some());
}

#[test]
fn files_that_cannot_be_read_are_grouped_by_reason_with_examples() {
    let library = Library::new("failures");
    let good = library.write("kick.wav", &kick(1));
    let mut files = vec![good];
    for i in 0..5 {
        let name = format!("broken {i}.wav");
        std::fs::write(library.root.join(&name), b"not audio").unwrap();
        files.push(library.file(&name));
    }
    files.push(library.write("silence.wav", &vec![0.0; 4410]));
    let mut map = analysed(&library, &files);
    let laid = map.map(&files, DEFAULT_MAX_SECONDS);
    assert_eq!(laid.failed, 6);
    let reasons: Vec<(&str, usize)> = laid.failures.iter().map(|f| (f.reason.as_str(), f.count)).collect();
    assert_eq!(reasons.len(), 2, "{:?}", laid.failures);
    // The commonest first; the reason is cut at its first colon, the detail kept whole.
    assert_eq!(reasons[0].1, 5);
    assert!(!reasons[0].0.contains(':'));
    assert_eq!(reasons[1], ("silent", 1));
    assert_eq!(laid.failures[0].examples.len(), 3);
    assert!(laid.failures[0].examples.iter().all(|p| p.to_string_lossy().contains("broken")));
}

#[test]
fn long_files_are_left_out_and_counted() {
    let library = Library::new("long");
    let short = library.write("kick.wav", &kick(1));
    let mut long_audio = kick(2);
    long_audio.resize(44_100 * 3, 0.0);
    let long = library.write("long kick.wav", &long_audio);
    let files = vec![short, long];
    let mut map = analysed(&library, &files);
    let laid = map.map(&files, 1.0);
    assert_eq!((laid.points.len(), laid.too_long), (1, 1));
    let laid = map.map(&files, 5.0);
    assert_eq!((laid.points.len(), laid.too_long), (2, 0));
}

#[test]
fn unanalysed_files_are_pending_and_an_empty_scope_is_empty() {
    let library = Library::new("pending");
    let file = library.write("kick.wav", &kick(1));
    let mut map = SoundMap::new(None, 1);
    let laid = map.map(&[file], DEFAULT_MAX_SECONDS);
    assert_eq!((laid.points.len(), laid.pending), (0, 1));
    assert!(map.map(&[], DEFAULT_MAX_SECONDS).points.is_empty());
    assert!(laid.similar(Path::new("nothing"), 5).is_empty());
}

#[test]
fn an_update_worked_out_on_a_thread_is_taken_in_once_and_not_before_it_is_ready() {
    let diff = || Diff { loaded: Vec::new(), work: Vec::new(), gone: Vec::new(), folders: HashSet::new() };
    let mut map = SoundMap::new(None, 1);
    let (sender, receiver) = std::sync::mpsc::channel();
    map.diffing = Some(receiver);
    // Not sent yet: it is waited for.
    assert!(!map.take_diff(false));
    assert!(map.diffing.is_some());
    // Sent, and the thread gone: the one poll takes it in, whole.
    sender.send(diff()).unwrap();
    drop(sender);
    assert!(map.take_diff(false));
    assert!(map.diffing.is_none());
    // A thread that ended without an update is let go of.
    let (sender, receiver) = std::sync::mpsc::channel::<Diff>();
    map.diffing = Some(receiver);
    drop(sender);
    assert!(!map.take_diff(false));
    assert!(map.diffing.is_none());
}

/// How long large scopes take to lay out and snap (run with `--ignored --nocapture` in release).
#[test]
#[ignore]
fn timing_of_large_scopes() {
    for n in [1_000, 10_000, 50_000] {
        let rows: Vec<features::Features> = (0..n)
            .map(|i| {
                let values = noise(i as u32 + 1, features::FEATURE_COUNT);
                let mut row = [0.0; features::FEATURE_COUNT];
                for (k, (slot, v)) in row.iter_mut().zip(values).enumerate() {
                    *slot = v + ((i % 12) * (k % 5)) as f32;
                }
                row
            })
            .collect();
        let started = std::time::Instant::now();
        let s = Standardised::new(&rows);
        let positions = layout::layout(&s, 1);
        let laid = started.elapsed();
        let started = std::time::Instant::now();
        let grid = grid::snap(&positions, 1.0);
        let snapped = started.elapsed();
        let started = std::time::Instant::now();
        let similar = s.similar(0, 16);
        println!("{n} items: layout {laid:?}, grid {snapped:?}, similar {:?} ({})", started.elapsed(), similar.len());
        assert_eq!(grid.cells.len(), n);
    }
}

/// Generated drum and bass sounds, at 44.1 kHz, for the tests of kinds (issue #325): every one a
/// little different by `v`.
mod sounds {
    use super::noise;

    const RATE: f32 = 44_100.0;

    fn samples(seconds: f32) -> usize {
        (seconds * RATE) as usize
    }

    /// A two-pole band-pass (constant peak gain) around `hz`.
    fn band(input: &[f32], hz: f32, q: f32) -> Vec<f32> {
        let w = std::f32::consts::TAU * hz / RATE;
        let alpha = w.sin() / (2.0 * q);
        let (b0, b2) = (alpha, -alpha);
        let (a0, a1, a2) = (1.0 + alpha, -2.0 * w.cos(), 1.0 - alpha);
        let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
        input
            .iter()
            .map(|&x| {
                let y = (b0 * x + b2 * x2 - a1 * y1 - a2 * y2) / a0;
                (x2, x1, y2, y1) = (x1, x, y1, y);
                y
            })
            .collect()
    }

    /// A one-pole high-pass at `hz`.
    fn high(input: &[f32], hz: f32) -> Vec<f32> {
        let r = (-std::f32::consts::TAU * hz / RATE).exp();
        let (mut x1, mut y1) = (0.0, 0.0);
        input
            .iter()
            .map(|&x| {
                let y = r * (y1 + x - x1);
                (x1, y1) = (x, y);
                y
            })
            .collect()
    }

    fn decay(input: &mut [f32], seconds: f32) {
        for (i, s) in input.iter_mut().enumerate() {
            *s *= (-(i as f32) / RATE / seconds).exp();
        }
    }

    fn normalise(mut s: Vec<f32>, peak: f32) -> Vec<f32> {
        let m = s.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-9);
        s.iter_mut().for_each(|v| *v *= peak / m);
        s
    }

    /// A falling low sine with a click.
    pub fn kick(v: u32) -> Vec<f32> {
        let mut phase = 0.0f32;
        let base = 45.0 + (v % 5) as f32 * 4.0;
        let sweep = 110.0 + (v % 3) as f32 * 30.0;
        let n = samples(0.45 + (v % 4) as f32 * 0.05);
        let click = noise(v + 100, 200);
        let out = (0..n)
            .map(|i| {
                let t = i as f32 / RATE;
                phase += std::f32::consts::TAU * (base + sweep * (-t * 35.0).exp()) / RATE;
                phase.sin() * (-t * 7.0).exp() + click.get(i).copied().unwrap_or(0.0) * 0.3
            })
            .collect();
        normalise(out, 0.9)
    }

    /// A short tone near 200 Hz under bright noise.
    pub fn snare(v: u32) -> Vec<f32> {
        let n = samples(0.3);
        let hz = 180.0 + (v % 4) as f32 * 15.0;
        let mut rattle = band(&noise(v + 200, n), 4_000.0 + (v % 3) as f32 * 800.0, 0.5);
        decay(&mut rattle, 0.07 + (v % 3) as f32 * 0.01);
        let out = (0..n)
            .map(|i| {
                let t = i as f32 / RATE;
                (std::f32::consts::TAU * hz * t).sin() * (-t * 30.0).exp() * 0.5 + rattle[i] * 2.0
            })
            .collect();
        normalise(out, 0.9)
    }

    /// Three or four noise bursts a few milliseconds apart, around 1.5 kHz, and a short tail.
    pub fn clap(v: u32) -> Vec<f32> {
        let n = samples(0.35);
        let raw = band(&noise(v + 300, n), 1_300.0 + (v % 4) as f32 * 150.0, 1.5);
        let gap = 0.009 + (v % 3) as f32 * 0.002;
        let bursts = 3 + v % 2;
        let out = raw
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let t = i as f32 / RATE;
                let last = bursts as f32 * gap;
                let level = if t < last { (-(t % gap) * 300.0).exp() } else { (-(t - last) * 18.0).exp() };
                s * level
            })
            .collect();
        normalise(out, 0.9)
    }

    /// Very short, very high noise.
    pub fn closed_hat(v: u32) -> Vec<f32> {
        let n = samples(0.12);
        let mut out = high(&high(&noise(v + 400, n), 7_000.0), 7_000.0);
        decay(&mut out, 0.012 + (v % 4) as f32 * 0.003);
        normalise(out, 0.9)
    }

    /// Jingles: high ringing partials, struck a few times, over high noise, ringing longer than a
    /// hat.
    pub fn tambourine(v: u32) -> Vec<f32> {
        let n = samples(0.5);
        let partials = [5_200.0, 6_900.0, 8_300.0, 9_700.0].map(|hz: f32| hz * (1.0 + (v % 5) as f32 * 0.01));
        let mut shimmer = high(&noise(v + 500, n), 5_000.0);
        decay(&mut shimmer, 0.08);
        let out = (0..n)
            .map(|i| {
                let t = i as f32 / RATE;
                let strikes: f32 =
                    [0.0, 0.025, 0.05].iter().map(|s| if t >= *s { (-(t - s) * 9.0).exp() } else { 0.0 }).sum();
                let ring: f32 = partials.iter().map(|hz| (std::f32::consts::TAU * hz * t).sin()).sum();
                ring * strikes * 0.2 + shimmer[i]
            })
            .collect();
        normalise(out, 0.8)
    }

    /// Band noise around 6 kHz that swells in and out, without a sharp attack.
    pub fn shaker(v: u32) -> Vec<f32> {
        let n = samples(0.22 + (v % 3) as f32 * 0.03);
        let raw = band(&noise(v + 600, n), 5_500.0 + (v % 4) as f32 * 400.0, 0.8);
        let length = n as f32 / RATE;
        let out = raw
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let t = i as f32 / RATE;
                s * (std::f32::consts::PI * t / length).sin().powi(2)
            })
            .collect();
        normalise(out, 0.7)
    }

    /// A sine between 90 and 200 Hz that falls a little, ringing longer than a snare.
    pub fn tom(v: u32) -> Vec<f32> {
        let n = samples(0.7);
        let hz = 90.0 + (v % 6) as f32 * 20.0;
        let mut phase = 0.0f32;
        let mut skin = band(&noise(v + 700, n), 900.0, 1.0);
        decay(&mut skin, 0.01);
        let out = (0..n)
            .map(|i| {
                let t = i as f32 / RATE;
                phase += std::f32::consts::TAU * hz * (1.0 + 0.25 * (-t * 12.0).exp()) / RATE;
                (phase.sin() + 0.3 * (2.0 * phase).sin()) * (-t * 5.0).exp() + skin[i] * 0.5
            })
            .collect();
        normalise(out, 0.9)
    }

    /// A held low tone, rich in harmonics, with no fall in pitch.
    pub fn bass(v: u32) -> Vec<f32> {
        let n = samples(1.0 + (v % 3) as f32 * 0.25);
        let hz = 41.0 + (v % 7) as f32 * 6.0;
        let out = (0..n)
            .map(|i| {
                let t = i as f32 / RATE;
                let tone: f32 = (1..=6).map(|h| (std::f32::consts::TAU * hz * h as f32 * t).sin() / h as f32).sum();
                let envelope = (t / 0.01).min(1.0) * (-t * 1.2).exp();
                tone * envelope
            })
            .collect();
        normalise(out, 0.8)
    }

    /// The sound after `seconds` of silence (with a faint noise floor, as a recording has).
    pub fn late(sound: &[f32], seconds: f32, seed: u32) -> Vec<f32> {
        let mut out: Vec<f32> = noise(seed, samples(seconds)).iter().map(|s| s * 1e-4).collect();
        out.extend_from_slice(sound);
        out
    }

    /// The sound with a room's tail, a second and a half long: four feedback combs in parallel
    /// (a Schroeder reverb without its all-passes), its peak 18 dB under the sound's.
    pub fn reverb(sound: &[f32], seed: u32) -> Vec<f32> {
        let mut dry = sound.to_vec();
        dry.resize(sound.len() + samples(1.5), 0.0);
        let mut wet = vec![0.0f32; dry.len()];
        for (k, delay) in [1557usize, 1617, 1491, 1422].into_iter().enumerate() {
            let delay = delay + (seed as usize + k) % 7;
            let mut line = vec![0.0f32; dry.len()];
            for i in 0..dry.len() {
                let back = if i >= delay { line[i - delay] } else { 0.0 };
                line[i] = dry[i] + 0.84 * back;
                wet[i] += back * 0.25;
            }
        }
        let peak = |s: &[f32]| s.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-9);
        let gain = 0.125 * peak(&dry) / peak(&wet);
        dry.iter().zip(&wet).map(|(d, w)| d + w * gain).collect()
    }

    /// The sound at another level.
    pub fn level(sound: &[f32], gain: f32) -> Vec<f32> {
        sound.iter().map(|s| s * gain).collect()
    }
}

/// The kinds the generated sounds have, each with its name and how it is made.
#[allow(clippy::type_complexity)]
const SOUNDS: [(Kind, &str, fn(u32) -> Vec<f32>); 8] = [
    (Kind::Kick, "kick", sounds::kick),
    (Kind::Snare, "snare", sounds::snare),
    (Kind::Clap, "clap", sounds::clap),
    (Kind::ClosedHat, "closed hat", sounds::closed_hat),
    (Kind::Percussion, "tambourine", sounds::tambourine),
    (Kind::Percussion, "shaker", sounds::shaker),
    (Kind::Tom, "tom", sounds::tom),
    (Kind::Bass, "bass", sounds::bass),
];

/// The variations of an unnamed sound: leading silence, a room's tail, another level, all three.
fn varied(sound: &[f32], variation: u32) -> Vec<f32> {
    match variation % 4 {
        0 => sounds::late(sound, 0.15 + variation as f32 * 0.05, variation + 1),
        1 => sounds::reverb(sound, variation + 1),
        2 => sounds::level(sound, 0.03 + variation as f32 * 0.01),
        _ => sounds::level(&sounds::reverb(&sounds::late(sound, 0.4, variation), variation), 0.2),
    }
}

/// A library of the generated sounds: named ones in folders of their kind, unnamed varied ones in
/// a folder that names nothing, and sounds of one kind in a folder named for another.
fn kinds_library(library: &Library) -> (Vec<SourceFile>, Vec<(SourceFile, Kind)>) {
    let mut files = Vec::new();
    let mut unnamed = Vec::new();
    for (k, (kind, name, make)) in SOUNDS.iter().enumerate() {
        for v in 0..10 {
            files.push(library.write(&format!("Drums/{name}s/{name} {v}.wav"), &make(v)));
        }
        for v in 0..4 {
            let file = library.write(&format!("Misc/hit {k}-{v}.wav"), &varied(&make(20 + v), v));
            unnamed.push((file.clone(), *kind));
            files.push(file);
        }
    }
    // A tambourine and a shaker in a folder of kicks, a kick in a folder of hats.
    for (relative, sound, kind) in [
        ("Drums/Kicks/01.wav", sounds::tambourine(31), Kind::Percussion),
        ("Drums/Kicks/02.wav", sounds::shaker(32), Kind::Percussion),
        ("Drums/Hats/03.wav", sounds::kick(33), Kind::Kick),
    ] {
        let file = library.write(relative, &sound);
        unnamed.push((file.clone(), kind));
        files.push(file);
    }
    (files, unnamed)
}

#[test]
fn generated_sounds_get_their_kind_and_land_in_its_region() {
    let library = Library::new("kinds");
    let (files, unnamed) = kinds_library(&library);
    let mut map = analysed(&library, &files);
    let laid = map.map(&files, DEFAULT_MAX_SECONDS);
    assert_eq!(laid.points.len(), files.len());
    let point = |file: &SourceFile| laid.points.iter().find(|p| p.path == file.path).unwrap();
    let mut wrong = Vec::new();
    for (file, kind) in &unnamed {
        let p = point(file);
        if (p.kind, p.guessed) != (*kind, true) {
            wrong.push(format!("{} is {:?} (guessed {}), not {kind:?}", file.path.display(), p.kind, p.guessed));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    // On the map, most of each one's nearest points are of its kind: it sits in its kind's region.
    for (file, kind) in &unnamed {
        let at = point(file).position;
        let mut near: Vec<&Point> = laid.points.iter().filter(|p| p.path != file.path).collect();
        near.sort_by(|a, b| {
            let d = |p: &Point| (p.position[0] - at[0]).powi(2) + (p.position[1] - at[1]).powi(2);
            d(a).total_cmp(&d(b))
        });
        let same = near.iter().take(7).filter(|p| p.kind == *kind).count();
        assert!(same >= 5, "{} has {same} of 7 neighbours of its kind {kind:?}", file.path.display());
    }
    // Every named item keeps the kind its file name gives.
    for p in &laid.points {
        if let Some(kind) = p.path.file_stem().and_then(|s| s.to_str()).and_then(kinds::kind_of_name) {
            assert_eq!((p.kind, p.guessed), (kind, false), "{}", p.path.display());
        }
    }
}

#[test]
fn a_kind_set_by_hand_wins_and_survives_reanalysis_and_new_layouts() {
    let library = Library::new("hand");
    let mut files = Vec::new();
    for v in 0..8 {
        files.push(library.write(&format!("kick {v}.wav"), &sounds::kick(v)));
        files.push(library.write(&format!("closed hat {v}.wav"), &sounds::closed_hat(v)));
    }
    let odd = library.write("Misc/odd.wav", &sounds::kick(40));
    files.push(odd.clone());
    let data = library.root.join(".cache");
    let mut map = analysed(&library, &files);
    let before = map.map(&files, DEFAULT_MAX_SECONDS);
    let kind_of = |m: &Map| m.points.iter().find(|p| p.path == odd.path).map(|p| (p.kind, p.guessed, p.by_hand));
    assert_eq!(kind_of(&before), Some((Kind::Kick, true, false)));

    // Set by hand: it wins over the sound at once, and the layout is made again with it.
    map.set_kind(&odd.path, Some(Kind::Tom)).unwrap();
    assert_eq!(map.kind_by_hand(&odd.path), Some(Kind::Tom));
    let after = map.map(&files, DEFAULT_MAX_SECONDS);
    assert_eq!(kind_of(&after), Some((Kind::Tom, false, true)));
    assert_ne!(after.key, before.key);
    // Over a file's own name as well.
    map.set_kind(&files[0].path, Some(Kind::Snare)).unwrap();
    let named = map.map(&files, DEFAULT_MAX_SECONDS);
    assert_eq!(named.points.iter().find(|p| p.path == files[0].path).map(|p| p.kind), Some(Kind::Snare));

    // It is kept: a new sound map reads it, and analysing the folder again keeps it.
    drop(map);
    let mut map = SoundMap::new(Some(data.clone()), 1);
    map.update(&files);
    map.reanalyse(&library.root, &files);
    map.wait();
    assert_eq!(kind_of(&map.map(&files, DEFAULT_MAX_SECONDS)), Some((Kind::Tom, false, true)));

    // Cleared, the sound decides again.
    map.set_kind(&odd.path, None).unwrap();
    assert_eq!(kind_of(&map.map(&files, DEFAULT_MAX_SECONDS)), Some((Kind::Kick, true, false)));
    drop(map);
    assert_eq!(SoundMap::new(Some(data), 1).kind_by_hand(&odd.path), None);
}

#[test]
fn the_file_of_kinds_set_by_hand_round_trips_and_refuses_the_malformed() {
    let mut kinds = HashMap::new();
    kinds.insert(PathBuf::from("/lib/a b.wav"), Kind::Tom);
    kinds.insert(PathBuf::from("/lib/tab\there.wav"), Kind::ClosedHat);
    assert_eq!(hand_kinds_from_text(&hand_kinds_to_text(&kinds)), kinds);
    assert!(hand_kinds_from_text("").is_empty());
    assert!(hand_kinds_from_text("another header\ntom\t/lib/a.wav\n").is_empty());
    // Lines that do not parse are left out; the rest are read.
    let text = format!("{HAND_KINDS_HEADER}\nnokind\t/x.wav\ntom\n\t/y.wav\ntom\t\nkick\t/z.wav\n");
    assert_eq!(hand_kinds_from_text(&text), HashMap::from([(PathBuf::from("/z.wav"), Kind::Kick)]));
    let mut map = SoundMap::new(None, 1);
    assert!(map.set_kind(Path::new("/a\nb.wav"), Some(Kind::Kick)).is_err());
    assert!(map.set_kind(Path::new("/a.wav"), Some(Kind::Kick)).is_ok());
    assert_eq!(map.kind_by_hand(Path::new("/a.wav")), Some(Kind::Kick));
}

#[test]
fn an_old_cache_is_analysed_again_never_misread() {
    let library = Library::new("old-cache");
    let file = library.write("kick.wav", &sounds::kick(1));
    let data = library.root.join(".cache");
    std::fs::create_dir_all(&data).unwrap();
    // A version 1 cache: the same header with the old feature version and 44 numbers.
    let mut bytes = b"LWSM".to_vec();
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&8u16.to_le_bytes());
    bytes.extend_from_slice(b"kick.wav");
    bytes.extend_from_slice(&file.size.to_le_bytes());
    bytes.extend_from_slice(&file.modified.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&0.5f32.to_le_bytes());
    for _ in 0..44 {
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
    }
    std::fs::write(data.join(cache::file_name(&library.root)), bytes).unwrap();
    let mut map = SoundMap::new(Some(data), 1);
    map.update(std::slice::from_ref(&file));
    assert!(map.queued.contains(&file.path));
    map.wait();
    match map.analysis(&file) {
        Some(Analysis::Analysed { features, .. }) => assert_ne!(features, [1.0; features::FEATURE_COUNT]),
        other => panic!("{other:?}"),
    }
}

/// While a background update is worked out, the map says so, and refuses another; once taken
/// in, it is free again.
#[test]
fn a_background_update_is_reported_until_it_is_taken_in() {
    let mut map = SoundMap::new(None, 1);
    assert!(!map.updating());
    let (go, wait) = std::sync::mpsc::channel::<()>();
    assert!(map.update_in_background(move || {
        let _ = wait.recv();
        Vec::new()
    }));
    assert!(map.updating());
    assert!(!map.update_in_background(Vec::new), "one at a time");
    go.send(()).unwrap();
    map.wait();
    assert!(!map.updating());
}

#[test]
fn a_diff_from_the_update_thread_is_taken_in_once_whatever_the_channel_says() {
    let diff = || Diff { loaded: Vec::new(), work: Vec::new(), gone: Vec::new(), folders: HashSet::new() };
    let mut map = SoundMap::new(None, 1);
    // Still working: nothing yet, and the update stays awaited.
    let (sender, receiver) = channel();
    map.diffing = Some(receiver);
    assert!(!map.take_diff(false));
    assert!(map.diffing.is_some());
    // Done and gone at once: the diff that came in is taken in, not dropped by a second read.
    sender.send(diff()).unwrap();
    drop(sender);
    assert!(map.take_diff(false));
    assert!(map.diffing.is_none());
    // Gone without a diff: nothing to wait for any more.
    let (sender, receiver) = channel::<Diff>();
    map.diffing = Some(receiver);
    drop(sender);
    assert!(!map.take_diff(false));
    assert!(map.diffing.is_none());
}
