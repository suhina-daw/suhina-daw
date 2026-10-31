// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The plug-in scanner (`suhina_plugins::scan`) against the real `suhina-plugin-host --scan`.
#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use suhina_plugins::scan::{
    Catalog, FolderFormat, HANG_RETRY, Scanned, find_files, find_files_of, hung_message, scan_file, standard_folders,
};

/// The timeout of a scan that is not meant to hang. It returns as soon as the host exits, so
/// this costs nothing; it is long so that a slow process start (on a busy Windows machine the
/// antivirus checks each one) is never mistaken for a hang.
const PATIENT: Duration = Duration::from_secs(60);
/// How long a test waits for a scan that is meant to hang to give up, before calling it a
/// stall. Far above any process start, so it never races the machine's load.
const STALL: Duration = Duration::from_secs(120);

fn host() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_suhina-plugin-host"))
}

/// A fresh, empty folder for one test.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("suhina-scan-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, text).unwrap();
}

#[test]
fn reads_what_a_plugin_file_holds() {
    let Scanned::Plugins(plugins) = scan_file(&host(), "test:clap", PATIENT) else { panic!("no plug-ins") };
    let [gain] = plugins.as_slice() else { panic!("{plugins:?}") };
    assert_eq!((gain.id.as_str(), gain.name.as_str()), ("org.suhina.test.gain", "Suhina Test Gain"));
}

#[test]
fn a_crash_or_a_hang_while_loading_is_a_failure_not_a_stall() {
    let crashed = scan_file(&host(), "test:crash", PATIENT);
    assert!(matches!(&crashed, Scanned::Failed(why) if why.contains("crashed")), "{crashed:?}");

    // A hang ends at the scanner's own deadline: the reason it gives is the one only that
    // deadline gives, with the timeout it was given. Whether the host started within the 500 ms
    // or not (a busy machine), `test:hang` never answers, so the answer is the same. The scan
    // runs on a thread so that a scanner that never gave up fails the test instead of stalling
    // it; that guard is not a measure of speed, only of "not forever".
    let timeout = Duration::from_millis(500);
    let (sender, answer) = mpsc::channel();
    let hang_host = host();
    std::thread::spawn(move || sender.send(scan_file(&hang_host, "test:hang", timeout)));
    let hung = answer.recv_timeout(STALL).unwrap_or_else(|e| panic!("the scan of a hang never ended ({e})"));
    assert_eq!(hung, Scanned::Failed(hung_message(timeout)));

    let dir = folder("not-a-plugin");
    let fake = dir.join("fake.clap");
    write(&fake, "not a plug-in");
    let failed = scan_file(&host(), &fake.display().to_string(), PATIENT);
    assert!(matches!(&failed, Scanned::Failed(why) if why.contains("could not load")), "{failed:?}");
    let missing = scan_file(Path::new("no-such-host"), "test:clap", PATIENT);
    assert!(matches!(&missing, Scanned::Failed(why) if why.contains("start")), "{missing:?}");
}

#[test]
fn finds_plugin_files_in_folders() {
    let dir = folder("find");
    write(&dir.join("a.clap"), "x");
    write(&dir.join("vendor/b.CLAP"), "x");
    write(&dir.join("vendor/readme.txt"), "x");
    write(&dir.join("1/2/3/4/5/6/7/deep.clap"), "x");
    write(&dir.join("1/2/3/4/5/6/7/8/9/too-deep.clap"), "x");
    // On macOS a plug-in is a folder named *.clap; it is found, not looked into.
    write(&dir.join("bundle.clap/Contents/inner.clap"), "x");
    // A folder linked into the standard one (as an installer may do) is looked into like any
    // other. Making a link needs a right the test may not have; then this part is skipped.
    write(&dir.join("elsewhere/linked.clap"), "x");
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_dir(dir.join("elsewhere"), dir.join("link")).is_ok();
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(dir.join("elsewhere"), dir.join("link")).is_ok();
    #[cfg(not(any(windows, unix)))]
    let linked = false;
    let found: Vec<String> = find_files(std::slice::from_ref(&dir))
        .iter()
        .map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().replace('\\', "/"))
        .collect();
    let mut expected = vec!["1/2/3/4/5/6/7/deep.clap", "a.clap", "bundle.clap", "elsewhere/linked.clap"];
    if linked {
        expected.push("link/linked.clap");
    } else {
        eprintln!("could not make a symbolic link here; the linked folder is not checked");
    }
    expected.push("vendor/b.CLAP");
    assert_eq!(found, expected);
    assert!(find_files(&[dir.join("no-such-folder")]).is_empty());
    assert!(standard_folders().iter().all(|f| f.is_dir()));
}

