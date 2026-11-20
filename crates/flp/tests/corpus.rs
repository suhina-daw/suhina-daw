// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests against the local, uncommitted FL Studio corpus (`corpus/flp`, see
//! `docs/spec/flp-corpus.md`). Run with `cargo test -p suhina-flp --features corpus`.
//!
//! Nothing here prints the values of events 28 and 200 (the license flag and the licensee's name).
#![cfg(feature = "corpus")]
#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

use std::path::PathBuf;

use suhina_flp::{Flp, TextEncoding, Value, ids, read_file};

const SERIES_1: [&str; 7] = [
    "00-empty",
    "00b-empty-resaved",
    "01-tempo-128",
    "02-channel",
    "03-pattern-note",
    "04-playlist-pattern",
    "05-mixer-insert",
];

fn dir() -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "..", "..", "corpus", "flp"].iter().collect()
}

fn open(name: &str) -> Flp {
    let path = dir().join(format!("{name}.flp"));
    assert!(path.exists(), "{} is missing; author it as docs/spec/flp-corpus.md describes", path.display());
    read_file(&path).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn tempo(flp: &Flp) -> u32 {
    match flp.value(flp.first(ids::TEMPO).unwrap()).unwrap() {
        Value::Unsigned(t) => t,
        other => panic!("tempo is {other:?}"),
    }
}

#[test]
fn every_series_1_file_reads() {
    for name in SERIES_1 {
        let flp = open(name);
        // read_file already refused a walk that does not end exactly at the end of the chunk.
        assert!(!flp.events.is_empty(), "{name}: no events");
        let version = flp.version().unwrap_or_else(|| panic!("{name}: no version event"));
        assert!(version.starts_with(|c: char| c.is_ascii_digit()), "{name}: version {version:?}");
    }
}

#[test]
fn every_file_in_the_folder_reads() {
    let entries =
        std::fs::read_dir(dir()).unwrap_or_else(|e| panic!("{}: {e}; see docs/spec/flp-corpus.md", dir().display()));
    for entry in entries {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("flp")) {
            let flp = read_file(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            // Every string the reader knows the kind of decodes in the file's encoding.
            // The errors name offsets, never the values.
            for event in &flp.events {
                flp.value(event).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            }
        }
    }
}

#[test]
fn files_from_fl_studio_26_are_utf16() {
    let flp = open("00-empty");
    if flp.version().is_some_and(|v| v.starts_with("26.")) {
        assert_eq!(flp.encoding().unwrap(), TextEncoding::Utf16Le);
    }
}

#[test]
fn tempo_is_140_by_default_and_128_when_set() {
    assert_eq!(tempo(&open("00-empty")), 140_000);
    assert_eq!(tempo(&open("01-tempo-128")), 128_000);
}

#[test]
fn every_file_opens_with_id_172_as_one_byte() {
    let flp = open("00-empty");
    let opening: Vec<(u8, usize)> = flp.events.iter().take(4).map(|e| (e.id, e.offset)).collect();
    assert_eq!(opening, vec![(169, 22), (28, 27), (172, 29), (1, 31)]);
}
