// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn scratch(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("suhina-library-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    folder
}

#[test]
fn the_old_settings_folder_is_copied_to_the_new_name_once() {
    let base = scratch("adopt");
    let (old, new) = (base.join("old"), base.join("deeper").join("new"));
    assert_eq!(adopt_old_folder(Some(old.clone()), Some(new.clone())), None, "no old folder: nothing to do");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(old.join("plugins.json"), "{}").unwrap();
    std::fs::create_dir_all(old.join("data")).unwrap();
    std::fs::write(old.join("data").join("app.ron"), "()").unwrap();
    assert_eq!(adopt_old_folder(Some(old.clone()), Some(new.clone())), None);
    assert_eq!(std::fs::read_to_string(new.join("plugins.json")).unwrap(), "{}");
    assert_eq!(std::fs::read_to_string(new.join("data").join("app.ron")).unwrap(), "()");
    assert!(old.join("plugins.json").exists(), "the old folder stays for builds from before the rename");
    assert!(!new.with_extension("partial").exists());
    // A new folder that already exists is never replaced.
    std::fs::write(old.join("plugins.json"), "changed").unwrap();
    assert_eq!(adopt_old_folder(Some(old.clone()), Some(new.clone())), None);
    assert_eq!(std::fs::read_to_string(new.join("plugins.json")).unwrap(), "{}");
    assert_eq!(adopt_old_folder(None, Some(new.clone())), None);
    // Settings are read from the old folder only while the new one is missing.
    assert_eq!(prefer_new(Some(new.clone()), Some(old.clone())), Some(new.clone()));
    let missing = base.join("missing");
    assert_eq!(prefer_new(Some(missing.clone()), Some(old.clone())), Some(old.clone()));
    assert_eq!(prefer_new(Some(missing.clone()), Some(base.join("gone"))), Some(missing));
    let _ = std::fs::remove_dir_all(&base);
}

fn info(path: &str, vendor: &str) -> PluginInfo {
    PluginInfo {
        path: PathBuf::from(path),
        id: "com.example.thing".into(),
        name: "Thing".into(),
        vendor: vendor.into(),
        version: String::new(),
        features: Vec::new(),
        replaces: Vec::new(),
        ara: false,
    }
}

#[test]
fn hosting_names_round_trip() {
    for hosting in [Hosting::Binary, Hosting::Instance, Hosting::Vendor] {
        assert_eq!(Hosting::parse(hosting.name()), Some(hosting));
    }
    assert_eq!(Hosting::parse("Binary"), None);
    assert_eq!(Hosting::parse(""), None);
    assert_eq!(Hosting::default(), Hosting::Binary);
}

#[test]
fn hosting_is_saved_and_read_back() {
    let folder = scratch("hosting");
    let mut library = Library::new(Some(folder.clone()), PathBuf::from("host"));
    assert_eq!(library.hosting("clap:a.clap#x"), Hosting::Binary);
    library.set_hosting("clap:a.clap#x", Hosting::Instance).unwrap();
    library.set_hosting("vst3:b.vst3#00", Hosting::Vendor).unwrap();
    library.set_hosting("vst3:c.vst3#11", Hosting::Vendor).unwrap();
    library.set_hosting("vst3:c.vst3#11", Hosting::Binary).unwrap();
    assert!(library.set_hosting("", Hosting::Vendor).is_err());

    let mut again = Library::new(Some(folder.clone()), PathBuf::from("host"));
    assert_eq!(again.hosting("clap:a.clap#x"), Hosting::Instance);
    assert_eq!(again.hosting("vst3:b.vst3#00"), Hosting::Vendor);
    assert_eq!(again.hosting("vst3:c.vst3#11"), Hosting::Binary);
    assert_eq!(again.hosting("unknown"), Hosting::Binary);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_malformed_hosting_file_is_ignored() {
    for text in [
        "",
        "not json",
        "[]",
        r#"{"a":5,"b":"nonsense","c":null,"":"vendor","d":"vendor"}"#,
        &format!("{{\"e\":\"{}\"}}", "x".repeat(2 << 20)),
    ] {
        let folder = scratch("malformed");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(HOSTING_FILE), text).unwrap();
        let mut library = Library::new(Some(folder.clone()), PathBuf::from("host"));
        for spec in ["a", "b", "c", "", "e"] {
            assert_eq!(library.hosting(spec), Hosting::Binary, "{spec} in {:.40}", text);
        }
        let expected = if text.contains("\"d\"") { Hosting::Vendor } else { Hosting::Binary };
        assert_eq!(library.hosting("d"), expected);
        let _ = std::fs::remove_dir_all(&folder);
    }
    let many: Map<String, Value> = (0..MAX_HOSTING + 10).map(|n| (format!("p{n}"), Value::from("vendor"))).collect();
    assert_eq!(parse_hosting(&Value::Object(many).to_string()).len(), MAX_HOSTING);
}

