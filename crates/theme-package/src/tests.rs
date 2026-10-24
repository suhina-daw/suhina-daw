// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn theme() -> BTreeMap<String, Vec<u8>> {
    [
        ("theme.css".to_owned(), b"theme { name: \"Night\"; } :root { --accent: #ff8800; }".to_vec()),
        ("artwork/play.svg".to_owned(), br#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"/>"#.to_vec()),
        ("variants/light.json".to_owned(), br#"{"name":"Light"}"#.to_vec()),
    ]
    .into()
}

#[test]
fn a_package_round_trips_with_its_manifest() {
    let bytes = pack("night", "Night", &theme()).unwrap();
    let package = read(&bytes).unwrap();
    assert_eq!(package.id, "night");
    assert_eq!(package.name, "Night");
    assert_eq!(package.files, theme());
    assert!(package.warnings.is_empty(), "{:?}", package.warnings);
    assert_eq!(sheet_name(std::str::from_utf8(&theme()["theme.css"]).unwrap()).as_deref(), Some("Night"));
}

/// Rewrites `bytes` with `change` applied to its entries.
fn repack(bytes: &[u8], change: impl FnOnce(&mut BTreeMap<String, Vec<u8>>)) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut entries = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        entries.insert(entry.name().unwrap().to_string(), data);
    }
    change(&mut entries);
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in entries {
        zip.start_file(name.as_str(), zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(&data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn a_changed_missing_or_extra_file_refuses_the_package() {
    let bytes = pack("night", "Night", &theme()).unwrap();
    let changed = repack(&bytes, |e| {
        e.insert("theme.css".into(), b":root { --accent: red; }".to_vec());
    });
    assert!(read(&changed).unwrap_err().contains("SHA-256"));
    let extra = repack(&bytes, |e| {
        e.insert("extra.css".into(), b"".to_vec());
    });
    assert!(read(&extra).unwrap_err().contains("not in the manifest"));
    let missing = repack(&bytes, |e| {
        e.remove("variants/light.json");
    });
    assert!(read(&missing).unwrap_err().contains("not in the package"));
    let no_manifest = repack(&bytes, |e| {
        e.remove(MANIFEST);
    });
    assert!(read(&no_manifest).unwrap_err().contains("manifest"));
    let newer = repack(&bytes, |e| {
        let mut m: serde_json::Value = serde_json::from_slice(&e[MANIFEST]).unwrap();
        m["version"] = 9.into();
        e.insert(MANIFEST.into(), serde_json::to_vec(&m).unwrap());
    });
    assert!(read(&newer).unwrap_err().contains("update"));
    assert!(read(b"not a zip").is_err());
}

#[test]
fn validation_refuses_what_the_app_would_not_read() {
    let mut files = theme();
    files.remove("theme.css");
    assert!(validate(&files).unwrap_err().contains("theme.css"));
    for (path, bytes) in [
        ("a.svg", b"<html/>".to_vec()),
        ("a.svg", br#"<svg xmlns="http://www.w3.org/2000/svg"><script/></svg>"#.to_vec()),
        ("a.png", b"GIF89a".to_vec()),
        ("variants/a.json", b"{".to_vec()),
        ("../outside.css", b"".to_vec()),
        ("big.css", vec![b' '; MAX_TEXT + 1]),
    ] {
        let mut files = theme();
        files.insert(path.into(), bytes);
        assert!(validate(&files).is_err(), "{path}");
    }
    let mut files = theme();
    files.insert("theme.css".into(), b".x { colour: red; }".to_vec());
    assert!(validate(&files).unwrap().iter().any(|w| w.contains("colour")), "a notice is a warning");
    assert!(pack("Bad Id", "x", &theme()).is_err());
}

#[test]
fn a_folder_packs_without_its_hidden_files() {
    let dir = std::env::temp_dir().join(format!("suhina-theme-package-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    for (path, bytes) in theme() {
        let at = dir.join(&path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, bytes).unwrap();
    }
    std::fs::write(dir.join(".DS_Store"), b"x").unwrap();
    assert_eq!(read_folder(&dir).unwrap(), theme());
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(is_package(Path::new("x/Night.THEME.suhina")));
}
