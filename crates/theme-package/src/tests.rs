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
    // A key it does not read is kept, and named again.
    assert_eq!(meta::ThemeMeta::parse(read.to_json().as_bytes()).unwrap(), (read, warnings));
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
        files.insert("theme.css".into(), b":root { --accent: #ff8800; --tint: #ff8800; }".to_vec());
        files.insert("variants/light.json".into(), variant.as_bytes().to_vec());
        validate(&files)
    };
    assert!(with("color-mix(in srgb, var(--accent), white 50%)", r##"{ "values": { "--accent": "#00f" } }"##).is_ok());
    // `--tint` is unregistered, so the variant's length stands (a core `<color>` would fall back).
    let error = with("var(--tint)", r#"{ "values": { "--tint": "12px" } }"#).unwrap_err();
    assert!(error.contains("variants/light.json"), "{error}");
    for refused in ["currentColor", "attr(color type(<color>))", "var(--missing)", "12px"] {
        assert!(with(refused, "{}").is_err(), "{refused}");
    }
    let variant = meta::variant_sheet(br##"{ "values": { "--a": "#fff", "b": "red", "--c": "x}" } }"##).unwrap();
    assert_eq!(variant, ":root { --a: #fff; }");
}

#[test]
fn the_swatch_resolves_against_the_core_properties() {
    // A duplicate of Suhina Studio: an empty `:root`, the swatch left as Suhina Studio's.
    let with = |swatch: &str| {
        let mut files = theme();
        files.insert(
            "theme.json".into(),
            format!(r#"{{ "format": 1, "name": "Copy", "swatch": "{swatch}" }}"#).into_bytes(),
        );
        files.insert("theme.css".into(), b":root {}".to_vec());
        files
    };
    let copy = with("color-mix(in oklab, var(--panel-background), var(--accent) 35%)");
    assert_eq!(validate(&copy), Ok(Vec::new()));
    assert!(read(&pack("copy", "Copy", &copy).unwrap()).is_ok());
    assert!(validate(&with("var(--not-a-core-property)")).is_err());
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
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:x="http://www.w3.org/1999/xlink" viewBox="0 0 160 90"><use x:href = "a.svg#b"/></svg>"#,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 160 90\"><use href\n=\"#b\"/></svg>",
    ] {
        assert!(with(refused).is_err(), "{refused}");
    }
    let mut files = theme();
    files.insert("layout.css".into(), b"".to_vec());
    assert!(validate(&files).unwrap().iter().any(|w| w.contains("layout.css")));
}

#[test]
fn validation_keeps_the_limits_the_disk_reader_keeps() {
    // theme.json and a variant are read up to 64 KiB, and at most 64 variants.
    let mut files = theme();
    files.insert(
        meta::FILE.into(),
        format!(
            r#"{{ "format": 1, "name": "Night", "swatch": "var(--accent)", "description": "{}" }}"#,
            "x".repeat(70_000)
        )
        .into_bytes(),
    );
    assert!(validate(&files).unwrap_err().contains("exceeds 64 KiB"));
    let mut files = theme();
    files.insert("variants/big.json".into(), format!(r#"{{"name":"{}"}}"#, "x".repeat(70_000)).into_bytes());
    assert!(validate(&files).unwrap_err().contains("exceeds 64 KiB"));
    let mut files = theme();
    for n in 0..65 {
        files.insert(format!("variants/v{n}.json"), br#"{"name":"V"}"#.to_vec());
    }
    assert!(validate(&files).unwrap_err().contains("at most 64 variants"));
}

#[test]
fn a_thumbnail_must_be_svg_by_namespace_and_have_a_finite_size() {
    let with = |svg: &str| {
        let mut files = theme();
        files.insert(meta::THUMBNAIL.into(), svg.as_bytes().to_vec());
        validate(&files)
    };
    assert!(with(r#"<svg viewBox="0 0 160 90"/>"#).is_ok(), "a root without a namespace is read");
    for refused in [
        r#"<svg xmlns="http://example.com/other" viewBox="0 0 160 90"/>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="inf" height="inf"/>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 inf inf"/>"#,
    ] {
        assert!(with(refused).is_err(), "{refused}");
    }
}

#[test]
fn rewriting_theme_json_keeps_the_schema_and_keys_it_does_not_read() {
    let text = br#"{ "$schema": "https://example.com/theme.json", "format": 1, "name": "Night", "swatch": "red", "next": { "a": 1 } }"#;
    let (parsed, warnings) = meta::ThemeMeta::parse(text).unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("next"), "the schema key is known, an unknown one is named");
    let (again, _) = meta::ThemeMeta::parse(parsed.to_json().as_bytes()).unwrap();
    assert_eq!(again.extra["$schema"], "https://example.com/theme.json");
    assert_eq!(again.extra["next"]["a"], 1);
}

/// The user-agent sheet styles elements by name, state and attribute only: no class, no id and
/// no variable, no `@media` (and so no `prefers-color-scheme`), and every element it names is in
/// the registry.
#[test]
fn the_user_agent_sheet_selects_by_name_state_and_attribute_only() {
    assert!(suhina_style::sheet::Sheet::parse(USER_AGENT_SHEET).is_ok(), "the user-agent sheet parses");
    assert!(!USER_AGENT_SHEET.contains("var("), "the user-agent sheet uses no variables");
    assert!(!USER_AGENT_SHEET.contains("--"), "the user-agent sheet declares no custom properties");
    assert!(!USER_AGENT_SHEET.contains('@'), "the user-agent sheet has no at-rules");
    let mut text = USER_AGENT_SHEET.to_owned();
    while let Some(start) = text.find("/*") {
        let end = text[start..].find("*/").unwrap() + start + 2;
        text.replace_range(start..end, "");
    }
    let mut rest = text.as_str();
    let mut rules = 0;
    while let Some(open) = rest.find('{') {
        let close = rest.find('}').unwrap();
        for selector in rest[..open].split(',').map(str::trim) {
            rules += 1;
            let name = selector.split(['[', ':']).next().unwrap();
            assert!(elements::is_element(name), "{selector}: {name} is not an element");
            // No classes yet: the standard toolbar group classes the spec allows come with the
            // toolbar's groups (#995 § 7).
            assert!(!selector.contains('.'), "{selector} has a class");
            assert!(!selector.contains('#'), "{selector} has an id");
        }
        rest = &rest[close + 1..];
    }
    assert!(rules > 30);
}

/// Every region keeps what its children paint (`overflow: hidden`), and only regions do.
#[test]
fn the_user_agent_sheet_clips_regions() {
    let rule = USER_AGENT_SHEET.split("*/").find(|part| part.contains("overflow: hidden")).unwrap();
    let listed: Vec<&str> = rule[..rule.find('{').unwrap()].split(',').map(str::trim).collect();
    let regions: Vec<&str> = elements::ELEMENTS
        .iter()
        .filter(|element| element.kind == elements::Kind::Region)
        .map(|element| element.name)
        .collect();
    assert_eq!(listed, regions);
}

/// The default artwork lives in the user-agent sheet: a rule for each role Suhina ships artwork
/// for, on the element that draws it, naming the built-in file.
#[test]
fn the_user_agent_sheet_gives_every_default_artwork() {
    let given = user_agent_artwork();
    let shipped: Vec<_> = elements::default_artwork().collect();
    assert_eq!(given, shipped);
    for art in elements::artwork().filter(|art| art.default.is_some()) {
        let rule = format!("{} {{ image: url(\"suhina:{}.svg\"); }}", art.selector, art.role);
        assert!(USER_AGENT_SHEET.contains(&rule), "the user-agent sheet lacks {rule}");
    }
    assert_eq!(built_in_artwork("knob.body.svg").map(|(role, _)| role), Some("knob.body"));
    assert_eq!(built_in_artwork("nothing.svg"), None);
}

/// Suhina's own artwork takes its colour only from `currentColor`: no attribute of Suhina's own
/// keeps a palette colour in it.
#[test]
fn default_artwork_is_coloured_by_current_color_alone() {
    for (role, svg) in elements::default_artwork() {
        assert!(!svg.contains("data-suhina-ink"), "{role} still names a palette colour");
        assert!(!svg.contains("var("), "{role} uses a variable");
    }
}

/// Sample points are styled by kind in the user-agent sheet, one rule per kind.
#[test]
fn the_user_agent_sheet_colours_sample_points_by_kind() {
    let kinds = USER_AGENT_SHEET.matches("sample-point[kind=").count();
    assert_eq!(kinds, 17);
}

#[test]
fn image_rules_read_selector_lists_and_skip_comments_and_at_rules() {
    let css = "/* a { image: url(x.svg); } */ a, b:hover { image: url('y.svg'); color: red; }\n\
               @media (max-width: 1px) { c { image: url(z.svg); } }";
    assert_eq!(
        image_rules(css),
        vec![("a".to_owned(), "y.svg".to_owned()), ("b:hover".to_owned(), "y.svg".to_owned())]
    );
    // A rule after an at-rule's block with rules in it is read as a rule of its own, and the
    // rules inside the block are not; a statement at-rule does not swallow the selector after it.
    let css = "@media (max-width: 1px) { c { image: url(z.svg); } d { color: red; } }
               e { image: url(e.svg); }
               @import \"more.css\";
               f { image: url(f.svg); }";
    assert_eq!(image_rules(css), vec![("e".to_owned(), "e.svg".to_owned()), ("f".to_owned(), "f.svg".to_owned())]);
}