#[test]
fn folders_keep_their_format_and_whether_they_are_on() {
    let folder = scratch("folders");
    let (vst2, mixed) = (folder.join("VstPlugins"), folder.join("Mixed"));
    std::fs::create_dir_all(&vst2).unwrap();
    std::fs::create_dir_all(&mixed).unwrap();
    let mut library = Library::new(Some(folder.join("settings")), PathBuf::from("host")).without_standard_folders();
    assert!(library.add_folder(&mixed).unwrap());
    assert!(library.add_folder_as(&vst2, FolderFormat::Vst2).unwrap());
    assert!(!library.add_folder_as(&vst2, FolderFormat::Vst2).unwrap(), "already there");
    assert!(library.set_folder_enabled(&mixed, false).unwrap());
    assert!(!library.set_folder_enabled(&mixed, false).unwrap(), "already off");
    assert!(library.set_folder_enabled(&folder.join("elsewhere"), false).is_err());

    let mut again = Library::new(Some(folder.join("settings")), PathBuf::from("host")).without_standard_folders();
    let list = again.folder_list();
    let formats: Vec<(FolderFormat, bool)> = list.iter().map(|f| (f.format, f.enabled)).collect();
    assert_eq!(formats, [(FolderFormat::Any, false), (FolderFormat::Vst2, true)]);
    assert_eq!(again.folders().len(), 1, "a folder switched off is not scanned");
    assert!(again.remove_folder(&mixed).unwrap());
    assert_eq!(again.folder_list().len(), 1);
    let _ = std::fs::remove_dir_all(&folder);
}

/// A leading `~` alone or before a separator is the home folder; `~name` and absolute paths stay.
#[test]
fn a_folder_typed_with_a_leading_tilde_expands_to_the_home_folder() {
    let home = PathBuf::from("/Users/me");
    let expand = |folder: &str, home: Option<&OsStr>| expand_home(Path::new(folder), home);
    let with_home = Some(home.as_os_str());
    assert_eq!(expand("~/Plug-ins/VST", with_home), PathBuf::from("/Users/me/Plug-ins/VST"));
    assert_eq!(expand("~", with_home), PathBuf::from("/Users/me"));
    assert_eq!(expand("~/", with_home), PathBuf::from("/Users/me"));
    assert_eq!(expand("~user/plug-ins", with_home), PathBuf::from("~user/plug-ins"), "another user's home stays");
    assert_eq!(expand("/plug-ins", with_home), PathBuf::from("/plug-ins"), "absolute stays");
    assert_eq!(expand("relative/plug-ins", with_home), PathBuf::from("relative/plug-ins"), "relative stays");
    assert_eq!(expand("~/Plug-ins", None), PathBuf::from("~/Plug-ins"), "no home folder, no expansion");
}

