// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Golden files: a render or a text compared with what a reviewed run wrote before. With
//! [`BLESS`] set, the comparison writes the file instead.

use std::path::Path;

/// The environment variable that rewrites golden files: `SUHINA_BLESS=1 cargo test …`.
pub const BLESS: &str = "SUHINA_BLESS";

/// Whether this run rewrites golden files.
pub fn blessing() -> bool {
    std::env::var_os(BLESS).is_some()
}

/// The golden samples in `path` (little-endian `f32`), for a test that compares them its own way;
/// with [`BLESS`] set, writes `render` there instead and returns `None`.
pub fn golden_f32(path: &Path, render: &[f32]) -> Option<Vec<f32>> {
    if blessing() {
        write(path, render.iter().flat_map(|s| s.to_le_bytes()).collect());
        return None;
    }
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}; run with {BLESS}=1", path.display()));
    Some(bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect())
}

/// Compares `render` with `path`, little-endian `f32` samples, and fails when the lengths differ
/// or a sample is `tolerance` or further from the file's. With [`BLESS`] set, writes the file.
pub fn assert_golden_f32(path: &Path, render: &[f32], tolerance: f32) {
    let Some(golden) = golden_f32(path, render) else { return };
    let name = path.display();
    assert_eq!(golden.len(), render.len(), "{name}: {} samples, the golden file has {}", render.len(), golden.len());
    let (at, worst) = render
        .iter()
        .zip(&golden)
        .map(|(a, b)| {
            let d = (a - b).abs();
            if d.is_nan() { f32::INFINITY } else { d }
        })
        .enumerate()
        .fold((0, 0.0f32), |m, d| if d.1 > m.1 { d } else { m });
    assert!(worst < tolerance, "{name}: differs from the golden file by {worst} at sample {at}");
}

/// Compares `actual` with the text in `path`, line endings aside. With [`BLESS`] set, writes the
/// file.
pub fn assert_golden_text(path: &Path, actual: &str) {
    if blessing() {
        write(path, actual.as_bytes().to_vec());
        return;
    }
    let expected = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{}: {e}; run with {BLESS}=1", path.display()))
        .replace("\r\n", "\n");
    assert_eq!(actual, expected, "differs from {}", path.display());
}

fn write(path: &Path, bytes: Vec<u8>) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}