/// A VST2 folder's files are found for its format and for `Any`: a `.vst` bundle on macOS, a
/// `.dll` on Windows, a `.so` on Linux — and nothing else of the folder's.
#[cfg(feature = "vst2")]
#[test]
fn a_vst2_folder_s_files_are_found() {
    let dir = folder("find-vst2");
    let (shape, other) = if cfg!(target_os = "macos") {
        ("Gain.vst", "Gain.vst3")
    } else if cfg!(windows) {
        ("Gain.dll", "Gain.vst3")
    } else {
        ("Gain.so", "Gain.vst3")
    };
    write(&dir.join(shape), "x");
    write(&dir.join(other), "x");
    write(&dir.join("readme.txt"), "x");
    let found = find_files_of(&[(dir.clone(), FolderFormat::Vst2)]);
    assert_eq!(found, vec![dir.join(shape)], "only the format's files");
    let any = find_files(std::slice::from_ref(&dir));
    assert!(any.contains(&dir.join(shape)), "Any takes the VST2 file too");
    // The vst folders of this system are standard, whether or not anything is in them.
    let _ = standard_folders();
}

/// Seconds since 1970, now.
fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

/// A saved catalog (format 3) with one entry for `file` as it is on disk now: `what` is the
/// JSON of what reading it gave (`"plugins": […]` or `"failed": "…"`).
fn saved_entry(file: &Path, what: &str, tried: u64) -> Catalog {
    let meta = std::fs::metadata(file).unwrap();
    let modified = meta.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let path = serde_json::to_string(&file.display().to_string()).unwrap();
    let json = format!(
        r#"{{"version":3,"entries":[{{"path":{path},"modified":{modified},"size":{},"tried":{tried},{what}}}]}}"#,
        meta.len()
    );
    let catalog = Catalog::from_json(&json);
    assert_eq!(catalog.entries().len(), 1, "{json}");
    catalog
}

#[test]
fn a_scan_reads_new_and_changed_files_and_tries_failed_ones_again() {
    let dir = folder("catalog");
    let (a, b) = (dir.join("a.clap"), dir.join("b.clap"));
    write(&a, "not a plug-in");
    write(&b, "not one either");
    let folders = [dir.clone()];
    let mut catalog = Catalog::new();

    // Both fail, and are read once more, one at a time, before the scan gives up on them.
    let first = catalog.scan(&host(), &folders, PATIENT);
    assert_eq!((first.read.len(), first.failed.len(), first.unchanged), (2, 2, 0));
    assert_eq!(first.retried, [a.clone(), b.clone()]);
    assert_eq!(catalog.entries().len(), 2);
    assert_eq!(catalog.plugins().count(), 0);
    assert_eq!(catalog.failed().count(), 2);

    // A failure may be passing, so a plain scan tries the unchanged files again.
    let second = catalog.scan(&host(), &folders, PATIENT);
    assert_eq!((second.read.len(), second.failed.len(), second.unchanged), (2, 2, 0));

    write(&a, "not a plug-in, and longer now");
    std::fs::remove_file(&b).unwrap();
    let third = catalog.scan(&host(), &folders, PATIENT);
    assert_eq!(third.read, std::slice::from_ref(&a));
    assert_eq!(third.removed, [b]);
    assert_eq!(catalog.entries().len(), 1);

    let saved = Catalog::from_json(&catalog.to_json());
    assert_eq!(saved, catalog);
}