/// `HOME` first; where it is missing on Windows, the profile stands in. On other systems the
/// profile does not count.
#[test]
fn the_home_falls_back_to_the_windows_profile() {
    let home = Some(OsString::from("/Users/me"));
    let profile = Some(OsString::from(r"C:\Users\me"));
    assert_eq!(home_folder(home.clone(), profile.clone()), home, "HOME comes first");
    assert_eq!(
        home_folder(None, profile.clone()),
        if cfg!(windows) { profile } else { None },
        "where HOME is missing, Windows's profile stands in; elsewhere there is none"
    );
    assert_eq!(home_folder(None, None), None);
}

#[test]
fn the_earlier_folders_file_is_read() {
    let (added, off) = parse_folders(r#"["/plugins/a", 5, "/plugins/b"]"#);
    assert_eq!(
        added,
        [(PathBuf::from("/plugins/a"), FolderFormat::Any), (PathBuf::from("/plugins/b"), FolderFormat::Any)]
    );
    assert!(off.is_empty());
    let (added, off) =
        parse_folders(r#"{"added":[{"path":"/p","format":"clap"},{"path":"/q","format":"?"},{}],"off":["/p"]}"#);
    assert_eq!(added, [(PathBuf::from("/p"), FolderFormat::Clap), (PathBuf::from("/q"), FolderFormat::Any)]);
    assert_eq!(off, [PathBuf::from("/p")]);
    assert_eq!(parse_folders("junk"), (Vec::new(), Vec::new()));
    assert_eq!(standard_format(Path::new("C:/Program Files/Common Files/VST3")), FolderFormat::Vst3);
    assert_eq!(standard_format(Path::new("/home/me/.clap")), FolderFormat::Clap);
}

#[test]
fn plugins_switched_off_are_kept_and_left_out_of_the_catalog_sessions_see() {
    let folder = scratch("disabled");
    let mut library = Library::new(Some(folder.clone()), PathBuf::from("host"));
    library.catalog.insert(PathBuf::from("/p/Thing.clap"), vec![info("/p/Thing.clap", "Maker")]);
    let spec = info("/p/Thing.clap", "Maker").spec();
    assert!(library.is_enabled(&spec));
    library.set_enabled(&spec, false).unwrap();
    assert_eq!(library.enabled_catalog().plugins().count(), 0);
    assert_eq!(library.catalog().plugins().count(), 1, "the catalog keeps it");
    let mut again = Library::new(Some(folder.clone()), PathBuf::from("host"));
    assert!(!again.is_enabled(&spec));
    again.set_enabled(&spec, true).unwrap();
    assert!(again.is_enabled(&spec));
    assert!(again.set_enabled("", false).is_err());
    assert_eq!(parse_disabled(r#"["a", 5, "", "b"]"#).len(), 2);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn groups_follow_the_hosting() {
    let plugin = info("C:/Plugins/Thing.vst3", "Example Audio");
    assert_eq!(group_for(Hosting::Binary, &plugin).as_deref(), Some("file:C:/Plugins/Thing.vst3"));
    assert_eq!(group_for(Hosting::Vendor, &plugin).as_deref(), Some("vendor:Example Audio"));
    assert_eq!(group_for(Hosting::Instance, &plugin), None);
    let unknown = info("C:/Plugins/Other.clap", " ");
    assert_eq!(group_for(Hosting::Vendor, &unknown).as_deref(), Some("file:C:/Plugins/Other.clap"));
    // Two plug-ins in one file share its process.
    let sibling = PluginInfo { id: "com.example.other".into(), ..plugin.clone() };
    assert_eq!(group_for(Hosting::Binary, &sibling), group_for(Hosting::Binary, &plugin));
    // An ARA plug-in never gets a process of its own: its document lives with its file's.
    let ara = PluginInfo { ara: true, ..plugin.clone() };
    assert_eq!(group_for(Hosting::Instance, &ara), group_for(Hosting::Binary, &plugin));
}
