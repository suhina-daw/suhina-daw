// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn theme() -> BTreeMap<String, Vec<u8>> {
    [
        ("theme.json".to_owned(), br#"{ "format": 1, "name": "Night", "swatch": "var(--accent)" }"#.to_vec()),
        ("theme.css".to_owned(), b":root { --accent: #ff8800; }".to_vec()),
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
    assert_eq!(theme_name(&theme()).as_deref(), Some("Night"));
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
    for root in ["theme.css", "theme.json"] {
        let mut files = theme();
        files.remove(root);
        assert!(validate(&files).unwrap_err().contains(root), "{root}");
    }
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
    files.insert("theme.css".into(), b":root { --accent: red; } .x { colour: red; }".to_vec());
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

#[test]
fn theme_json_round_trips_and_refuses_a_file_without_a_name_swatch_or_known_format() {
    let text = br#"{ "format": 1, "name": " Night ", "author": "Ari", "description": "Blue",
        "default-variant": "Dark", "swatch": "var(--accent)", "toolbar": { "items": ["play"] }, "extra": 1 }"#;
    let (read, warnings) = meta::ThemeMeta::parse(text).unwrap();
    assert_eq!(read.name, "Night");
    assert_eq!(read.default_variant(), "Dark");
    assert_eq!(read.toolbar, Some(serde_json::json!({ "items": ["play"] })));
    assert!(warnings.iter().any(|w| w.contains("extra")), "{warnings:?}");
    assert_eq!(meta::ThemeMeta::parse(read.to_json().as_bytes()).unwrap(), (read, Vec::new()));
    assert_eq!(meta::ThemeMeta::new("A", "red").default_variant(), meta::DEFAULT_VARIANT);
    for (text, why) in [
        (r#"{ "name": "A", "swatch": "red" }"#, "format"),
        (r#"{ "format": 2, "name": "A", "swatch": "red" }"#, "newer"),
        (r#"{ "format": 1, "swatch": "red" }"#, "name"),
        (r#"{ "format": 1, "name": "A" }"#, "swatch"),
        (r#"{ "format": 1, "name": "A", "swatch": "red; x" }"#, "swatch"),
        ("[]", "object"),
    ] {
        let error = meta::ThemeMeta::parse(text.as_bytes()).unwrap_err();
        assert!(error.contains(why), "{text}: {error}");
    }
}

#[test]
fn the_swatch_must_resolve_to_a_colour_for_every_variant() {
    let with = |swatch: &str, variant: &str| {
        let mut files = theme();
        files.insert(
            "theme.json".into(),
            format!(r#"{{ "format": 1, "name": "Night", "swatch": "{swatch}" }}"#).into_bytes(),
        );
        files.insert("variants/light.json".into(), variant.as_bytes().to_vec());
        validate(&files)
    };
    assert!(with("color-mix(in srgb, var(--accent), white 50%)", r##"{ "values": { "--accent": "#00f" } }"##).is_ok());
    let error = with("var(--accent)", r#"{ "values": { "--accent": "12px" } }"#).unwrap_err();
    assert!(error.contains("variants/light.json"), "{error}");
    for refused in ["currentColor", "attr(color type(<color>))", "var(--missing)", "12px"] {
        assert!(with(refused, "{}").is_err(), "{refused}");
    }
    let variant = meta::variant_sheet(br##"{ "values": { "--a": "#fff", "b": "red", "--c": "x}" } }"##).unwrap();
    assert_eq!(variant, ":root { --a: #fff; }");
}

#[test]
fn a_thumbnail_is_a_sixteen_by_nine_svg_and_layout_css_is_named() {
    let with = |svg: &str| {
        let mut files = theme();
        files.insert(meta::THUMBNAIL.into(), svg.as_bytes().to_vec());
        validate(&files)
    };
    assert!(with(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 160 90"/>"#).is_ok());
    assert!(with(r#"<svg xmlns="http://www.w3.org/2000/svg" width="320px" height="180"/>"#).is_ok());
    for refused in [
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"/>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg"/>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 160 90"><image href="a.png"/></svg>"#,
    ] {
        assert!(with(refused).is_err(), "{refused}");
    }
    let mut files = theme();
    files.insert("layout.css".into(), b"".to_vec());
    assert!(validate(&files).unwrap().iter().any(|w| w.contains("layout.css")));
}