#[test]
fn a_file_that_holds_no_plugins_is_settled_until_it_changes() {
    let dir = folder("settled");
    let empty = dir.join("Empty.clap");
    write(&empty, "");
    let mut catalog = saved_entry(&empty, r#""plugins":[]"#, 0);
    assert_eq!(catalog.ignored().collect::<Vec<_>>(), [empty.as_path()]);
    assert_eq!((catalog.failed().count(), catalog.plugins().count()), (0, 0));

    let unchanged = catalog.scan(&host(), std::slice::from_ref(&dir), PATIENT);
    assert_eq!((unchanged.read.len(), unchanged.unchanged), (0, 1));
    assert!(unchanged.failed.is_empty() && unchanged.ignored.is_empty());
    assert_eq!(Catalog::from_json(&catalog.to_json()), catalog);

    // Changed, it is read again (and, not being a plug-in, fails to load).
    write(&empty, "something now");
    let changed = catalog.scan(&host(), std::slice::from_ref(&dir), PATIENT);
    assert_eq!(changed.read, std::slice::from_ref(&empty));
    assert_eq!(catalog.ignored().count(), 0);
}

#[test]
fn a_folder_gives_only_its_formats_files() {
    let dir = folder("formats");
    write(&dir.join("clap/a.clap"), "x");
    write(&dir.join("clap/b.vst3"), "x");
    write(&dir.join("vst3/c.vst3"), "x");
    // The VST2 shape of this system, which its folder now gives up, and a library file of
    // another system's, which is nothing here.
    let (vst2_file, foreign) = if cfg!(target_os = "macos") {
        ("e.vst", "d.dll")
    } else if cfg!(windows) {
        ("e.dll", "d.so")
    } else {
        ("e.so", "d.dll")
    };
    write(&dir.join("vst2").join(vst2_file), "x");
    write(&dir.join("vst2").join(foreign), "x");
    let name = |found: Vec<PathBuf>| -> Vec<String> {
        found.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect()
    };
    let folders = [
        (dir.join("clap"), FolderFormat::Clap),
        (dir.join("vst3"), FolderFormat::Vst3),
        (dir.join("vst2"), FolderFormat::Vst2),
    ];
    // The found list sorts by its paths, and the folders' names decide that: clap/, then
    // vst2/, then vst3/.
    assert_eq!(name(find_files_of(&folders)), ["a.clap", vst2_file, "c.vst3"]);
    assert_eq!(name(find_files_of(&[(dir.join("clap"), FolderFormat::Any)])), ["a.clap", "b.vst3"]);
    // The VST2 folder for Any gives its own shape only.
    assert_eq!(
        name(find_files_of(&[(dir.join("vst2"), FolderFormat::Any)])),
        [vst2_file.to_string()],
        "the other system's library file is not a plug-in file here"
    );
}

#[test]
fn a_file_built_for_another_processor_is_settled_without_loading_it() {
    use suhina_plugins::arch::Arch;
    // A universal Mach-O header that lists only the processor this test does not run on.
    let Some(here) = Arch::this_program().filter(|a| matches!(a, Arch::X86_64 | Arch::Arm64)) else { return };
    let other: u32 = if here == Arch::Arm64 { 0x0100_0007 } else { 0x0100_000C };
    let mut header = 0xCAFE_BABE_u32.to_be_bytes().to_vec();
    header.extend(1_u32.to_be_bytes());
    header.extend(other.to_be_bytes());
    header.extend([0; 16]);
    let dir = folder("foreign");
    let file = dir.join("Old.clap");
    std::fs::write(&file, &header).unwrap();
    let mut catalog = Catalog::new();
    let first = catalog.scan(&host(), std::slice::from_ref(&dir), PATIENT);
    assert_eq!(first.incompatible.len(), 1, "{first:?}");
    assert!(first.failed.is_empty() && first.retried.is_empty(), "not loaded, so not failed: {first:?}");
    let (path, why) = &first.incompatible[0];
    assert_eq!(path, &file);
    assert!(why.contains(here.name()), "{why}");
    assert_eq!(catalog.incompatible().count(), 1);
    assert_eq!(catalog.failed().count(), 0);
    // Settled: the next scan does not read it again while it is unchanged.
    let second = catalog.scan(&host(), std::slice::from_ref(&dir), PATIENT);
    assert_eq!((second.read.len(), second.unchanged), (0, 1));
    assert_eq!(Catalog::from_json(&catalog.to_json()), catalog);
}

#[test]
fn a_file_that_hung_is_tried_again_once_a_day() {
    let dir = folder("hung");
    let file = dir.join("slow.clap");
    write(&file, "not a plug-in");
    let hung = format!(r#""failed":{}"#, serde_json::to_string(&hung_message(Duration::from_secs(30))).unwrap());

    let mut recent = saved_entry(&file, &hung, now());
    let scan = recent.scan(&host(), std::slice::from_ref(&dir), PATIENT);
    assert_eq!((scan.read.len(), scan.unchanged), (0, 1));
    // Still listed as failed, so the user can scan again.
    assert_eq!(recent.failed().count(), 1);

    let mut old = saved_entry(&file, &hung, now() - HANG_RETRY.as_secs());
    let scan = old.scan(&host(), std::slice::from_ref(&dir), PATIENT);
    assert_eq!((scan.read.len(), scan.unchanged), (1, 0));
    assert!(old.entries()[0].tried + 60 >= now());

    // Any other failure is tried again by every scan.
    let mut crashed = saved_entry(&file, r#""failed":"crashed while loading""#, now());
    assert_eq!(crashed.scan(&host(), std::slice::from_ref(&dir), PATIENT).read.len(), 1);
}

#[test]
fn a_load_that_fails_once_is_found_by_the_same_scan() {
    use suhina_plugins::scan_pool::{scan_files, scan_files_retrying};
    let dir = folder("flaky");
    let marker = dir.join("loaded-once");
    let flaky = format!("test:flaky:{}", marker.display());
    let files: Vec<String> = vec![flaky.clone(), "test:clap".into(), "test:crash".into()];
    let (results, retried) = scan_files_retrying(&host(), &files, 2, &|_| PATIENT);
    assert_eq!(retried, [0, 2]);
    let Scanned::Plugins(plugins) = &results[0] else { panic!("{results:?}") };
    assert_eq!(plugins[0].name, "Suhina Test Gain");
    assert!(matches!(&results[1], Scanned::Plugins(_)), "{results:?}");
    assert!(matches!(&results[2], Scanned::Failed(why) if why.contains("crashed")), "{results:?}");
    // Without the second try, its first load fails, as a passing failure does.
    std::fs::remove_file(&marker).unwrap();
    let first = scan_files(&host(), &[flaky], 1, &|_| PATIENT);
    assert_eq!(first, [Scanned::Failed("the plug-in has no factory".into())]);
}

#[test]
fn a_saved_catalog_finds_plugins_by_id_or_name() {
    let json = r#"{"version":2,"entries":[
        {"path":"/plugins/fx.clap","modified":5,"size":7,"plugins":[
            {"id":"com.example.delay","name":"Echo Box","vendor":"Example","version":"1.2","features":["audio-effect","stereo"]},
            {"id":"com.example.verb","name":"Room","vendor":"Example","version":"1.2","features":[]}]},
        {"path":"/plugins/broken.clap","modified":1,"size":2,"failed":"crashed while loading"},
        {"path":"/plugins/odd.clap","modified":"x","size":2,"failed":"?"}]}"#;
    let catalog = Catalog::from_json(json);
    assert_eq!(catalog.entries().len(), 2);
    assert_eq!(catalog.plugins().count(), 2);
    let delay = catalog.find("com.example.delay").unwrap();
    assert_eq!(delay.features, ["audio-effect", "stereo"]);
    assert_eq!(delay.spec(), format!("clap:{}#com.example.delay", Path::new("/plugins/fx.clap").display()));
    assert_eq!(catalog.find("room").map(|p| p.id.as_str()), Some("com.example.verb"));
    assert!(catalog.find("nothing").is_none());
    assert_eq!(Catalog::from_json(&catalog.to_json()), catalog);
    // A catalog saved in format 1 is dropped, so the next scan reads everything; one in format
    // 2 is read, and its "holds no plug-ins" failures are settled.
    assert_eq!(Catalog::from_json(&json.replace(r#""version":2"#, r#""version":1"#)).entries().len(), 0);
    let empty = Catalog::from_json(&json.replace("crashed while loading", "the file holds no plug-ins"));
    assert_eq!((empty.ignored().count(), empty.failed().count()), (1, 0));

    for garbage in ["", "{", r#"{"version":2,"entries":[]}"#, r#"{"version":1}"#] {
        assert_eq!(Catalog::from_json(garbage), Catalog::new(), "{garbage}");
    }
}

#[test]
fn the_library_scans_its_folders_and_remembers_across_starts() {
    use suhina_plugins::library::Library;
    let dir = folder("library");
    write(&dir.join("plugins/fake.clap"), "not a plug-in");
    let mut library = Library::new(Some(dir.join("settings")), host()).without_standard_folders();
    assert!(library.add_folder(&dir.join("plugins")).unwrap());
    let first = library.scan(false).unwrap();
    assert_eq!((first.read.len(), first.failed.len()), (1, 1));

    // A new start loads the saved catalog.
    let mut later = Library::new(Some(dir.join("settings")), host()).without_standard_folders();
    assert_eq!(later.catalog().entries().len(), 1);
    // Its file failed, so a plain scan tries it again.
    assert_eq!(later.scan(false).unwrap().read.len(), 1);
    assert_eq!(later.scan(true).unwrap().read.len(), 1);
}

#[test]
fn worker_processes_read_many_files_and_survive_crashes_and_hangs() {
    use suhina_plugins::scan_pool::scan_files;
    let files: Vec<String> =
        ["test:clap", "test:crash", "test:vst3", "test:hang", "test:clap", "test:vst3"].map(String::from).to_vec();
    let started = Instant::now();
    let results = scan_files(&host(), &files, 2, &|_| Duration::from_secs(3));
    assert_eq!(results.len(), files.len());
    let names: Vec<String> = results
        .iter()
        .map(|r| match r {
            Scanned::Plugins(p) => p.iter().map(|p| p.name.clone()).collect::<Vec<_>>().join(","),
            Scanned::Failed(why) => format!("failed: {}", why.split(':').next().unwrap_or_default()),
            Scanned::NoPlugins => "no plug-ins".into(),
            Scanned::Incompatible(why) => format!("incompatible: {why}"),
        })
        .collect();
    assert_eq!(names[0], "Suhina Test Gain");
    assert!(names[1].starts_with("failed: crashed"), "{names:?}");
    assert_eq!(names[2], "Suhina Test Gain");
    assert!(names[3].starts_with("failed: hung"), "{names:?}");
    assert_eq!(names[4..], ["Suhina Test Gain", "Suhina Test Gain"]);
    // The VST3 test plug-in's id is in the canonical text form.
    let Scanned::Plugins(vst3) = &results[2] else { panic!() };
    assert_eq!(vst3[0].id.len(), 32);
    // What its compatibility class says it replaces: a VST2 plug-in, by its derived class id.
    assert_eq!(vst3[0].replaces, ["5653544761696E6761696E0000000000"]);
    assert_eq!(vst3[0].replaces[0], suhina_plugins::matching::vst2_class_id(0x4761_696E, "Gain"));
    // Its module declares no ARA factory class, so the scan does not mark it as ARA.
    assert!(!vst3[0].ara);
    // One hang costs its timeout once, not per file. The room is for slow process starts: on
    // a loaded Windows machine each start can take seconds while the antivirus checks it.
    assert!(started.elapsed() < Duration::from_secs(60), "{:?}", started.elapsed());
}
